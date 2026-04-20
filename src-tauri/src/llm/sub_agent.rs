//! Sub-agent executor — runs a mini agent loop with its own system prompt,
//! tool set, and iteration budget. Used by delegation tools like `browse_data`
//! to isolate complex multi-step tasks from the main conversation context.

use futures::StreamExt;
use log::{info, warn};
use std::sync::Arc;

use crate::llm::gateway::LlmGateway;
use crate::llm::masking::MaskingLevel;
use crate::llm::streaming::{ChatMessage, StopReason, StreamEvent, ToolDefinition};
use crate::models::settings::AppSettings;
use crate::plugin::registry::{RequestScopedRuntimeDeps, ToolRegistry};
use crate::plugin::tool_trait::ToolError as LegacyToolError;
use crate::runtime::agent::message_bridge;
use crate::runtime::agent::subagent_result_envelope::{
    build_subagent_transcript_ref, SubAgentResultEnvelope, SubAgentTerminalToolResult,
    SubAgentTranscriptEntry,
};
use crate::runtime::agent::{AgentRuntime, SpawnChildRunRequest, SubagentTranscriptEntryRecord};
use crate::runtime::ids::RunId;
use crate::runtime::tools::capability::DefaultFileOperations;
use crate::runtime::tools::permission::PermissionDecision;
use crate::runtime::tools::{
    CapabilityContext, FileReadingLimits, StorageCapability, ToolDispatchOutcome,
    ToolExecutionContext,
};

use tauri::Emitter;

/// Truncate a string at a safe UTF-8 boundary.
fn safe_truncate(s: &str, max_bytes: usize) -> &str {
    if s.len() <= max_bytes {
        return s;
    }
    let mut end = max_bytes;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    &s[..end]
}

#[cfg(test)]
fn take_ask_required_decision(err: &LegacyToolError) -> Option<PermissionDecision> {
    match err {
        LegacyToolError::AskRequired(decision) => Some(decision.clone()),
        _ => None,
    }
}

#[derive(Clone)]
pub struct SubAgentRuntimeDeps {
    pub storage: Arc<crate::storage::file_store::AppStorage>,
    pub file_manager: Arc<crate::storage::file_manager::FileManager>,
    pub workspace_path: std::path::PathBuf,
    pub conversation_id: String,
    pub session_id: crate::runtime::ids::SessionId,
    pub run_id: Option<RunId>,
    pub agent_id: Option<crate::runtime::ids::AgentId>,
    pub session_manager: Arc<crate::python::session::PythonSessionManager>,
    pub connector_engine: Option<Arc<crate::connector::ConnectorEngine>>,
    pub agent_runtime: Option<Arc<AgentRuntime>>,
    pub event_bus: Option<crate::runtime::event_bus::RuntimeEventBus>,
    pub authorized_workspace: Option<crate::runtime::store::AuthorizedWorkspaceRef>,
    pub read_file_state: Option<Arc<crate::runtime::tools::capability::FileStateCache>>,
    pub app_handle: Option<tauri::AppHandle>,
}

impl SubAgentRuntimeDeps {
    pub fn request_scoped_tool_deps(
        &self,
        run_id: RunId,
        agent_id: Option<crate::runtime::ids::AgentId>,
        cancellation: Option<crate::runtime::cancellation::CancellationToken>,
        read_file_state: Option<Arc<crate::runtime::tools::capability::FileStateCache>>,
    ) -> RequestScopedRuntimeDeps {
        RequestScopedRuntimeDeps {
            storage: self.storage.clone(),
            file_manager: self.file_manager.clone(),
            workspace_path: self.workspace_path.clone(),
            conversation_id: self.conversation_id.clone(),
            session_id: self.session_id.clone(),
            run_id: Some(run_id),
            agent_id,
            tavily_api_key: None,
            bocha_api_key: None,
            app_handle: self.app_handle.clone(),
            session_manager: self.session_manager.clone(),
            auth_manager: None,
            connector_engine: self.connector_engine.clone(),
            use_cloud: false,
            model: String::new(),
            gateway: None,
            tool_registry: None,
            app_settings: None,
            agent_runtime: self.agent_runtime.clone(),
            event_bus: self.event_bus.clone(),
            authorized_workspace: self.authorized_workspace.clone(),
            read_file_state,
            cancellation,
        }
    }
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

fn build_subagent_capability(
    tool_name: &str,
    runtime_deps: &SubAgentRuntimeDeps,
    scoped_request_deps: &RequestScopedRuntimeDeps,
    is_subagent: bool,
) -> Arc<CapabilityContext> {
    let storage = StorageCapability {
        workspace_path: runtime_deps.workspace_path.clone(),
        authorized_workspace: runtime_deps.authorized_workspace.clone(),
    };
    let file_ops = (tool_name == "load_file").then(|| {
        let (python_binary, python_home) =
            crate::python::runner::resolve_python_path(runtime_deps.app_handle.as_ref());
        Arc::new(DefaultFileOperations {
            storage: runtime_deps.storage.clone(),
            file_manager: runtime_deps.file_manager.clone(),
            workspace_path: runtime_deps.workspace_path.clone(),
            conversation_id: runtime_deps.conversation_id.clone(),
            run_id: scoped_request_deps.run_id.clone(),
            python_binary: Some(python_binary),
            python_home,
        }) as Arc<dyn crate::runtime::tools::capability::FileOperations>
    });

    Arc::new(CapabilityContext {
        storage: Some(storage),
        workspace_id: Some(runtime_deps.conversation_id.clone()),
        browser_available: runtime_deps.connector_engine.is_some(),
        file_ops,
        read_file_state: scoped_request_deps.read_file_state.clone(),
        file_reading_limits: Some(FileReadingLimits::default()),
        notification_sink: None,
        is_subagent,
    })
}

fn collect_generated_files(
    runtime_deps: &SubAgentRuntimeDeps,
    tool_result: &crate::runtime::tools::ToolResult,
) -> Vec<String> {
    let mut files = Vec::new();

    if let Some(meta) = tool_result.file_meta.as_ref() {
        let full_path = runtime_deps.file_manager.full_path(&meta.stored_path);
        files.push(full_path.display().to_string());
    }

    for line in tool_result.content.lines() {
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

/// Configuration for a sub-agent run.
pub struct SubAgentConfig {
    /// The task description (becomes the initial user message).
    pub task: String,
    /// System prompt for the sub-agent.
    pub system_prompt: String,
    /// Which tools the sub-agent can use (names must match registry).
    pub allowed_tools: Vec<String>,
    /// Max iterations before forced stop.
    pub max_iterations: usize,
    /// Dynamic context injected alongside the system prompt.
    pub dynamic_context: String,
    /// Conversation ID of the parent (for emitting heartbeat events to prevent watchdog timeout).
    pub conversation_id: String,
    /// Parent run identity for child-run isolation.
    pub parent_run_id: Option<RunId>,
    /// Whether this sub-agent should run in background mode.
    pub background: bool,
    /// App handle for emitting Tauri events.
    pub app_handle: Option<tauri::AppHandle>,
    /// Parent cancellation token.  When `Some`, tool executions inside this
    /// sub-agent receive a **child** of this token so that cancelling the
    /// parent turn propagates into the sub-agent's tool calls.
    ///
    /// `None` means no cancel cascade (isolated root token per tool call).
    ///
    /// FIXME(S4/blocker): `LegacyToolAdapter::from_plugin` currently drops the
    /// `ToolExecutionContext` (and its cancel token), so the cancel signal does
    /// not reach inside individual tool plugins even when this field is `Some`.
    /// Wiring requires either (a) plumbing the token through `PluginContext`, or
    /// (b) migrating the relevant tools to `RuntimeTool`.  Until then, `Some`
    /// here at least means the sub-agent loop itself observes the cancel cascade.
    pub cancel_token: Option<crate::runtime::cancellation::CancellationToken>,
}

/// Result from a sub-agent run.
pub struct SubAgentResult {
    /// Final text output from the sub-agent.
    pub output: String,
    /// File paths produced during execution.
    pub files: Vec<String>,
    /// How many iterations were used.
    pub iterations_used: usize,
    /// Structured result envelope shared by foreground/background parent flows.
    pub envelope: SubAgentResultEnvelope,
}

/// Run a sub-agent loop: LLM + tool execution with isolated context.
///
/// The sub-agent has its own system prompt, tool set, and message history.
/// It does not emit streaming events to the frontend (silent execution).
pub async fn run_sub_agent(
    gateway: &LlmGateway,
    tool_registry: &ToolRegistry,
    runtime_deps: &SubAgentRuntimeDeps,
    config: SubAgentConfig,
    settings: &AppSettings,
) -> std::result::Result<SubAgentResult, LegacyToolError> {
    info!(
        "[SubAgent] Starting: task_len={}, tools={:?}, max_iter={}",
        config.task.len(),
        config.allowed_tools,
        config.max_iterations
    );

    // Guard against recursive sub-agent calls
    if config.allowed_tools.contains(&"browse_data".to_string()) {
        return Err(anyhow::anyhow!(
            "Sub-agent must not include 'browse_data' in allowed_tools (recursion guard)"
        )
        .into());
    }

    // Build filtered tool schemas
    let all_schemas = tool_registry.get_all_schemas().await;
    let tool_defs: Vec<ToolDefinition> = all_schemas
        .into_iter()
        .filter(|s| config.allowed_tools.contains(&s.name))
        .collect();

    info!("[SubAgent] Tool schemas loaded: {}", tool_defs.len());

    // Initialize message history with the task
    let mut messages = vec![ChatMessage::text("user", &config.task)];

    let mut output = String::new();
    let mut files: Vec<String> = vec![];
    let mut iterations_used = 0;
    let mut pending_ask: Option<PermissionDecision> = None;
    let mut terminal_tool_results: Vec<SubAgentTerminalToolResult> = Vec::new();

    let agent_runtime = runtime_deps
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

    let request_scoped_runtime_deps = runtime_deps.request_scoped_tool_deps(
        child_run_id.clone(),
        child_agent_id.clone(),
        config.cancel_token.as_ref().map(|parent| parent.child_token()),
        runtime_deps
            .read_file_state
            .as_ref()
            .map(|cache| cache.clone_for_child()),
    );
    let dispatcher = tool_registry
        .to_runtime_dispatcher(request_scoped_runtime_deps.clone())
        .await;

    let dynamic_ctx = if config.dynamic_context.is_empty() {
        None
    } else {
        Some(config.dynamic_context.as_str())
    };

    'agent_loop: for iteration in 0..config.max_iterations {
        iterations_used = iteration + 1;

        info!(
            "[SubAgent] iter={}/{}, messages={}",
            iteration,
            config.max_iterations,
            messages.len()
        );

        // Call LLM
        let stream_result = gateway
            .stream_message(
                settings,
                messages.clone(),
                MaskingLevel::Relaxed,
                Some(&config.system_prompt),
                dynamic_ctx,
                Some(tool_defs.clone()),
                4096,
                Some(&sub_conv_id),
            )
            .await;

        let (_task_id, mut stream, _mask_ctx, _cancel_rx) = match stream_result {
            Ok(r) => r,
            Err(e) => {
                warn!("[SubAgent] LLM call failed at iter {}: {}", iteration, e);
                output = format!("Sub-agent LLM error: {}", e);
                break;
            }
        };

        // Collect stream events
        let mut iter_content = String::new();
        let mut tool_calls = vec![];
        let mut stop_reason = StopReason::EndTurn;

        while let Some(event) = stream.next().await {
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
                    break;
                }
                _ => {}
            }
        }

        info!(
            "[SubAgent] iter={} content_len={} tool_calls={} stop={:?}",
            iteration,
            iter_content.len(),
            tool_calls.len(),
            stop_reason
        );

        // If no tool calls, we're done
        if stop_reason != StopReason::ToolUse || tool_calls.is_empty() {
            output = iter_content.clone();
            if !iter_content.is_empty() {
                messages.push(ChatMessage::text("assistant", iter_content));
            }
            break;
        }

        // Push assistant message with tool calls
        messages.push(ChatMessage::assistant_with_tool_calls(
            iter_content.clone(),
            tool_calls
                .iter()
                .map(|tc| crate::llm::streaming::ToolCall {
                    id: tc.id.clone(),
                    name: tc.name.clone(),
                    arguments: tc.arguments.clone(),
                })
                .collect(),
        ));

        let (permitted_tool_calls, denied_tool_calls): (Vec<_>, Vec<_>) = tool_calls
            .iter()
            .cloned()
            .partition(|tc| config.allowed_tools.contains(&tc.name));

        for tc in denied_tool_calls {
            let err_msg = format!("Tool '{}' not available in this sub-agent", tc.name);
            terminal_tool_results.push(SubAgentTerminalToolResult {
                tool_call_id: tc.id.clone(),
                tool_name: tc.name.clone(),
                success: false,
                summary: err_msg.clone(),
                generated_files: Vec::new(),
            });
            messages.push(ChatMessage::tool_result(&tc.id, &tc.name, err_msg));
        }

        for tc in &permitted_tool_calls {
            info!("[SubAgent] Executing tool '{}' (id={})", tc.name, tc.id);
            if let Some(ref app) = config.app_handle {
                let _ = app.emit(
                    "tool:executing",
                    serde_json::json!({
                        "conversationId": config.conversation_id,
                        "toolName": tc.name,
                        "toolId": tc.id,
                        "purpose": format!("[Browser Agent] {}", tc.name),
                    }),
                );
            }
        }

        let dispatch_calls: Vec<_> = permitted_tool_calls
            .iter()
            .map(|tc| {
                let sub_cancel = match config.cancel_token.as_ref() {
                    Some(parent) => parent.child_token(),
                    None => crate::runtime::cancellation::CancellationToken::new(),
                };
                let capability =
                    build_subagent_capability(
                        &tc.name,
                        runtime_deps,
                        &request_scoped_runtime_deps,
                        child_agent_id.is_some(),
                    );
                let exec_ctx = ToolExecutionContext::new(
                    runtime_deps.session_id.clone(),
                    child_run_id.clone(),
                    child_agent_id.clone(),
                    tc.id.clone(),
                    sub_cancel,
                )
                .with_capability(capability);
                (tc.name.clone(), tc.arguments.clone(), exec_ctx)
            })
            .collect();
        let dispatch_results = dispatcher.dispatch_batch(dispatch_calls).await;

        for (tc, dispatch_result) in permitted_tool_calls.into_iter().zip(dispatch_results) {
            match dispatch_result {
                Ok(ToolDispatchOutcome::Completed { result, .. }) => {
                    let tool_summary = if result.content.len() > 240 {
                        format!("{}...", safe_truncate(&result.content, 240))
                    } else {
                        result.content.clone()
                    };
                    let generated_files = collect_generated_files(runtime_deps, &result);
                    terminal_tool_results.push(SubAgentTerminalToolResult {
                        tool_call_id: tc.id.clone(),
                        tool_name: tc.name.clone(),
                        success: true,
                        summary: tool_summary,
                        generated_files: generated_files.clone(),
                    });
                    if let Some(ref app) = config.app_handle {
                        let summary = if result.content.len() > 100 {
                            format!("{}...", safe_truncate(&result.content, 100))
                        } else {
                            result.content.clone()
                        };
                        let _ = app.emit(
                            "tool:completed",
                            serde_json::json!({
                                "conversationId": config.conversation_id,
                                "toolId": tc.id,
                                "success": true,
                                "summary": summary,
                            }),
                        );
                    }
                    files.extend(generated_files);
                    let content = if result.content.len() > 8000 {
                        format!("{}...(truncated)", safe_truncate(&result.content, 8000))
                    } else {
                        result.content
                    };
                    messages.push(ChatMessage::tool_result(&tc.id, &tc.name, content));
                }
                Ok(ToolDispatchOutcome::AskRequired(decision)) => {
                    let bubbled = annotate_subagent_ask_decision(&tc.name, &tc.id, decision);
                    terminal_tool_results.push(SubAgentTerminalToolResult {
                        tool_call_id: tc.id.clone(),
                        tool_name: tc.name.clone(),
                        success: false,
                        summary: "Permission Ask required".to_string(),
                        generated_files: Vec::new(),
                    });
                    messages.push(ChatMessage::tool_result(
                        &tc.id,
                        &tc.name,
                        "Permission Ask required".to_string(),
                    ));
                    warn!(
                        "[SubAgent] Tool '{}' returned AskRequired; bubbling to parent: {}",
                        tc.name, bubbled
                    );
                    pending_ask = Some(bubbled);
                    break 'agent_loop;
                }
                Err(e) => {
                    let err_msg = format!("Tool error: {}", e);
                    terminal_tool_results.push(SubAgentTerminalToolResult {
                        tool_call_id: tc.id.clone(),
                        tool_name: tc.name.clone(),
                        success: false,
                        summary: err_msg.clone(),
                        generated_files: Vec::new(),
                    });
                    warn!("[SubAgent] Tool '{}' failed: {}", tc.name, err_msg);
                    if let Some(ref app) = config.app_handle {
                        let _ = app.emit(
                            "tool:completed",
                            serde_json::json!({
                                "conversationId": config.conversation_id,
                                "toolId": tc.id,
                                "success": false,
                                "summary": err_msg.clone(),
                            }),
                        );
                    }
                    messages.push(ChatMessage::tool_result(&tc.id, &tc.name, err_msg));
                }
            }
        }
    }

    if iterations_used >= config.max_iterations && output.is_empty() {
        output = "Sub-agent reached iteration limit.".to_string();
    }

    let mut generated_files = files.clone();
    generated_files.sort();
    generated_files.dedup();

    let transcript_ref = build_subagent_transcript_ref(child_run_id.as_str());
    let transcript_entries: Vec<SubagentTranscriptEntryRecord> = messages
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

    // Clean up gateway active task entry
    gateway.clear_task(&sub_conv_id);
    runtime_deps.session_manager.destroy_run(&child_run_id).await;

    // Route completion through the correct path depending on whether this was
    // a background sub-agent run.  Background runs must emit `AgentIdle` via
    // the runtime event bus so the UI knows the work has finished.
    if let Some(ref handle) = child_handle {
        if handle.invocation().background {
            // Background path: persist summary + emit AgentIdle
            if let (Some(bus), Some(parent_run_id)) =
                (runtime_deps.event_bus.clone(), config.parent_run_id.clone())
            {
                let summary = message_bridge::format_sub_agent_envelope_summary(&envelope);
                let _ = agent_runtime
                    .complete_background_run(
                        &child_run_id,
                        Some(&summary),
                        Some(&transcript_ref),
                        runtime_deps.session_id.clone(),
                        parent_run_id,
                        bus,
                    )
                    .await;
            } else {
                // Bus or parent_run_id not available — fall back to plain complete
                let _ = agent_runtime.complete_run(&child_run_id).await;
            }
        } else {
            // Foreground path: plain status update, no AgentIdle event
            let _ = agent_runtime.complete_run(&child_run_id).await;
        }
    }

    info!(
        "[SubAgent] Complete: iterations={}, output_len={}, files={}",
        iterations_used,
        output.len(),
        files.len()
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plugin::tool_trait::ToolError as LegacyToolError;
    use crate::runtime::tools::permission::{
        default_permission_ask, PermissionDecision, PermissionReason,
    };

    #[test]
    fn take_ask_required_decision_preserves_structured_permission_request() {
        let decision = PermissionDecision::Ask {
            message: "need approval".to_string(),
            suggestions: vec!["Allow once".to_string(), "Deny".to_string()],
            remember_options: default_permission_ask().0,
            default_destination: default_permission_ask().1,
            reason: PermissionReason::Other("subagent-inner".to_string()),
        };

        let extracted = take_ask_required_decision(&LegacyToolError::AskRequired(decision.clone()))
            .expect("AskRequired must stay structured");

        match extracted {
            PermissionDecision::Ask {
                message,
                suggestions,
                ..
            } => {
                assert_eq!(message, "need approval");
                assert_eq!(
                    suggestions,
                    vec!["Allow once".to_string(), "Deny".to_string()]
                );
            }
            other => panic!("expected ask decision, got: {:?}", other),
        }
    }
}
