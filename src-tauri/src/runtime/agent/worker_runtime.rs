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
    pub openai_system_message: Option<ChatMessage>,
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
            openai_system_message: Some(ChatMessage::text("system", config.system_prompt.clone())),
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
        let workspace = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
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

            let stream_result = self
                .gateway
                .stream_message(
                    &effective_settings,
                    request.messages.clone(),
                    MaskingLevel::Relaxed,
                    worker_system_prompt_for_gateway(&request),
                    request.dynamic_context.as_deref(),
                    Some(request.tool_defs.clone()),
                    4096,
                    Some(&sub_conv_id),
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
                    StreamEvent::ToolCallStart { tool_call } => {
                        tool_calls.push(tool_call);
                    }
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
                        break;
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
                output = iter_content.clone();
                if !iter_content.is_empty() {
                    request
                        .messages
                        .push(ChatMessage::text("assistant", iter_content));
                }
                break;
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
                .map(|tool_call| RuntimeToolCallRequest {
                    tool_call_id: tool_call.id,
                    tool_name: tool_call.name.clone(),
                    args: tool_call.arguments,
                    purpose: Some(format!("[Browser Agent] {}", tool_call.name)),
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
                        request.messages.push(ChatMessage::tool_result(
                            &blocked.tool_call_id,
                            &blocked.tool_name,
                            blocked.reason,
                        ));
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
                        request.messages.push(ChatMessage::tool_result(
                            &tool_call_id,
                            &tool_name,
                            content_for_message,
                        ));
                        if let Some(modifier) = context_modifier_message
                            .as_ref()
                            .and_then(context_modifier_to_message)
                        {
                            request.messages.push(modifier);
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
                        request.messages.push(ChatMessage::tool_result(
                            &tool_call_id,
                            &tool_name,
                            "Permission Ask required".to_string(),
                        ));
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
                        request.messages.push(ChatMessage::tool_result(
                            &tool_call_id,
                            &tool_name,
                            "User interaction required; sub-agents cannot ask the user directly."
                                .to_string(),
                        ));
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
            .with_browser_available(self.runtime_deps.connector_engine.is_some())
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
            conversation_id, tool_name, tool_call_id, parent_tool_use_id
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
            conversation_id, tool_call_id, success, parent_tool_use_id
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
        .openai_system_message
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
        let all_schemas = vec![
            tool_schema("Read"),
            tool_schema("Agent"),
        ];
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
        assert!(run_config
            .allowed_tools
            .contains(&"Read".to_string()));
        assert!(!run_config
            .allowed_tools
            .contains(&"Agent".to_string()));
        let tool_def_names: Vec<&str> = turn_request
            .tool_defs
            .iter()
            .map(|tool_def| tool_def.name.as_str())
            .collect();
        assert!(tool_def_names.contains(&"Read"));
        assert!(!tool_def_names.contains(&"Agent"));
    }

    #[test]
    fn worker_system_prompt_for_gateway_prefers_openai_system_message_without_mutating_messages() {
        let request = WorkerTurnRequest {
            subagent_conversation_id: "sub-conv".to_string(),
            messages: vec![ChatMessage::text("user", "task")],
            tool_defs: Vec::new(),
            system_prompt: "fallback prompt".to_string(),
            openai_system_message: Some(ChatMessage::text("system", "openai prompt")),
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
            openai_system_message: None,
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
}
