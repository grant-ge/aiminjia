use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;
use std::collections::HashSet;
use std::time::Duration;

use crate::llm::context_decay::{
    context_window_for_provider, estimate_tokens_from_json, CONTEXT_OVERFLOW_THRESHOLD,
};
use crate::runtime::agent::task_notification::{QueuedNotification, TaskNotificationQueue};
use crate::runtime::cancellation::{CancellationReason, CancellationToken};
use crate::runtime::chat::context_builder::build_iteration_context;
use crate::runtime::chat::post_process;
use crate::runtime::chat::preprocess::{
    prepare_messages_for_llm, PreprocessConfig, PreprocessRetryAction, PreprocessTrigger,
};
use crate::runtime::chat::safeguard::{self, SafeguardAction};
use crate::runtime::chat::tool_result_collector;
use crate::runtime::chat::tool_round_driver::{ToolRoundDriver, ToolRoundResult};
use crate::runtime::chat::tool_round_types::RuntimeToolCallOutcome;
use crate::runtime::chat::turn_config::{
    LlmStepInput, LlmStepResult, ResolvedLlmSettings, TurnConfig, TurnConfigOverrides, TurnError,
    TurnIterationState, MAX_OUTPUT_TOKENS_RECOVERY_LIMIT,
};
use crate::runtime::chat::turn_outcome::ChatTurnOutcome;
use crate::runtime::event_bus::RuntimeEventBus;
use crate::runtime::events::{AgentIdleScope, RuntimeEvent, RuntimeEventKind};
use crate::runtime::hooks::config::{HookEvent, HookRegistry};
use crate::runtime::hooks::HookRunner;
use crate::runtime::ids::{AgentId, RunId, SessionId};
use crate::runtime::interaction::{
    InteractionId, InteractionResolution, PendingInteractionControlPlane,
};
use crate::runtime::query_engine::QueryEngine;
use crate::runtime::state::TurnState;
use crate::runtime::store::{
    PendingPermissionControlPlane, PendingPermissionRequest, PendingPermissionResolution,
};
use crate::runtime::tools::permission::{PermissionDecision, PermissionMode};
use crate::telemetry::{record_diagnostic, DiagnosticEvent, DiagnosticSource};

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatAttachmentRef {
    pub id: String,
    pub file_name: String,
    pub file_path: String,
    pub kind: String,
    pub file_size: u64,
    pub file_type: String,
    pub mime_type: Option<String>,
}

pub fn build_user_content_json(
    content: &str,
    attachments: &[ChatAttachmentRef],
    selected_skill_id: Option<&str>,
    selected_skill_label: Option<&str>,
) -> serde_json::Value {
    let mut value = serde_json::json!({ "text": content });
    if !attachments.is_empty() {
        value["files"] = serde_json::Value::Array(
            attachments
                .iter()
                .map(|att| {
                    serde_json::json!({
                        "id": att.id,
                        "fileName": att.file_name,
                        "filePath": att.file_path,
                        "kind": att.kind,
                        "fileSize": att.file_size,
                        "fileType": att.file_type,
                        "mimeType": att.mime_type,
                        "status": "uploaded"
                    })
                })
                .collect(),
        );
    }
    if let Some(skill_id) = selected_skill_id.map(str::trim).filter(|id| !id.is_empty()) {
        let command = format!("/{skill_id}");
        value["commandText"] = serde_json::json!(format!("{} {}", command, content).trim());
        value["skillCommand"] = serde_json::json!({
            "id": skill_id,
            "label": selected_skill_label
                .map(str::trim)
                .filter(|label| !label.is_empty())
                .unwrap_or(skill_id),
            "command": command,
        });
    }
    value
}

/// The chat turn request type.  Defined here to avoid circular imports between
/// `session_runtime` and `chat`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChatTurnRequest {
    pub conversation_id: SessionId,
    pub content: String,
    pub attachments: Vec<ChatAttachmentRef>,
    pub agent_name: Option<String>,
    pub permission_mode: PermissionMode,
    /// The authoritative run_id for this turn.
    /// `ChatTurnRequest::new` generates a fresh id; transport code may reserve
    /// resources with it before handing the request to `SessionRuntime`.
    pub run_id: RunId,
    pub hook_registry: Option<Arc<HookRegistry>>,
    pub client_message_id: Option<String>,
    pub selected_skill_id: Option<String>,
    pub selected_skill_label: Option<String>,
    pub persona_id_override: Option<String>,
}

impl ChatTurnRequest {
    pub fn new(
        conversation_id: impl Into<SessionId>,
        content: impl Into<String>,
        attachments: Vec<ChatAttachmentRef>,
    ) -> Self {
        Self {
            conversation_id: conversation_id.into(),
            content: content.into(),
            attachments,
            agent_name: None,
            permission_mode: PermissionMode::Default,
            run_id: RunId::new(uuid::Uuid::new_v4().to_string()),
            hook_registry: None,
            client_message_id: None,
            selected_skill_id: None,
            selected_skill_label: None,
            persona_id_override: None,
        }
    }

    pub fn with_persona_id_override(mut self, persona_id: String) -> Self {
        self.persona_id_override = Some(persona_id);
        self
    }
}

/// S4 新 trait：executor 只做 provider streaming adapter。
/// Driver 拥有 query loop 和状态变更，executor 不修改外部状态。
#[async_trait]
pub trait RuntimeLlmExecutor: Send + Sync {
    /// 单步 LLM 调用。接收只读输入，返回结构化结果。
    /// 内部调用 gateway.stream_message()，通过 bus emit StreamDelta/StreamError。
    async fn run_llm_step(
        &self,
        input: &LlmStepInput<'_>,
        bus: &RuntimeEventBus,
        cancel: &CancellationToken,
    ) -> Result<LlmStepResult, TurnError>;

    /// 解析本次 turn 使用的 LLM 路由设置。
    ///
    /// 生产 executor 应在这里做一次性的 DB 读取与密钥解密；driver 会把
    /// 返回值存入 `TurnConfig` 并在线程内复用到每次 `run_llm_step`。
    async fn load_llm_settings(&self) -> Result<ResolvedLlmSettings, TurnError> {
        Ok(ResolvedLlmSettings::default())
    }

    async fn load_llm_settings_for_turn(
        &self,
        _request: &ChatTurnRequest,
    ) -> Result<ResolvedLlmSettings, TurnError> {
        self.load_llm_settings().await
    }

    /// 持久化 assistant message 到存储。纯 I/O，不含事件发射。
    async fn persist_assistant_message(
        &self,
        conversation_id: &str,
        content: &str,
        tool_calls: &[serde_json::Value],
        generated_file_ids: &[String],
        file_metas: &[serde_json::Value],
    ) -> Result<String, TurnError>;

    /// 持久化 user message 到存储。纯 I/O，不含事件发射。
    /// 默认 no-op（返回空 message_id）；生产 executor 必须 override。
    async fn persist_user_message(
        &self,
        _conversation_id: &str,
        _content: &str,
        _attachments: &[ChatAttachmentRef],
        _client_message_id: Option<&str>,
    ) -> Result<String, TurnError> {
        Ok(String::new())
    }

    /// 持久化 tool result 消息到存储。纯 I/O，不含事件发射。
    /// 默认 no-op；生产 executor 必须 override。
    async fn persist_tool_messages(
        &self,
        _conversation_id: &str,
        _tool_messages: &[serde_json::Value],
    ) -> Result<(), TurnError> {
        Ok(())
    }

    /// 持久化单次 iteration 的 assistant[toolCalls] 消息（无文字内容，无生成文件）。
    /// 在工具执行前调用，使存储顺序正确反映 assistant → tools 穿插结构。
    /// 返回新建消息的 id（无 tool_calls 时返回 None）。默认 no-op。
    async fn persist_iteration_assistant_message(
        &self,
        _conversation_id: &str,
        _tool_calls: &[serde_json::Value],
    ) -> Result<Option<String>, TurnError> {
        Ok(None)
    }

    /// Step 后处理。默认 no-op。
    async fn finalize_step(
        &self,
        _state: &TurnIterationState,
        _config: &TurnConfig,
    ) -> Result<(), TurnError> {
        Ok(())
    }

    /// 构建 Turn 级的 system prompt。
    /// 由 executor 从 DB / settings / persona / product_name 合成。
    /// 默认 no-op（返回空字符串），生产 executor 必须 override。
    async fn build_system_prompt(&self, _conversation_id: &str) -> Result<String, TurnError> {
        Ok(String::new())
    }

    async fn build_prompt_snapshot(
        &self,
        _conversation_id: &str,
    ) -> Result<Option<crate::runtime::chat::prompt::TurnPromptSnapshot>, TurnError> {
        Ok(None)
    }

    /// 构建发给 LLM 的当前用户消息内容。
    ///
    /// 允许生产 executor 将上传附件提示、workspace 提示等附加到用户消息，
    /// 但持久化到 DB 的原始 user message 仍由 `persist_user_message` 负责。
    async fn build_user_message_content(
        &self,
        _conversation_id: &str,
        content: &str,
        _attachments: &[ChatAttachmentRef],
    ) -> Result<String, TurnError> {
        Ok(content.to_string())
    }

    /// 返回本次 Turn 使用的 tool definitions（JSON schema）。
    ///
    /// 默认实现返回空 vec（向后兼容旧 mock executor）。
    /// 生产 executor（TauriLegacyTurnExecutor）必须 override。
    async fn get_tool_defs(&self) -> Result<Vec<serde_json::Value>, TurnError> {
        Ok(vec![])
    }

    /// 加载 conversation 的历史对话消息（格式：[{role, content}, ...]）。
    ///
    /// 返回的消息将被插入到 messages 中（在 system-reminder 之后、当前 user message 之前）。
    /// 默认实现返回空 vec（无历史）。生产 executor 必须 override。
    async fn load_history(
        &self,
        _conversation_id: &str,
    ) -> Result<Vec<serde_json::Value>, TurnError> {
        Ok(vec![])
    }

    /// 返回会话级环境信息字符串（工作目录、git 状态、平台）。
    ///
    /// 返回值将被注入到每次 iteration 的 dynamic_context 中。
    /// 默认实现返回空字符串（向后兼容旧 mock executor）。
    /// 生产 executor 必须 override。
    async fn get_env_info(&self, conversation_id: &str) -> Result<String, TurnError> {
        let _ = conversation_id;
        Ok(String::new())
    }

    /// 返回当前 turn 可见的 skill catalog。
    ///
    /// 默认返回空字符串，生产 executor 从 SkillRegistry 构建。
    async fn get_skill_catalog(&self, _agent_id: Option<&str>) -> String {
        String::new()
    }

    /// 加载 turn 对应的 workspace 路径。
    async fn load_workspace_path(&self) -> Result<PathBuf, TurnError> {
        Ok(PathBuf::new())
    }

    /// Resolve optional per-turn overrides after transport/runtime state has
    /// been loaded for the request but before the driver snapshots TurnConfig.
    async fn load_turn_config_overrides(
        &self,
        _request: &ChatTurnRequest,
    ) -> Result<TurnConfigOverrides, TurnError> {
        Ok(TurnConfigOverrides::default())
    }

    /// 加载 AGENT.md user-context 文件。
    async fn load_renlijia_md(
        &self,
        _workspace_path: &Path,
    ) -> Result<Vec<crate::runtime::renlijia_md::RenlijiaMdFile>, TurnError> {
        Ok(vec![])
    }

    /// 加载当前 workspace 对应的项目记忆上下文。
    ///
    /// 默认返回空上下文，便于旧测试 executor 无需感知此能力。
    async fn load_project_memory(
        &self,
        _workspace_path: &Path,
        _query: &str,
    ) -> Result<crate::runtime::project_memory::ProjectMemoryContext, TurnError> {
        Ok(crate::runtime::project_memory::ProjectMemoryContext::default())
    }

    /// 加载跨对话共享的 core memory。
    ///
    /// 默认返回空字符串，便于旧测试 executor 无需感知此能力。
    async fn load_core_memory(&self, _conversation_id: &str) -> Result<String, TurnError> {
        Ok(String::new())
    }

    async fn compact_summary(
        &self,
        _conversation_id: &str,
        _messages: &[serde_json::Value],
    ) -> Result<String, TurnError> {
        Ok(String::new())
    }

    /// Persist a compact boundary record after successful compaction.
    async fn save_compact_boundary(
        &self,
        _record: crate::runtime::chat::compaction::CompactBoundaryRecord,
    ) -> Result<(), TurnError> {
        Ok(())
    }
}

/// Runtime-owned chat turn driver.
///
/// Single entry point for chat turn orchestration.  There are two execution modes:
///
/// **No-executor mode** (pure runtime path, used in tests):
///   `run_chat_turn` calls `QueryEngine::run()` which emits
///   `StreamDelta → MessagePersisted → StreamDone` on the bus.  The
///   `TauriEventAdapter` translates these to the expected frontend legacy events.
///
/// **S4 executor mode** (production):
///   `run_chat_turn_s4` is the driver-owned loop.  The `RuntimeLlmExecutor` is a
///   pure provider streaming adapter; the driver owns the query/tool loop and
///   emits all lifecycle events through the bus.
#[derive(Clone)]
pub struct RuntimeChatTurnDriver {
    query_engine: QueryEngine,
    event_bus: RuntimeEventBus,
    /// S4 executor：只做 provider streaming adapter。
    llm_executor: Option<Arc<dyn RuntimeLlmExecutor>>,
    pending_permission_control_plane: Option<Arc<dyn PendingPermissionControlPlane>>,
    pending_interaction_control_plane: Option<Arc<dyn PendingInteractionControlPlane>>,
    task_notification_queue: Option<Arc<TaskNotificationQueue>>,
}

fn synthetic_cancelled_tool_result(reason: Option<CancellationReason>) -> &'static str {
    match reason {
        Some(CancellationReason::Interrupt) => "Tool execution was interrupted before completion.",
        Some(CancellationReason::SiblingError) => {
            "Tool execution was cancelled because another tool call failed."
        }
        Some(CancellationReason::BackgroundStop) => {
            "Tool execution was cancelled because the background run stopped."
        }
        Some(CancellationReason::UserCancel) => {
            "Tool execution was interrupted by user cancellation."
        }
        None => "Tool execution was interrupted before completion.",
    }
}

fn drain_and_inject_task_notifications(
    queue: &Option<Arc<TaskNotificationQueue>>,
    session_id: &SessionId,
    messages: &mut Vec<serde_json::Value>,
) -> Vec<QueuedNotification> {
    let Some(queue) = queue.as_ref() else {
        return Vec::new();
    };

    let notifications = queue.drain_for_session(session_id);
    if notifications.is_empty() {
        return Vec::new();
    }

    let count = notifications.len();
    for notification in &notifications {
        messages.push(serde_json::json!({
            "role": "user",
            "content": notification.xml.clone(),
        }));
    }
    log::info!(
        "[chat_turn_driver] injected {} pending task notification(s) into LLM messages",
        count
    );
    notifications
}

fn re_enqueue_task_notifications(
    queue: &Option<Arc<TaskNotificationQueue>>,
    notifications: Vec<QueuedNotification>,
) {
    if notifications.is_empty() {
        return;
    }
    let Some(queue) = queue.as_ref() else {
        return;
    };

    log::warn!(
        "[chat_turn_driver] re-enqueueing {} task notification(s) after step failure or cancellation",
        notifications.len()
    );
    queue.re_enqueue(notifications);
}

/// Inject synthetic tool results for assistant tool calls that have no matching
/// tool response yet. Returns the number of injected messages.
pub fn inject_synthetic_tool_results_for_missing_calls(
    messages: &mut Vec<serde_json::Value>,
    reason: Option<CancellationReason>,
) -> usize {
    let mut pending_ids: Vec<(String, Option<String>)> = Vec::new();
    let mut seen_assistant_ids: HashSet<String> = HashSet::new();
    let mut existing_tool_result_ids: HashSet<String> = HashSet::new();

    for message in messages.iter() {
        let role = message
            .get("role")
            .and_then(|v| v.as_str())
            .unwrap_or_default();
        if role == "assistant" {
            if let Some(tool_calls) = message.get("toolCalls").and_then(|v| v.as_array()) {
                for tc in tool_calls {
                    let Some(id) = tc.get("id").and_then(|v| v.as_str()) else {
                        continue;
                    };
                    if seen_assistant_ids.insert(id.to_string()) {
                        let name = tc
                            .get("name")
                            .and_then(|v| v.as_str())
                            .map(|s| s.to_string());
                        pending_ids.push((id.to_string(), name));
                    }
                }
            }
        } else if role == "tool" {
            let tool_call_id = message
                .get("toolCallId")
                .or_else(|| message.get("tool_call_id"))
                .and_then(|v| v.as_str());
            if let Some(id) = tool_call_id {
                existing_tool_result_ids.insert(id.to_string());
            }
        }
    }

    let mut injected = 0usize;
    for (id, name) in pending_ids {
        if existing_tool_result_ids.contains(&id) {
            continue;
        }
        messages.push(serde_json::json!({
            "role": "tool",
            "toolCallId": id,
            "name": name.unwrap_or_else(|| "unknown_tool".to_string()),
            "content": synthetic_cancelled_tool_result(reason),
        }));
        injected += 1;
    }

    injected
}

/// Shared cancel finalizer for driver-owned turn checkpoints.
///
/// Injects any missing synthetic tool results before marking the turn as
/// cancelled so the in-memory transcript never contains orphaned tool calls.
fn mark_turn_cancelled_with_synthetic_results(
    state: &mut TurnIterationState,
    reason: Option<CancellationReason>,
) {
    inject_synthetic_tool_results_for_missing_calls(&mut state.messages, reason);
    state.stream_cancelled = true;
}

fn build_renlijia_md_context_message(
    renlijia_md_files: &[crate::runtime::renlijia_md::RenlijiaMdFile],
) -> Option<serde_json::Value> {
    if renlijia_md_files.is_empty() {
        return None;
    }

    let renlijia_md_section = renlijia_md_files
        .iter()
        .map(|file| format!("{}:\n{}", file.path.display(), file.content))
        .collect::<Vec<_>>()
        .join("\n\n");

    Some(serde_json::json!({
        "role": "user",
        "content": format!(
            "<system-reminder>\nAs you answer the user's questions, you can use the following context:\n# renlijiaMd\n{}\n\nIMPORTANT: this context may or may not be relevant to your tasks. You should not respond to this context unless it is highly relevant to your task.\n</system-reminder>\n",
            renlijia_md_section
        ),
        "isMeta": true,
    }))
}

fn record_turn_diagnostic(
    workspace_path: &Path,
    event: &str,
    session_id: &crate::runtime::ids::SessionId,
    run_id: &crate::runtime::ids::RunId,
    ok: Option<bool>,
    error: Option<String>,
    payload: Option<serde_json::Value>,
) {
    let mut diag = DiagnosticEvent::new(event, DiagnosticSource::Backend)
        .conversation_id(session_id.as_str())
        .run_id(run_id.as_str());
    if let Some(ok) = ok {
        diag = diag.ok(ok);
    }
    if let Some(error) = error {
        diag = diag.error(error);
    }
    if let Some(payload) = payload {
        diag = diag.payload(payload);
    }
    record_diagnostic(workspace_path, diag);
}

impl RuntimeChatTurnDriver {
    pub fn new(query_engine: QueryEngine, event_bus: RuntimeEventBus) -> Self {
        Self {
            query_engine,
            event_bus,
            llm_executor: None,
            pending_permission_control_plane: None,
            pending_interaction_control_plane: None,
            task_notification_queue: None,
        }
    }

    pub fn with_llm_executor(
        query_engine: QueryEngine,
        event_bus: RuntimeEventBus,
        executor: Arc<dyn RuntimeLlmExecutor>,
    ) -> Self {
        Self {
            query_engine,
            event_bus,
            llm_executor: Some(executor),
            pending_permission_control_plane: None,
            pending_interaction_control_plane: None,
            task_notification_queue: None,
        }
    }

    pub fn with_llm_executor_and_permission_control_plane(
        query_engine: QueryEngine,
        event_bus: RuntimeEventBus,
        executor: Arc<dyn RuntimeLlmExecutor>,
        pending_permission_control_plane: Arc<dyn PendingPermissionControlPlane>,
    ) -> Self {
        Self {
            query_engine,
            event_bus,
            llm_executor: Some(executor),
            pending_permission_control_plane: Some(pending_permission_control_plane),
            pending_interaction_control_plane: None,
            task_notification_queue: None,
        }
    }

    pub fn with_llm_executor_and_control_planes(
        query_engine: QueryEngine,
        event_bus: RuntimeEventBus,
        executor: Arc<dyn RuntimeLlmExecutor>,
        pending_permission_control_plane: Arc<dyn PendingPermissionControlPlane>,
        pending_interaction_control_plane: Arc<dyn PendingInteractionControlPlane>,
    ) -> Self {
        Self {
            query_engine,
            event_bus,
            llm_executor: Some(executor),
            pending_permission_control_plane: Some(pending_permission_control_plane),
            pending_interaction_control_plane: Some(pending_interaction_control_plane),
            task_notification_queue: None,
        }
    }

    pub fn with_task_notification_queue(
        mut self,
        queue: Arc<TaskNotificationQueue>,
    ) -> Self {
        self.task_notification_queue = Some(queue);
        self
    }

    async fn await_permission_resolution(
        &self,
        cancel: &CancellationToken,
        tool_call_id: &str,
        mut rx: tokio::sync::oneshot::Receiver<PendingPermissionResolution>,
    ) -> PendingPermissionResolution {
        loop {
            tokio::select! {
                resolution = &mut rx => {
                    return resolution.unwrap_or_else(|_| PendingPermissionResolution::Cancel {
                        message: "Permission request was closed before it was resolved.".to_string(),
                    });
                }
                _ = tokio::time::sleep(Duration::from_millis(50)) => {
                    if cancel.is_cancelled() {
                        if let Some(control_plane) = self.pending_permission_control_plane.as_ref() {
                            let _ = control_plane.resolve_pending_request(
                                &tool_call_id.into(),
                                PendingPermissionResolution::Cancel {
                                    message: "Permission request cancelled because the turn was cancelled.".to_string(),
                                },
                            );
                        }
                    }
                }
            }
        }
    }

    async fn resolve_permission_asks(
        &self,
        turn: &TurnState,
        cancel: &CancellationToken,
        round_results: Vec<ToolRoundResult>,
    ) -> Result<Vec<ToolRoundResult>> {
        let mut resolved_results = Vec::with_capacity(round_results.len());

        for round_result in round_results {
            let ToolRoundResult::Ok(RuntimeToolCallOutcome::AskRequired {
                tool_call_id,
                tool_name,
                capability_scopes,
                original_request,
                decision,
            }) = &round_result
            else {
                resolved_results.push(round_result);
                continue;
            };
            let Some(control_plane) = self.pending_permission_control_plane.as_ref() else {
                return Err(anyhow::anyhow!(
                    "permission control plane is required to handle AskRequired tool outcomes"
                ));
            };

            let PermissionDecision::Ask {
                message,
                suggestions,
                remember_options,
                default_destination,
                ..
            } = decision
            else {
                resolved_results.push(round_result);
                continue;
            };
            let mode = turn.permission_mode();
            let pending_request = PendingPermissionRequest {
                tool_call_id: tool_call_id.clone().into(),
                session_id: turn.session_id().clone(),
                run_id: turn.run_id().clone(),
                tool_name: tool_name.clone(),
                capability_scopes: capability_scopes.clone(),
                message: message.clone(),
                suggestions: suggestions.clone(),
                mode,
                remember_options: remember_options.clone(),
                default_destination: *default_destination,
                original_request: original_request.clone(),
            };
            let resolution_rx = control_plane.insert_pending_request(pending_request)?;

            self.event_bus
                .emit(RuntimeEvent::new(
                    turn.session_id().clone(),
                    turn.run_id().clone(),
                    RuntimeEventKind::PermissionAskRequired {
                        tool_call_id: tool_call_id.clone().into(),
                        tool_name: tool_name.clone(),
                        message: message.clone(),
                        suggestions: suggestions.clone(),
                        mode,
                        remember_options: remember_options.clone(),
                        default_destination: *default_destination,
                    },
                ))
                .await?;

            let resolution = self
                .await_permission_resolution(cancel, tool_call_id, resolution_rx)
                .await;

            let resolved = match resolution {
                PendingPermissionResolution::Allow { updated_input, .. } => ToolRoundResult::Ok(
                    self.query_engine
                        .replay_tool_call_with_bus(
                            turn,
                            &self.event_bus,
                            original_request.clone(),
                            updated_input,
                        )
                        .await
                        .map_err(|err| {
                            anyhow::anyhow!("failed to replay approved tool call: {err}")
                        })?,
                ),
                PendingPermissionResolution::Deny { message, .. } => {
                    record_turn_diagnostic(
                        &std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from(".")),
                        "permission.resolve.completed",
                        turn.session_id(),
                        turn.run_id(),
                        Some(true),
                        None,
                        Some(serde_json::json!({
                            "toolCallId": tool_call_id,
                            "toolName": tool_name,
                            "resolution": "deny",
                        })),
                    );
                    let msg_id = format!("tool-{}", uuid::Uuid::new_v4());
                    self.event_bus
                        .emit(RuntimeEvent::new(
                            turn.session_id().clone(),
                            turn.run_id().clone(),
                            RuntimeEventKind::ToolCallCompleted {
                                tool_call_id: tool_call_id.clone().into(),
                                tool_name: tool_name.clone(),
                                is_error: true,
                                content: message.clone(),
                                msg_id: msg_id.clone(),
                                duration_ms: None,
                            },
                        ))
                        .await?;
                    ToolRoundResult::Ok(RuntimeToolCallOutcome::Completed {
                        tool_call_id: tool_call_id.clone(),
                        tool_name: tool_name.clone(),
                        content: message,
                        is_error: true,
                        msg_id,
                        file_meta: None,
                        is_degraded: false,
                        degradation_notice: None,
                        max_result_size_chars: 8_000,
                        context_modifier_message: None,
                    })
                }
                PendingPermissionResolution::Cancel { message } => {
                    record_turn_diagnostic(
                        &std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from(".")),
                        "permission.resolve.completed",
                        turn.session_id(),
                        turn.run_id(),
                        Some(true),
                        None,
                        Some(serde_json::json!({
                            "toolCallId": tool_call_id,
                            "toolName": tool_name,
                            "resolution": "cancel",
                        })),
                    );
                    let msg_id = format!("tool-{}", uuid::Uuid::new_v4());
                    self.event_bus
                        .emit(RuntimeEvent::new(
                            turn.session_id().clone(),
                            turn.run_id().clone(),
                            RuntimeEventKind::ToolCallCompleted {
                                tool_call_id: tool_call_id.clone().into(),
                                tool_name: tool_name.clone(),
                                is_error: true,
                                content: message.clone(),
                                msg_id: msg_id.clone(),
                                duration_ms: None,
                            },
                        ))
                        .await?;
                    ToolRoundResult::Ok(RuntimeToolCallOutcome::Completed {
                        tool_call_id: tool_call_id.clone(),
                        tool_name: tool_name.clone(),
                        content: message,
                        is_error: true,
                        msg_id,
                        file_meta: None,
                        is_degraded: false,
                        degradation_notice: None,
                        max_result_size_chars: 8_000,
                        context_modifier_message: None,
                    })
                }
            };

            resolved_results.push(resolved);
        }

        Ok(resolved_results)
    }

    async fn await_interaction_resolution(
        &self,
        cancel: &CancellationToken,
        interaction_id: &InteractionId,
        mut rx: tokio::sync::oneshot::Receiver<InteractionResolution>,
    ) -> InteractionResolution {
        loop {
            tokio::select! {
                resolution = &mut rx => {
                    return resolution.unwrap_or_else(|_| InteractionResolution::Cancel {
                        message: "Interaction request was closed before it was resolved.".to_string(),
                    });
                }
                _ = tokio::time::sleep(Duration::from_millis(50)) => {
                    if cancel.is_cancelled() {
                        if let Some(control_plane) = self.pending_interaction_control_plane.as_ref() {
                            let _ = control_plane.resolve(
                                interaction_id,
                                InteractionResolution::Cancel {
                                    message: "Interaction request cancelled because the turn was cancelled.".to_string(),
                                },
                            );
                        }
                    }
                }
            }
        }
    }

    async fn resolve_interaction_requests(
        &self,
        turn: &TurnState,
        cancel: &CancellationToken,
        round_results: Vec<ToolRoundResult>,
    ) -> Result<Vec<ToolRoundResult>> {
        let mut resolved_results = Vec::with_capacity(round_results.len());

        for round_result in round_results {
            let ToolRoundResult::Ok(RuntimeToolCallOutcome::InteractionRequired {
                tool_call_id,
                tool_name,
                original_request,
                interaction_request,
            }) = &round_result
            else {
                resolved_results.push(round_result);
                continue;
            };
            let Some(control_plane) = self.pending_interaction_control_plane.as_ref() else {
                return Err(anyhow::anyhow!(
                    "interaction control plane is required to handle InteractionRequired tool outcomes"
                ));
            };

            let resolution_rx = control_plane.insert_pending(interaction_request.clone())?;
            self.event_bus
                .emit(RuntimeEvent::new(
                    turn.session_id().clone(),
                    turn.run_id().clone(),
                    RuntimeEventKind::UserInteractionRequired {
                        interaction_id: interaction_request.interaction_id.clone(),
                        tool_call_id: interaction_request.tool_call_id.clone(),
                        tool_name: interaction_request.tool_name.clone(),
                        kind: interaction_request.kind.clone(),
                        payload: interaction_request.payload.clone(),
                    },
                ))
                .await?;

            let resolution = self
                .await_interaction_resolution(
                    cancel,
                    &interaction_request.interaction_id,
                    resolution_rx,
                )
                .await;

            let resolved = match resolution {
                InteractionResolution::Submit { value } => {
                    record_turn_diagnostic(
                        &std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from(".")),
                        "interaction.resolve.completed",
                        turn.session_id(),
                        turn.run_id(),
                        Some(true),
                        None,
                        Some(serde_json::json!({
                            "interactionId": interaction_request.interaction_id.as_str(),
                            "toolCallId": tool_call_id,
                            "toolName": tool_name,
                            "resolution": "submit",
                        })),
                    );
                    self.event_bus
                        .emit(RuntimeEvent::new(
                            turn.session_id().clone(),
                            turn.run_id().clone(),
                            RuntimeEventKind::UserInteractionResolved {
                                interaction_id: interaction_request.interaction_id.clone(),
                            },
                        ))
                        .await?;
                    ToolRoundResult::Ok(
                        self.query_engine
                            .replay_interaction_tool_call_with_bus(
                                turn,
                                &self.event_bus,
                                original_request.clone(),
                                value,
                            )
                            .await
                            .map_err(|err| {
                                anyhow::anyhow!("failed to replay interaction tool call: {err}")
                            })?,
                    )
                }
                InteractionResolution::Cancel { message } => {
                    record_turn_diagnostic(
                        &std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from(".")),
                        "interaction.resolve.completed",
                        turn.session_id(),
                        turn.run_id(),
                        Some(true),
                        None,
                        Some(serde_json::json!({
                            "interactionId": interaction_request.interaction_id.as_str(),
                            "toolCallId": tool_call_id,
                            "toolName": tool_name,
                            "resolution": "cancel",
                        })),
                    );
                    let msg_id = format!("tool-{}", uuid::Uuid::new_v4());
                    self.event_bus
                        .emit(RuntimeEvent::new(
                            turn.session_id().clone(),
                            turn.run_id().clone(),
                            RuntimeEventKind::ToolCallCompleted {
                                tool_call_id: tool_call_id.clone().into(),
                                tool_name: tool_name.clone(),
                                is_error: true,
                                content: message.clone(),
                                msg_id: msg_id.clone(),
                                duration_ms: None,
                            },
                        ))
                        .await?;
                    ToolRoundResult::Ok(RuntimeToolCallOutcome::Completed {
                        tool_call_id: tool_call_id.clone(),
                        tool_name: tool_name.clone(),
                        content: message,
                        is_error: true,
                        msg_id,
                        file_meta: None,
                        is_degraded: false,
                        degradation_notice: None,
                        max_result_size_chars: 8_000,
                        context_modifier_message: None,
                    })
                }
            };
            resolved_results.push(resolved);
        }

        Ok(resolved_results)
    }

    pub async fn run_chat_turn(
        &self,
        turn: &mut TurnState,
        request: &ChatTurnRequest,
    ) -> Result<()> {
        // S4 path: when an llm_executor is present, use the S4 driver loop.
        if let Some(ref executor) = self.llm_executor {
            return self
                .run_chat_turn_s4(turn, request, executor.as_ref())
                .await;
        }

        // Pure runtime mode: QueryEngine drives the full turn and emits
        // StreamDelta → MessagePersisted → StreamDone through the bus.
        // TauriEventAdapter translates these to the expected frontend events.
        self.query_engine.run(turn, &self.event_bus).await?;

        Ok(())
    }

    /// S4 core loop: driver owns the query loop when an `RuntimeLlmExecutor` is present.
    ///
    /// ## Flow
    /// 1. Build `TurnConfig` with defaults (executor reads settings/tool_registry internally).
    /// 2. Initialize `TurnIterationState`.
    /// 3. `executor.run_precompute` (currently a no-op extension hook).
    /// 4. Emit `StreamStarted`.
    /// 5. Iteration loop (up to `config.max_iterations`):
    ///    a. Build `LlmStepInput` (read-only view).
    ///    b. Call `executor.run_llm_step` → `LlmStepResult`.
    ///    c. On `ContentComplete` → merge content, break.
    ///    d. On `Cancelled` → mark cancelled, break.
    ///    e. On `ToolCalls` → `ToolRoundDriver::execute_round`, collect results, merge,
    ///       then run safeguard checks.
    ///    f. Check cancellation at end of each iteration.
    /// 6. `post_process::finalize_content`.
    /// 7. `executor.persist_assistant_message`.
    /// 8. Emit `MessagePersisted`, `StreamDone`, `AgentIdle`.
    async fn run_chat_turn_s4(
        &self,
        turn: &mut TurnState,
        request: &ChatTurnRequest,
        executor: &dyn RuntimeLlmExecutor,
    ) -> Result<()> {
        // ── Step 1: Build TurnConfig ──────────────────────────────────────────
        // Executor loads the turn-scoped LLM settings/tool defs/history from its
        // own services; the driver snapshots those results into immutable config
        // so repeated iterations do not re-read DB state.

        let llm_settings = executor
            .load_llm_settings_for_turn(request)
            .await
            .map_err(|e| anyhow::anyhow!("{}", e))?;

        // Build the tool definitions via the executor.
        let tool_defs = executor
            .get_tool_defs()
            .await
            .map_err(|e| anyhow::anyhow!("{}", e))?;
        let workspace_path = executor
            .load_workspace_path()
            .await
            .map_err(|e| anyhow::anyhow!("{}", e))?;
        let overrides = executor
            .load_turn_config_overrides(request)
            .await
            .map_err(|e| anyhow::anyhow!("{}", e))?;

        let prompt_snapshot = match executor
            .build_prompt_snapshot(request.conversation_id.as_str())
            .await
            .map_err(|e| anyhow::anyhow!("{}", e))?
        {
            Some(snapshot) => snapshot,
            None => {
                let system_prompt = executor
                    .build_system_prompt(request.conversation_id.as_str())
                    .await
                    .map_err(|e| anyhow::anyhow!("{}", e))?;
                single_dynamic_prompt_snapshot("legacy_system_prompt", system_prompt)
            }
        };
        let system_prompt = prompt_snapshot.compat_system_prompt();
        let effective_system_prompt = overrides
            .system_prompt
            .clone()
            .unwrap_or_else(|| system_prompt.clone());
        let effective_prompt_snapshot = if overrides.system_prompt.is_some() {
            single_dynamic_prompt_snapshot(
                "override_system_prompt",
                effective_system_prompt.clone(),
            )
        } else {
            prompt_snapshot
        };

        let config = TurnConfig {
            system_prompt: effective_system_prompt,
            prompt_snapshot: Some(effective_prompt_snapshot),
            tool_defs: overrides.tool_defs.unwrap_or(tool_defs),
            allowed_tools: overrides.allowed_tools,
            max_iterations: overrides.max_iterations.unwrap_or(30),
            token_budget: overrides.token_budget.unwrap_or(100000),
            chunk_timeout_secs: 90,
            masking_level: llm_settings.masking_level.clone(),
            workspace_path: workspace_path.clone(),
            llm_settings,
            conversation_id: request.conversation_id.clone(),
            run_id: request.run_id.clone(),
            hook_registry: request.hook_registry.clone(),
        };
        if let Some(snapshot) = &config.prompt_snapshot {
            let diagnostics =
                crate::runtime::chat::prompt::PromptDiagnostics::from_assembly(snapshot.assembly());
            record_turn_diagnostic(
                &config.workspace_path,
                "turn.prompt.loaded",
                turn.session_id(),
                turn.run_id(),
                Some(true),
                None,
                serde_json::to_value(diagnostics).ok(),
            );
        }

        record_turn_diagnostic(
            &config.workspace_path,
            "turn.config.loaded",
            turn.session_id(),
            turn.run_id(),
            Some(true),
            None,
            Some(serde_json::json!({
                "maxIterations": config.max_iterations,
                "tokenBudget": config.token_budget,
            })),
        );

        // ── Step 2: Initialize iteration state ───────────────────────────────
        // messages 顺序：[system-reminder, renlijia-md-meta?, ...history, current-user-content]
        let now = chrono::Local::now();
        let today = now.format("%Y年%m月%d日").to_string();
        let today_iso = now.format("%Y-%m-%d").to_string();
        let system_reminder_message =
            crate::runtime::chat::prompt::ReminderBuilder::date_message(&today, &today_iso);
        let renlijia_md_files = executor
            .load_renlijia_md(&config.workspace_path)
            .await
            .unwrap_or_else(|e| {
                log::warn!(
                    "[run_chat_turn_s4] load_renlijia_md failed for workspace '{}': {}",
                    config.workspace_path.display(),
                    e
                );
                Vec::new()
            });
        let renlijia_md_context_message = build_renlijia_md_context_message(&renlijia_md_files);

        // Sentinel content sent by the frontend when a background sub-agent
        // completes — we want to wake the parent up so it drains pending
        // task-notifications, but we must NOT persist or surface a fake user
        // turn. The drain step below will inject the actual notification XML
        // as the user-role message that the LLM responds to.
        let is_resume_for_task_notification =
            request.content == "__resume_from_task_notification__"
                && request.attachments.is_empty();

        let llm_user_content = if is_resume_for_task_notification {
            String::new()
        } else {
            executor
                .build_user_message_content(
                    request.conversation_id.as_str(),
                    &request.content,
                    &request.attachments,
                )
                .await
                .map_err(|e| anyhow::anyhow!("{}", e))?
        };

        // 加载历史对话；失败时直接返回错误，避免静默丢失上下文。
        let history = executor
            .load_history(request.conversation_id.as_str())
            .await
            .map_err(|e| anyhow::anyhow!("{}", e))?;
        record_turn_diagnostic(
            &workspace_path,
            "turn.history.loaded",
            turn.session_id(),
            turn.run_id(),
            Some(true),
            None,
            Some(serde_json::json!({
                "historyCount": history.len(),
            })),
        );

        let user_message = serde_json::json!({
            "role": "user",
            "content": llm_user_content,
        });

        let mut initial_messages = Vec::with_capacity(2 + history.len() + 1);
        initial_messages.push(system_reminder_message);
        if let Some(renlijia_md_context_message) = renlijia_md_context_message {
            initial_messages.push(renlijia_md_context_message);
        }
        initial_messages.extend(history);
        if !is_resume_for_task_notification {
            initial_messages.push(user_message);
        }
        // Inject pending <task-notification> messages AFTER the current user
        // message so that async sub-agent completions appear as the most recent
        // user input. If injected before user_message, the LLM tends to respond
        // to user_message and ignore the notifications.
        let mut pending_task_notifications = drain_and_inject_task_notifications(
            &self.task_notification_queue,
            turn.session_id(),
            &mut initial_messages,
        );

        // Guard: if this turn was triggered purely to resume from a task
        // notification but the queue is now empty (race: another turn drained
        // it first), there is no useful work to do — exit cleanly without
        // calling the LLM, which would otherwise see no user message and fail.
        if is_resume_for_task_notification && pending_task_notifications.is_empty() {
            log::info!(
                "[chat_turn_driver] resume-from-task-notification turn skipped: queue empty session={}",
                turn.session_id().as_str()
            );
            return Ok(());
        }

        let mut state = TurnIterationState::new(initial_messages);
        record_turn_diagnostic(
            &workspace_path,
            "turn.started",
            turn.session_id(),
            turn.run_id(),
            Some(true),
            None,
            Some(serde_json::json!({
                "conversationId": request.conversation_id.as_str(),
                "userMessageLength": request.content.len(),
            })),
        );

        // ── Step 2b: Persist user message (mirrors legacy_send_message_impl) ──
        // Legacy path wrote the user message to DB before spawning agent_loop.
        // The driver must do the same so the frontend message list is durable.
        // Skip when this turn was triggered by a task-notification resume:
        // there is no real user input to persist.
        let _user_msg_id = if is_resume_for_task_notification {
            String::new()
        } else {
            executor
                .persist_user_message(
                    request.conversation_id.as_str(),
                    &request.content,
                    &request.attachments,
                    request.client_message_id.as_deref(),
                )
                .await
                .map_err(|e| anyhow::anyhow!("{}", e))?
        };
        let pending_user_msg_id = _user_msg_id;
        let pending_client_msg_id = request.client_message_id.clone();

        // ── Step 3: Emit StreamStarted ────────────────────────────────────────
        let session_id = turn.session_id().clone();
        let run_id = turn.run_id().clone();
        self.event_bus
            .emit(RuntimeEvent::new(
                session_id.clone(),
                run_id.clone(),
                RuntimeEventKind::StreamStarted,
            ))
            .await?;
        let pending_user_content = build_user_content_json(
            &request.content,
            &request.attachments,
            request.selected_skill_id.as_deref(),
            request.selected_skill_label.as_deref(),
        );
        self.event_bus
            .emit(RuntimeEvent::new(
                session_id.clone(),
                run_id.clone(),
                RuntimeEventKind::MessagePersisted {
                    message_id: pending_user_msg_id.clone(),
                    role: "user".to_string(),
                    content: pending_user_content,
                    client_message_id: pending_client_msg_id.clone(),
                    tool_calls: None,
                },
            ))
            .await?;

        // Build the cancel token for this turn.
        let cancel = turn.cancellation();
        let mut turn_completed_normally = false;

        // ── Step 5: Iteration loop ────────────────────────────────────────────
        let round_driver = ToolRoundDriver::new(self.query_engine.clone())
            .with_allowed_tools_opt(stable_allowed_tools_vec(&config.allowed_tools));

        // 获取会话级环境信息（整个 turn 内稳定）
        let env_info = executor
            .get_env_info(request.conversation_id.as_str())
            .await
            .unwrap_or_else(|e| {
                log::warn!("[run_chat_turn_s4] get_env_info failed: {}", e);
                String::new()
            });
        let skill_catalog = executor.get_skill_catalog(None).await;
        let project_memory_ctx = executor
            .load_project_memory(&config.workspace_path, request.content.as_str())
            .await
            .unwrap_or_else(|e| {
                log::warn!("[run_chat_turn_s4] load_project_memory failed: {}", e);
                crate::runtime::project_memory::ProjectMemoryContext::default()
            });
        let core_memory_str = if project_memory_ctx.is_empty() {
            executor
                .load_core_memory(request.conversation_id.as_str())
                .await
                .unwrap_or_else(|e| {
                    log::warn!("[run_chat_turn_s4] load_core_memory failed: {}", e);
                    String::new()
                })
        } else {
            String::new()
        };
        let project_memory_prompt = project_memory_ctx.render_for_prompt();

        'turn: for iteration in 0..config.max_iterations {
            let preprocess_config = PreprocessConfig::default();
            let conversation_id = config.conversation_id.as_str().to_string();
            let prepared = prepare_messages_for_llm(
                std::mem::take(&mut state.messages),
                conversation_id.as_str(),
                PreprocessTrigger::Normal,
                &preprocess_config,
                &mut state.compact_state,
                &mut state.preprocess_state,
                state.stop_hook_active,
                |messages| {
                    let conversation_id = conversation_id.clone();
                    async move {
                        executor
                            .compact_summary(conversation_id.as_str(), &messages)
                            .await
                    }
                },
            )
            .await
            .map_err(|err| anyhow::anyhow!("{}", err))?;
            state.messages = prepared.messages;
            // Drain any task notifications that completed since the previous drain and
            // inject them as synthetic user messages so the parent LLM sees the
            // completion event on this iteration.
            let newly_drained_notifications = drain_and_inject_task_notifications(
                &self.task_notification_queue,
                turn.session_id(),
                &mut state.messages,
            );
            // Accumulate this iteration's drained notifications; these have been injected
            // into state.messages but not yet seen by the LLM. Track them so we can
            // re-enqueue them if the step fails before the LLM consumes them.
            pending_task_notifications.extend(newly_drained_notifications);
            if let Some(boundary_record) = prepared.compact_boundary {
                if let Err(err) = executor.save_compact_boundary(boundary_record).await {
                    log::warn!(
                        "[run_chat_turn_s4] failed to persist compact boundary: {}",
                        err
                    );
                }
            }

            let iteration_delta_context = build_iteration_context(
                &core_memory_str,
                &project_memory_prompt,
                &env_info,
                "",
                "",
                None,
                None,
                &skill_catalog,
            );

            // Build the read-only executor input.
            let estimated_tokens = estimate_tokens_from_json(&state.messages);
            let context_window = context_window_for_provider(&config.llm_settings.primary_model);
            if (estimated_tokens as f64) > (context_window as f64 * CONTEXT_OVERFLOW_THRESHOLD) {
                log::warn!(
                    "[AD2] Context overflow risk: estimated {} tokens > {}% of {} window for provider {}",
                    estimated_tokens,
                    (CONTEXT_OVERFLOW_THRESHOLD * 100.0) as u32,
                    context_window,
                    config.llm_settings.primary_model
                );
            }

            let input = LlmStepInput {
                system_prompt: &config.system_prompt,
                openai_system_message: config
                    .prompt_snapshot
                    .as_ref()
                    .and_then(|snapshot| snapshot.openai_system_message()),
                dynamic_context: &iteration_delta_context,
                // Pass a clone of the current messages slice so executor cannot
                // mutate driver state.
                messages: state.messages.clone(),
                tool_defs: &config.tool_defs,
                token_budget: config.token_budget,
                chunk_timeout_secs: config.chunk_timeout_secs,
                masking_level: &config.masking_level,
                force_no_tools: state.force_no_tools,
                llm_settings: &config.llm_settings,
                conversation_id: config.conversation_id.as_str(),
                run_id: config.run_id.as_str(),
                estimated_tokens,
            };

            // CP-1: check cancellation before invoking provider.
            if cancel.is_cancelled() {
                re_enqueue_task_notifications(&self.task_notification_queue, std::mem::take(&mut pending_task_notifications));
                mark_turn_cancelled_with_synthetic_results(&mut state, cancel.reason());
                break 'turn;
            }

            // ── Step 5b: single LLM step ─────────────────────────────────────
            let step_result = match executor
                .run_llm_step(&input, &self.event_bus, &cancel)
                .await
            {
                Ok(result) => result,
                Err(TurnError::PromptTooLong(message)) => {
                    let prepared = prepare_messages_for_llm(
                        std::mem::take(&mut state.messages),
                        conversation_id.as_str(),
                        PreprocessTrigger::PromptTooLongRecovery,
                        &PreprocessConfig::default(),
                        &mut state.compact_state,
                        &mut state.preprocess_state,
                        state.stop_hook_active,
                        |messages| {
                            let conversation_id = conversation_id.clone();
                            async move {
                                executor
                                    .compact_summary(conversation_id.as_str(), &messages)
                                    .await
                            }
                        },
                    )
                    .await
                    .map_err(|err| anyhow::anyhow!("{}", err))?;
                    state.messages = prepared.messages;
                    if let Some(boundary_record) = prepared.compact_boundary {
                        if let Err(err) = executor.save_compact_boundary(boundary_record).await {
                            log::warn!(
                                "[run_chat_turn_s4] failed to persist compact boundary: {}",
                                err
                            );
                        }
                    }
                    if prepared.retry == PreprocessRetryAction::RetryTurn {
                        // Re-enqueue so the notifications are tried again on retry.
                        re_enqueue_task_notifications(&self.task_notification_queue, std::mem::take(&mut pending_task_notifications));
                        continue 'turn;
                    }

                    re_enqueue_task_notifications(&self.task_notification_queue, std::mem::take(&mut pending_task_notifications));
                    self.event_bus
                        .emit(RuntimeEvent::new(
                            session_id.clone(),
                            run_id.clone(),
                            RuntimeEventKind::StreamError {
                                error: message.clone(),
                                raw_error: Some("prompt_too_long".to_string()),
                            },
                        ))
                        .await?;
                    inject_synthetic_tool_results_for_missing_calls(
                        &mut state.messages,
                        cancel.reason(),
                    );
                    return Err(anyhow::anyhow!(message));
                }
                Err(err) => {
                    re_enqueue_task_notifications(&self.task_notification_queue, std::mem::take(&mut pending_task_notifications));
                    inject_synthetic_tool_results_for_missing_calls(
                        &mut state.messages,
                        cancel.reason(),
                    );
                    return Err(anyhow::anyhow!("{}", err));
                }
            };

            match step_result {
                // ── 5c: pure content response — done ─────────────────────────
                LlmStepResult::ContentComplete {
                    content,
                    tokens_in,
                    tokens_out,
                    stop_reason,
                } => {
                    state.full_content.push_str(&content);
                    state.step_tokens_in += tokens_in;
                    state.step_tokens_out += tokens_out;
                    state.iteration_count = iteration + 1;

                    if stop_reason.as_deref() == Some("max_tokens") {
                        if state.max_output_tokens_recovery_count < MAX_OUTPUT_TOKENS_RECOVERY_LIMIT
                        {
                            state.max_output_tokens_recovery_count += 1;
                            state.messages.push(serde_json::json!({
                                "role": "assistant",
                                "content": content,
                            }));
                            state.messages.push(serde_json::json!({
                                "role": "user",
                                "content": "Output token limit hit. Resume directly — no apology, no recap of what you were doing. Pick up mid-thought if that is where the cut happened. Break remaining work into smaller pieces.",
                                "isMeta": true,
                            }));
                            // LLM consumed this iteration's notifications; clear before retry.
                            pending_task_notifications.clear();
                            continue 'turn;
                        }

                        state.full_content.push_str(
                            "

[输出 token 上限已达到，系统已停止自动续写；以上为当前已生成内容。]",
                        );
                        turn_completed_normally = true;
                        break 'turn;
                    }

                    if let Some(registry) = config.hook_registry.as_ref() {
                        if !state.stop_hook_active {
                            let stop_hooks = registry.hooks_for(HookEvent::Stop, "__stop__");
                            if !stop_hooks.is_empty() {
                                let stop_input = serde_json::json!({
                                    "stop_reason": if state.stream_cancelled { "cancelled" } else { "content_complete" },
                                    "content": &state.full_content,
                                });
                                let runner = HookRunner::new();
                                if let Ok(outcome) =
                                    runner.run_hooks(&stop_hooks, "__stop__", &stop_input).await
                                {
                                    state.stop_hook_prevent_continuation =
                                        outcome.prevent_continuation;
                                    state.stop_hook_reason = outcome.stop_reason;
                                    if !outcome.blocking_errors.is_empty() {
                                        state.stop_hook_active = true;
                                        state.max_output_tokens_recovery_count = 0;
                                        for error_msg in outcome.blocking_errors {
                                            state.messages.push(serde_json::json!({
                                                "role": "user",
                                                "content": error_msg,
                                                "isMeta": true,
                                            }));
                                        }
                                        // LLM consumed this iteration's notifications; clear before retry.
                                        pending_task_notifications.clear();
                                        continue 'turn;
                                    }
                                    if state.stop_hook_prevent_continuation {
                                        turn_completed_normally = true;
                                        break 'turn;
                                    }
                                }
                            }
                        }
                    }

                    turn_completed_normally = true;
                    break 'turn;
                }

                // ── 5d: user / token cancellation ────────────────────────────
                LlmStepResult::Cancelled => {
                    re_enqueue_task_notifications(&self.task_notification_queue, std::mem::take(&mut pending_task_notifications));
                    mark_turn_cancelled_with_synthetic_results(&mut state, cancel.reason());
                    break 'turn;
                }

                // ── 5e: tool calls ────────────────────────────────────────────
                LlmStepResult::ToolCalls {
                    assistant_content,
                    tool_calls,
                    tokens_in,
                    tokens_out,
                } => {
                    if !assistant_content.is_empty() {
                        state.full_content.push_str(&assistant_content);
                    }
                    let normalized_tool_calls: Vec<serde_json::Value> = tool_calls
                        .iter()
                        .map(|call| {
                            serde_json::json!({
                                "id": call.tool_call_id,
                                "name": call.tool_name,
                                "arguments": call.args,
                            })
                        })
                        .collect();
                    state.all_tool_calls.extend(normalized_tool_calls.clone());
                    let assistant_history_message = serde_json::json!({
                        "role": "assistant",
                        "content": assistant_content,
                        "toolCalls": normalized_tool_calls,
                    });
                    state.step_tokens_in += tokens_in;
                    state.step_tokens_out += tokens_out;
                    state.iteration_count = iteration + 1;

                    // Execute the tool round.
                    let round_results = round_driver
                        .execute_round(turn, &self.event_bus, tool_calls)
                        .await;

                    // CP-2: check cancellation right after execute_round.
                    if cancel.is_cancelled() {
                        state.append_messages_batch(vec![assistant_history_message.clone()]);
                        re_enqueue_task_notifications(
                            &self.task_notification_queue,
                            std::mem::take(&mut pending_task_notifications),
                        );
                        mark_turn_cancelled_with_synthetic_results(&mut state, cancel.reason());
                        break 'turn;
                    }

                    let round_results = self
                        .resolve_permission_asks(turn, &cancel, round_results)
                        .await?;
                    let round_results = self
                        .resolve_interaction_requests(turn, &cancel, round_results)
                        .await?;

                    // Collect and merge results into state.
                    let results = tool_result_collector::collect_results(round_results);
                    let mut history_batch = Vec::with_capacity(
                        1 + results.tool_result_messages.len()
                            + results.context_modifier_messages.len(),
                    );
                    history_batch.push(assistant_history_message);

                    let tool_msgs_for_persist: Vec<serde_json::Value> =
                        results.tool_result_messages.clone();

                    for msg in results.tool_result_messages {
                        history_batch.push(msg);
                        // CP-3: check cancellation after each staged tool result.
                        if cancel.is_cancelled() {
                            state.append_messages_batch(history_batch);
                            re_enqueue_task_notifications(
                                &self.task_notification_queue,
                                std::mem::take(&mut pending_task_notifications),
                            );
                            mark_turn_cancelled_with_synthetic_results(&mut state, cancel.reason());
                            break 'turn;
                        }
                    }
                    for msg in results.context_modifier_messages {
                        history_batch.push(msg);
                    }
                    state.append_messages_batch(history_batch);

                    // 先持久化本次 iteration 的 assistant[toolCalls]，再持久化 tool results，
                    // 保证存储顺序与执行顺序一致（assistant → tools 穿插）。
                    if !normalized_tool_calls.is_empty() {
                        match executor
                            .persist_iteration_assistant_message(
                                config.conversation_id.as_str(),
                                &normalized_tool_calls,
                            )
                            .await
                        {
                            Ok(Some(iter_msg_id)) => {
                                // 推送给前端，让流式 UI 立即拿到 toolCalls.arguments
                                // 而不必等到刷新会话。
                                if let Err(emit_err) = self
                                    .event_bus
                                    .emit(RuntimeEvent::new(
                                        session_id.clone(),
                                        run_id.clone(),
                                        RuntimeEventKind::MessagePersisted {
                                            message_id: iter_msg_id,
                                            role: "assistant".to_string(),
                                            content: serde_json::json!({ "text": "" }),
                                            client_message_id: None,
                                            tool_calls: Some(normalized_tool_calls.clone()),
                                        },
                                    ))
                                    .await
                                {
                                    log::warn!(
                                        "[chat_turn_driver] Failed to emit MessagePersisted for iteration assistant: {}",
                                        emit_err
                                    );
                                }
                            }
                            Ok(None) => {}
                            Err(e) => {
                                log::warn!("[chat_turn_driver] Failed to persist iteration assistant message: {}", e);
                            }
                        }
                    }
                    // 持久化本轮 tool 消息（忽略错误，不阻断流程）
                    if !tool_msgs_for_persist.is_empty() {
                        if let Err(e) = executor
                            .persist_tool_messages(
                                config.conversation_id.as_str(),
                                &tool_msgs_for_persist,
                            )
                            .await
                        {
                            log::warn!("[chat_turn_driver] Failed to persist tool messages: {}", e);
                        }
                    }
                    state.all_file_metas.extend(results.new_file_metas);
                    state
                        .generated_file_ids
                        .extend(results.new_generated_file_ids);

                    // Safeguard check.
                    match safeguard::check_iteration(
                        iteration,
                        config.max_iterations,
                        &state.full_content,
                    ) {
                        SafeguardAction::Continue => {}
                        SafeguardAction::InjectPromptAndContinue(msg) => {
                            state.messages.push(serde_json::json!({
                                "role": "user",
                                "content": msg,
                            }));
                        }
                    }
                }
            }

            // LLM step returned successfully and consumed this iteration's injected
            // task notifications. Do not re-enqueue them on later iteration failures.
            pending_task_notifications.clear();

            // ── 5f: per-iteration cancel check ───────────────────────────────
            if cancel.is_cancelled() {
                mark_turn_cancelled_with_synthetic_results(&mut state, cancel.reason());
                break 'turn;
            }

            if state.compact_state.compacted {
                state.compact_state.increment_turn();
            }
        }

        // ── Step 6: Post-process content ──────────────────────────────────────
        post_process::finalize_content(
            &mut state.full_content,
            state.iteration_count,
            config.max_iterations,
            state.stream_cancelled,
        );

        self.query_engine
            .accumulate_usage(state.step_tokens_in, state.step_tokens_out);

        let final_outcome = if state.stream_cancelled {
            ChatTurnOutcome::Cancelled
        } else if self.query_engine.is_budget_exceeded() {
            ChatTurnOutcome::BudgetExceeded {
                reason: format!(
                    "Reached maximum budget (${:.2}); estimated cost: ${:.4}",
                    self.query_engine.max_budget_usd().unwrap_or(0.0),
                    self.query_engine.estimated_cost_usd()
                ),
                total_cost_usd: self.query_engine.estimated_cost_usd(),
            }
        } else if !turn_completed_normally {
            ChatTurnOutcome::MaxIterationsReached {
                iterations: config.max_iterations,
            }
        } else {
            ChatTurnOutcome::Success
        };
        match &final_outcome {
            ChatTurnOutcome::Cancelled => record_turn_diagnostic(
                &config.workspace_path,
                "turn.cancelled",
                &session_id,
                &run_id,
                Some(true),
                None,
                None,
            ),
            ChatTurnOutcome::Success => record_turn_diagnostic(
                &config.workspace_path,
                "turn.completed",
                &session_id,
                &run_id,
                Some(true),
                None,
                None,
            ),
            other => record_turn_diagnostic(
                &config.workspace_path,
                "turn.completed",
                &session_id,
                &run_id,
                Some(true),
                None,
                Some(serde_json::json!({
                    "outcome": format!("{:?}", other),
                })),
            ),
        }

        // ── Step 7: Persist assistant message ─────────────────────────────────
        // tool_calls 已在每次 iteration 通过 persist_iteration_assistant_message 存入，
        // 这里只需存最终文字总结和 generatedFiles。
        let message_id = executor
            .persist_assistant_message(
                config.conversation_id.as_str(),
                &state.full_content,
                &[],
                &state.generated_file_ids,
                &state.all_file_metas,
            )
            .await
            .map_err(|e| anyhow::anyhow!("{}", e))?;

        if let Some(control_plane) = self.pending_permission_control_plane.as_ref() {
            let orphaned_count = control_plane.pending_count_for_session(&session_id);
            if orphaned_count > 0 {
                state.orphaned_permission_count = orphaned_count;
                control_plane.cancel_for_session(
                    &session_id,
                    "Permission request was not resolved before the turn ended (orphaned).",
                );
                self.event_bus
                    .emit(RuntimeEvent::new(
                        session_id.clone(),
                        run_id.clone(),
                        RuntimeEventKind::OrphanedPermissionDetected {
                            count: orphaned_count,
                        },
                    ))
                    .await?;
            }
        }

        // ── Step 8: Emit terminal events ──────────────────────────────────────
        // content must be a MessageContent object (matching the frontend Message type),
        // not a raw string.  The legacy finish_agent path always emitted {"text": "..."},
        // so we must do the same here or the frontend will discard the message.
        self.event_bus
            .emit(RuntimeEvent::message_persisted(
                session_id.clone(),
                run_id.clone(),
                message_id,
                "assistant",
                serde_json::json!({ "text": state.full_content }),
            ))
            .await?;
        self.event_bus
            .emit(RuntimeEvent::stream_done(
                session_id.clone(),
                run_id.clone(),
            ))
            .await?;
        self.event_bus
            .emit(RuntimeEvent::new(
                session_id.clone(),
                run_id.clone(),
                RuntimeEventKind::TurnCompleted {
                    outcome: final_outcome,
                    total_input_tokens: state.step_tokens_in,
                    total_output_tokens: state.step_tokens_out,
                    total_cost_usd: self
                        .query_engine
                        .max_budget_usd()
                        .map(|_| self.query_engine.estimated_cost_usd()),
                    permission_denial_count: self.query_engine.get_permission_denials().len(),
                },
            ))
            .await?;
        if state.stop_hook_prevent_continuation {
            self.event_bus
                .emit(RuntimeEvent::new(
                    session_id.clone(),
                    run_id.clone(),
                    RuntimeEventKind::StopHookPreventedContinuation {
                        reason: state.stop_hook_reason.clone(),
                    },
                ))
                .await?;
        }
        self.event_bus
            .emit(RuntimeEvent::new(
                session_id,
                run_id.clone(),
                RuntimeEventKind::AgentIdle {
                    agent_id: AgentId::new(format!("agent-{}", run_id.as_str())),
                    scope: AgentIdleScope::Primary,
                },
            ))
            .await?;

        Ok(())
    }
}

fn single_dynamic_prompt_snapshot(
    section_id: &str,
    text: impl Into<String>,
) -> crate::runtime::chat::prompt::TurnPromptSnapshot {
    crate::runtime::chat::prompt::TurnPromptSnapshot::new(
        crate::runtime::chat::prompt::PromptAssembly::new(vec![
            crate::runtime::chat::prompt::PromptBlock::dynamic_block(
                crate::runtime::chat::prompt::PromptSectionId::new(section_id),
                text,
            ),
        ]),
        Vec::new(),
    )
}

fn stable_allowed_tools_vec(allowed_tools: &Option<HashSet<String>>) -> Option<Vec<String>> {
    allowed_tools.as_ref().map(|allowed| {
        let mut names: Vec<String> = allowed.iter().cloned().collect();
        names.sort();
        names
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::chat::tool_round_driver::ToolRoundResult;
    use crate::runtime::chat::tool_round_types::{RuntimeToolCallOutcome, RuntimeToolCallRequest};
    use crate::runtime::events::RuntimeEventKind;
    use crate::runtime::identity::IdentityMapping;
    use crate::runtime::ids::{RunId, ToolCallId};
    use crate::runtime::store::{
        PendingPermissionControlPlane, PendingPermissionRequest, PendingPermissionResolution,
    };
    use crate::runtime::tools::permission::{PermissionDecision, PermissionMode, PermissionReason};
    use async_trait::async_trait;
    use serde_json::json;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Mutex;
    use tokio::sync::oneshot;

    struct RecordingPermissionControlPlane {
        inserted: Mutex<Vec<PendingPermissionRequest>>,
        resolution: PendingPermissionResolution,
    }

    impl RecordingPermissionControlPlane {
        fn new(resolution: PendingPermissionResolution) -> Self {
            Self {
                inserted: Mutex::new(Vec::new()),
                resolution,
            }
        }

        fn inserted_requests(&self) -> Vec<PendingPermissionRequest> {
            self.inserted.lock().unwrap().clone()
        }
    }

    impl PendingPermissionControlPlane for RecordingPermissionControlPlane {
        fn insert_pending_request(
            &self,
            request: PendingPermissionRequest,
        ) -> anyhow::Result<oneshot::Receiver<PendingPermissionResolution>> {
            self.inserted.lock().unwrap().push(request);
            let (tx, rx) = oneshot::channel();
            let _ = tx.send(self.resolution.clone());
            Ok(rx)
        }

        fn resolve_pending_request(
            &self,
            _tool_call_id: &ToolCallId,
            _resolution: PendingPermissionResolution,
        ) -> anyhow::Result<()> {
            Ok(())
        }

        fn cancel_for_session(
            &self,
            _session_id: &crate::runtime::ids::SessionId,
            _message: &str,
        ) -> usize {
            0
        }

        fn pending_count_for_session(&self, _session_id: &crate::runtime::ids::SessionId) -> usize {
            0
        }
    }

    #[test]
    fn chat_turn_request_new_defaults_permission_mode() {
        let request = ChatTurnRequest::new("conv-chat-mode", "hello", vec![]);

        assert_eq!(request.permission_mode, PermissionMode::Default);
        assert_eq!(request.selected_skill_id, None);
        assert_eq!(request.selected_skill_label, None);
    }

    #[test]
    fn build_user_content_json_includes_structured_attachments() {
        let content = build_user_content_json(
            "看下附件",
            &[ChatAttachmentRef {
                id: "attachment-1".to_string(),
                file_name: "report.csv".to_string(),
                file_path: "/tmp/report.csv".to_string(),
                kind: "file".to_string(),
                file_size: 0,
                file_type: "csv".to_string(),
                mime_type: Some("text/csv".to_string()),
            }],
            None,
            None,
        );

        assert_eq!(
            content
                .get("files")
                .and_then(|value| value.as_array())
                .and_then(|items| items.first())
                .and_then(|value| value.get("fileName"))
                .and_then(|value| value.as_str()),
            Some("report.csv")
        );
        assert_eq!(
            content
                .get("files")
                .and_then(|value| value.as_array())
                .and_then(|items| items.first())
                .and_then(|value| value.get("filePath"))
                .and_then(|value| value.as_str()),
            Some("/tmp/report.csv")
        );
    }

    #[test]
    fn build_user_content_json_includes_selected_skill_metadata() {
        let content =
            build_user_content_json("用这个技能吧", &[], Some("salary-query"), Some("薪资查询"));

        assert_eq!(
            content.get("text").and_then(|value| value.as_str()),
            Some("用这个技能吧")
        );
        assert_eq!(
            content
                .get("skillCommand")
                .and_then(|value| value.get("id"))
                .and_then(|value| value.as_str()),
            Some("salary-query")
        );
        assert_eq!(
            content
                .get("skillCommand")
                .and_then(|value| value.get("command"))
                .and_then(|value| value.as_str()),
            Some("/salary-query")
        );
        assert_eq!(
            content
                .get("skillCommand")
                .and_then(|value| value.get("label"))
                .and_then(|value| value.as_str()),
            Some("薪资查询")
        );
    }

    #[tokio::test]
    async fn resolve_permission_asks_uses_runtime_permission_mode_instead_of_decision_reason() {
        let bus = RuntimeEventBus::new();
        let control_plane = Arc::new(RecordingPermissionControlPlane::new(
            PendingPermissionResolution::Deny {
                message: "Denied by test".to_string(),
                remember: false,
                destination: None,
            },
        ));
        let driver = RuntimeChatTurnDriver {
            query_engine: QueryEngine::new(),
            event_bus: bus.clone(),
            llm_executor: None,
            pending_permission_control_plane: Some(control_plane.clone()),
            pending_interaction_control_plane: None,
            task_notification_queue: None,
        };
        let turn = TurnState::new(
            IdentityMapping::from_legacy_conversation_id("conv-ask-mode".to_string()),
            RunId::new("run-ask-mode"),
            "hello".to_string(),
        )
        .with_permission_mode(PermissionMode::Plan);
        let round_results = vec![ToolRoundResult::Ok(RuntimeToolCallOutcome::AskRequired {
            tool_call_id: "tc-ask-mode".to_string(),
            tool_name: "danger_tool".to_string(),
            capability_scopes: vec!["cap:danger".to_string()],
            original_request: RuntimeToolCallRequest {
                tool_call_id: "tc-ask-mode".to_string(),
                tool_name: "danger_tool".to_string(),
                args: json!({"path":"/tmp"}),
                purpose: Some("test ask".to_string()),
            },
            decision: PermissionDecision::Ask {
                message: "need approval".to_string(),
                suggestions: vec!["Allow once".to_string()],
                remember_options: vec![],
                default_destination: None,
                reason: PermissionReason::Other("not-mode".to_string()),
            },
        })];

        let _resolved = driver
            .resolve_permission_asks(&turn, &turn.cancellation(), round_results)
            .await
            .expect("ask resolution should succeed");

        let inserted = control_plane.inserted_requests();
        assert_eq!(inserted.len(), 1);
        assert_eq!(inserted[0].mode, PermissionMode::Plan);

        let ask_mode = bus
            .recorded()
            .into_iter()
            .find_map(|event| match event.kind {
                RuntimeEventKind::PermissionAskRequired { mode, .. } => Some(mode),
                _ => None,
            })
            .expect("permission ask event should be recorded");
        assert_eq!(ask_mode, PermissionMode::Plan);
    }

    #[test]
    fn cancel_finalizer_injects_missing_tool_results() {
        let mut state = TurnIterationState::new(vec![json!({
            "role": "assistant",
            "content": "",
            "toolCalls": [
                {"id": "tc-b3-cp1", "name": "unknown_tool", "arguments": {}}
            ]
        })]);

        mark_turn_cancelled_with_synthetic_results(&mut state, Some(CancellationReason::Interrupt));

        assert!(
            state.stream_cancelled,
            "cancel finalizer must mark stream_cancelled"
        );
        let synthetic = state
            .messages
            .iter()
            .find(|msg| {
                msg.get("role").and_then(|v| v.as_str()) == Some("tool")
                    && msg.get("toolCallId").and_then(|v| v.as_str()) == Some("tc-b3-cp1")
            })
            .expect("cancel finalizer should inject missing synthetic tool result");
        let content = synthetic
            .get("content")
            .and_then(|v| v.as_str())
            .unwrap_or_default();
        assert!(
            content.contains("interrupted before completion"),
            "synthetic tool result should reflect the cancel reason"
        );
    }

    #[test]
    fn cancel_finalizer_defaults_missing_reason_to_generic_interrupt() {
        let mut state = TurnIterationState::new(vec![json!({
            "role": "assistant",
            "content": "",
            "toolCalls": [
                {"id": "tc-b6-none", "name": "unknown_tool", "arguments": {}}
            ]
        })]);

        mark_turn_cancelled_with_synthetic_results(&mut state, None);

        let synthetic = state
            .messages
            .iter()
            .find(|msg| {
                msg.get("role").and_then(|v| v.as_str()) == Some("tool")
                    && msg.get("toolCallId").and_then(|v| v.as_str()) == Some("tc-b6-none")
            })
            .expect("cancel finalizer should inject missing synthetic tool result");
        let content = synthetic
            .get("content")
            .and_then(|v| v.as_str())
            .unwrap_or_default();

        assert!(
            content.contains("interrupted before completion"),
            "missing cancel reason should fall back to generic interrupt wording"
        );
    }

    struct SnapshotPromptExecutor {
        legacy_calls: AtomicUsize,
        seen_system_prompts: Mutex<Vec<String>>,
        seen_dynamic_contexts: Mutex<Vec<String>>,
        skill_catalog: Option<String>,
    }

    impl SnapshotPromptExecutor {
        fn new() -> Self {
            Self {
                legacy_calls: AtomicUsize::new(0),
                seen_system_prompts: Mutex::new(Vec::new()),
                seen_dynamic_contexts: Mutex::new(Vec::new()),
                skill_catalog: None,
            }
        }

        fn with_skill_catalog(skill_catalog: impl Into<String>) -> Self {
            Self {
                legacy_calls: AtomicUsize::new(0),
                seen_system_prompts: Mutex::new(Vec::new()),
                seen_dynamic_contexts: Mutex::new(Vec::new()),
                skill_catalog: Some(skill_catalog.into()),
            }
        }
    }

    #[async_trait]
    impl RuntimeLlmExecutor for SnapshotPromptExecutor {
        async fn run_llm_step(
            &self,
            input: &LlmStepInput<'_>,
            _bus: &RuntimeEventBus,
            _cancel: &CancellationToken,
        ) -> Result<LlmStepResult, TurnError> {
            self.seen_system_prompts
                .lock()
                .unwrap()
                .push(input.system_prompt.to_string());
            self.seen_dynamic_contexts
                .lock()
                .unwrap()
                .push(input.dynamic_context.to_string());
            Ok(LlmStepResult::ContentComplete {
                content: "snapshot done".to_string(),
                tokens_in: 0,
                tokens_out: 0,
                stop_reason: Some("end_turn".to_string()),
            })
        }

        async fn persist_assistant_message(
            &self,
            _conversation_id: &str,
            _content: &str,
            _tool_calls: &[serde_json::Value],
            _generated_file_ids: &[String],
            _file_metas: &[serde_json::Value],
        ) -> Result<String, TurnError> {
            Ok("assistant-msg".to_string())
        }

        async fn persist_user_message(
            &self,
            _conversation_id: &str,
            _content: &str,
            _attachments: &[ChatAttachmentRef],
            _client_message_id: Option<&str>,
        ) -> Result<String, TurnError> {
            Ok("user-msg".to_string())
        }

        async fn build_prompt_snapshot(
            &self,
            _conversation_id: &str,
        ) -> Result<Option<crate::runtime::chat::prompt::TurnPromptSnapshot>, TurnError> {
            Ok(Some(crate::runtime::chat::prompt::TurnPromptSnapshot::new(
                crate::runtime::chat::prompt::PromptAssembly::new(vec![
                    crate::runtime::chat::prompt::PromptBlock::static_block(
                        crate::runtime::chat::prompt::PromptSectionId::new("snapshot_static"),
                        "snapshot static",
                    ),
                    crate::runtime::chat::prompt::PromptBlock::dynamic_block(
                        crate::runtime::chat::prompt::PromptSectionId::new("snapshot_dynamic"),
                        "snapshot dynamic",
                    ),
                ]),
                Vec::new(),
            )))
        }

        async fn build_system_prompt(&self, _conversation_id: &str) -> Result<String, TurnError> {
            self.legacy_calls.fetch_add(1, Ordering::SeqCst);
            Ok("legacy prompt should not be used".to_string())
        }

        async fn load_workspace_path(&self) -> Result<PathBuf, TurnError> {
            Ok(std::env::temp_dir())
        }

        async fn get_skill_catalog(&self, _agent_id: Option<&str>) -> String {
            self.skill_catalog.clone().unwrap_or_default()
        }
    }

    #[tokio::test]
    async fn driver_prefers_prompt_snapshot_over_legacy_system_prompt() {
        let executor = Arc::new(SnapshotPromptExecutor::new());
        let bus = RuntimeEventBus::new();
        let driver =
            RuntimeChatTurnDriver::with_llm_executor(QueryEngine::new(), bus, executor.clone());
        let mut turn = TurnState::new(
            IdentityMapping::from_legacy_conversation_id("conv-driver-snapshot".to_string()),
            RunId::new("run-driver-snapshot"),
            "use snapshot".to_string(),
        );
        let request = ChatTurnRequest::new("conv-driver-snapshot", "use snapshot", vec![]);

        driver
            .run_chat_turn(&mut turn, &request)
            .await
            .expect("driver should run with prompt snapshot");

        let expected_snapshot_prompt = "snapshot static\n\nsnapshot dynamic".to_string();
        let prompts = executor.seen_system_prompts.lock().unwrap().clone();
        assert_eq!(prompts, vec![expected_snapshot_prompt]);
        assert_eq!(executor.legacy_calls.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn driver_injects_skill_catalog_into_dynamic_context() {
        let executor = Arc::new(SnapshotPromptExecutor::with_skill_catalog(
            "## 可用专项技能\n- `biz-writing` — 商务写作",
        ));
        let bus = RuntimeEventBus::new();
        let driver =
            RuntimeChatTurnDriver::with_llm_executor(QueryEngine::new(), bus, executor.clone());
        let mut turn = TurnState::new(
            IdentityMapping::from_legacy_conversation_id("conv-driver-skill-catalog".to_string()),
            RunId::new("run-driver-skill-catalog"),
            "write an email".to_string(),
        );
        let request = ChatTurnRequest::new("conv-driver-skill-catalog", "write an email", vec![]);

        driver
            .run_chat_turn(&mut turn, &request)
            .await
            .expect("driver should run with skill catalog");

        let dynamic_contexts = executor.seen_dynamic_contexts.lock().unwrap().clone();
        assert_eq!(dynamic_contexts.len(), 1);
        assert!(dynamic_contexts[0].contains("可用专项技能"));
        assert!(dynamic_contexts[0].contains("biz-writing"));
    }

    #[test]
    fn chat_turn_request_default_has_no_persona_override() {
        let req = ChatTurnRequest::new("conv-1".to_string(), "hello".to_string(), vec![]);
        assert!(req.persona_id_override.is_none());
    }

    #[test]
    fn chat_turn_request_with_persona_override() {
        let req = ChatTurnRequest::new("conv-1".to_string(), "hello".to_string(), vec![])
            .with_persona_id_override("persona-x".into());
        assert_eq!(req.persona_id_override.as_deref(), Some("persona-x"));
    }
}
