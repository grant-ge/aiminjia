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

use crate::llm::sub_agent::{SubAgentConfig, SubAgentResult, SubAgentRuntimeDeps};

/// 单次 worker turn 所需的 LLM 请求快照。
pub struct WorkerTurnRequest {
    pub subagent_conversation_id: String,
    pub messages: Vec<ChatMessage>,
    pub tool_defs: Vec<ToolDefinition>,
    pub system_prompt: String,
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
    /// 权限模式，从父 run 传入。Default 时行为不变。
    pub permission_mode: PermissionMode,
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
        let turn_request = self.build_turn_request(&config).await;
        let run_config = WorkerRunConfig {
            allowed_tools: config.allowed_tools.clone(),
            conversation_id: config.conversation_id,
            parent_run_id: config.parent_run_id,
            background: config.background,
            app_handle: config.app_handle,
            cancel_token: config.cancel_token,
            permission_mode: config.permission_mode,
        };
        self.run_worker_turn(turn_request, run_config).await
    }

    async fn build_turn_request(&self, config: &SubAgentConfig) -> WorkerTurnRequest {
        let all_schemas = self.tool_registry.get_all_schemas().await;
        let tool_defs: Vec<ToolDefinition> = all_schemas
            .into_iter()
            .filter(|schema| config.allowed_tools.contains(&schema.name))
            .collect();

        info!("[SubAgent] Tool schemas loaded: {}", tool_defs.len());

        WorkerTurnRequest {
            subagent_conversation_id: String::new(),
            messages: vec![ChatMessage::text("user", &config.task)],
            tool_defs,
            system_prompt: config.system_prompt.clone(),
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

        let child_cancel = if config.background {
            CancellationToken::new()
        } else {
            config
                .cancel_token
                .as_ref()
                .map(|parent| parent.child_token())
                .unwrap_or_else(CancellationToken::new)
        };
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
        let round_driver =
            ToolRoundDriver::new(query_engine).with_allowed_tools(config.allowed_tools.clone());
        let tool_event_bus = RuntimeEventBus::new();

        let mut turn = TurnState::new(
            IdentityMapping::from_legacy_conversation_id(self.runtime_deps.conversation_id.clone()),
            child_run_id.clone(),
            request
                .messages
                .first()
                .map(|message| message.content.clone())
                .unwrap_or_default(),
        )
        .with_cancellation(child_cancel.clone());
        if let Some(agent_id) = child_agent_id.clone() {
            turn.set_agent_id(agent_id);
        }

        let mut output = String::new();
        let mut files: Vec<String> = Vec::new();
        let mut iterations_used = 0;
        let mut pending_ask: Option<PermissionDecision> = None;
        let mut terminal_tool_results: Vec<SubAgentTerminalToolResult> = Vec::new();
        let mut cancelled = false;
        let mut failed = false;

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
                    self.settings,
                    request.messages.clone(),
                    MaskingLevel::Relaxed,
                    Some(request.system_prompt.as_str()),
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
                    failed = true;
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
                );
            }

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
                }
            }
        }

        if iterations_used >= request.max_iterations
            && output.is_empty()
            && pending_ask.is_none()
            && !cancelled
        {
            output = "Sub-agent reached iteration limit.".to_string();
            failed = true;
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
        self.runtime_deps
            .session_manager
            .destroy_run(&child_run_id)
            .await;

        if let Some(handle) = child_handle.as_ref() {
            if cancelled {
                let _ = agent_runtime.cancel_run(child_run_id.clone()).await;
            } else if failed {
                if handle.invocation().background {
                    if let (Some(bus), Some(parent_run_id)) = (
                        self.runtime_deps.event_bus.clone(),
                        config.parent_run_id.clone(),
                    ) {
                        let _ = agent_runtime
                            .fail_background_run(
                                &child_run_id,
                                Some(&output),
                                self.runtime_deps.session_id.clone(),
                                parent_run_id,
                                bus,
                            )
                            .await;
                    } else {
                        let _ = agent_runtime.fail_run(&child_run_id).await;
                    }
                } else {
                    let _ = agent_runtime.fail_run(&child_run_id).await;
                }
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
            return Err(LegacyToolError::AskRequired(decision));
        }

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
        let (python_binary, python_home) =
            crate::python::runner::resolve_python_path(self.runtime_deps.app_handle.as_ref());
        let file_ops = Arc::new(DefaultFileOperations {
            storage: self.runtime_deps.storage.clone(),
            file_manager: self.runtime_deps.file_manager.clone(),
            workspace_path: self.runtime_deps.workspace_path.clone(),
            conversation_id: self.runtime_deps.conversation_id.clone(),
            run_id: Some(child_run_id),
            python_binary: Some(python_binary),
            python_home,
        });

        QueryEngine::with_dispatcher(dispatcher)
            .with_workspace_path(self.runtime_deps.workspace_path.clone())
            .with_authorized_workspace(self.runtime_deps.authorized_workspace.clone())
            .with_browser_available(self.runtime_deps.connector_engine.is_some())
            .with_file_ops(file_ops)
            .with_read_file_state(child_read_file_state)
    }
}

fn emit_tool_executing(
    app_handle: Option<&tauri::AppHandle>,
    conversation_id: &str,
    tool_name: &str,
    tool_call_id: &str,
    purpose: Option<&str>,
) {
    if let Some(app) = app_handle {
        let _ = tauri::Emitter::emit(
            app,
            "tool:executing",
            serde_json::json!({
                "conversationId": conversation_id,
                "toolName": tool_name,
                "toolId": tool_call_id,
                "purpose": purpose,
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
) {
    if let Some(app) = app_handle {
        let _ = tauri::Emitter::emit(
            app,
            "tool:completed",
            serde_json::json!({
                "conversationId": conversation_id,
                "toolId": tool_call_id,
                "success": success,
                "summary": summary,
            }),
        );
    }
}

fn context_modifier_to_message(value: &serde_json::Value) -> Option<ChatMessage> {
    let role = value.get("role")?.as_str()?;
    let content = value.get("content")?.as_str()?;
    Some(ChatMessage::text(role, content))
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
        } => PermissionDecision::Ask {
            message: format!(
                "Subagent tool '{}' (tool_call_id={}) requires confirmation: {}",
                tool_name, tool_call_id, message
            ),
            suggestions,
            remember_options,
            default_destination,
            reason,
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
