// TODO(P-runtime-host-trait): worker_runtime currently holds tauri::AppHandle
// and emits stream deltas via tauri::Emitter directly. This is the only
// LEGACY_TAURI_ALLOWED entry in tests/review_agent_b_constraints.rs. To remove
// it, route stream events through a RuntimeHost / event sink trait and inject
// from the transport layer (transport/tauri_commands/) instead. Until then
// this file is the documented exception to the runtime/ purity rule.

use std::sync::Arc;

use futures::StreamExt;
use log::{info, warn};

use crate::llm::gateway::LlmGateway;
use crate::llm::masking::MaskingLevel;
use crate::llm::streaming::{ChatMessage, StopReason, StreamEvent, ToolDefinition};
use crate::models::settings::AppSettings;
use crate::plugin::registry::ToolRegistry;
use crate::plugin::tool_trait::ToolError as LegacyToolError;
use crate::runtime::agent::empty_response_recovery::{
    EmptyResponseRecoveryConfig, EmptyResponseRecoveryState, RecoveryDecision,
};
use crate::runtime::agent::message_bridge;
use crate::runtime::agent::subagent_result_envelope::{
    build_subagent_transcript_ref, SubAgentResultEnvelope, SubAgentTerminalToolResult,
    SubAgentTranscriptEntry,
};
use crate::runtime::agent::{AgentRuntime, SpawnChildRunRequest, SubagentTranscriptEntryRecord};
use crate::runtime::cancellation::CancellationToken;
use crate::runtime::chat::tool_round_driver::{ToolRoundDriver, ToolRoundResult};
use crate::runtime::chat::tool_round_types::{RuntimeToolCallOutcome, RuntimeToolCallRequest};
use crate::runtime::event_bus::RuntimeEventBus;
use crate::runtime::identity::IdentityMapping;
use crate::runtime::ids::RunId;
use crate::runtime::query_engine::QueryEngine;
use crate::runtime::state::TurnState;
use crate::runtime::tools::capability::{DefaultFileOperations, FileStateCache};
use crate::runtime::tools::permission::{PermissionDecision, PermissionMode};
use crate::telemetry::{record_diagnostic, DiagnosticEvent, DiagnosticSource};

use crate::llm::sub_agent::{SubAgentConfig, SubAgentResult, SubAgentRuntimeDeps};

/// Compute effective AppSettings for a sub-agent invocation.
///
/// If `model_override` is `Some(non_empty)`, returns a clone of `base` with
/// `primary_model` replaced. Otherwise returns a clone of `base` unchanged.
/// Empty string is treated as "no override" to defend against bad caller input.
pub fn effective_settings_for_subagent(
    base: &AppSettings,
    model_override: Option<&str>,
) -> AppSettings {
    let mut s = base.clone();
    if let Some(model) = model_override.filter(|m| !m.is_empty()) {
        s.primary_model = model.to_string();
    }
    s
}

/// 单次 worker turn 所需的 LLM 请求快照。
pub struct WorkerTurnRequest {
    pub subagent_conversation_id: String,
    pub messages: Vec<ChatMessage>,
    pub tool_defs: Vec<ToolDefinition>,
    pub system_prompt: String,
    pub system_message: Option<ChatMessage>,
    pub dynamic_context: Option<String>,
    pub max_iterations: usize,
}

/// worker 运行时的生命周期配置。
pub struct WorkerRunConfig {
    pub allowed_tools: Vec<String>,
    pub conversation_id: String,
    pub parent_run_id: Option<RunId>,
    pub background: bool,
    pub app_handle: Option<tauri::AppHandle>,
    pub cancel_token: Option<CancellationToken>,
    pub permission_mode: PermissionMode,
    /// Caller-supplied model override forwarded from SubAgentConfig.
    /// Consumed by run_worker_turn to compute effective AppSettings (P2.2).
    pub model_override: Option<String>,
    /// The parent's spawn_subagent tool_call_id, forwarded from SubAgentConfig.
    /// Stamped onto every emitted `tool:executing` / `tool:completed` event so
    /// downstream UI can fold sub-agent tool history under the originating
    /// spawn step (claude-code-best parity).
    pub parent_tool_use_id: Option<String>,
}

/// 一等 subagent worker runtime：拥有 LLM loop、tool round、转录与 completion。
pub struct SubagentWorkerRuntime<'a> {
    gateway: &'a LlmGateway,
    tool_registry: &'a ToolRegistry,
    runtime_deps: &'a SubAgentRuntimeDeps,
    settings: &'a AppSettings,
}

impl<'a> SubagentWorkerRuntime<'a> {
    pub fn new(
        gateway: &'a LlmGateway,
        tool_registry: &'a ToolRegistry,
        runtime_deps: &'a SubAgentRuntimeDeps,
        settings: &'a AppSettings,
    ) -> Self {
        Self {
            gateway,
            tool_registry,
            runtime_deps,
            settings,
        }
    }

    pub async fn run(
        &self,
        config: SubAgentConfig,
    ) -> std::result::Result<SubAgentResult, LegacyToolError> {
        let all_schemas = self.tool_registry.get_all_schemas().await;
        let available_names: Vec<String> = all_schemas.iter().map(|s| s.name.clone()).collect();

        let final_allowed = crate::runtime::agent::tool_whitelist::resolve_agent_tools(
            &config.allowed_tools,
            &config.disallowed_tools,
            &available_names,
            config.background,
            false,
        );

        let turn_request =
            Self::build_turn_request_with_allowed(&config, all_schemas, &final_allowed);
        let run_config = Self::build_run_config_with_allowed(&config, final_allowed);
        self.run_worker_turn(turn_request, run_config).await
    }

    fn build_run_config_with_allowed(
        config: &SubAgentConfig,
        final_allowed: Vec<String>,
    ) -> WorkerRunConfig {
        WorkerRunConfig {
            allowed_tools: final_allowed,
            conversation_id: config.conversation_id.clone(),
            parent_run_id: config.parent_run_id.clone(),
            background: config.background,
            app_handle: config.app_handle.clone(),
            cancel_token: config.cancel_token.clone(),
            permission_mode: config.permission_mode,
            model_override: config.model_override.clone(),
            parent_tool_use_id: config.parent_tool_use_id.clone(),
        }
    }

    fn build_turn_request_with_allowed(
        config: &SubAgentConfig,
        all_schemas: Vec<ToolDefinition>,
        final_allowed: &[String],
    ) -> WorkerTurnRequest {
        let tool_defs: Vec<ToolDefinition> = all_schemas
            .into_iter()
            .filter(|schema| final_allowed.contains(&schema.name))
            .collect();

        info!(
            "[SubAgent] Tool schemas after whitelist: {} (was {} available)",
            tool_defs.len(),
            final_allowed.len()
        );

        WorkerTurnRequest {
            subagent_conversation_id: String::new(),
            messages: vec![ChatMessage::text("user", &config.task)],
            tool_defs,
            system_prompt: config.system_prompt.clone(),
            system_message: Some(ChatMessage::text("system", config.system_prompt.clone())),
            dynamic_context: (!config.dynamic_context.is_empty())
                .then(|| config.dynamic_context.clone()),
            max_iterations: config.max_iterations,
        }
    }

    async fn run_worker_turn(
        &self,
        mut request: WorkerTurnRequest,
        config: WorkerRunConfig,
    ) -> std::result::Result<SubAgentResult, LegacyToolError> {
        let workspace = crate::telemetry::diagnostics_workspace();
        let subagent_diag = |event: &str,
                             ok: Option<bool>,
                             error: Option<String>,
                             payload: Option<serde_json::Value>| {
            let mut diag = DiagnosticEvent::new(event, DiagnosticSource::Backend)
                .conversation_id(self.runtime_deps.conversation_id.clone())
                .payload(payload.unwrap_or_else(|| serde_json::json!({})));
            if let Some(ok) = ok {
                diag = diag.ok(ok);
            }
            if let Some(error) = error {
                diag = diag.error(error);
            }
            record_diagnostic(&workspace, diag);
        };
        subagent_diag(
            "subagent.spawn.started",
            Some(true),
            None,
            Some(serde_json::json!({
                "conversationId": self.runtime_deps.conversation_id,
                "parentRunId": config.parent_run_id.as_ref().map(|id| id.as_str().to_string()),
                "background": config.background,
            })),
        );
        let agent_runtime = self
            .runtime_deps
            .agent_runtime
            .clone()
            .unwrap_or_else(|| Arc::new(AgentRuntime::for_test()));

        let child_handle = if let Some(parent_run_id) = config.parent_run_id.clone() {
            Some(
                agent_runtime
                    .spawn_child_run(SpawnChildRunRequest {
                        parent_run_id,
                        background: config.background,
                        allowed_tools: config.allowed_tools.clone(),
                    })
                    .await
                    .map_err(LegacyToolError::Other)?,
            )
        } else {
            None
        };
        let child_run_id = child_handle
            .as_ref()
            .map(|handle| handle.child_run_id().clone())
            .unwrap_or_else(|| RunId::new(format!("sub-{}", uuid::Uuid::new_v4())));
        let child_agent_id = child_handle
            .as_ref()
            .map(|handle| handle.invocation().agent_id.clone());
        let sub_conv_id = child_run_id.as_str().to_string();
        request.subagent_conversation_id = sub_conv_id.clone();

        let child_cancel = config
            .cancel_token
            .as_ref()
            .map(|parent| parent.child_token())
            .unwrap_or_default();
        let child_read_file_state = self
            .runtime_deps
            .read_file_state
            .as_ref()
            .map(|cache| cache.clone_for_child())
            .unwrap_or_else(|| Arc::new(FileStateCache::new()));

        let request_scoped_runtime_deps = self.runtime_deps.request_scoped_tool_deps(
            child_run_id.clone(),
            child_agent_id.clone(),
            Some(child_cancel.clone()),
            Some(child_read_file_state.clone()),
        );
        let dispatcher = self
            .tool_registry
            .to_runtime_dispatcher(request_scoped_runtime_deps)
            .await;

        let query_engine =
            self.build_query_engine(dispatcher, child_read_file_state, child_run_id.clone());
        let tool_event_bus = RuntimeEventBus::new();
        let allowed_tools = config.allowed_tools.clone();
        let effective_settings =
            effective_settings_for_subagent(self.settings, config.model_override.as_deref());

        let mut turn = TurnState::new(
            IdentityMapping::from_legacy_conversation_id(self.runtime_deps.conversation_id.clone()),
            child_run_id.clone(),
            request
                .messages
                .first()
                .map(|message| message.content.clone())
                .unwrap_or_default(),
        )
        .with_cancellation(child_cancel.clone())
        .with_permission_mode(config.permission_mode);
        if let Some(agent_id) = child_agent_id.clone() {
            turn.set_agent_id(agent_id);
        }

        let mut output = String::new();
        let mut files: Vec<String> = Vec::new();
        let mut iterations_used = 0;
        let mut pending_ask: Option<PermissionDecision> = None;
        let mut terminal_tool_results: Vec<SubAgentTerminalToolResult> = Vec::new();
        let mut cancelled = false;
        let mut last_stop_reason: Option<StopReason> = None;
        let mut recovery = EmptyResponseRecoveryState::new(EmptyResponseRecoveryConfig::default());
        let max_tokens =
            crate::llm::max_tokens::default_max_tokens_for_model(&effective_settings.primary_model);

        'agent_loop: for iteration in 0..request.max_iterations {
            if child_cancel.is_cancelled() {
                cancelled = true;
                break;
            }

            iterations_used = iteration + 1;
            info!(
                "[SubAgent] iter={}/{}, messages={}",
                iteration,
                request.max_iterations,
                request.messages.len()
            );

            {
                let agent_found = request.tool_defs.iter().any(|d| d.name == "Agent");
                let agent_has_emp = request
                    .tool_defs
                    .iter()
                    .find(|d| d.name == "Agent")
                    .map(|d| d.description.contains("<available_subagent_types>"))
                    .unwrap_or(false);
                log::info!(
                    "[tool-desc-trace] Teammate LLM request built: tools={} agent_found_in_tools={} agent_has_emp_section={}",
                    request.tool_defs.len(),
                    agent_found,
                    agent_has_emp,
                );
            }
            let stream_result = self
                .gateway
                .stream_message(
                    &effective_settings,
                    request.messages.clone(),
                    MaskingLevel::Relaxed,
                    worker_system_prompt_for_gateway(&request),
                    request.dynamic_context.as_deref(),
                    Some(request.tool_defs.clone()),
                    max_tokens,
                    Some(&sub_conv_id),
                    None,
                )
                .await;

            let (_task_id, mut stream, _mask_ctx, _cancel_rx) = match stream_result {
                Ok(result) => result,
                Err(err) => {
                    warn!("[SubAgent] LLM call failed at iter {}: {}", iteration, err);
                    output = format!("Sub-agent LLM error: {}", err);
                    break;
                }
            };

            let mut iter_content = String::new();
            let mut tool_calls = Vec::new();
            let mut stop_reason = StopReason::EndTurn;

            while let Some(event) = stream.next().await {
                if child_cancel.is_cancelled() {
                    self.gateway.cancel_conversation(&sub_conv_id).ok();
                    cancelled = true;
                    break;
                }
                match event {
                    StreamEvent::ContentDelta { delta } => {
                        iter_content.push_str(&delta);
                    }
                    StreamEvent::ToolCallStart { tool_call } => match tool_call.into_valid() {
                        Ok(tool_call) => tool_calls.push(tool_call),
                        Err(err) => {
                            warn!("[SubAgent] Dropping invalid stream tool_call: {err}");
                            output = "Sub-agent stream error: invalid tool call".to_string();
                            break 'agent_loop;
                        }
                    },
                    StreamEvent::Done {
                        stop_reason: sr, ..
                    } => {
                        stop_reason = sr;
                        break;
                    }
                    StreamEvent::Error { error } => {
                        warn!("[SubAgent] Stream error: {}", error);
                        if output.is_empty() {
                            output = format!("Sub-agent stream error: {}", error);
                        }
                        // Break the outer agent_loop directly. Breaking only the
                        // inner `while` would fall through to the empty-content
                        // branch below, which used to overwrite `output` with the
                        // empty `iter_content`, losing the stream error message.
                        break 'agent_loop;
                    }
                    _ => {}
                }
            }

            if cancelled {
                break;
            }

            info!(
                "[SubAgent] iter={} content_len={} tool_calls={} stop={:?}",
                iteration,
                iter_content.len(),
                tool_calls.len(),
                stop_reason
            );

            if stop_reason != StopReason::ToolUse || tool_calls.is_empty() {
                last_stop_reason = Some(stop_reason.clone());
                let had_content = !iter_content.is_empty();
                let had_tools = !tool_calls.is_empty();

                // Always push an assistant turn so the transcript reflects that
                // the LLM call actually happened. When content is empty we use
                // a placeholder rather than an empty string, so future retry /
                // compaction paths feeding these messages back to Anthropic
                // don't trip the "text block cannot be empty" reject.
                let assistant_text = if had_content {
                    iter_content.clone()
                } else {
                    format!("[empty turn: stop_reason={:?}]", stop_reason)
                };
                request
                    .messages
                    .push(ChatMessage::text("assistant", assistant_text));

                match recovery.decide(
                    stop_reason.clone(),
                    had_content,
                    had_tools,
                    max_tokens,
                    iterations_used as u32,
                ) {
                    RecoveryDecision::Retry { hint_message } => {
                        info!(
                            "[SubAgent] empty-response recovery attempt {} (stop={:?})",
                            recovery.attempts_used(),
                            stop_reason
                        );
                        request
                            .messages
                            .push(ChatMessage::text("user", hint_message.to_string()));
                        continue 'agent_loop;
                    }
                    RecoveryDecision::Surface { fallback_output } => {
                        output = fallback_output;
                        break;
                    }
                    RecoveryDecision::NoRecovery => {
                        // had_content == true && stop_reason != ToolUse
                        output = iter_content;
                        break;
                    }
                }
            }

            request
                .messages
                .push(ChatMessage::assistant_with_tool_calls(
                    iter_content.clone(),
                    tool_calls
                        .iter()
                        .map(|tool_call| crate::llm::streaming::ToolCall {
                            id: tool_call.id.clone(),
                            name: tool_call.name.clone(),
                            arguments: tool_call.arguments.clone(),
                        })
                        .collect(),
                    None,
                    None,
                ));

            let runtime_tool_calls: Vec<RuntimeToolCallRequest> = tool_calls
                .into_iter()
                .filter_map(|tool_call| {
                    let purpose = Some(format!("[Browser Agent] {}", tool_call.name));
                    match RuntimeToolCallRequest::from_tool_call(tool_call, purpose) {
                        Ok(call) => Some(call),
                        Err(err) => {
                            warn!("[SubAgent] Dropping invalid runtime tool_call: {err}");
                            None
                        }
                    }
                })
                .collect();

            for tool_call in &runtime_tool_calls {
                info!(
                    "[SubAgent] Executing tool '{}' (id={})",
                    tool_call.tool_name, tool_call.tool_call_id
                );
                emit_tool_executing(
                    config.app_handle.as_ref(),
                    &config.conversation_id,
                    &tool_call.tool_name,
                    &tool_call.tool_call_id,
                    tool_call.purpose.as_deref(),
                    config.parent_tool_use_id.as_deref(),
                );
            }

            let round_driver = ToolRoundDriver::new(query_engine.clone())
                .with_allowed_tools(allowed_tools.clone());
            let round_results = round_driver
                .execute_round(&turn, &tool_event_bus, runtime_tool_calls)
                .await;

            // Anthropic rejects multiple tool_result blocks sharing the same
            // tool_use_id within one assistant turn. Dedup at push time so a
            // glitched round (Blocked + Completed for the same id, retry
            // double-emit, etc.) cannot poison this sub-agent's transcript.
            let mut pushed_tool_call_ids: std::collections::HashSet<String> =
                std::collections::HashSet::new();

            for round_result in round_results {
                match round_result {
                    ToolRoundResult::Blocked(blocked) => {
                        terminal_tool_results.push(SubAgentTerminalToolResult {
                            tool_call_id: blocked.tool_call_id.clone(),
                            tool_name: blocked.tool_name.clone(),
                            success: false,
                            summary: blocked.reason.clone(),
                            generated_files: Vec::new(),
                        });
                        emit_tool_completed(
                            config.app_handle.as_ref(),
                            &config.conversation_id,
                            &blocked.tool_call_id,
                            false,
                            Some(blocked.reason.as_str()),
                            config.parent_tool_use_id.as_deref(),
                        );
                        if pushed_tool_call_ids.insert(blocked.tool_call_id.clone()) {
                            request.messages.push(ChatMessage::tool_result_with_status(
                                &blocked.tool_call_id,
                                &blocked.tool_name,
                                blocked.reason,
                                true,
                            ));
                        } else {
                            log::warn!(
                                "[SubAgent] dropped duplicate tool_result for id={} tool={} (Blocked)",
                                blocked.tool_call_id,
                                blocked.tool_name
                            );
                        }
                    }
                    ToolRoundResult::Ok(RuntimeToolCallOutcome::Completed {
                        tool_call_id,
                        tool_name,
                        content,
                        is_error,
                        file_meta,
                        context_modifier_message,
                        max_result_size_chars,
                        ..
                    }) => {
                        let content_for_message =
                            truncate_tool_content(&content, max_result_size_chars);
                        let generated_files = collect_generated_files(
                            self.runtime_deps,
                            file_meta.as_ref(),
                            &content,
                        );
                        let tool_summary = summarize_tool_content(&content, 240);
                        let frontend_summary = summarize_tool_content(&content, 100);
                        terminal_tool_results.push(SubAgentTerminalToolResult {
                            tool_call_id: tool_call_id.clone(),
                            tool_name: tool_name.clone(),
                            success: !is_error,
                            summary: tool_summary.clone(),
                            generated_files: generated_files.clone(),
                        });
                        emit_tool_completed(
                            config.app_handle.as_ref(),
                            &config.conversation_id,
                            &tool_call_id,
                            !is_error,
                            Some(frontend_summary.as_str()),
                            config.parent_tool_use_id.as_deref(),
                        );
                        files.extend(generated_files);
                        if pushed_tool_call_ids.insert(tool_call_id.clone()) {
                            request.messages.push(ChatMessage::tool_result_with_status(
                                &tool_call_id,
                                &tool_name,
                                content_for_message,
                                is_error,
                            ));
                            if let Some(modifier) = context_modifier_message
                                .as_ref()
                                .and_then(context_modifier_to_message)
                            {
                                request.messages.push(modifier);
                            }
                        } else {
                            log::warn!(
                                "[SubAgent] dropped duplicate tool_result for id={} tool={} (Completed)",
                                tool_call_id,
                                tool_name
                            );
                        }
                    }
                    ToolRoundResult::Ok(RuntimeToolCallOutcome::AskRequired {
                        tool_call_id,
                        tool_name,
                        decision,
                        ..
                    }) => {
                        let bubbled =
                            annotate_subagent_ask_decision(&tool_name, &tool_call_id, decision);
                        terminal_tool_results.push(SubAgentTerminalToolResult {
                            tool_call_id: tool_call_id.clone(),
                            tool_name: tool_name.clone(),
                            success: false,
                            summary: "Permission Ask required".to_string(),
                            generated_files: Vec::new(),
                        });
                        emit_tool_completed(
                            config.app_handle.as_ref(),
                            &config.conversation_id,
                            &tool_call_id,
                            false,
                            Some("Permission Ask required"),
                            config.parent_tool_use_id.as_deref(),
                        );
                        if pushed_tool_call_ids.insert(tool_call_id.clone()) {
                            request.messages.push(ChatMessage::tool_result_with_status(
                                &tool_call_id,
                                &tool_name,
                                "Permission Ask required".to_string(),
                                true,
                            ));
                        } else {
                            log::warn!(
                                "[SubAgent] dropped duplicate tool_result for id={} tool={} (AskRequired)",
                                tool_call_id,
                                tool_name
                            );
                        }
                        warn!(
                            "[SubAgent] Tool '{}' returned AskRequired; bubbling to parent: {}",
                            tool_name, bubbled
                        );
                        pending_ask = Some(bubbled);
                        break 'agent_loop;
                    }
                    ToolRoundResult::Ok(RuntimeToolCallOutcome::InteractionRequired {
                        tool_call_id,
                        tool_name,
                        ..
                    }) => {
                        terminal_tool_results.push(SubAgentTerminalToolResult {
                            tool_call_id: tool_call_id.clone(),
                            tool_name: tool_name.clone(),
                            success: false,
                            summary: "User interaction required".to_string(),
                            generated_files: Vec::new(),
                        });
                        emit_tool_completed(
                            config.app_handle.as_ref(),
                            &config.conversation_id,
                            &tool_call_id,
                            false,
                            Some("User interaction required"),
                            config.parent_tool_use_id.as_deref(),
                        );
                        if pushed_tool_call_ids.insert(tool_call_id.clone()) {
                            request.messages.push(ChatMessage::tool_result_with_status(
                                &tool_call_id,
                                &tool_name,
                                "User interaction required; sub-agents cannot ask the user directly."
                                    .to_string(),
                                true,
                            ));
                        } else {
                            log::warn!(
                                "[SubAgent] dropped duplicate tool_result for id={} tool={} (InteractionRequired)",
                                tool_call_id,
                                tool_name
                            );
                        }
                    }
                }
            }
        }

        if iterations_used >= request.max_iterations
            && output.is_empty()
            && pending_ask.is_none()
            && !cancelled
        {
            output = "Sub-agent reached iteration limit.".to_string();
        }
        if cancelled && output.is_empty() {
            output = "Sub-agent cancelled.".to_string();
        }

        let mut generated_files = files;
        generated_files.sort();
        generated_files.dedup();

        let transcript_ref = build_subagent_transcript_ref(child_run_id.as_str());
        let transcript_entries: Vec<SubagentTranscriptEntryRecord> = request
            .messages
            .iter()
            .map(|message| SubagentTranscriptEntryRecord {
                role: message.role.clone(),
                content: message.content.clone(),
                tool_call_id: message.tool_call_id.clone(),
                tool_name: message.name.clone(),
            })
            .collect();
        agent_runtime
            .store_transcript(&transcript_ref, &transcript_entries)
            .map_err(LegacyToolError::Other)?;

        let mut transcript_snapshot: Vec<SubAgentTranscriptEntry> = transcript_entries
            .iter()
            .rev()
            .take(16)
            .map(|entry| SubAgentTranscriptEntry {
                role: entry.role.clone(),
                content: safe_truncate(&entry.content, 800).to_string(),
                tool_call_id: entry.tool_call_id.clone(),
                tool_name: entry.tool_name.clone(),
            })
            .collect();
        transcript_snapshot.reverse();

        let envelope = SubAgentResultEnvelope {
            schema_version: 1,
            output: output.clone(),
            iterations_used,
            generated_files: generated_files.clone(),
            terminal_tool_results,
            transcript_snapshot,
            transcript_ref: Some(transcript_ref.clone()),
            // Use serde's snake_case rename instead of Debug derive so the
            // string format stays stable and matches Anthropic protocol field
            // names ("max_tokens" / "end_turn" / "tool_use" / "stop_sequence").
            terminal_stop_reason: last_stop_reason.as_ref().and_then(|r| {
                serde_json::to_value(r)
                    .ok()
                    .and_then(|v| v.as_str().map(String::from))
            }),
            max_tokens_recovery_attempts: recovery.attempts_used(),
        };

        self.gateway.clear_task(&sub_conv_id);

        if let Some(handle) = child_handle.as_ref() {
            if cancelled {
                let _ = agent_runtime.cancel_run(child_run_id.clone()).await;
            } else if handle.invocation().background {
                if let (Some(bus), Some(parent_run_id)) = (
                    self.runtime_deps.event_bus.clone(),
                    config.parent_run_id.clone(),
                ) {
                    let summary = message_bridge::format_sub_agent_envelope_summary(&envelope);
                    let _ = agent_runtime
                        .complete_background_run(
                            &child_run_id,
                            Some(&summary),
                            Some(&transcript_ref),
                            self.runtime_deps.session_id.clone(),
                            parent_run_id,
                            bus,
                        )
                        .await;
                } else {
                    let _ = agent_runtime.complete_run(&child_run_id).await;
                }
            } else {
                let _ = agent_runtime.complete_run(&child_run_id).await;
            }
        }

        info!(
            "[SubAgent] Complete: iterations={}, output_len={}, files={}",
            iterations_used,
            output.len(),
            generated_files.len()
        );

        if let Some(decision) = pending_ask {
            subagent_diag(
                "subagent.failed",
                Some(false),
                None,
                Some(serde_json::json!({
                    "reason": "permission_ask",
                    "childRunId": child_run_id.as_str(),
                })),
            );
            return Err(LegacyToolError::AskRequired(decision));
        }

        subagent_diag(
            if cancelled {
                "subagent.failed"
            } else {
                "subagent.completed"
            },
            Some(!cancelled),
            if cancelled {
                Some("subagent cancelled".to_string())
            } else {
                None
            },
            Some(serde_json::json!({
                "childRunId": child_run_id.as_str(),
                "iterationsUsed": iterations_used,
                "generatedFiles": generated_files.len(),
                "cancelled": cancelled,
            })),
        );

        Ok(SubAgentResult {
            output,
            files: generated_files,
            iterations_used,
            envelope,
        })
    }

    fn build_query_engine(
        &self,
        dispatcher: Arc<crate::runtime::tools::ToolDispatcher>,
        child_read_file_state: Arc<FileStateCache>,
        child_run_id: RunId,
    ) -> QueryEngine {
        let (python_binary, python_home) = self
            .runtime_deps
            .runtime_resolver
            .as_ref()
            .and_then(|resolver| {
                resolver
                    .workspace_dependencies()
                    .ok()
                    .map(|deps| (deps.python, None))
            })
            .unwrap_or_else(|| {
                log::warn!(
                    "managed runtime resolver unavailable for worker file operations; using inert Python path"
                );
                (std::path::PathBuf::from("__managed_runtime_resolver_missing__"), None)
            });
        let file_ops = Arc::new(DefaultFileOperations {
            storage: self.runtime_deps.storage.clone(),
            file_manager: self.runtime_deps.file_manager.clone(),
            workspace_path: self.runtime_deps.workspace_path.clone(),
            conversation_id: self.runtime_deps.conversation_id.clone(),
            run_id: Some(child_run_id),
            python_binary: Some(python_binary),
            python_home,
        });

        let engine = QueryEngine::with_dispatcher(dispatcher)
            .with_workspace_path(self.runtime_deps.workspace_path.clone())
            .with_authorized_workspace(self.runtime_deps.authorized_workspace.clone())
            .with_file_ops(file_ops)
            .with_runtime_resolver(self.runtime_deps.runtime_resolver.clone())
            .with_read_file_state(child_read_file_state);

        // Phase 5: seed child QueryEngine with parent's permission_ctx snapshot
        // so path tools see the same authorized dirs (UserSettings working dirs +
        // session attachment dirs merged in at spawn time).
        if let Some(ctx) = self.runtime_deps.permission_ctx.clone() {
            engine.with_permission_ctx(ctx)
        } else {
            engine
        }
    }
}

fn emit_tool_executing(
    app_handle: Option<&tauri::AppHandle>,
    conversation_id: &str,
    tool_name: &str,
    tool_call_id: &str,
    purpose: Option<&str>,
    parent_tool_use_id: Option<&str>,
) {
    if let Some(app) = app_handle {
        log::info!(
            "[SubAgent-emit] tool:executing conv={} tool={} id={} parent_tool_use_id={:?}",
            conversation_id,
            tool_name,
            tool_call_id,
            parent_tool_use_id
        );
        let _ = tauri::Emitter::emit(
            app,
            "tool:executing",
            serde_json::json!({
                "conversationId": conversation_id,
                "toolName": tool_name,
                "toolId": tool_call_id,
                "purpose": purpose,
                // Sub-agent internal tool calls. Frontend filters these out of
                // the parent conversation's tool-execution panel — they belong
                // logically to the sub-agent's own run, not to the parent's
                // visible work. Kept on the wire so a future "sub-agent detail
                // pane" can subscribe and show them.
                "scope": "child",
                // claude-code-best parity: stamp the spawn_subagent tool_call_id
                // so the future detail pane can fold sub-agent tool history
                // under the originating spawn step.
                "parentToolUseId": parent_tool_use_id,
            }),
        );
    }
}

fn emit_tool_completed(
    app_handle: Option<&tauri::AppHandle>,
    conversation_id: &str,
    tool_call_id: &str,
    success: bool,
    summary: Option<&str>,
    parent_tool_use_id: Option<&str>,
) {
    if let Some(app) = app_handle {
        log::info!(
            "[SubAgent-emit] tool:completed conv={} id={} success={} parent_tool_use_id={:?}",
            conversation_id,
            tool_call_id,
            success,
            parent_tool_use_id
        );
        let _ = tauri::Emitter::emit(
            app,
            "tool:completed",
            serde_json::json!({
                "conversationId": conversation_id,
                "toolId": tool_call_id,
                "success": success,
                "summary": summary,
                // Same rationale as emit_tool_executing — front-end filters
                // by `scope:'child'`.
                "scope": "child",
                "parentToolUseId": parent_tool_use_id,
            }),
        );
    }
}

fn context_modifier_to_message(value: &serde_json::Value) -> Option<ChatMessage> {
    let role = value.get("role")?.as_str()?;
    let content = value.get("content")?.as_str()?;
    Some(ChatMessage::text(role, content))
}

fn worker_system_prompt_for_gateway(request: &WorkerTurnRequest) -> Option<&str> {
    request
        .system_message
        .as_ref()
        .filter(|message| message.role == "system" && !message.content.trim().is_empty())
        .map(|message| message.content.as_str())
        .or_else(|| {
            (!request.system_prompt.trim().is_empty()).then_some(request.system_prompt.as_str())
        })
}

fn annotate_subagent_ask_decision(
    tool_name: &str,
    tool_call_id: &str,
    decision: PermissionDecision,
) -> PermissionDecision {
    match decision {
        PermissionDecision::Ask {
            message,
            suggestions,
            remember_options,
            default_destination,
            reason,
            path_auth_scope,
        } => PermissionDecision::Ask {
            message: format!(
                "Subagent tool '{}' (tool_call_id={}) requires confirmation: {}",
                tool_name, tool_call_id, message
            ),
            suggestions,
            remember_options,
            default_destination,
            reason,
            path_auth_scope,
        },
        other => other,
    }
}

fn collect_generated_files(
    runtime_deps: &SubAgentRuntimeDeps,
    file_meta: Option<&crate::plugin::tool_trait::FileMeta>,
    tool_content: &str,
) -> Vec<String> {
    let mut files = Vec::new();

    if let Some(meta) = file_meta {
        let full_path = runtime_deps.file_manager.full_path(&meta.stored_path);
        files.push(full_path.display().to_string());
    }

    for line in tool_content.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed
            .strip_prefix("File: ")
            .or_else(|| trimmed.strip_prefix("**File**: "))
            .or_else(|| trimmed.strip_prefix("- **File**: "))
        {
            let path = rest.trim();
            if path.starts_with('/') && !files.iter().any(|existing| existing == path) {
                files.push(path.to_string());
            }
        }
        if trimmed.contains("/generated/") && trimmed.contains(".json") {
            for word in trimmed.split_whitespace() {
                let clean = word.trim_matches(|c: char| {
                    !c.is_alphanumeric() && c != '/' && c != '.' && c != '_' && c != '-'
                });
                if clean.starts_with('/')
                    && clean.contains("/generated/")
                    && !files.iter().any(|existing| existing == clean)
                {
                    files.push(clean.to_string());
                }
            }
        }
    }

    files
}

fn summarize_tool_content(content: &str, max_bytes: usize) -> String {
    if content.len() > max_bytes {
        format!("{}...", safe_truncate(content, max_bytes))
    } else {
        content.to_string()
    }
}

fn truncate_tool_content(content: &str, max_bytes: usize) -> String {
    if content.len() > max_bytes {
        format!(
            "{}\n[Output truncated: exceeded {} char limit. Use a more specific query to get smaller results.]",
            safe_truncate(content, max_bytes),
            max_bytes
        )
    } else {
        content.to_string()
    }
}

fn safe_truncate(content: &str, max_bytes: usize) -> &str {
    if content.len() <= max_bytes {
        return content;
    }
    let mut end = max_bytes;
    while end > 0 && !content.is_char_boundary(end) {
        end -= 1;
    }
    &content[..end]
}

// ═══════════════════════════════════════════════════════════════════════════════
// P1.6: WorkerMode + TeammateIdle idle loop
// ═══════════════════════════════════════════════════════════════════════════════

use crate::runtime::agent::inbox::{AgentInbox, InboxItem, MessageSource};
use crate::runtime::agent::name_registry::AgentNameRegistry;
use crate::runtime::agent::output_writer::{
    append_line, transcript_path_for_kind, write_meta, AgentTranscriptMeta, TranscriptKind,
    TranscriptLine,
};
use crate::runtime::agent::team::Team;
use crate::runtime::cancellation::wait_for_cancellation;
use crate::runtime::ids::{AgentId, SessionId};
use std::path::PathBuf;
use tokio::sync::Mutex;

/// Discriminates the execution mode for a worker run.
///
/// `AsyncOneShot` is the legacy one-shot subagent path.  `TeammateIdle` is the
/// new long-lived Teammate path introduced in P1.6.
pub enum WorkerMode {
    /// Legacy path: async sub-agent, runs to completion and exits.
    AsyncOneShot,
    /// New path: Teammate, stays in idle loop until cancelled or inbox closed.
    TeammateIdle {
        /// Team membership handle — used to update `last_active_at` on
        /// heartbeat and to call `remove_teammate` on cleanup.
        team_handle: Arc<Mutex<Team>>,
        /// Human-readable name of this Teammate in the team (e.g. "researcher").
        agent_name: String,
    },
}

/// Context required by the Teammate idle loop.
///
/// Constructed by the `spawn_subagent` tool and passed into `run_worker`.
/// All fields are `Clone`-able so the idle loop can be `tokio::spawn`ed.
#[derive(Clone)]
pub struct TeammateWorkerCtx {
    /// Unique identity of this Teammate agent.
    pub agent_id: AgentId,
    /// Session owning this Teammate (used to unregister from `AgentNameRegistry`).
    pub session_id: SessionId,
    /// Team this Teammate belongs to — used as the second key in all three
    /// per-team registries (PR4).  Set at spawn time and never changes.
    pub team_name: String,
    /// Conversation id — used for transcript path derivation.
    pub conv_id: String,
    /// Cancellation token; the loop exits when this is triggered.
    pub cancel: CancellationToken,
    /// In-process message inbox.
    pub inbox: Arc<AgentInbox>,
    /// Name registry — used to unregister the Teammate's name on cleanup.
    pub agent_names: Arc<AgentNameRegistry>,
    /// Optional inbox registry — when present, cleanup will deregister this
    /// Teammate's inbox so SendMessage stops resolving it (P2.2).
    pub inbox_registry: Option<Arc<crate::runtime::agent::InboxRegistry>>,
    /// Optional cancellation registry — cleanup deregisters here too (P2.7).
    pub cancellation_registry: Option<Arc<crate::runtime::agent::CancellationRegistry>>,
    /// Conversation root directory for transcript writes.
    /// `None` means "no logging" (test-only or legacy path).
    pub conv_dir: Option<PathBuf>,
    /// Metadata written to the `.meta.json` sidecar once at spawn time.
    pub meta: AgentTranscriptMeta,
    /// Runtime engine dependencies for running LLM iterations on inbox
    /// messages.  `None` for legacy / test paths where the idle loop should
    /// stay in stub mode (no real LLM call).  When `Some`, the idle loop
    /// uses the gateway + tool registry + settings to run a real turn per
    /// inbox message.
    pub llm_engine: Option<TeammateLlmEngine>,
}

/// Bundle of runtime engine dependencies needed to run a Teammate LLM turn.
///
/// Constructed by the infrastructure layer (`llm/tool_executor/spawn_subagent.rs::
/// SpawnSubagentLauncherImpl`) and injected into `TeammateWorkerCtx` at spawn
/// time.  Holding it as `Option<>` lets test paths construct a Teammate
/// without booting the full gateway.
#[derive(Clone)]
pub struct TeammateLlmEngine {
    pub gateway: Arc<LlmGateway>,
    pub tool_registry: Arc<ToolRegistry>,
    pub runtime_deps: SubAgentRuntimeDeps,
    pub settings: AppSettings,
    /// Maximum iterations per inbox-driven turn.  Bounds runaway tool loops
    /// per message; not the lifetime of the Teammate.
    pub max_iterations_per_turn: usize,
    /// LTR registries — required so tools like SendMessage / TaskList /
    /// TaskClaim can resolve teammate names, write to peer inboxes, and
    /// kick the Lead idle supervisor.  Without these, those tools see
    /// `ctx.lead_idle = None` etc. and skip the cross-agent side effects.
    pub team_registry: Option<Arc<crate::runtime::agent::TeamRegistry>>,
    pub agent_names: Option<Arc<crate::runtime::agent::AgentNameRegistry>>,
    pub inbox_registry: Option<Arc<crate::runtime::agent::InboxRegistry>>,
    pub lead_idle: Option<Arc<crate::runtime::agent::LeadIdleSupervisor>>,
    pub cancellation_registry: Option<Arc<crate::runtime::agent::CancellationRegistry>>,
}

/// Error type for `run_worker`.
#[derive(Debug, thiserror::Error)]
pub enum WorkerError {
    #[error("transcript write failed: {0}")]
    TranscriptWrite(#[from] anyhow::Error),
}

/// Top-level entry point for worker execution.
///
/// `initial_prompt` is the task description/prompt provided by the caller at
/// spawn time (e.g. the `prompt` field of `spawn_subagent`).  When present, it
/// is injected into the inbox as an initial `ChatMessage` before the select!
/// loop starts.
pub async fn run_worker(
    mode: WorkerMode,
    ctx: TeammateWorkerCtx,
    initial_prompt: Option<String>,
) -> Result<(), WorkerError> {
    match mode {
        WorkerMode::AsyncOneShot => {
            // Legacy path is handled by SubagentWorkerRuntime; this variant is
            // here for API completeness and future unification.
            log::info!("[TeammateIdle] agent={} WorkerMode::AsyncOneShot — delegating to SubagentWorkerRuntime", ctx.agent_id.as_str());
            Ok(())
        }
        WorkerMode::TeammateIdle {
            team_handle: th,
            agent_name,
        } => run_teammate_idle(ctx, th, agent_name, initial_prompt).await,
    }
}

async fn run_teammate_idle(
    ctx: TeammateWorkerCtx,
    team_handle: Arc<Mutex<Team>>,
    agent_name: String,
    initial_prompt: Option<String>,
) -> Result<(), WorkerError> {
    log::info!(
        "[TeammateIdle] agent={} name={} idle loop started",
        ctx.agent_id.as_str(),
        agent_name
    );

    // Write `.meta.json` sidecar once at entry.
    if let Some(ref conv_dir) = ctx.conv_dir {
        if let Err(e) = write_meta(conv_dir, &ctx.meta) {
            log::warn!(
                "[TeammateIdle] agent={} failed to write transcript sidecar: {}; continuing without it",
                ctx.agent_id.as_str(),
                e
            );
        }
    }

    // P2.3b: inject the team_context <system-reminder> as the FIRST item in
    // the inbox so the LLM sees it before the dispatch prompt.  Only fires
    // when we know where on disk the team / tasks data lives (conv_dir set);
    // without that we can't render absolute paths and skip the attachment.
    if let Some(ref conv_dir) = ctx.conv_dir {
        let team_name_snapshot = {
            let team = team_handle.lock().await;
            team.team_name.clone()
        };
        let attachment = crate::runtime::agent::team_context::render_for_conv_dir(
            &team_name_snapshot,
            &agent_name,
            conv_dir,
        );
        let _ = ctx
            .inbox
            .send(InboxItem::ChatMessage {
                message: crate::runtime::messaging::StructuredMessage::text(attachment),
                source: MessageSource::System,
            })
            .await;
    }

    // If an initial prompt was given, seed the inbox so the first iteration
    // of the select! loop immediately processes it.
    if let Some(prompt) = initial_prompt {
        let _ = ctx
            .inbox
            .send(InboxItem::ChatMessage {
                message: crate::runtime::messaging::StructuredMessage::text(prompt),
                source: MessageSource::Lead,
            })
            .await;
    }

    // Heartbeat interval — 60s in production; tests can override by controlling
    // how many ticks they drive before cancelling.
    let heartbeat_secs = if cfg!(test) { 1 } else { 60 };
    let mut heartbeat = tokio::time::interval(std::time::Duration::from_secs(heartbeat_secs));
    // Skip the first tick that fires immediately.
    heartbeat.tick().await;

    // ── Per-Teammate persistent state ─────────────────────────────────────
    // `messages` accumulates across every inbox-driven turn so the LLM sees
    // the full conversation history each call (cf. claude-code-best's
    // `allMessages` accumulator in inProcessRunner.ts).  It's a local
    // variable — when the idle loop exits, it dies with the worker; future
    // resume-from-transcript paths would rebuild this from JSONL.
    let mut messages: Vec<crate::llm::streaming::ChatMessage> = Vec::new();

    loop {
        tokio::select! {
            biased;

            // ── Cancellation (highest priority) ───────────────────────────
            _ = wait_for_cancellation(ctx.cancel.clone()) => {
                log::info!(
                    "[TeammateIdle] agent={} name={} cancelled — running cleanup",
                    ctx.agent_id.as_str(),
                    agent_name
                );
                cleanup_teammate(&ctx, &team_handle, &agent_name).await;
                return Ok(());
            }

            // ── Inbox message ─────────────────────────────────────────────
            item = ctx.inbox.recv() => {
                match item {
                    Some(InboxItem::ChatMessage { message, source }) => {
                        let mode = if ctx.llm_engine.is_some() { "LLM" } else { "stub" };
                        log::info!(
                            "[TeammateIdle] agent={} name={} received {} from {:?} — running turn ({})",
                            ctx.agent_id.as_str(),
                            agent_name,
                            message.variant_name(),
                            source,
                            mode,
                        );
                        if ctx.llm_engine.is_some() {
                            teammate_real_turn(&ctx, &agent_name, &message, &source, &mut messages).await;
                        } else {
                            // Legacy / test path: no engine wired, fall back
                            // to the placeholder stub (transcript only).
                            teammate_stub_turn(&ctx, &agent_name, &message, &source).await;
                        }
                    }
                    Some(InboxItem::Shutdown(req)) => {
                        // P2.6 NOTE: SendMessage now packs ShutdownRequest as
                        // a ChatMessage variant — this raw Inbox::Shutdown
                        // arm only fires if a future internal caller pushes
                        // it directly (e.g. supervisory cancellation path).
                        // Per v4 §5.3 the Teammate must NOT self-terminate;
                        // it should run a turn and let the LLM produce a
                        // shutdown_response.  For now we just log and stay
                        // idle so a misrouted Shutdown doesn't kill us.
                        log::warn!(
                            "[TeammateIdle] agent={} name={} legacy InboxItem::Shutdown received (reason={}) — ignored, awaiting Lead cancel",
                            ctx.agent_id.as_str(),
                            agent_name,
                            req.reason
                        );
                    }
                    Some(InboxItem::TaskNotification(notif)) => {
                        // P2: forward notification into the LLM turn as a user
                        // message.  For P1 we just log and continue.
                        log::info!(
                            "[TeammateIdle] agent={} name={} TaskNotification received (xml_len={}) — ignored in P1",
                            ctx.agent_id.as_str(),
                            agent_name,
                            notif.xml.len()
                        );
                    }
                    None => {
                        // All senders dropped — inbox closed; exit gracefully.
                        log::info!(
                            "[TeammateIdle] agent={} name={} inbox closed — exiting gracefully",
                            ctx.agent_id.as_str(),
                            agent_name
                        );
                        cleanup_teammate(&ctx, &team_handle, &agent_name).await;
                        return Ok(());
                    }
                }
            }

            // ── Heartbeat ─────────────────────────────────────────────────
            _ = heartbeat.tick() => {
                log::debug!(
                    "[TeammateIdle] agent={} name={} heartbeat — updating last_active_at",
                    ctx.agent_id.as_str(),
                    agent_name
                );
                let mut team = team_handle.lock().await;
                if let Some(m) = team
                    .teammates
                    .iter_mut()
                    .find(|m| m.name == agent_name)
                {
                    m.last_active_at = chrono::Utc::now();
                }
            }
        }
    }
}

/// Render a `StructuredMessage` into a plain-text user-message body suitable
/// for either transcript or LLM consumption.  Shared by stub and real turn.
fn render_inbox_message_as_user_text(
    message: &crate::runtime::messaging::StructuredMessage,
) -> String {
    use crate::runtime::messaging::StructuredMessage as M;
    match message {
        M::Text { content } => content.clone(),
        M::ShutdownRequest { reason } => format!(
            "<shutdown-request reason=\"{}\">请用 SendMessage shutdown_response 回应（approve=true 表示已收尾，approve=false 并附 reason 表示需保留）。</shutdown-request>",
            reason.as_deref().unwrap_or("")
        ),
        M::PlanApprovalRequest { request_id, plan } => format!(
            "<plan-approval-request id=\"{}\">\n  <plan>{}</plan>\n  <instructions>请用 SendMessage plan_approval_response (相同 request_id) 表态：approve=true 通过，approve=false 并附 feedback 拒绝。</instructions>\n</plan-approval-request>",
            request_id, plan
        ),
        M::PlanApprovalResponse { request_id, approve, feedback } => format!(
            "<plan-approval-response id=\"{}\" approve=\"{}\">\n  <feedback>{}</feedback>\n</plan-approval-response>",
            request_id,
            approve,
            feedback.as_deref().unwrap_or("")
        ),
        _ => serde_json::to_string(message)
            .unwrap_or_else(|_| message.variant_name().to_string()),
    }
}

/// Map a [`MessageSource`] to a stable `from` string suitable for
/// teammate transcript user rows.  `team-lead` is the canonical Lead name
/// (matches `LEAD_NAME` in `team_tools.rs` and the values written to
/// `team-chat.jsonl`), so the frontend can identify Lead-vs-peer messages
/// without parsing free-form text.
fn from_label_for_source(source: &MessageSource) -> String {
    match source {
        MessageSource::Lead => "team-lead".to_string(),
        MessageSource::Teammate(name) => name.clone(),
        MessageSource::System => "system".to_string(),
    }
}

/// P1 stub: record a user message + placeholder assistant reply in the
/// transcript JSONL.  Real LLM call is in [`teammate_real_turn`]; this stub
/// is kept as a fallback for legacy / test paths where `TeammateLlmEngine`
/// is not wired into `TeammateWorkerCtx`.
///
/// Non-`Text` variants (shutdown_request etc.) are serialized as JSON for the
/// transcript so the structure is preserved.
async fn teammate_stub_turn(
    ctx: &TeammateWorkerCtx,
    agent_name: &str,
    message: &crate::runtime::messaging::StructuredMessage,
    source: &MessageSource,
) {
    if let Some(ref conv_dir) = ctx.conv_dir {
        let jl_path = transcript_path_for_kind(
            conv_dir,
            &TranscriptKind::Teammate,
            &ctx.team_name,
            ctx.agent_id.as_str(),
        );

        let user_text = render_inbox_message_as_user_text(message);

        let user_line = TranscriptLine::user_from(user_text.clone(), from_label_for_source(source));
        let _ = append_line(&jl_path, &user_line);

        // P2.6 stub reply: explicitly NOT a self-shutdown.  Real LLM wiring
        // (in `teammate_real_turn`) replaces this with actual model output
        // + tool calls; this fallback only fires when the engine isn't
        // injected (tests / legacy paths).
        let reply = TranscriptLine::assistant(format!(
            "[stub fallback] {} received: {}",
            agent_name, user_text
        ));
        let _ = append_line(&jl_path, &reply);
    }
}

/// LTR-P2 real Teammate turn: runs a full agentic iteration loop against the
/// injected `TeammateLlmEngine`.  Mirrors `SubagentWorkerRuntime::run_worker_turn`
/// in shape but trimmed for Teammate semantics:
///   - no front-end `tool_executing` / `tool_completed` event emission (a
///     Teammate is async and invisible to the chat UI; the Lead sees results
///     via SendMessage)
///   - no `terminal_tool_results` / `generated_files` collection (Teammate
///     never returns a result envelope to a parent — it reports via
///     SendMessage to the Lead)
///   - `messages` is borrowed from the caller (the idle loop) so history
///     persists across inbox-driven turns
///   - each appended message is mirrored to the transcript JSONL as it
///     happens, so a crashed / cancelled Teammate never loses partial work
async fn teammate_real_turn(
    ctx: &TeammateWorkerCtx,
    agent_name: &str,
    message: &crate::runtime::messaging::StructuredMessage,
    source: &MessageSource,
    messages: &mut Vec<crate::llm::streaming::ChatMessage>,
) {
    use crate::llm::masking::MaskingLevel;
    use crate::llm::streaming::{ChatMessage, StopReason, StreamEvent};

    let Some(engine) = ctx.llm_engine.as_ref() else {
        // Should be unreachable — caller (run_teammate_idle) already
        // checked ctx.llm_engine.is_some().  Defensive fall-through.
        teammate_stub_turn(ctx, agent_name, message, source).await;
        return;
    };

    let jl_path = ctx.conv_dir.as_ref().map(|conv_dir| {
        transcript_path_for_kind(
            conv_dir,
            &TranscriptKind::Teammate,
            &ctx.team_name,
            ctx.agent_id.as_str(),
        )
    });

    // System prompt for this Teammate.  Constructed once at spawn time
    // (Employee prompt + TEAMMATE_ADDENDUM) and frozen on `meta` so every
    // turn sees the same persona.
    let system_prompt = ctx.meta.boot_system_prompt.clone().unwrap_or_default();

    // Tool definitions filtered by the per-Teammate whitelist.  Resolved
    // freshly each turn so newly-loaded tools (e.g. MCP servers connected
    // after spawn) are picked up.
    let all_schemas = engine.tool_registry.get_all_schemas().await;
    let final_allowed = crate::runtime::agent::tool_whitelist::resolve_agent_tools_ex(
        &ctx.meta.tool_whitelist,
        &[], // no per-Teammate disallow list yet
        &all_schemas
            .iter()
            .map(|s| s.name.clone())
            .collect::<Vec<_>>(),
        true,  // is_async = true (Teammate runs in background)
        false, // allow_recursive_spawn = false
        true,  // is_teammate = true → injects TEAMMATE_TOOLS (SendMessage / TaskList / ...)
    );
    let tool_defs: Vec<crate::llm::streaming::ToolDefinition> = all_schemas
        .into_iter()
        .filter(|schema| final_allowed.contains(&schema.name))
        .collect();

    // 1. Render inbox message → user ChatMessage, append to in-memory
    //    history AND mirror to transcript.
    let user_text = render_inbox_message_as_user_text(message);
    let user_msg = ChatMessage::text("user", user_text.clone());
    messages.push(user_msg.clone());
    if let Some(ref path) = jl_path {
        let _ = append_line(
            path,
            &TranscriptLine::user_from(user_text, from_label_for_source(source)),
        );
    }

    // 2. Per-turn cancellation token (child of the worker's lifecycle
    //    token so a TeammateStop kills the in-flight LLM call too).
    let turn_cancel = ctx.cancel.child_token();
    let sub_conv_id = format!(
        "teammate-{}-{}",
        ctx.agent_id.as_str(),
        uuid::Uuid::new_v4()
    );

    // 3. Build a per-turn TurnState + tool_event_bus + QueryEngine so
    //    ToolRoundDriver has everything it needs.  Reuses the
    //    SubagentWorkerRuntime pattern verbatim.
    let child_read_file_state = engine
        .runtime_deps
        .read_file_state
        .as_ref()
        .map(|cache| cache.clone_for_child())
        .unwrap_or_else(|| Arc::new(FileStateCache::new()));
    log::info!(
        "[teammate_real_turn][diag] entering turn: ctx.agent_id={} session={} (will be passed to request_scoped_tool_deps)",
        ctx.agent_id.as_str(),
        ctx.session_id.as_str()
    );
    let request_scoped = engine.runtime_deps.request_scoped_tool_deps(
        crate::runtime::ids::RunId::new(format!("teammate-turn-{}", uuid::Uuid::new_v4())),
        Some(ctx.agent_id.clone()),
        Some(turn_cancel.clone()),
        Some(child_read_file_state.clone()),
    );
    let dispatcher = engine
        .tool_registry
        .to_runtime_dispatcher(request_scoped)
        .await;

    let permission_ask = crate::runtime::tools::permission::default_permission_ask();
    let _ = permission_ask; // reserved for future explicit injection; QueryEngine uses default ask internally
                            // Build QueryEngine mirroring SubagentWorkerRuntime::build_query_engine
                            // but inlined here since this is a free function (not a method).
    let (python_binary, python_home) = engine
        .runtime_deps
        .runtime_resolver
        .as_ref()
        .and_then(|resolver| {
            resolver
                .workspace_dependencies()
                .ok()
                .map(|deps| (deps.python, None))
        })
        .unwrap_or_else(|| {
            log::warn!(
                "[TeammateIdle] agent={} managed runtime resolver unavailable; using inert Python path",
                ctx.agent_id.as_str()
            );
            (std::path::PathBuf::from("__managed_runtime_resolver_missing__"), None)
        });
    let file_ops = Arc::new(DefaultFileOperations {
        storage: engine.runtime_deps.storage.clone(),
        file_manager: engine.runtime_deps.file_manager.clone(),
        workspace_path: engine.runtime_deps.workspace_path.clone(),
        conversation_id: engine.runtime_deps.conversation_id.clone(),
        run_id: Some(crate::runtime::ids::RunId::new(format!(
            "teammate-{}",
            ctx.agent_id.as_str()
        ))),
        python_binary: Some(python_binary),
        python_home,
    });
    let dispatcher_arc: Arc<crate::runtime::tools::ToolDispatcher> = dispatcher;
    let mut query_engine = QueryEngine::with_dispatcher(dispatcher_arc)
        .with_workspace_path(engine.runtime_deps.workspace_path.clone())
        .with_authorized_workspace(engine.runtime_deps.authorized_workspace.clone())
        .with_file_ops(file_ops)
        .with_runtime_resolver(engine.runtime_deps.runtime_resolver.clone())
        .with_read_file_state(child_read_file_state.clone());
    // Inject the conv_dir so teammate-originated tool calls (notably
    // SendMessage) carry it via ToolExecutionContext.conv_dir. Without
    // this, team-chat.jsonl never records teammate → Lead / teammate →
    // teammate messages and the team view stays one-sided.
    if let Some(dir) = ctx.conv_dir.clone() {
        query_engine = query_engine.with_conv_dir(dir);
    }
    // LTR: attach Team / AgentName / Inbox / LeadIdle / Cancellation registries
    // so cross-agent tools (SendMessage, TaskList, TaskClaim, ...) can:
    //   - resolve teammate names to AgentId (agent_names)
    //   - write to peer inboxes (inbox_registry)
    //   - kick the Lead idle supervisor (lead_idle) so SendMessage(to="team-lead")
    //     actually wakes the Lead instead of silently dropping
    //   - register their own cancellation token so TaskStop can find them
    if let (Some(team), Some(names), Some(inboxes)) = (
        engine.team_registry.clone(),
        engine.agent_names.clone(),
        engine.inbox_registry.clone(),
    ) {
        query_engine = query_engine.with_ltr_registries(team, names, inboxes);
    } else {
        log::warn!(
            "[TeammateIdle] agent={} name={} LTR registries missing (team={} names={} inboxes={}); cross-agent tools will be no-ops",
            ctx.agent_id.as_str(),
            agent_name,
            engine.team_registry.is_some(),
            engine.agent_names.is_some(),
            engine.inbox_registry.is_some(),
        );
    }
    if let Some(sup) = engine.lead_idle.clone() {
        query_engine = query_engine.with_lead_idle(sup);
    }
    if let Some(creg) = engine.cancellation_registry.clone() {
        query_engine = query_engine.with_cancellation_registry(creg);
    }
    // LTR: build the Teammate's effective permission ctx by merging parent's
    // permission_ctx (if any) with the teammate-specific working dirs (its
    // conversation dir for team.json / tasks/, plus skill dirs).  Without
    // this the LLM gets a `Decision::Ask` for paths inside its own working
    // area; combined with `is_async = true` that becomes Deny (no UI to
    // prompt), and the model can't read team.json.
    let teammate_pctx = build_teammate_permission_ctx(
        engine.runtime_deps.permission_ctx.as_deref(),
        ctx.conv_dir.as_deref(),
        &ctx.team_name,
        engine.runtime_deps.workspace_path.as_path(),
    );
    log::info!(
        "[TeammateIdle][permission-trace] agent={} name={} pctx.dirs={:?} pctx.allow={} pctx.deny={}",
        ctx.agent_id.as_str(),
        agent_name,
        teammate_pctx.additional_working_dirs.keys().collect::<Vec<_>>(),
        teammate_pctx.allow_rules.len(),
        teammate_pctx.deny_rules.len(),
    );
    query_engine = query_engine.with_permission_ctx(Arc::new(teammate_pctx));
    let tool_event_bus = RuntimeEventBus::new();

    let mut turn = TurnState::new(
        IdentityMapping::from_legacy_conversation_id(ctx.conv_id.clone()),
        crate::runtime::ids::RunId::new(format!("teammate-{}", ctx.agent_id.as_str())),
        user_text_for_turn_state(messages),
    )
    .with_cancellation(turn_cancel.clone())
    .with_permission_mode(crate::runtime::tools::permission::PermissionMode::Default)
    // LTR P2.8: mark this turn as async so every tool's permission Ask
    // gets auto-denied instead of blocking the idle loop forever.
    .with_async(true);
    // CRITICAL: stamp the teammate's agent_id onto the turn so every
    // ToolExecutionContext built from it carries the caller identity.
    // Without this, SendMessage can't reverse-lookup the caller name via
    // AgentNameRegistry::name_for, and `<peer-message from="...">` falls
    // back to `from="system"` — losing who actually sent the message.
    turn.set_agent_id(ctx.agent_id.clone());

    log::info!(
        "[TeammateIdle][permission-trace] agent={} name={} turn.is_async=true tool_count={} allowed={:?}",
        ctx.agent_id.as_str(),
        agent_name,
        tool_defs.len(),
        final_allowed,
    );

    // 4. Iteration loop — mirrors SubagentWorkerRuntime::run_worker_turn
    //    287-617 but trimmed for Teammate.
    let max_iterations = engine.max_iterations_per_turn;
    let mut cancelled = false;
    for iteration in 0..max_iterations {
        if turn_cancel.is_cancelled() {
            cancelled = true;
            break;
        }

        log::info!(
            "[TeammateIdle] agent={} name={} iter={}/{} messages={}",
            ctx.agent_id.as_str(),
            agent_name,
            iteration,
            max_iterations,
            messages.len()
        );

        let max_tokens =
            crate::llm::max_tokens::default_max_tokens_for_model(&engine.settings.primary_model);
        let system_msg_opt = if system_prompt.is_empty() {
            None
        } else {
            Some(system_prompt.as_str())
        };

        let stream_result = engine
            .gateway
            .stream_message(
                &engine.settings,
                messages.clone(),
                MaskingLevel::Relaxed,
                system_msg_opt,
                None,
                Some(tool_defs.clone()),
                max_tokens,
                Some(&sub_conv_id),
                None,
            )
            .await;

        let (_task_id, mut stream, _mask_ctx, _cancel_rx) = match stream_result {
            Ok(result) => result,
            Err(err) => {
                warn!(
                    "[TeammateIdle] agent={} LLM call failed at iter {}: {}",
                    ctx.agent_id.as_str(),
                    iteration,
                    err
                );
                if let Some(ref path) = jl_path {
                    let _ = append_line(path, &TranscriptLine::failed(format!("LLM error: {err}")));
                }
                break;
            }
        };

        let mut iter_content = String::new();
        let mut tool_calls: Vec<crate::llm::streaming::ToolCall> = Vec::new();
        let mut stop_reason = StopReason::EndTurn;

        while let Some(event) = stream.next().await {
            if turn_cancel.is_cancelled() {
                engine.gateway.cancel_conversation(&sub_conv_id).ok();
                cancelled = true;
                break;
            }
            match event {
                StreamEvent::ContentDelta { delta } => iter_content.push_str(&delta),
                StreamEvent::ToolCallStart { tool_call } => match tool_call.into_valid() {
                    Ok(tool_call) => tool_calls.push(tool_call),
                    Err(err) => {
                        warn!(
                            "[TeammateIdle] agent={} dropping invalid stream tool_call: {}",
                            ctx.agent_id.as_str(),
                            err
                        );
                        break;
                    }
                },
                StreamEvent::Done {
                    stop_reason: sr, ..
                } => {
                    stop_reason = sr;
                    break;
                }
                StreamEvent::Error { error } => {
                    warn!(
                        "[TeammateIdle] agent={} stream error: {}",
                        ctx.agent_id.as_str(),
                        error
                    );
                    if let Some(ref path) = jl_path {
                        let _ = append_line(
                            path,
                            &TranscriptLine::failed(format!("Stream error: {error}")),
                        );
                    }
                    break;
                }
                _ => {}
            }
        }

        if cancelled {
            break;
        }

        // EndTurn (no more tool calls) — push final assistant text and exit
        if stop_reason != StopReason::ToolUse || tool_calls.is_empty() {
            if !iter_content.is_empty() {
                let assistant = ChatMessage::text("assistant", iter_content.clone());
                messages.push(assistant.clone());
                if let Some(ref path) = jl_path {
                    let _ = append_line(path, &TranscriptLine::from_chat_message(&assistant));
                }
            }
            break;
        }

        // ToolUse — push assistant w/ tool_calls, execute round, push tool_results
        let assistant_with_calls = ChatMessage::assistant_with_tool_calls(
            iter_content.clone(),
            tool_calls
                .iter()
                .map(|tc| crate::llm::streaming::ToolCall {
                    id: tc.id.clone(),
                    name: tc.name.clone(),
                    arguments: tc.arguments.clone(),
                })
                .collect(),
            None,
            None,
        );
        messages.push(assistant_with_calls.clone());
        if let Some(ref path) = jl_path {
            let _ = append_line(
                path,
                &TranscriptLine::from_chat_message(&assistant_with_calls),
            );
        }

        let runtime_tool_calls: Vec<RuntimeToolCallRequest> = tool_calls
            .into_iter()
            .filter_map(|tc| {
                let purpose = Some(format!("[Teammate {}] {}", agent_name, tc.name));
                match RuntimeToolCallRequest::from_tool_call(tc, purpose) {
                    Ok(call) => Some(call),
                    Err(err) => {
                        warn!(
                            "[TeammateIdle] agent={} dropping invalid runtime tool_call: {}",
                            ctx.agent_id.as_str(),
                            err
                        );
                        None
                    }
                }
            })
            .collect();

        let round_driver =
            ToolRoundDriver::new(query_engine.clone()).with_allowed_tools(final_allowed.clone());
        let round_results = round_driver
            .execute_round(&turn, &tool_event_bus, runtime_tool_calls)
            .await;

        // Dedup tool_results by tool_call_id (Anthropic rejects duplicates).
        let mut pushed_ids: std::collections::HashSet<String> = std::collections::HashSet::new();
        for round_result in round_results {
            let (tcid, tname, content_str, is_error, ask_required) = match round_result {
                ToolRoundResult::Blocked(blocked) => (
                    blocked.tool_call_id,
                    blocked.tool_name,
                    blocked.reason,
                    true,
                    false,
                ),
                ToolRoundResult::Ok(RuntimeToolCallOutcome::Completed {
                    tool_call_id,
                    tool_name,
                    content,
                    is_error,
                    max_result_size_chars,
                    ..
                }) => (
                    tool_call_id,
                    tool_name,
                    truncate_tool_content(&content, max_result_size_chars),
                    is_error,
                    false,
                ),
                ToolRoundResult::Ok(RuntimeToolCallOutcome::AskRequired {
                    tool_call_id,
                    tool_name,
                    ..
                }) => (
                    tool_call_id,
                    tool_name,
                    "Permission Ask required (Teammate is async — request auto-denied)".to_string(),
                    true,
                    true,
                ),
                ToolRoundResult::Ok(RuntimeToolCallOutcome::InteractionRequired {
                    tool_call_id,
                    tool_name,
                    ..
                }) => (
                    tool_call_id,
                    tool_name,
                    "User interaction required; Teammate cannot ask the user directly.".to_string(),
                    true,
                    false,
                ),
            };

            if !pushed_ids.insert(tcid.clone()) {
                log::warn!(
                    "[TeammateIdle] agent={} dropped duplicate tool_result id={} name={}",
                    ctx.agent_id.as_str(),
                    tcid,
                    tname
                );
                continue;
            }

            let tool_result_msg =
                ChatMessage::tool_result_with_status(&tcid, &tname, content_str.clone(), is_error);
            messages.push(tool_result_msg.clone());
            if let Some(ref path) = jl_path {
                let _ = append_line(path, &TranscriptLine::from_chat_message(&tool_result_msg));
            }

            if ask_required {
                // P2.8: Teammate is async, Ask auto-denies and we bubble the
                // result back to the model so it can react.  We don't break
                // the turn (model may pivot to a different tool).
                log::warn!(
                    "[TeammateIdle] agent={} tool {} bubbled AskRequired -> auto-denied to LLM",
                    ctx.agent_id.as_str(),
                    tname
                );
            }
        }

        // Loop continues to next iteration so LLM sees tool_results.
        let _ = iteration;
    }

    if cancelled {
        log::info!(
            "[TeammateIdle] agent={} name={} turn cancelled mid-stream",
            ctx.agent_id.as_str(),
            agent_name
        );
        if let Some(ref path) = jl_path {
            let _ = append_line(path, &TranscriptLine::failed("turn cancelled".to_string()));
        }
    }

    engine.gateway.clear_task(&sub_conv_id);
}

/// Helper: extract a representative user-input string for `TurnState`
/// initialization.  Uses the most recent user message in the history, or
/// empty when none exists yet.
fn user_text_for_turn_state(messages: &[crate::llm::streaming::ChatMessage]) -> String {
    messages
        .iter()
        .rev()
        .find(|m| m.role == "user")
        .map(|m| m.content.clone())
        .unwrap_or_default()
}

/// Build the effective `ToolPermissionContext` for a Teammate turn.
///
/// Starts from the parent's `permission_ctx` (or empty) and adds a set of
/// "Teammate-default" working dirs so the LLM can read its own team config,
/// task list, transcript, and skill directories without triggering a
/// permission `Ask` (which would auto-deny under `is_async = true`).
///
/// The dirs we add:
///   1. `teams/{team_name}/` — **PR5 收窄**：以前是整个 conv_dir，现在只放行该 team 自己
///      的目录（含 config.json / team-chat.jsonl / tasks/ / teammates/）。team-A 的
///      Teammate 无法读写 team-B 的数据。conv 根目录（messages.jsonl 等）也不再放行。
///   2. `workspace_path` — 父工作区，跨 team 共享读写。
///   3. `~/.renlijia/skills/` — 全局 skill bundle（managed + user-installed）。
///
/// 当 `team_name` 为空字符串（Lead 单飞场景兜底），仍按旧行为放行整个 conv_dir。
/// 实际上 PR5 之后 Teammate 必然有 team_name；空 fallback 仅保留给历史调用方。
fn build_teammate_permission_ctx(
    parent: Option<&crate::runtime::path_auth::ToolPermissionContext>,
    conv_dir: Option<&std::path::Path>,
    team_name: &str,
    workspace_path: &std::path::Path,
) -> crate::runtime::path_auth::ToolPermissionContext {
    use crate::runtime::path_auth::{RuleSource, ToolPermissionContext};

    let mut ctx = parent.cloned().unwrap_or_else(ToolPermissionContext::empty);

    // 1. team 子目录（或 conv_dir 兜底）
    if let Some(dir) = conv_dir {
        let team_root = if !team_name.is_empty() {
            dir.join("teams").join(team_name)
        } else {
            dir.to_path_buf()
        };
        ctx.additional_working_dirs
            .entry(team_root)
            .or_insert(RuleSource::Session);
    }

    // 2. Parent workspace.
    ctx.additional_working_dirs
        .entry(workspace_path.to_path_buf())
        .or_insert(RuleSource::Session);

    // 3. Global skill directory (managed skills + user-installed skills).
    if let Some(home) = dirs::home_dir() {
        let global_skills = home.join(".renlijia").join("skills");
        ctx.additional_working_dirs
            .entry(global_skills)
            .or_insert(RuleSource::Session);
    }

    ctx
}

/// Cleanup performed when the idle loop exits (cancellation, shutdown, inbox
/// closed).  Removes the Teammate from `Team` and unregisters its name from
/// `AgentNameRegistry`.
async fn cleanup_teammate(ctx: &TeammateWorkerCtx, team_handle: &Arc<Mutex<Team>>, name: &str) {
    // 1. Remove from Team roster.
    {
        let mut team = team_handle.lock().await;
        team.remove_teammate(name);
    }
    // 2. Unregister from AgentNameRegistry so the name can be reused.
    ctx.agent_names
        .unregister(&ctx.session_id, &ctx.team_name, name)
        .await;
    // 3. Deregister from InboxRegistry so SendMessage stops resolving this
    //    Teammate (P2.2).  Skipped if no registry was injected.
    if let Some(reg) = ctx.inbox_registry.as_ref() {
        reg.unregister(&ctx.session_id, &ctx.team_name, &ctx.agent_id)
            .await;
    }
    // 4. Deregister from CancellationRegistry (P2.7).
    if let Some(reg) = ctx.cancellation_registry.as_ref() {
        reg.unregister(&ctx.session_id, &ctx.team_name, &ctx.agent_id)
            .await;
    }

    log::info!(
        "[TeammateIdle] agent={} name={} cleanup complete",
        ctx.agent_id.as_str(),
        name
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::tools::permission::PermissionMode;

    #[test]
    fn build_run_config_preserves_permission_mode_from_subagent_config() {
        let config = SubAgentConfig {
            task: "collect data".to_string(),
            system_prompt: "system".to_string(),
            allowed_tools: vec!["Read".to_string()],
            max_iterations: 3,
            dynamic_context: String::new(),
            conversation_id: "conv-worker-mode".to_string(),
            parent_run_id: Some(RunId::new("run-parent-worker-mode")),
            background: false,
            app_handle: None,
            cancel_token: None,
            permission_mode: PermissionMode::Plan,
            model_override: None,
            agent_name: None,
            parent_tool_use_id: None,
            disallowed_tools: vec![],
        };

        let final_allowed = vec!["Read".to_string()];
        let run_config =
            SubagentWorkerRuntime::build_run_config_with_allowed(&config, final_allowed);

        assert_eq!(run_config.permission_mode, PermissionMode::Plan);
    }

    #[test]
    fn final_whitelist_is_used_in_run_config_and_turn_request() {
        fn tool_schema(name: &str) -> ToolDefinition {
            ToolDefinition {
                name: name.to_string(),
                description: format!("{name} schema"),
                parameters: serde_json::json!({"type": "object"}),
            }
        }

        let config = SubAgentConfig {
            task: "collect data".to_string(),
            system_prompt: "system".to_string(),
            allowed_tools: vec![],
            max_iterations: 3,
            dynamic_context: String::new(),
            conversation_id: "conv-worker-whitelist".to_string(),
            parent_run_id: Some(RunId::new("run-parent-worker-whitelist")),
            background: false,
            app_handle: None,
            cancel_token: None,
            permission_mode: PermissionMode::Default,
            model_override: None,
            agent_name: None,
            parent_tool_use_id: None,
            disallowed_tools: vec![],
        };
        let all_schemas = vec![tool_schema("Read"), tool_schema("Agent")];
        let available_names: Vec<String> = all_schemas
            .iter()
            .map(|schema| schema.name.clone())
            .collect();
        let final_allowed = crate::runtime::agent::tool_whitelist::resolve_agent_tools(
            &config.allowed_tools,
            &config.disallowed_tools,
            &available_names,
            config.background,
            false,
        );

        let turn_request = SubagentWorkerRuntime::build_turn_request_with_allowed(
            &config,
            all_schemas,
            &final_allowed,
        );
        let run_config =
            SubagentWorkerRuntime::build_run_config_with_allowed(&config, final_allowed.clone());

        assert!(final_allowed.contains(&"Read".to_string()));
        assert!(!final_allowed.contains(&"Agent".to_string()));
        assert!(run_config.allowed_tools.contains(&"Read".to_string()));
        assert!(!run_config.allowed_tools.contains(&"Agent".to_string()));
        let tool_def_names: Vec<&str> = turn_request
            .tool_defs
            .iter()
            .map(|tool_def| tool_def.name.as_str())
            .collect();
        assert!(tool_def_names.contains(&"Read"));
        assert!(!tool_def_names.contains(&"Agent"));
    }

    #[test]
    fn worker_system_prompt_for_gateway_prefers_system_message_without_mutating_messages() {
        let request = WorkerTurnRequest {
            subagent_conversation_id: "sub-conv".to_string(),
            messages: vec![ChatMessage::text("user", "task")],
            tool_defs: Vec::new(),
            system_prompt: "fallback prompt".to_string(),
            system_message: Some(ChatMessage::text("system", "openai prompt")),
            dynamic_context: None,
            max_iterations: 1,
        };

        assert_eq!(
            worker_system_prompt_for_gateway(&request),
            Some("openai prompt")
        );
        assert!(request
            .messages
            .iter()
            .all(|message| message.role != "system"));
    }

    #[test]
    fn worker_system_prompt_for_gateway_falls_back_to_legacy_system_prompt() {
        let request = WorkerTurnRequest {
            subagent_conversation_id: "sub-conv".to_string(),
            messages: vec![ChatMessage::text("user", "task")],
            tool_defs: Vec::new(),
            system_prompt: "fallback prompt".to_string(),
            system_message: None,
            dynamic_context: None,
            max_iterations: 1,
        };

        assert_eq!(
            worker_system_prompt_for_gateway(&request),
            Some("fallback prompt")
        );
        assert!(request
            .messages
            .iter()
            .all(|message| message.role != "system"));
    }

    #[test]
    fn status_aware_tool_result_marks_error() {
        let message =
            ChatMessage::tool_result_with_status("call_1", "Bash", "failed".to_string(), true);

        assert!(message.is_error);
        assert_eq!(message.role, "tool");
    }

    #[test]
    fn default_tool_result_remains_success() {
        let message = ChatMessage::tool_result("call_1", "Bash", "ok".to_string());

        assert!(!message.is_error);
    }
}
