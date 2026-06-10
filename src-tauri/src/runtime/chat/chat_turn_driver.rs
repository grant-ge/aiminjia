use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;
use std::collections::HashSet;
use std::time::Duration;

use crate::llm::context_decay::{resolve_context_window, CONTEXT_OVERFLOW_THRESHOLD};
use crate::llm::streaming::SystemPromptSegment;
use crate::runtime::agent::task_notification::{QueuedNotification, TaskNotificationQueue};
use crate::runtime::cancellation::{CancellationReason, CancellationToken};
use crate::runtime::chat::compact_client::CompactSummaryClient;
use crate::runtime::chat::compaction::{
    append_literal_anchor_hints, append_transcript_path_hint,
    compact_transcript_path_for_conversation_dir, AutoCompactConfig, CompactTrigger,
};
use crate::runtime::chat::context_builder::build_iteration_context;
use crate::runtime::chat::multimodal::{
    build_anthropic_image_blocks, retain_text_fallback_attachments,
};
use crate::runtime::chat::post_process;
use crate::runtime::chat::preprocess::{
    prepare_messages_for_llm, PreprocessConfig, PreprocessRetryAction, PreprocessTrigger,
};
use crate::runtime::chat::safeguard::{self, SafeguardAction};
use crate::runtime::chat::tool_result_artifact::{
    apply_tool_result_artifact_replacements,
    build_tool_result_artifact_replacements_from_round_results,
};
use crate::runtime::chat::tool_result_collector;
use crate::runtime::chat::tool_round_driver::{ToolRoundDriver, ToolRoundResult};
use crate::runtime::chat::tool_round_types::{RuntimeToolCallOutcome, RuntimeToolCallRequest};
use crate::runtime::chat::turn_config::{
    LlmStepInput, LlmStepResult, ResolvedLlmSettings, TurnConfig, TurnConfigOverrides, TurnError,
    TurnIterationState, MAX_OUTPUT_TOKENS_RECOVERY_LIMIT,
};
use crate::runtime::chat::turn_outcome::ChatTurnOutcome;
use crate::runtime::chat::turn_stage::TurnStageEmitter;
use crate::runtime::event_bus::RuntimeEventBus;
use crate::runtime::events::{AgentIdleScope, RunningTool, RuntimeEvent, RuntimeEventKind};
use crate::runtime::hooks::config::{HookEvent, HookRegistry};
use crate::runtime::hooks::HookRunner;
use crate::runtime::human_interaction::{OutputBinding, TurnOrigin};
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

fn push_unique_system_segment(
    segments: &mut Vec<SystemPromptSegment>,
    segment: SystemPromptSegment,
) {
    if !segments
        .iter()
        .any(|existing| existing.text == segment.text && existing.cache == segment.cache)
    {
        segments.push(segment);
    }
}

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

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillCommandRef {
    pub id: String,
    pub label: Option<String>,
    pub command: Option<String>,
}

pub fn build_user_content_json_with_skill(
    content: &str,
    attachments: &[ChatAttachmentRef],
    skill_command: Option<&SkillCommandRef>,
) -> serde_json::Value {
    let mut value = serde_json::json!({ "text": content });
    if let Some(skill) = skill_command {
        let label = skill.label.as_deref().unwrap_or(skill.id.as_str());
        let command = skill
            .command
            .clone()
            .unwrap_or_else(|| format!("/{}", skill.id));
        value["commandText"] = serde_json::Value::String(command.clone());
        value["skillCommand"] = serde_json::json!({
            "id": skill.id,
            "label": label,
            "command": command,
        });
    }
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
    value
}

pub fn build_user_content_json(
    content: &str,
    attachments: &[ChatAttachmentRef],
) -> serde_json::Value {
    build_user_content_json_with_skill(content, attachments, None)
}

pub fn selected_skill_instruction(skill_command: Option<&SkillCommandRef>) -> Option<String> {
    let skill = skill_command?;
    let label = skill.label.as_deref().unwrap_or(skill.id.as_str());
    let command = skill.command.as_deref().unwrap_or("");
    Some(format!(
        "\n\n<system-reminder>\n本轮用户显式选择技能：id={id}, label={label}, command={command}\n请优先调用 Skill({{ skill_id: \"{id}\" }}) 加载该技能指令，然后按该技能要求处理用户请求。\n不要使用 label 作为 skill_id；后端只识别稳定 id `{id}`。\n</system-reminder>",
        id = skill.id,
        label = label,
        command = command,
    ))
}

pub const IM_MOBILE_CHANNEL_CONTEXT: &str = "当前请求来自 IM/移动端渠道。用户通常只能看到 IM 回复，看不到本机桌面弹出的浏览器或 127.0.0.1 回调页面。若后续技能或工具需要用户完成浏览器授权，请优先使用可在移动端访问的授权方式，并把完整授权链接和必要验证码直接回复给用户；不要只说“浏览器已打开”。";

/// The chat turn request type.  Defined here to avoid circular imports between
/// `session_runtime` and `chat`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChatTurnRequest {
    pub conversation_id: SessionId,
    pub content: String,
    pub attachments: Vec<ChatAttachmentRef>,
    pub skill_command: Option<SkillCommandRef>,
    /// Optional per-turn channel context from transports such as IM connectors.
    /// This is injected into dynamic context only; it must not be persisted as
    /// user-visible message content.
    pub channel_context: Option<String>,
    pub turn_origin: TurnOrigin,
    pub output_binding: OutputBinding,
    pub agent_name: Option<String>,
    pub permission_mode: PermissionMode,
    /// The authoritative run_id for this turn.
    /// `ChatTurnRequest::new` generates a fresh id; transport code may reserve
    /// resources with it before handing the request to `SessionRuntime`.
    pub run_id: RunId,
    pub hook_registry: Option<Arc<HookRegistry>>,
    pub client_message_id: Option<String>,
    pub persona_id_override: Option<String>,
    /// Working directories derived from this turn's attachments at the transport
    /// layer (backend side only — frontend paths are untrusted).  These are merged
    /// into the per-turn `ToolPermissionContext.additional_working_dirs` with
    /// `RuleSource::Session` before tool execution.  Empty when there are no
    /// attachments or all attachment paths failed validation.
    pub session_attachment_dirs: Vec<std::path::PathBuf>,
    /// When set, this turn originated from a drained PendingQueue batch.
    /// The dispatcher could use this metadata to e.g. persist each item as an
    /// independent user message before invoking the LLM. Default: None.
    pub pending_batch: Option<Vec<crate::runtime::pending::PendingItem>>,
    /// When `true`, the caller already persisted the user message + emitted
    /// `MessagePersisted` before constructing the request. The driver must
    /// skip step 2b (persist) and the matching event emit, but must still
    /// include the message in `initial_messages` (the caller is responsible
    /// for that via DB load). Used by `dispatch_employee_run` so that an
    /// agent-spawn failure cannot leave a conversation with no user message
    /// on disk.
    pub pre_persisted: bool,
    /// Force-override the active team for this turn.
    ///
    /// Semantics (do not confuse with "value source" — this is binary):
    /// - `Some(name)`: this turn MUST run with `active_team_name = name`,
    ///   regardless of what `conv.json::active_team_name` says.
    /// - `None`:        this turn uses whatever `conv.json::active_team_name`
    ///   currently holds (or `None` for non-team conversations).
    ///
    /// The `_override` suffix is load-bearing: every call site that doesn't
    /// already know the team should leave this `None` and let
    /// `SessionRuntime::query_engine_for_session` resolve from `conv.json`.
    /// The only legitimate `Some` setter today is the Path C wake closure
    /// (`wire_path_c_wake_to_self`), which knows the originating team
    /// directly from the `LeadIdleSupervisor::enqueue` payload — and may
    /// race ahead of `conv.json` writes.  Per-team disk layout v2 §6.
    pub active_team_name_override: Option<String>,
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
            skill_command: None,
            channel_context: None,
            turn_origin: TurnOrigin::App,
            output_binding: OutputBinding::AppOnly,
            agent_name: None,
            permission_mode: PermissionMode::Default,
            run_id: RunId::new(uuid::Uuid::new_v4().to_string()),
            hook_registry: None,
            client_message_id: None,
            persona_id_override: None,
            session_attachment_dirs: Vec::new(),
            pending_batch: None,
            pre_persisted: false,
            active_team_name_override: None,
        }
    }

    pub fn with_persona_id_override(mut self, persona_id: String) -> Self {
        self.persona_id_override = Some(persona_id);
        self
    }

    /// Force-override the active team for this single turn.  See
    /// `active_team_name_override` field doc for when (and only when) this
    /// is the right thing to call — most call sites should NOT use it.
    pub fn with_active_team_name_override(mut self, team_name: String) -> Self {
        self.active_team_name_override = Some(team_name);
        self
    }
}

fn should_build_image_blocks_for_turn(
    llm_settings: &ResolvedLlmSettings,
    is_resume_for_task_notification: bool,
) -> bool {
    if is_resume_for_task_notification {
        return false;
    }

    if llm_settings.cloud_gateway_mode == crate::models::settings::CloudGatewayMode::V2 {
        return true;
    }

    crate::llm::vision_support::supports_lotus_anthropic_vision(&llm_settings.cloud_model)
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
    ///
    /// `error` 为 `Some` 时表示这是一条终止错误占位（emit_terminal_error_message_and_idle
    /// 路径用），生产 impl 必须把 error 字段一并写入 StoredMessage 才能让下次 reload
    /// 时前端拿到 error 渲染红色 callout，并让 history.rs 过滤这条不进 LLM context。
    async fn persist_assistant_message(
        &self,
        conversation_id: &str,
        content: &str,
        tool_calls: &[serde_json::Value],
        generated_file_ids: &[String],
        file_metas: &[serde_json::Value],
        thinking_blocks: &[serde_json::Value],
        error: Option<&crate::storage::file_store::types::MessageError>,
    ) -> Result<String, TurnError>;

    /// 持久化 user message 到存储。纯 I/O，不含事件发射。
    /// 默认 no-op（返回空 message_id）；生产 executor 必须 override。
    async fn persist_user_message(
        &self,
        _conversation_id: &str,
        _content: &str,
        _attachments: &[ChatAttachmentRef],
        _skill_command: Option<&SkillCommandRef>,
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

    /// 持久化单次 iteration 的 assistant 消息（文字内容 + toolCalls，无生成文件）。
    /// 在工具执行前调用，使存储顺序正确反映 assistant → tools 穿插结构。
    /// 返回新建消息的 id（无 tool_calls 时返回 None）。默认 no-op。
    async fn persist_iteration_assistant_message(
        &self,
        _conversation_id: &str,
        _assistant_content: &str,
        _tool_calls: &[serde_json::Value],
        _thinking_blocks: &[serde_json::Value],
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
    async fn build_system_prompt(&self, _request: &ChatTurnRequest) -> Result<String, TurnError> {
        Ok(String::new())
    }

    async fn build_prompt_snapshot(
        &self,
        _request: &ChatTurnRequest,
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
    /// 不再提供默认实现——所有 mock executor 必须显式 override，
    /// 否则会因为返回空 vec 让测试静默通过。
    async fn get_tool_defs(&self) -> Result<Vec<serde_json::Value>, TurnError>;

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

    fn conversation_dir(&self, _conversation_id: &str) -> Option<PathBuf> {
        None
    }

    /// Resolve optional per-turn overrides after transport/runtime state has
    /// been loaded for the request but before the driver snapshots TurnConfig.
    async fn load_turn_config_overrides(
        &self,
        _request: &ChatTurnRequest,
    ) -> Result<TurnConfigOverrides, TurnError> {
        Ok(TurnConfigOverrides::default())
    }

    /// 加载 AGENTS.md user-context 文件。
    async fn load_agents_md(
        &self,
        _authorized_workspace: Option<&crate::runtime::store::AuthorizedWorkspaceRef>,
    ) -> Result<Vec<crate::runtime::agents_md::AgentsMdFile>, TurnError> {
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

    /// Persist a compact boundary record after successful compaction.
    async fn save_compact_boundary(
        &self,
        _record: crate::runtime::chat::compaction::CompactBoundaryRecord,
    ) -> Result<(), TurnError> {
        Ok(())
    }

    /// Persist compact artifacts into the visible transcript.
    ///
    /// This mirrors claude-code-best's compact boundary message: the sidecar
    /// boundary record remains an index, while the transcript carries the
    /// `system/compact_boundary` and compact summary messages for reload/UI.
    async fn persist_compact_messages(
        &self,
        _conversation_id: &str,
        _messages: &[serde_json::Value],
    ) -> Result<(), TurnError> {
        Ok(())
    }

    /// Load the most recent compact boundary for a conversation, if any.
    ///
    /// Driver passes this into `PreprocessConfig.compact_boundary` so each
    /// preprocess pass can operate on the post-boundary message slice (R3.2
    /// "boundary view isolation").  Default is `None` for test executors that
    /// don't persist boundaries.
    async fn latest_compact_boundary(
        &self,
        _conversation_id: &str,
    ) -> Result<Option<crate::runtime::chat::compaction::CompactBoundaryRecord>, TurnError> {
        Ok(None)
    }
}
// NOTE: this marker is load-bearing for tests/review_compact_summary_trait_isolation_test.rs — do not remove.
// END_TRAIT_RuntimeLlmExecutor — sentinel for review_compact_summary_trait_isolation_test

fn is_compact_boundary_message(message: &serde_json::Value) -> bool {
    message.get("role").and_then(|value| value.as_str()) == Some("system")
        && message.get("subtype").and_then(|value| value.as_str()) == Some("compact_boundary")
}

fn is_compact_summary_message(message: &serde_json::Value) -> bool {
    message
        .get("isCompactSummary")
        .and_then(|value| value.as_bool())
        == Some(true)
}

fn compact_trigger_event_value(trigger: &CompactTrigger) -> &'static str {
    match trigger {
        CompactTrigger::Auto => "auto",
        CompactTrigger::Manual => "manual",
    }
}

fn compact_artifact_messages(messages: &[serde_json::Value]) -> Vec<serde_json::Value> {
    messages
        .iter()
        .take_while(|message| {
            is_compact_boundary_message(message) || is_compact_summary_message(message)
        })
        .cloned()
        .collect()
}

fn attach_persisted_user_id_to_pending_message(
    messages: &mut [serde_json::Value],
    user_content: &str,
    message_id: &str,
    conversation_id: &str,
) {
    if message_id.is_empty() {
        return;
    }

    for message in messages.iter_mut().rev() {
        if message.get("role").and_then(|value| value.as_str()) != Some("user") {
            continue;
        }
        if message
            .get("isCompactSummary")
            .and_then(|value| value.as_bool())
            == Some(true)
        {
            continue;
        }
        if message.get("id").and_then(|value| value.as_str()).is_some() {
            continue;
        }
        if message.get("content").and_then(|value| value.as_str()) != Some(user_content) {
            continue;
        }

        if let Some(object) = message.as_object_mut() {
            object.insert(
                "id".to_string(),
                serde_json::Value::String(message_id.to_string()),
            );
            object.insert(
                "conversationId".to_string(),
                serde_json::Value::String(conversation_id.to_string()),
            );
        }
        break;
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
/// Resolves the on-disk write-through path for a conversation's turn-stage
/// snapshot (spec 2026-05-17-turn-stages §5).  Returns `None` when no user is
/// logged in — the emitter degrades to in-memory only.
///
/// Injected via [`RuntimeChatTurnDriver::with_turn_stage_path_resolver`]; in
/// production wired by `SessionRuntime::build_driver_for_turn` to delegate to
/// `RuntimeHost::resolve_turn_stage_path` which reads the active user scope
/// from `CurrentUserStorage`.  Tests can leave it unset for pure in-memory.
pub type TurnStagePathResolver = Arc<dyn Fn(&str) -> Option<std::path::PathBuf> + Send + Sync>;

#[async_trait]
pub trait RunActivityController: Send + Sync {
    async fn suspend_for_user_interaction(
        &self,
        session_id: &SessionId,
        run_id: &RunId,
    ) -> Result<()>;

    async fn resume_after_user_interaction(
        &self,
        session_id: &SessionId,
        run_id: &RunId,
        cancel: &CancellationToken,
    ) -> Result<()>;
}

#[derive(Clone)]
pub struct RuntimeChatTurnDriver {
    query_engine: QueryEngine,
    event_bus: RuntimeEventBus,
    /// S4 executor：只做 provider streaming adapter。
    llm_executor: Option<Arc<dyn RuntimeLlmExecutor>>,
    pending_permission_control_plane: Option<Arc<dyn PendingPermissionControlPlane>>,
    pending_interaction_control_plane: Option<Arc<dyn PendingInteractionControlPlane>>,
    task_notification_queue: Option<Arc<TaskNotificationQueue>>,
    /// Compaction backend, decoupled from llm_executor (P0.2).
    compact_client: Option<Arc<dyn CompactSummaryClient>>,
    /// User-scoped resolver for `turn_stage.json` write path.  None ⇒ emitter
    /// runs in-memory only (no persistence).  Spec §5.
    turn_stage_path_resolver: Option<TurnStagePathResolver>,
    run_activity_controller: Option<Arc<dyn RunActivityController>>,
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

/// Drain the Lead's `AgentInbox` and append any pending peer messages
/// (delivered by teammates via `SendMessage(to: "team-lead", ...)`) to the
/// next user-role message of the current turn.
///
/// Returns the number of peer messages folded in.  Returns `0` (a no-op)
/// when:
/// - the engine wasn't wired with `agent_names` / `inbox_registry`
///   (test/legacy paths), or
/// - this session has no `team-lead` registration (no `TeamCreate` was
///   called), or
/// - the Lead's inbox exists but is empty.
///
/// Mirrors `claude-code-best`'s `getTeammateMailboxAttachments()` — the
/// content shape is a single `<peer-messages>` XML block listing all
/// drained items.  Crucially we drain only the currently-buffered items
/// (no `recv().await`) so the turn can run even if no peer is talking.
async fn drain_and_inject_lead_inbox_messages(
    query_engine: &QueryEngine,
    session_id: &SessionId,
    messages: &mut Vec<serde_json::Value>,
) -> (usize, Option<String>) {
    let Some(names) = query_engine.agent_names() else {
        return (0, None);
    };
    let Some(inbox_reg) = query_engine.inbox_registry() else {
        return (0, None);
    };
    let active_team = query_engine
        .active_team_name(session_id)
        .await
        .unwrap_or_default();
    let Some(lead_id) = names
        .resolve(
            session_id,
            active_team.as_str(),
            crate::runtime::tools::builtin::team_tools::LEAD_NAME,
        )
        .await
    else {
        return (0, None);
    };
    let Some(lead_inbox) = inbox_reg
        .get(session_id, active_team.as_str(), &lead_id)
        .await
    else {
        return (0, None);
    };
    let drained = lead_inbox.drain_pending().await;
    if drained.is_empty() {
        return (0, None);
    }

    let xml = render_peer_messages_xml(&drained);
    messages.push(serde_json::json!({
        "role": "user",
        "content": xml.clone(),
    }));

    let count = drained.len();
    log::info!(
        "[chat_turn_driver] drained {} peer message(s) into Lead's next user message",
        count
    );
    let ws = crate::telemetry::diagnostics_workspace();
    record_diagnostic(
        &ws,
        DiagnosticEvent::new("turn.lead_inbox.drained", DiagnosticSource::Backend)
            .conversation_id(session_id.as_str())
            .ok(true)
            .payload(serde_json::json!({ "count": count })),
    );
    (count, Some(xml))
}

/// Render drained inbox items as a `<peer-messages>` XML attachment, in
/// the shape the Lead's prompt instructions teach it to read.  Skips
/// non-chat items (Shutdown / TaskNotification) because the chat-turn
/// driver does not own their semantics.
fn render_peer_messages_xml(items: &[crate::runtime::agent::inbox::InboxItem]) -> String {
    use crate::runtime::agent::inbox::{InboxItem, MessageSource};

    let mut s = String::new();
    s.push_str("<peer-messages>\n");
    for item in items {
        let InboxItem::ChatMessage { message, source } = item else {
            // Shutdown / TaskNotification are routed elsewhere; ignore here.
            continue;
        };
        let from = match source {
            MessageSource::Lead => "team-lead".to_string(),
            MessageSource::Teammate(name) => name.clone(),
            MessageSource::System => "system".to_string(),
        };
        let variant = message.variant_name();
        let body =
            message.as_text().map(|t| escape_xml(t)).unwrap_or_else(
                || match serde_json::to_string(message) {
                    Ok(j) => escape_xml(&j),
                    Err(_) => String::new(),
                },
            );
        s.push_str(&format!(
            "  <peer-message from=\"{}\" variant=\"{}\">{}</peer-message>\n",
            escape_xml_attr(&from),
            escape_xml_attr(variant),
            body
        ));
    }
    s.push_str("</peer-messages>");
    s
}

fn escape_xml(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

fn escape_xml_attr(s: &str) -> String {
    escape_xml(s).replace('"', "&quot;")
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

fn build_agents_md_context_message(
    agents_md_files: &[crate::runtime::agents_md::AgentsMdFile],
) -> Option<serde_json::Value> {
    if agents_md_files.is_empty() {
        return None;
    }

    let agents_md_section = agents_md_files
        .iter()
        .map(|file| format!("{}:\n{}", file.path.display(), file.content))
        .collect::<Vec<_>>()
        .join("\n\n");

    Some(serde_json::json!({
        "role": "user",
        "content": format!(
            "<system-reminder>\nProject instructions are shown below. These instructions OVERRIDE any default behavior — you MUST follow them exactly as written.\n# agentsMd\n{}\n</system-reminder>\n",
            agents_md_section
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

#[cold]
fn warn_no_compact_client() {
    log::warn!(
        "[chat_turn_driver] no CompactSummaryClient configured; skipping compaction. \
         Wire one via RuntimeChatTurnDriver::with_compact_client (typically in \
         SessionRuntime::build_driver_for_turn) to enable compaction."
    );
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
            compact_client: None,
            turn_stage_path_resolver: None,
            run_activity_controller: None,
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
            compact_client: None,
            turn_stage_path_resolver: None,
            run_activity_controller: None,
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
            compact_client: None,
            turn_stage_path_resolver: None,
            run_activity_controller: None,
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
            compact_client: None,
            turn_stage_path_resolver: None,
            run_activity_controller: None,
        }
    }

    pub fn with_task_notification_queue(mut self, queue: Arc<TaskNotificationQueue>) -> Self {
        self.task_notification_queue = Some(queue);
        self
    }

    /// Attach a compaction backend (P0.2).  When `None` (the default),
    /// compaction requests warn-log and return an empty summary.
    pub fn with_compact_client(mut self, client: Arc<dyn CompactSummaryClient>) -> Self {
        self.compact_client = Some(client);
        self
    }

    /// Inject the resolver that returns the user-scoped write path for
    /// `turn_stage.json`.  Without this, the emitter cannot persist (in-memory
    /// only) — production wires this in `SessionRuntime::build_driver_for_turn`.
    pub fn with_turn_stage_path_resolver(mut self, resolver: TurnStagePathResolver) -> Self {
        self.turn_stage_path_resolver = Some(resolver);
        self
    }

    pub fn with_run_activity_controller(
        mut self,
        controller: Arc<dyn RunActivityController>,
    ) -> Self {
        self.run_activity_controller = Some(controller);
        self
    }

    async fn suspend_run_for_user_interaction(&self, turn: &TurnState) {
        let Some(controller) = self.run_activity_controller.as_ref() else {
            return;
        };
        if let Err(err) = controller
            .suspend_for_user_interaction(turn.session_id(), turn.run_id())
            .await
        {
            log::warn!(
                "[chat_turn_driver] failed to suspend active run for user interaction session={} run={}: {:#}",
                turn.session_id().as_str(),
                turn.run_id().as_str(),
                err
            );
        }
    }

    async fn resume_run_after_user_interaction(
        &self,
        turn: &TurnState,
        cancel: &CancellationToken,
    ) -> Result<()> {
        let Some(controller) = self.run_activity_controller.as_ref() else {
            return Ok(());
        };
        controller
            .resume_after_user_interaction(turn.session_id(), turn.run_id(), cancel)
            .await
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

    /// Emit a one-shot stage transition without going through the per-turn
    /// `TurnStageEmitter`.  Used from helpers that don't own the emitter
    /// (resolve_permission_asks / resolve_interaction_requests).  Respects
    /// the `AIJIA_TURN_STAGES` feature flag.
    async fn emit_stage_oneshot(
        &self,
        session_id: SessionId,
        run_id: RunId,
        stage: crate::runtime::events::TurnStage,
    ) {
        if !crate::runtime::chat::turn_stage::turn_stages_enabled() {
            return;
        }
        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);
        if let Err(e) = self
            .event_bus
            .emit(RuntimeEvent::turn_stage_changed(
                session_id, run_id, stage, now_ms,
            ))
            .await
        {
            log::warn!("[turn-stage] emit_stage_oneshot failed: {e}");
        }
    }

    async fn resolve_permission_asks(
        &self,
        turn: &TurnState,
        cancel: &CancellationToken,
        round_results: Vec<ToolRoundResult>,
        stage_emitter: Option<&TurnStageEmitter>,
    ) -> Result<Vec<ToolRoundResult>> {
        struct PendingAskSlot {
            tool_call_id: String,
            tool_name: String,
            original_request: RuntimeToolCallRequest,
            path_auth_scope: Option<String>,
            resolution_rx: Option<tokio::sync::oneshot::Receiver<PendingPermissionResolution>>,
            resolution: Option<PendingPermissionResolution>,
        }

        enum PermissionSlot {
            Original(ToolRoundResult),
            Pending(PendingAskSlot),
        }

        let mut slots = Vec::with_capacity(round_results.len());
        let mut resolved_results = Vec::with_capacity(round_results.len());
        let mut first_pending_stage: Option<(String, String)> = None;

        for round_result in round_results {
            let ToolRoundResult::Ok(RuntimeToolCallOutcome::AskRequired {
                tool_call_id,
                tool_name,
                capability_scopes,
                original_request,
                decision,
            }) = round_result
            else {
                slots.push(PermissionSlot::Original(round_result));
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
                path_auth_scope,
                ..
            } = decision
            else {
                slots.push(PermissionSlot::Original(ToolRoundResult::Ok(
                    RuntimeToolCallOutcome::AskRequired {
                        tool_call_id,
                        tool_name,
                        capability_scopes,
                        original_request,
                        decision,
                    },
                )));
                continue;
            };
            let mode = turn.permission_mode();
            let pending_request = PendingPermissionRequest {
                tool_call_id: tool_call_id.clone().into(),
                session_id: turn.session_id().clone(),
                run_id: turn.run_id().clone(),
                tool_name: tool_name.clone(),
                capability_scopes,
                message: message.clone(),
                suggestions: suggestions.clone(),
                mode,
                remember_options: remember_options.clone(),
                default_destination,
                original_request: original_request.clone(),
                turn_origin: TurnOrigin::App,
                output_binding: OutputBinding::AppOnly,
                path_auth_scope: path_auth_scope.clone(),
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
                        default_destination,
                        path_auth_scope: path_auth_scope.clone(),
                        primary_model: turn.primary_model().to_string(),
                    },
                ))
                .await?;

            if first_pending_stage.is_none() {
                first_pending_stage = Some((tool_name.clone(), tool_call_id.clone()));
            }

            slots.push(PermissionSlot::Pending(PendingAskSlot {
                tool_call_id,
                tool_name,
                original_request,
                path_auth_scope,
                resolution_rx: Some(resolution_rx),
                resolution: None,
            }));
        }

        if let Some((tool_name, tool_call_id)) = first_pending_stage {
            // Stage: WaitingPermission — UI shows "等待你审批：<tool>" until the
            // user resolves the ask.  The next stage transition (Tools resume
            // or WaitingLlm continuation) is emitted by the main loop.
            if let Some(stage_emitter) = stage_emitter {
                stage_emitter
                    .waiting_permission(tool_name.clone(), tool_call_id.clone())
                    .await;
            } else {
                self.emit_stage_oneshot(
                    turn.session_id().clone(),
                    turn.run_id().clone(),
                    crate::runtime::events::TurnStage::WaitingPermission {
                        tool_name: tool_name.clone(),
                        tool_call_id: tool_call_id.clone(),
                    },
                )
                .await;
            }

            self.suspend_run_for_user_interaction(turn).await;

            for slot in &mut slots {
                let PermissionSlot::Pending(pending) = slot else {
                    continue;
                };
                let resolution_rx = pending
                    .resolution_rx
                    .take()
                    .expect("pending permission slot should retain its receiver");
                pending.resolution = Some(
                    self.await_permission_resolution(
                        cancel,
                        pending.tool_call_id.as_str(),
                        resolution_rx,
                    )
                    .await,
                );
            }

            self.resume_run_after_user_interaction(turn, cancel).await?;
        }

        for slot in slots {
            let PendingAskSlot {
                tool_call_id,
                tool_name,
                original_request,
                path_auth_scope,
                resolution,
                ..
            } = match slot {
                PermissionSlot::Original(result) => {
                    resolved_results.push(result);
                    continue;
                }
                PermissionSlot::Pending(pending) => pending,
            };
            let resolution =
                resolution.expect("pending permission slot should be resolved before replay");
            let resolved = match resolution {
                PendingPermissionResolution::Allow {
                    updated_input,
                    message,
                    ..
                } => {
                    let mut outcome = self
                        .query_engine
                        .replay_tool_call_with_bus(
                            turn,
                            &self.event_bus,
                            original_request.clone(),
                            updated_input,
                        )
                        .await
                        .map_err(|err| {
                            anyhow::anyhow!("failed to replay approved tool call: {err}")
                        })?;
                    if let (
                        Some(message),
                        RuntimeToolCallOutcome::Completed {
                            context_modifier_message,
                            ..
                        },
                    ) = (message, &mut outcome)
                    {
                        *context_modifier_message = Some(serde_json::json!({
                            "role": "user",
                            "content": message,
                        }));
                    }
                    ToolRoundResult::Ok(outcome)
                }
                PendingPermissionResolution::Deny {
                    message,
                    path_auth_scope_override,
                    ..
                } => {
                    let deny_scope = path_auth_scope_override
                        .as_deref()
                        .or(path_auth_scope.as_deref());
                    if let Some(scope) = deny_scope {
                        self.query_engine
                            .record_run_path_deny_for_scope(turn.run_id(), scope);
                    }
                    record_turn_diagnostic(
                        &crate::telemetry::diagnostics_workspace(),
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
                        &crate::telemetry::diagnostics_workspace(),
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
                        primary_model: turn.primary_model().to_string(),
                    },
                ))
                .await?;

            // Stage: WaitingInteraction — UI shows "等待你回答…" until resolved.
            self.emit_stage_oneshot(
                turn.session_id().clone(),
                turn.run_id().clone(),
                crate::runtime::events::TurnStage::WaitingInteraction {
                    interaction_kind: format!("{:?}", interaction_request.kind),
                    interaction_id: interaction_request.interaction_id.as_str().to_string(),
                },
            )
            .await;

            self.suspend_run_for_user_interaction(turn).await;
            let resolution = self
                .await_interaction_resolution(
                    cancel,
                    &interaction_request.interaction_id,
                    resolution_rx,
                )
                .await;
            self.resume_run_after_user_interaction(turn, cancel).await?;

            let resolved = match resolution {
                InteractionResolution::Submit { value } => {
                    record_turn_diagnostic(
                        &crate::telemetry::diagnostics_workspace(),
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
                        &crate::telemetry::diagnostics_workspace(),
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
        *turn = turn.clone().with_human_interaction_metadata(
            request.turn_origin.clone(),
            request.output_binding.clone(),
        );
        // Merge this turn's attachment-derived directories into the session-scoped
        // accumulator so they remain available for all subsequent tool calls.
        // This must happen before any tool dispatching (both S4 and legacy paths).
        if !request.session_attachment_dirs.is_empty() {
            self.query_engine
                .merge_session_attachment_dirs(&request.session_attachment_dirs);
        }

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
        // Turn-stage emitter (spec §3 + §8 + §5).  When the env flag is off
        // this is a zero-cost no-op for every emit / persist call.
        //
        // Persist path is user-scoped via the injected resolver
        // (`users/{scope}/turn_stages/{conv_id}.json`).  When no user is
        // logged in (or the resolver isn't wired in tests), the emitter runs
        // in-memory only — no on-disk snapshot, no cross-user leakage.
        let mut stage_emitter = TurnStageEmitter::new(
            self.event_bus.clone(),
            turn.session_id().clone(),
            turn.run_id().clone(),
        );
        if let Some(resolver) = self.turn_stage_path_resolver.as_ref() {
            if let Some(path) = resolver(turn.session_id().as_str()) {
                stage_emitter = stage_emitter.with_persist_path(path);
            }
        }
        stage_emitter.submitted().await;
        // RAII guard — spec §8.  Spawning here ensures every ? / early return
        // / panic / cancel path inside this function drops the guard and the
        // 2s heartbeat task stops cleanly.  No spawn when the feature flag
        // is off (inert guard).
        let _heartbeat_guard = stage_emitter.spawn_heartbeat();
        // RAII guard — spec §5: delete the on-disk snapshot at every exit
        // path so the recovery sweep at next startup doesn't mistake this
        // turn for a crash.
        let _persist_cleanup_guard = stage_emitter.cleanup_guard();

        // LTR (B-gap1) Path A entry: marks the Lead as Running and returns
        // the resolved (session, lead_agent_id) key.  None when the session
        // is not in Team mode or registries aren't wired.  The exit half of
        // Path A — mark_idle + maybe-emit `LeadHasPendingMessages` — fires
        // before the AgentIdle event near the end of this function.
        let lead_key_for_path_a = self.lead_key_and_mark_running(turn).await;

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
        {
            let agent_has_emp = tool_defs
                .iter()
                .find(|v| v.get("name").and_then(|n| n.as_str()) == Some("Agent"))
                .and_then(|v| v.get("description").and_then(|d| d.as_str()))
                .map(|d| d.contains("<available_subagent_types>"))
                .unwrap_or(false);
            log::debug!(
                "[tool-desc-trace] get_tool_defs returned: count={} agent_desc_has_emp_section={}",
                tool_defs.len(),
                agent_has_emp,
            );
        }
        let workspace_path = executor
            .load_workspace_path()
            .await
            .map_err(|e| anyhow::anyhow!("{}", e))?;
        let overrides = executor
            .load_turn_config_overrides(request)
            .await
            .map_err(|e| anyhow::anyhow!("{}", e))?;

        let prompt_snapshot = match executor
            .build_prompt_snapshot(request)
            .await
            .map_err(|e| anyhow::anyhow!("{}", e))?
        {
            Some(snapshot) => snapshot,
            None => {
                let system_prompt = match overrides.system_prompt.clone() {
                    Some(prompt) => prompt,
                    None => executor
                        .build_system_prompt(request)
                        .await
                        .map_err(|e| anyhow::anyhow!("{}", e))?,
                };
                single_dynamic_prompt_snapshot("legacy_system_prompt", system_prompt)
            }
        };
        let effective_system_prompt = prompt_snapshot.compat_system_prompt();
        let effective_prompt_snapshot = prompt_snapshot;

        {
            let default_count = tool_defs.len();
            let overrides_has = overrides.tool_defs.is_some();
            log::debug!(
                "[tool-desc-trace] merge: overrides.tool_defs.is_some={} default_count={}",
                overrides_has,
                default_count,
            );
        }
        let final_tool_defs = overrides.tool_defs.unwrap_or(tool_defs);
        {
            let agent_has_emp = final_tool_defs
                .iter()
                .find(|v| v.get("name").and_then(|n| n.as_str()) == Some("Agent"))
                .and_then(|v| v.get("description").and_then(|d| d.as_str()))
                .map(|d| d.contains("<available_subagent_types>"))
                .unwrap_or(false);
            log::debug!(
                "[tool-desc-trace] final tool_defs: count={} agent_desc_has_emp_section={}",
                final_tool_defs.len(),
                agent_has_emp,
            );
        }
        let config = TurnConfig {
            system_prompt: effective_system_prompt,
            prompt_snapshot: Some(effective_prompt_snapshot),
            tool_defs: final_tool_defs,
            allowed_tools: overrides.allowed_tools,
            max_iterations: overrides.max_iterations.unwrap_or(120),
            // All chat routes through the lotus gateway now: ask for an
            // aspirational ceiling and let the gateway clamp to the real
            // per-upstream-model cap (Step 1).
            token_budget: overrides.token_budget.unwrap_or(1_000_000),
            chunk_timeout_secs: 90,
            masking_level: llm_settings.masking_level.clone(),
            workspace_path: workspace_path.clone(),
            authorized_workspace: overrides.authorized_workspace,
            llm_settings,
            conversation_id: request.conversation_id.clone(),
            run_id: request.run_id.clone(),
            trace_id: request.client_message_id.clone().unwrap_or_default(),
            hook_registry: request.hook_registry.clone(),
        };
        // Make primary_model available on TurnState so downstream emit sites
        // (resolve_permission_asks / resolve_interaction_requests) can forward it.
        turn.set_primary_model(config.llm_settings.primary_model.clone());
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
        // messages 顺序：[system-reminder, agents-md-meta?, ...history, current-user-content]
        use chrono::Datelike;

        let now = chrono::Local::now();
        let today = now.format("%Y年%m月%d日").to_string();
        let today_iso = now.format("%Y-%m-%d").to_string();
        let weekday_cn = crate::runtime::chat::prompt::ReminderBuilder::weekday_cn(now.weekday());
        let today_with_weekday = format!("{today} {weekday_cn}");
        let local_time_hms = now.format("%H:%M:%S").to_string();
        let system_reminder_message =
            crate::runtime::chat::prompt::ReminderBuilder::date_time_message(
                &today_with_weekday,
                &today_iso,
                &local_time_hms,
            );
        let agents_md_files = executor
            .load_agents_md(config.authorized_workspace.as_ref())
            .await
            .unwrap_or_else(|e| {
                log::warn!(
                    "[run_chat_turn_s4] load_agents_md failed for workspace '{}': {}",
                    config.workspace_path.display(),
                    e
                );
                Vec::new()
            });
        let agents_md_context_message = build_agents_md_context_message(&agents_md_files);
        let project_instruction_reinjection_content = agents_md_context_message
            .as_ref()
            .and_then(|message| message.get("content").and_then(|value| value.as_str()))
            .map(str::to_string);

        // Sentinel content sent by the frontend when a background sub-agent
        // completes — we want to wake the parent up so it drains pending
        // task-notifications, but we must NOT persist or surface a fake user
        // turn. The drain step below will inject the actual notification XML
        // as the user-role message that the LLM responds to.
        let is_resume_for_task_notification = request.content
            == "__resume_from_task_notification__"
            && request.attachments.is_empty();

        let anthropic_image_result = if should_build_image_blocks_for_turn(
            &config.llm_settings,
            is_resume_for_task_notification,
        ) {
            build_anthropic_image_blocks(&request.attachments)
        } else {
            crate::runtime::chat::multimodal::AnthropicImageBuildResult::empty()
        };
        let attachments_for_text = retain_text_fallback_attachments(
            &request.attachments,
            &anthropic_image_result.converted_attachment_ids,
        );
        let anthropic_multimodal_turn = if anthropic_image_result.image_blocks.is_empty() {
            None
        } else {
            Some(crate::llm::streaming::AnthropicMultimodalTurn {
                image_count: anthropic_image_result.image_blocks.len(),
                image_bytes_total: anthropic_image_result.image_bytes_total,
                degraded_count: anthropic_image_result.degraded_attachment_ids.len(),
                image_blocks: anthropic_image_result.image_blocks.clone(),
            })
        };

        let llm_user_content = if is_resume_for_task_notification {
            String::new()
        } else {
            executor
                .build_user_message_content(
                    request.conversation_id.as_str(),
                    &request.content,
                    &attachments_for_text,
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
        if let Some(agents_md_context_message) = agents_md_context_message {
            initial_messages.push(agents_md_context_message);
        }
        initial_messages.extend(history);
        // When pre_persisted, the caller already wrote the user message to DB,
        // so `load_history` above already returned it — pushing user_message
        // again would duplicate the bubble for the LLM (and waste tokens).
        if !is_resume_for_task_notification && !request.pre_persisted {
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

        // persist each task-notification XML as user message (best-effort)
        for notification in &pending_task_notifications {
            match executor
                .persist_user_message(
                    request.conversation_id.as_str(),
                    &notification.xml,
                    &[],
                    None,
                    None,
                )
                .await
            {
                Ok(msg_id) => {
                    // Emit MessagePersisted so the frontend sees the
                    // <task-notification> XML in real time, not only after
                    // a conversation reload. Best-effort: never fail the turn.
                    if let Err(e) = self
                        .event_bus
                        .emit(RuntimeEvent::new(
                            turn.session_id().clone(),
                            turn.run_id().clone(),
                            RuntimeEventKind::MessagePersisted {
                                message_id: msg_id,
                                role: "user".to_string(),
                                content: build_user_content_json(&notification.xml, &[]),
                                client_message_id: None,
                                tool_calls: None,
                                error: None,
                            },
                        ))
                        .await
                    {
                        log::warn!(
                            "[chat_turn_driver] emit MessagePersisted for task-notification failed: {e}"
                        );
                    }
                }
                Err(e) => log::warn!(
                    "[chat_turn_driver] persist task-notification failed (best-effort): {e}"
                ),
            }
        }

        // LTR P2: drain the Lead's inbox so any messages peers delivered via
        // SendMessage(to: "team-lead", ...) while the Lead was busy / idle
        // are folded into this turn as the latest user-role attachment.
        // Mirrors cc-best's `getTeammateMailboxAttachments()`.
        let (drained_peer_messages, peer_xml) = drain_and_inject_lead_inbox_messages(
            &self.query_engine,
            turn.session_id(),
            &mut initial_messages,
        )
        .await;

        // persist peer messages XML as user message (best-effort)
        if let Some(xml) = peer_xml {
            match executor
                .persist_user_message(request.conversation_id.as_str(), &xml, &[], None, None)
                .await
            {
                Ok(msg_id) => {
                    // Emit MessagePersisted so the frontend can render the
                    // <peer-messages> XML during streaming. Without this,
                    // the team-message banners would only appear after a
                    // conversation reload.
                    if let Err(e) = self
                        .event_bus
                        .emit(RuntimeEvent::new(
                            turn.session_id().clone(),
                            turn.run_id().clone(),
                            RuntimeEventKind::MessagePersisted {
                                message_id: msg_id,
                                role: "user".to_string(),
                                content: build_user_content_json(&xml, &[]),
                                client_message_id: None,
                                tool_calls: None,
                                error: None,
                            },
                        ))
                        .await
                    {
                        log::warn!(
                            "[chat_turn_driver] emit MessagePersisted for peer-messages failed: {e}"
                        );
                    }
                }
                Err(e) => {
                    log::warn!("[chat_turn_driver] persist peer-messages failed (best-effort): {e}")
                }
            }
        }

        // Guard: if this turn was triggered purely to resume from a task
        // notification or a Path-C inbox wake but BOTH queues are empty
        // (race: another turn drained them first), there is no useful work
        // to do — exit cleanly without calling the LLM, which would
        // otherwise see no user message and fail.
        if is_resume_for_task_notification
            && pending_task_notifications.is_empty()
            && drained_peer_messages == 0
        {
            log::info!(
                "[chat_turn_driver] resume turn skipped: both task-notification queue and lead inbox empty session={}",
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
        // Also skip when the caller (e.g. `dispatch_employee_run`) already
        // pre-persisted the user message + emitted MessagePersisted, so that
        // an agent-spawn failure cannot leave a conversation with no user
        // message on disk.
        let skip_user_persist = is_resume_for_task_notification || request.pre_persisted;
        let _user_msg_id = if skip_user_persist {
            String::new()
        } else {
            executor
                .persist_user_message(
                    request.conversation_id.as_str(),
                    &request.content,
                    &request.attachments,
                    request.skill_command.as_ref(),
                    request.client_message_id.as_deref(),
                )
                .await
                .map_err(|e| anyhow::anyhow!("{}", e))?
        };
        let pending_user_msg_id = _user_msg_id;
        let pending_client_msg_id = request.client_message_id.clone();
        if !skip_user_persist {
            attach_persisted_user_id_to_pending_message(
                &mut state.messages,
                &llm_user_content,
                &pending_user_msg_id,
                request.conversation_id.as_str(),
            );
        }

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
        let pending_user_content = build_user_content_json_with_skill(
            &request.content,
            &request.attachments,
            request.skill_command.as_ref(),
        );
        // Skip emitting MessagePersisted for the resume-sentinel: it's an
        // internal wake signal, not a user-visible turn. Emitting it would
        // surface a fake "__resume_from_task_notification__" bubble in the
        // chat UI (and worse, with an empty message_id since the persistence
        // step above was also skipped).
        // Also skip when caller pre-persisted (and already emitted the
        // event) to avoid a duplicate user bubble in the chat list.
        if !skip_user_persist {
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
                        error: None,
                    },
                ))
                .await?;
        }

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
        let skill_context = match selected_skill_instruction(request.skill_command.as_ref()) {
            Some(instruction) if !skill_catalog.is_empty() => {
                format!("{skill_catalog}{instruction}")
            }
            Some(instruction) => instruction,
            None => skill_catalog,
        };
        let skill_context = match request.channel_context.as_deref() {
            Some(channel_context)
                if !channel_context.trim().is_empty() && !skill_context.is_empty() =>
            {
                format!("{channel_context}\n\n{skill_context}")
            }
            Some(channel_context) if !channel_context.trim().is_empty() => {
                channel_context.to_string()
            }
            _ => skill_context,
        };
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
        let resolved_context_window = resolve_context_window(
            config.llm_settings.context_window,
            Some(&config.llm_settings.cloud_model),
        );
        let mut post_compact_system_segments: Vec<SystemPromptSegment> = Vec::new();
        let compact_transcript_path = executor
            .conversation_dir(config.conversation_id.as_str())
            .map(|dir| compact_transcript_path_for_conversation_dir(&dir));

        'turn: for iteration in 0..config.max_iterations {
            let mut preprocess_config = PreprocessConfig::default();
            preprocess_config.context_window = resolved_context_window;
            preprocess_config.query_source = Some("chat_turn".to_string());
            preprocess_config.auto_compact =
                AutoCompactConfig::with_context_window(resolved_context_window);
            // R3.2 boundary view isolation: load latest compact boundary so
            // preprocess only operates on the post-boundary slice.
            preprocess_config.compact_boundary = executor
                .latest_compact_boundary(config.conversation_id.as_str())
                .await
                .unwrap_or_else(|err| {
                    log::warn!("[run_chat_turn_s4] latest_compact_boundary failed: {}", err);
                    None
                });
            preprocess_config.project_instruction_content =
                project_instruction_reinjection_content.clone();
            if preprocess_config.compact_boundary.is_some() {
                if let Some(segment) = preprocess_config.project_instruction_system_segment() {
                    push_unique_system_segment(&mut post_compact_system_segments, segment);
                }
            }
            let conversation_id = config.conversation_id.as_str().to_string();
            let compact_client_ref = self.compact_client.clone();
            let compact_llm_settings = config.llm_settings.clone();
            let compact_trace_id = config.trace_id.clone();
            let compact_run_id = config.run_id.as_str().to_string();
            // Captures for the Compacting stage emit inside the summary closure.
            // The closure runs `async move` so it can't reach `&self`; we hand
            // it a bus clone + ids.
            let stage_bus = self.event_bus.clone();
            let stage_session = turn.session_id().clone();
            let stage_run = turn.run_id().clone();
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
                    let compact_client = compact_client_ref.clone();
                    let compact_llm_settings = compact_llm_settings.clone();
                    let compact_trace_id = compact_trace_id.clone();
                    let compact_run_id = compact_run_id.clone();
                    let bus = stage_bus.clone();
                    let session_id = stage_session.clone();
                    let run_id = stage_run.clone();
                    let compact_transcript_path = compact_transcript_path.clone();
                    async move {
                        crate::runtime::chat::turn_stage::emit_oneshot(
                            &bus,
                            session_id,
                            run_id,
                            crate::runtime::events::TurnStage::Compacting,
                        )
                        .await;
                        match compact_client.as_ref() {
                            Some(client) => {
                                let summary = client
                                    .compact_summary(
                                        conversation_id.as_str(),
                                        &messages,
                                        &compact_llm_settings,
                                        Some(compact_trace_id.as_str()),
                                        Some(compact_run_id.as_str()),
                                    )
                                    .await?;
                                let summary = append_literal_anchor_hints(summary, &messages);
                                Ok(append_transcript_path_hint(
                                    summary,
                                    compact_transcript_path.as_deref(),
                                ))
                            }
                            None => {
                                warn_no_compact_client();
                                Ok(String::new())
                            }
                        }
                    }
                },
            )
            .await
            .map_err(|err| anyhow::anyhow!("{}", err))?;
            if !prepared.post_compact_system_segments.is_empty() {
                for segment in prepared.post_compact_system_segments.clone() {
                    push_unique_system_segment(&mut post_compact_system_segments, segment);
                }
            }
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
            if let Some(ref boundary_record) = prepared.compact_boundary {
                let compact_messages = compact_artifact_messages(&state.messages);
                log::info!(
                    "[auto-compact][boundary-ready] conv={} boundary_id={} trigger={:?} compact_messages={} pre_tokens={} post_tokens={} messages_summarized={} tail_message_id={:?}",
                    boundary_record.conversation_id,
                    boundary_record.id,
                    boundary_record.trigger,
                    compact_messages.len(),
                    boundary_record.pre_tokens,
                    boundary_record.post_tokens,
                    boundary_record.messages_summarized,
                    boundary_record.tail_message_id,
                );
                if !compact_messages.is_empty() {
                    match executor
                        .persist_compact_messages(
                            boundary_record.conversation_id.as_str(),
                            &compact_messages,
                        )
                        .await
                    {
                        Ok(()) => log::info!(
                            "[auto-compact][persist-transcript-ok] conv={} boundary_id={} compact_messages={}",
                            boundary_record.conversation_id,
                            boundary_record.id,
                            compact_messages.len(),
                        ),
                        Err(err) => log::warn!(
                            "[auto-compact][persist-transcript-error] conv={} boundary_id={} compact_messages={} error={}",
                            boundary_record.conversation_id,
                            boundary_record.id,
                            compact_messages.len(),
                            err
                        ),
                    }
                } else {
                    log::warn!(
                        "[auto-compact][persist-transcript-skip] conv={} boundary_id={} reason=no_compact_artifact_messages",
                        boundary_record.conversation_id,
                        boundary_record.id,
                    );
                }
                // R5.1: Emit CompactCompleted event so the frontend can show
                // a compact summary (e.g. "保存了 X 个 token").
                match self
                    .event_bus
                    .emit(RuntimeEvent::compact_completed(
                        turn.session_id().clone(),
                        turn.run_id().clone(),
                        boundary_record.conversation_id.clone(),
                        boundary_record.id.clone(),
                        compact_trigger_event_value(&boundary_record.trigger).to_string(),
                        boundary_record.created_at.clone(),
                        boundary_record.tail_message_id.clone(),
                        boundary_record.pre_tokens,
                        boundary_record.post_tokens,
                        boundary_record.messages_summarized,
                    ))
                    .await
                {
                    Ok(()) => log::info!(
                        "[auto-compact][event-ok] conv={} boundary_id={} event=compact_completed",
                        boundary_record.conversation_id,
                        boundary_record.id,
                    ),
                    Err(err) => log::warn!(
                        "[auto-compact][event-error] conv={} boundary_id={} event=compact_completed error={}",
                        boundary_record.conversation_id,
                        boundary_record.id,
                        err
                    ),
                }
                match executor
                    .save_compact_boundary(boundary_record.clone())
                    .await
                {
                    Ok(()) => log::info!(
                        "[auto-compact][persist-boundary-ok] conv={} boundary_id={}",
                        boundary_record.conversation_id,
                        boundary_record.id,
                    ),
                    Err(err) => log::warn!(
                        "[auto-compact][persist-boundary-error] conv={} boundary_id={} error={}",
                        boundary_record.conversation_id,
                        boundary_record.id,
                        err
                    ),
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
                &skill_context,
            );

            // Build the read-only executor input.
            let system_chars = config.system_prompt.len();
            let dynamic_chars = iteration_delta_context.len();
            let messages_chars = serde_json::to_string(&state.messages)
                .map(|s| s.len())
                .unwrap_or(0);
            let tools_chars = serde_json::to_string(&config.tool_defs)
                .map(|s| s.len())
                .unwrap_or(0);
            let estimated_tokens =
                (system_chars + dynamic_chars + messages_chars + tools_chars) / 4;
            record_turn_diagnostic(
                &config.workspace_path,
                "turn.tokens.estimated",
                turn.session_id(),
                turn.run_id(),
                None,
                None,
                Some(serde_json::json!({
                    "system_chars": system_chars,
                    "dynamic_chars": dynamic_chars,
                    "messages_chars": messages_chars,
                    "tools_chars": tools_chars,
                    "estimated_input_tokens": estimated_tokens,
                })),
            );
            if (estimated_tokens as f64)
                > (resolved_context_window as f64 * CONTEXT_OVERFLOW_THRESHOLD)
            {
                log::warn!(
                    "[AD2] Context overflow risk: estimated {} tokens > {}% of {} window (cloud_model: {})",
                    estimated_tokens,
                    (CONTEXT_OVERFLOW_THRESHOLD * 100.0) as u32,
                    resolved_context_window,
                    config.llm_settings.cloud_model,
                );
            }

            let input = LlmStepInput {
                system_prompt: &config.system_prompt,
                system_message: config
                    .prompt_snapshot
                    .as_ref()
                    .and_then(|snapshot| snapshot.system_message()),
                extra_system_segments: post_compact_system_segments.clone(),
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
                trace_id: config.trace_id.as_str(),
                estimated_tokens,
                anthropic_multimodal_turn: anthropic_multimodal_turn.clone(),
            };

            // CP-1: check cancellation before invoking provider.
            if cancel.is_cancelled() {
                re_enqueue_task_notifications(
                    &self.task_notification_queue,
                    std::mem::take(&mut pending_task_notifications),
                );
                mark_turn_cancelled_with_synthetic_results(&mut state, cancel.reason());
                break 'turn;
            }

            // ── Step 5b: single LLM step ─────────────────────────────────────
            // Emit a paragraph separator before streaming the next iteration,
            // so the frontend's streamingContent shows distinct paragraphs.
            if iteration > 0 && !state.full_content.is_empty() {
                let _ = self
                    .event_bus
                    .emit(RuntimeEvent::stream_delta(
                        session_id.clone(),
                        run_id.clone(),
                        "\n\n".to_string(),
                    ))
                    .await;
            }
            stage_emitter.waiting_llm(iteration as u32).await;
            let step_result = match executor
                .run_llm_step(&input, &self.event_bus, &cancel)
                .await
            {
                Ok(result) => result,
                Err(TurnError::PromptTooLong(message)) => {
                    let recovery_stage_bus = self.event_bus.clone();
                    let recovery_stage_session = turn.session_id().clone();
                    let recovery_stage_run = turn.run_id().clone();
                    let mut recovery_preprocess_config = PreprocessConfig::default();
                    recovery_preprocess_config.context_window = resolved_context_window;
                    recovery_preprocess_config.query_source = Some("ptl_recovery".to_string());
                    recovery_preprocess_config.auto_compact =
                        AutoCompactConfig::with_context_window(resolved_context_window);
                    recovery_preprocess_config.project_instruction_content =
                        project_instruction_reinjection_content.clone();
                    // R3.2 boundary view isolation: same as the main loop.
                    recovery_preprocess_config.compact_boundary = executor
                        .latest_compact_boundary(conversation_id.as_str())
                        .await
                        .unwrap_or_else(|err| {
                            log::warn!(
                                "[run_chat_turn_s4 ptl-recovery] latest_compact_boundary failed: {}",
                                err
                            );
                            None
                        });
                    let prepared = prepare_messages_for_llm(
                        std::mem::take(&mut state.messages),
                        conversation_id.as_str(),
                        PreprocessTrigger::PromptTooLongRecovery,
                        &recovery_preprocess_config,
                        &mut state.compact_state,
                        &mut state.preprocess_state,
                        state.stop_hook_active,
                        |messages| {
                            let conversation_id = conversation_id.clone();
                            let compact_client = compact_client_ref.clone();
                            let compact_llm_settings = compact_llm_settings.clone();
                            let compact_trace_id = compact_trace_id.clone();
                            let compact_run_id = compact_run_id.clone();
                            let bus = recovery_stage_bus.clone();
                            let session_id = recovery_stage_session.clone();
                            let run_id = recovery_stage_run.clone();
                            let compact_transcript_path = compact_transcript_path.clone();
                            async move {
                                crate::runtime::chat::turn_stage::emit_oneshot(
                                    &bus,
                                    session_id,
                                    run_id,
                                    crate::runtime::events::TurnStage::Compacting,
                                )
                                .await;
                                match compact_client.as_ref() {
                                    Some(client) => {
                                        let summary = client
                                            .compact_summary(
                                                conversation_id.as_str(),
                                                &messages,
                                                &compact_llm_settings,
                                                Some(compact_trace_id.as_str()),
                                                Some(compact_run_id.as_str()),
                                            )
                                            .await?;
                                        let summary =
                                            append_literal_anchor_hints(summary, &messages);
                                        Ok(append_transcript_path_hint(
                                            summary,
                                            compact_transcript_path.as_deref(),
                                        ))
                                    }
                                    None => {
                                        warn_no_compact_client();
                                        Ok(String::new())
                                    }
                                }
                            }
                        },
                    )
                    .await
                    .map_err(|err| anyhow::anyhow!("{}", err))?;
                    if !prepared.post_compact_system_segments.is_empty() {
                        for segment in prepared.post_compact_system_segments.clone() {
                            push_unique_system_segment(&mut post_compact_system_segments, segment);
                        }
                    }
                    state.messages = prepared.messages;
                    if let Some(boundary_record) = prepared.compact_boundary {
                        let compact_messages = compact_artifact_messages(&state.messages);
                        log::info!(
                            "[auto-compact][boundary-ready] conv={} boundary_id={} trigger={:?} path=ptl_recovery compact_messages={} pre_tokens={} post_tokens={} messages_summarized={} tail_message_id={:?}",
                            boundary_record.conversation_id,
                            boundary_record.id,
                            boundary_record.trigger,
                            compact_messages.len(),
                            boundary_record.pre_tokens,
                            boundary_record.post_tokens,
                            boundary_record.messages_summarized,
                            boundary_record.tail_message_id,
                        );
                        if !compact_messages.is_empty() {
                            match executor
                                .persist_compact_messages(
                                    boundary_record.conversation_id.as_str(),
                                    &compact_messages,
                                )
                                .await
                            {
                                Ok(()) => log::info!(
                                    "[auto-compact][persist-transcript-ok] conv={} boundary_id={} path=ptl_recovery compact_messages={}",
                                    boundary_record.conversation_id,
                                    boundary_record.id,
                                    compact_messages.len(),
                                ),
                                Err(err) => log::warn!(
                                    "[auto-compact][persist-transcript-error] conv={} boundary_id={} path=ptl_recovery compact_messages={} error={}",
                                    boundary_record.conversation_id,
                                    boundary_record.id,
                                    compact_messages.len(),
                                    err
                                ),
                            }
                        } else {
                            log::warn!(
                                "[auto-compact][persist-transcript-skip] conv={} boundary_id={} path=ptl_recovery reason=no_compact_artifact_messages",
                                boundary_record.conversation_id,
                                boundary_record.id,
                            );
                        }
                        match self
                            .event_bus
                            .emit(RuntimeEvent::compact_completed(
                                turn.session_id().clone(),
                                turn.run_id().clone(),
                                boundary_record.conversation_id.clone(),
                                boundary_record.id.clone(),
                                compact_trigger_event_value(&boundary_record.trigger).to_string(),
                                boundary_record.created_at.clone(),
                                boundary_record.tail_message_id.clone(),
                                boundary_record.pre_tokens,
                                boundary_record.post_tokens,
                                boundary_record.messages_summarized,
                            ))
                            .await
                        {
                            Ok(()) => log::info!(
                                "[auto-compact][event-ok] conv={} boundary_id={} path=ptl_recovery event=compact_completed",
                                boundary_record.conversation_id,
                                boundary_record.id,
                            ),
                            Err(err) => log::warn!(
                                "[auto-compact][event-error] conv={} boundary_id={} path=ptl_recovery event=compact_completed error={}",
                                boundary_record.conversation_id,
                                boundary_record.id,
                                err
                            ),
                        }
                        match executor.save_compact_boundary(boundary_record.clone()).await {
                            Ok(()) => log::info!(
                                "[auto-compact][persist-boundary-ok] conv={} boundary_id={} path=ptl_recovery",
                                boundary_record.conversation_id,
                                boundary_record.id,
                            ),
                            Err(err) => log::warn!(
                                "[auto-compact][persist-boundary-error] conv={} boundary_id={} path=ptl_recovery error={}",
                                boundary_record.conversation_id,
                                boundary_record.id,
                                err
                            ),
                        }
                    }
                    if prepared.retry == PreprocessRetryAction::RetryTurn {
                        // Re-enqueue so the notifications are tried again on retry.
                        re_enqueue_task_notifications(
                            &self.task_notification_queue,
                            std::mem::take(&mut pending_task_notifications),
                        );
                        continue 'turn;
                    }

                    re_enqueue_task_notifications(
                        &self.task_notification_queue,
                        std::mem::take(&mut pending_task_notifications),
                    );
                    self.event_bus
                        .emit(RuntimeEvent::new(
                            session_id.clone(),
                            run_id.clone(),
                            RuntimeEventKind::StreamError {
                                error: message.clone(),
                                raw_error: Some("prompt_too_long".to_string()),
                                code: None,
                                retryable: None,
                                handling: None,
                                request_phase: None,
                                current_route: None,
                                alternatives: None,
                            },
                        ))
                        .await?;
                    inject_synthetic_tool_results_for_missing_calls(
                        &mut state.messages,
                        cancel.reason(),
                    );

                    // PR2: PromptTooLong 用专用 kind 让 UI 显示"压缩历史/新建会话"指引
                    let error_text =
                        "对话上下文已超出模型限制。请新建会话或精简历史后再试。".to_string();
                    let error = crate::storage::file_store::types::MessageError {
                        kind: crate::storage::file_store::types::ErrorKind::PromptTooLong,
                        message: error_text.clone(),
                        raw: Some(sanitize_error_raw(&message)),
                    };
                    if let Err(emit_err) = self
                        .emit_terminal_error_message_and_idle(
                            executor,
                            &session_id,
                            &run_id,
                            conversation_id.as_str(),
                            &error_text,
                            error,
                        )
                        .await
                    {
                        log::error!(
                            "[chat_turn_driver] failed to emit terminal error events on PromptTooLong: {}",
                            emit_err
                        );
                    }

                    return Err(anyhow::anyhow!(message));
                }
                Err(err) => {
                    re_enqueue_task_notifications(
                        &self.task_notification_queue,
                        std::mem::take(&mut pending_task_notifications),
                    );
                    inject_synthetic_tool_results_for_missing_calls(
                        &mut state.messages,
                        cancel.reason(),
                    );

                    // PR2: 构造结构化 MessageError 替代 PR1 纯字符串占位。
                    // 通用 LLM 错误归 kind=Unknown（PR3 fallback 后这里基本不会触达）。
                    // spec: docs/superpowers/specs/2026-05-28-streaming-error-handling-design.md §3.1
                    let error_text =
                        "抱歉，AI 服务暂时无法响应（已自动尝试多次）。请稍后再试，或换个方式提问。"
                            .to_string();
                    let error = crate::storage::file_store::types::MessageError {
                        kind: crate::storage::file_store::types::ErrorKind::Unknown,
                        message: error_text.clone(),
                        raw: Some(sanitize_error_raw(&err.to_string())),
                    };
                    if let Err(emit_err) = self
                        .emit_terminal_error_message_and_idle(
                            executor,
                            &session_id,
                            &run_id,
                            conversation_id.as_str(),
                            &error_text,
                            error,
                        )
                        .await
                    {
                        log::error!(
                            "[chat_turn_driver] failed to emit terminal error events on stream Err: {}",
                            emit_err
                        );
                    }

                    return Err(anyhow::anyhow!("{}", err));
                }
            };

            match step_result {
                // ── 5c: pure content response — done ─────────────────────────
                LlmStepResult::ContentComplete {
                    content,
                    thinking_blocks,
                    tokens_in,
                    tokens_out,
                    cache_creation_input_tokens,
                    cache_read_input_tokens,
                    stop_reason,
                } => {
                    if !thinking_blocks.is_empty() {
                        state.last_thinking_blocks = thinking_blocks.clone();
                        state.final_thinking_blocks = thinking_blocks.clone();
                    }
                    state.final_only_content = content.clone();
                    if !content.is_empty() && !state.full_content.is_empty() {
                        state.full_content.push_str("\n\n");
                    }
                    state.full_content.push_str(&content);
                    state.step_tokens_in += tokens_in;
                    state.step_tokens_out += tokens_out;
                    state.step_cache_creation_input_tokens += cache_creation_input_tokens;
                    state.step_cache_read_input_tokens += cache_read_input_tokens;
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

                        let max_tokens_notice =
                            "\n\n[输出 token 上限已达到，系统已停止自动续写；以上为当前已生成内容。]";
                        state.full_content.push_str(max_tokens_notice);
                        state.final_only_content.push_str(max_tokens_notice);
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
                LlmStepResult::Cancelled { partial_content } => {
                    if !partial_content.is_empty() {
                        state.final_only_content = partial_content.clone();
                        if !state.full_content.is_empty() {
                            state.full_content.push_str("\n\n");
                        }
                        state.full_content.push_str(&partial_content);
                    }
                    re_enqueue_task_notifications(
                        &self.task_notification_queue,
                        std::mem::take(&mut pending_task_notifications),
                    );
                    mark_turn_cancelled_with_synthetic_results(&mut state, cancel.reason());
                    break 'turn;
                }

                // ── 5e: tool calls ────────────────────────────────────────────
                LlmStepResult::ToolCalls {
                    assistant_content,
                    thinking_blocks,
                    tool_calls,
                    tokens_in,
                    tokens_out,
                    cache_creation_input_tokens,
                    cache_read_input_tokens,
                } => {
                    if !thinking_blocks.is_empty() {
                        state.last_thinking_blocks = thinking_blocks.clone();
                    }
                    if !assistant_content.is_empty() {
                        if !state.full_content.is_empty() {
                            state.full_content.push_str("\n\n");
                        }
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
                    let mut assistant_history_message = serde_json::json!({
                        "role": "assistant",
                        "content": assistant_content,
                        "toolCalls": normalized_tool_calls,
                    });
                    if !thinking_blocks.is_empty() {
                        assistant_history_message["thinkingBlocks"] =
                            serde_json::Value::Array(thinking_blocks.clone());
                    }
                    state.step_tokens_in += tokens_in;
                    state.step_tokens_out += tokens_out;
                    state.step_cache_creation_input_tokens += cache_creation_input_tokens;
                    state.step_cache_read_input_tokens += cache_read_input_tokens;
                    state.iteration_count = iteration + 1;

                    // Stage: Tools — emit the planned batch so the UI immediately
                    // shows "正在执行 X / 正在并行运行 N 个工具".  Per-tool
                    // start/completion granularity is handled by the existing
                    // tool:executing / tool:completed events that the
                    // round_driver fires below.
                    {
                        let now_ms = std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .map(|d| d.as_millis() as u64)
                            .unwrap_or(0);
                        let running: Vec<RunningTool> = tool_calls
                            .iter()
                            .map(|call| RunningTool {
                                tool_name: call.tool_name.clone(),
                                tool_call_id: call.tool_call_id.clone(),
                                started_at_ms: now_ms,
                            })
                            .collect();
                        stage_emitter.tools_started(iteration as u32, running).await;
                    }

                    // 先持久化 + emit iter assistant message（含 toolCalls），
                    // 再 execute_round。这样前端的 messages 在 tool 执行**之前**
                    // 就已经多了这条 iter，buildTurnsFromMessages 把 toolStep
                    // 推进 blocks 的 persistedBlockCount 段（status='running'），
                    // 后续 tool:executing / tool:completed 事件通过 existing
                    // 分支只更新 status——React key 和位置全程不变，避免了
                    // "live tool block → persisted tool block" 接力导致 React
                    // unmount/mount、loading 跳到 done 时页面跳动。
                    //
                    // ⚠️ cancel 副作用：若用户在 execute_round 中途 cancel，
                    // iter assistant message 已经落盘但部分 tool 没产 tool_result，
                    // 刷新会话后这些 step 会一直显示 running 态。turn 已 finalize
                    // 时 streaming bubble 已消失，视觉上的歧义可接受；如要根治
                    // 需要 cancel 路径把 synthesized tool_result 也 persist。
                    if !normalized_tool_calls.is_empty() {
                        match executor
                            .persist_iteration_assistant_message(
                                config.conversation_id.as_str(),
                                &assistant_content,
                                &normalized_tool_calls,
                                &state.last_thinking_blocks,
                            )
                            .await
                        {
                            Ok(Some(iter_msg_id)) => {
                                if let Err(emit_err) = self
                                    .event_bus
                                    .emit(RuntimeEvent::new(
                                        session_id.clone(),
                                        run_id.clone(),
                                        RuntimeEventKind::MessagePersisted {
                                            message_id: iter_msg_id,
                                            role: "assistant".to_string(),
                                            content: serde_json::json!({ "text": assistant_content }),
                                            client_message_id: None,
                                            tool_calls: Some(normalized_tool_calls.clone()),
                                            error: None,
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
                                log::warn!(
                                    "[chat_turn_driver] Failed to persist iteration assistant message: {}",
                                    e
                                );
                            }
                        }
                    }

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
                        .resolve_permission_asks(turn, &cancel, round_results, Some(&stage_emitter))
                        .await?;
                    let round_results = self
                        .resolve_interaction_requests(turn, &cancel, round_results)
                        .await?;

                    let artifact_replacements = executor
                        .conversation_dir(config.conversation_id.as_str())
                        .map(|conv_dir| {
                            build_tool_result_artifact_replacements_from_round_results(
                                &conv_dir,
                                &round_results,
                            )
                        })
                        .unwrap_or_default();

                    // Collect and merge results into state.
                    let mut results = tool_result_collector::collect_results(round_results);
                    apply_tool_result_artifact_replacements(
                        &mut results.tool_result_messages,
                        &artifact_replacements,
                    );
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

                    // iter assistant message 已在 execute_round 之前提前持久化 +
                    // emit（见上方），这里只需把 tool_result 落盘。
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
        // 直接对 `final_only_content` 跑 post-process。这里的三件事（max_iter
        // 通知 / empty fallback / strip_hallucinated_xml）语义上都是针对"最终
        // 总结"的处理，而不是跨 iter 累积内容。
        //
        // 历史背景：旧实现把 post-process 跑在 `full_content`（跨 iter 拼接的
        // 累积流）上，再用 "前后长度差" 反推回 `final_only_content`。这套反推
        // 在 strip 遇到未闭合 `<function_calls>` 半标签时翻车——strip 会从那
        // 个位置 truncate 到末尾，导致 full_content 变短，进而触发"empty
        // fallback 替换"分支，把整段累积内容（包含前面已落盘的 iter 文字）
        // 当成 final message 复读一遍存盘。
        //
        // 现在 `full_content` 退化为 safeguard 检查 / Stop hook 输入的旁路缓
        // 冲，不再参与最终回答的组装。
        stage_emitter.completing().await;
        post_process::finalize_content(
            &mut state.final_only_content,
            state.iteration_count,
            config.max_iterations,
            state.stream_cancelled,
        );

        self.query_engine
            .accumulate_usage(state.step_tokens_in, state.step_tokens_out);
        self.query_engine.accumulate_cache_usage(
            state.step_cache_creation_input_tokens,
            state.step_cache_read_input_tokens,
        );

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
        // 取消场景下 final_only_content 通常为空（cancel 发生在 ContentComplete 之前），
        // 写一条 "（已取消）" 占位文字保证用户刷新页面后能看到本轮被取消，而不是
        // 看到一个工具卡片后就空白。
        if state.stream_cancelled && state.final_only_content.trim().is_empty() {
            state.final_only_content = "（已取消）".to_string();
        } else if state.stream_cancelled && !state.final_only_content.contains("（已取消）") {
            state.final_only_content.push_str("\n\n（已取消）");
        }
        let message_id = executor
            .persist_assistant_message(
                config.conversation_id.as_str(),
                &state.final_only_content,
                &[],
                &state.generated_file_ids,
                &state.all_file_metas,
                &state.last_thinking_blocks,
                None,
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
        //
        // 必须 emit final_only_content（与磁盘上 persist_assistant_message 一致），
        // 而不是 full_content。每个 iter 都已通过 iteration assistant message 单独 emit
        // 了自己的 text + toolCalls，如果这里再发 full_content（累积全部 iter 文字），
        // 前端 store 里前面 iter 的文字会被这条 final message 再重复一遍。
        // ⚠️ 已知隐患（不在本次范围）：max_output_tokens recovery 路径下，多次
        // ContentComplete 的 partial 内容只在 state.full_content 内存里，从未通过
        // persist_iteration_assistant_message 落盘——改 emit 后这部分内容刷新前与
        // 刷新后都会消失，前后行为一致；但 disk 丢数据本身是个独立 bug。
        //
        // PR2: 业务终止 outcome 也带 MessageError（让 UI 显红色 callout，让 history 过滤）.
        let outcome_error: Option<crate::storage::file_store::types::MessageError> =
            match &final_outcome {
                ChatTurnOutcome::MaxIterationsReached { iterations } => {
                    Some(crate::storage::file_store::types::MessageError {
                        kind: crate::storage::file_store::types::ErrorKind::MaxIterations,
                        message: format!(
                            "分析步骤超过上限 ({} 次)，已停止。可继续追问深入。",
                            iterations
                        ),
                        raw: None,
                    })
                }
                ChatTurnOutcome::BudgetExceeded {
                    reason,
                    total_cost_usd,
                } => Some(crate::storage::file_store::types::MessageError {
                    kind: crate::storage::file_store::types::ErrorKind::BudgetExceeded,
                    message: format!(
                        "已超出预算（约 ${:.4}），请调整预算或新建会话。",
                        total_cost_usd
                    ),
                    raw: Some(reason.clone()),
                }),
                ChatTurnOutcome::ExecutionError { message } => {
                    Some(crate::storage::file_store::types::MessageError {
                        kind: crate::storage::file_store::types::ErrorKind::ExecutionError,
                        message: "处理过程中发生错误，请重试或换个方式提问。".to_string(),
                        raw: Some(sanitize_error_raw(message)),
                    })
                }
                _ => None,
            };

        let persisted_event = if let Some(err) = outcome_error.clone() {
            RuntimeEvent::message_persisted_with_error(
                session_id.clone(),
                run_id.clone(),
                message_id,
                "assistant",
                serde_json::json!({ "text": state.final_only_content }),
                err,
            )
        } else {
            RuntimeEvent::message_persisted(
                session_id.clone(),
                run_id.clone(),
                message_id,
                "assistant",
                serde_json::json!({ "text": state.final_only_content }),
            )
        };
        self.event_bus.emit(persisted_event).await?;
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
                    total_cache_creation_input_tokens: state.step_cache_creation_input_tokens,
                    total_cache_read_input_tokens: state.step_cache_read_input_tokens,
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
        // ── LTR (B-gap1) Path A exit: before emitting AgentIdle, ask the
        // supervisor whether a SendMessage arrived during this turn's
        // Running window.  If so, emit LeadHasPendingMessages so the
        // transport / front-end knows it should spawn a continuation turn.
        // (Path C — in-process auto-spawn from SendMessage — lands later.)
        //
        // FALLBACK: even if `lead_key_for_path_a` was None at turn entry
        // (because TeamCreate hadn't yet registered the Lead in agent_names),
        // re-resolve here at exit so the very-first turn that *creates* the
        // team also gets a balanced mark_idle.  Without this, supervisor
        // stays Running forever and pending teammate messages never trigger
        // Path A continuation.
        log::info!(
            "[chat_turn_driver][diag] reached exit-block session={} lead_key_for_path_a_is_some={}",
            session_id.as_str(),
            lead_key_for_path_a.is_some()
        );
        let exit_lead_key = if lead_key_for_path_a.is_some() {
            lead_key_for_path_a
        } else if let (Some(sup), Some(names)) = (
            self.query_engine.lead_idle_supervisor(),
            self.query_engine.agent_names(),
        ) {
            if let Some(lead_id) = names
                .resolve(
                    &session_id,
                    self.query_engine
                        .active_team_name(&session_id)
                        .await
                        .unwrap_or_default()
                        .as_str(),
                    crate::runtime::tools::builtin::team_tools::LEAD_NAME,
                )
                .await
            {
                log::info!(
                    "[chat_turn_driver][diag] exit-time lead_key resolved (turn-entry was None) session={} lead={}",
                    session_id.as_str(),
                    lead_id.as_str()
                );
                let _ = sup;
                Some((session_id.clone(), lead_id))
            } else {
                None
            }
        } else {
            None
        };
        if let Some(ref key) = exit_lead_key {
            self.mark_idle_and_maybe_emit_pending(&session_id, &run_id, key)
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

    /// 在错误终态（stream error / PromptTooLong）下补发 Step 6-8 的三件套
    /// 事件，避免前端 chat 区白屏。
    ///
    /// 行为对齐 `run_chat_turn_s4` 主路径的 Step 8（行 2502-2510）+
    /// Step 8 末尾的 AgentIdle emit（行 2596-2604）：
    ///
    /// 1. `MessagePersisted` — 让前端把错误占位文本作为 assistant 消息渲染
    /// 2. `StreamDone`        — 让前端 streamingState.isStreaming 复位
    /// 3. `AgentIdle`         — 让前端解锁输入框，agent 不再"思考中"
    ///
    /// PR1 范围内 `error_text` 是纯字符串占位，直接写入 `MessagePersisted`
    /// 的 `content.text`。PR2 会扩为结构化 `error: Option<MessageError>`
    /// 字段（spec §3.1）。
    ///
    /// `message_id` 通过 `executor.persist_assistant_message` 落盘后取得 —
    /// 与正常路径完全一致，保证 messages.jsonl 不丢条。
    /// 在错误终态（stream error / PromptTooLong / 业务终止）下补发 Step 6-8
    /// 三件套事件，避免前端 chat 区白屏（PR1）+ 携带结构化错误信息（PR2）。
    ///
    /// 行为对齐 `run_chat_turn_s4` 主路径的 Step 8 三件套。
    /// `error` 字段会写到 StoredMessage 顶层 + MessagePersisted event，让前端
    /// 识别后渲染红色 callout；`history.rs::build_chat_history` 装载下一轮
    /// LLM 历史时会过滤掉它，避免错误回灌（spec §3.2）。
    async fn emit_terminal_error_message_and_idle(
        &self,
        executor: &dyn RuntimeLlmExecutor,
        session_id: &SessionId,
        run_id: &RunId,
        conversation_id: &str,
        error_text: &str,
        error: crate::storage::file_store::types::MessageError,
    ) -> anyhow::Result<()> {
        // Step 7：持久化 error 占位为一条 assistant message（含 error 字段落盘 — PR2 收尾）.
        // 这样下次 reload 时前端从 disk 拿到 error 仍能显示红色 callout；
        // history.rs 也能用 stored.error 过滤掉这条不进 LLM context.
        let message_id = executor
            .persist_assistant_message(
                conversation_id,
                error_text,
                &[],
                &[],
                &[],
                &[],
                Some(&error),
            )
            .await
            .map_err(|e| anyhow::anyhow!("{}", e))?;

        // Step 8a：MessagePersisted（前端 message:updated 渲染气泡 + error callout）
        // PR4 Layer 2 诊断：先 clone 出 kind 字符串和 message_id（emit 会消耗 error / message_id）.
        let error_kind_for_diag = serde_json::to_value(&error.kind)
            .ok()
            .and_then(|v| v.as_str().map(|s| s.to_string()));
        let message_id_for_diag = message_id.clone();
        self.event_bus
            .emit(RuntimeEvent::message_persisted_with_error(
                session_id.clone(),
                run_id.clone(),
                message_id,
                "assistant",
                serde_json::json!({ "text": error_text }),
                error,
            ))
            .await?;
        record_diagnostic(
            &crate::telemetry::diagnostics_workspace(),
            DiagnosticEvent::new(
                "streaming.error_message.persisted",
                DiagnosticSource::Backend,
            )
            .conversation_id(session_id.as_str())
            .run_id(run_id.as_str())
            .message_id(message_id_for_diag)
            .ok(false)
            .payload(serde_json::json!({
                "kind": error_kind_for_diag,
            })),
        );

        // Step 8b：StreamDone
        self.event_bus
            .emit(RuntimeEvent::stream_done(
                session_id.clone(),
                run_id.clone(),
            ))
            .await?;

        // Step 8c：AgentIdle
        self.event_bus
            .emit(RuntimeEvent::new(
                session_id.clone(),
                run_id.clone(),
                RuntimeEventKind::AgentIdle {
                    agent_id: AgentId::new(format!("agent-{}", run_id.as_str())),
                    scope: AgentIdleScope::Primary,
                },
            ))
            .await?;

        Ok(())
    }

    /// LTR (B-gap1) Path A — exit helper.
    ///
    /// Calls `mark_idle` on the supervisor and, when it reports `pending ==
    /// true`, emits a [`RuntimeEventKind::LeadHasPendingMessages`] event so
    /// downstream consumers (transport / front-end) can decide to spawn a
    /// continuation turn.  Idempotent and a no-op when the supervisor
    /// reports no pending — the regular `AgentIdle` event still fires.
    async fn mark_idle_and_maybe_emit_pending(
        &self,
        session_id: &SessionId,
        run_id: &RunId,
        key: &crate::runtime::agent::LeadKey,
    ) -> Result<()> {
        let Some(sup) = self.query_engine.lead_idle_supervisor() else {
            return Ok(());
        };
        let ws = crate::telemetry::diagnostics_workspace();
        let active_team = self
            .query_engine
            .active_team_name(session_id)
            .await
            .unwrap_or_default();
        record_diagnostic(
            &ws,
            DiagnosticEvent::new("turn.path_a.mark_idle.entry", DiagnosticSource::Backend)
                .conversation_id(session_id.as_str())
                .run_id(run_id.as_str())
                .agent_id(key.1.as_str())
                .team_name(active_team.as_str()),
        );
        let pending = sup.mark_idle(key).await;
        if pending {
            record_diagnostic(
                &ws,
                DiagnosticEvent::new(
                    "turn.path_a.mark_idle.pending_true",
                    DiagnosticSource::Backend,
                )
                .conversation_id(session_id.as_str())
                .run_id(run_id.as_str())
                .agent_id(key.1.as_str())
                .team_name(active_team.as_str())
                .ok(true)
                .payload(serde_json::json!({ "action": "emitting_lead_has_pending_messages" })),
            );
            self.event_bus
                .emit(RuntimeEvent::new(
                    session_id.clone(),
                    run_id.clone(),
                    RuntimeEventKind::LeadHasPendingMessages {
                        agent_id: key.1.clone(),
                    },
                ))
                .await?;
            // Path A wake: enqueue 一次让 supervisor 走 Idle→Running CAS，触发已
            // 注册的 wake_fn（与 Path C 同一条续接 turn 链路）。否则 pending
            // 信号仅以事件形式 emit，无消费者，Lead 永远不会续接 turn。
            // PR6: team_name 透传给 wake_fn，让 continuation turn 用 wake 来源
            // team_name 而非 conv.json 持久化值（避免读 conv.json 时序问题）。
            let woke = sup.enqueue(key, active_team.clone()).await;
            record_diagnostic(
                &ws,
                DiagnosticEvent::new("turn.path_a.wake_fn_fired", DiagnosticSource::Backend)
                    .conversation_id(session_id.as_str())
                    .run_id(run_id.as_str())
                    .agent_id(key.1.as_str())
                    .team_name(active_team.as_str())
                    .ok(woke)
                    .payload(serde_json::json!({ "transition_won": woke })),
            );
        } else {
            record_diagnostic(
                &ws,
                DiagnosticEvent::new(
                    "turn.path_a.mark_idle.no_pending",
                    DiagnosticSource::Backend,
                )
                .conversation_id(session_id.as_str())
                .run_id(run_id.as_str())
                .agent_id(key.1.as_str())
                .team_name(active_team.as_str())
                .ok(true),
            );
        }
        Ok(())
    }

    /// LTR (B-gap1) Path A — entry helper.
    ///
    /// When the current session is in Team mode (i.e. a Lead has been
    /// registered via `TeamCreate`), mark the supervisor's state machine as
    /// `Running` and return the Lead's `LeadKey = (session, agent_id)` so
    /// the exit half of the turn can `mark_idle` against the same key
    /// without re-resolving the registry.
    ///
    /// Returns `None` in any of these cases (all treated identically: no
    /// Path A wiring this turn):
    ///   - no `LeadIdleSupervisor` injected
    ///   - no `AgentNameRegistry` injected
    ///   - this session has not registered the `team-lead` name yet
    async fn lead_key_and_mark_running(
        &self,
        turn: &TurnState,
    ) -> Option<crate::runtime::agent::LeadKey> {
        let sup = self.query_engine.lead_idle_supervisor()?;
        let names = self.query_engine.agent_names()?;
        let session = turn.session_id().clone();
        let active_team = self
            .query_engine
            .active_team_name(&session)
            .await
            .unwrap_or_default();
        let ws = crate::telemetry::diagnostics_workspace();
        record_diagnostic(
            &ws,
            DiagnosticEvent::new("turn.path_a.mark_running.entry", DiagnosticSource::Backend)
                .conversation_id(session.as_str())
                .team_name(active_team.as_str()),
        );
        let lead_id = names
            .resolve(
                &session,
                active_team.as_str(),
                crate::runtime::tools::builtin::team_tools::LEAD_NAME,
            )
            .await?;
        let key = (session.clone(), lead_id.clone());
        sup.mark_running(&key).await;
        record_diagnostic(
            &ws,
            DiagnosticEvent::new(
                "turn.path_a.mark_running.resolved",
                DiagnosticSource::Backend,
            )
            .conversation_id(session.as_str())
            .agent_id(lead_id.as_str())
            .team_name(active_team.as_str())
            .ok(true)
            .payload(serde_json::json!({ "state": "running" })),
        );
        Some(key)
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

/// 脱敏原始错误文案，避免敏感信息（token / api_key / session）落盘到 messages.jsonl。
/// PR2: 截断 ≤500 字符 + 移除已知敏感 query string 参数。
/// 参考 spec §3.1 raw 字段脱敏约定。
fn sanitize_error_raw(raw: &str) -> String {
    const MAX_LEN: usize = 500;
    const REDACTED: &str = "REDACTED";
    let mut s = raw.to_string();
    // 粗粒度替换：对 known sensitive keys 做 prefix 匹配
    // 使用显式偏移量避免替换后再次匹配 REDACTED 占位符造成无限循环
    for key in &[
        "token=",
        "api_key=",
        "apiKey=",
        "session=",
        "session_key=",
        "sessionKey=",
    ] {
        let mut search_from: usize = 0;
        while let Some(rel_start) = s[search_from..].find(key) {
            let start = search_from + rel_start;
            let value_start = start + key.len();
            let value_end = s[value_start..]
                .find(|c: char| c == '&' || c == ' ' || c == '"' || c == '\\' || c == '\n')
                .map(|i| value_start + i)
                .unwrap_or(s.len());
            s.replace_range(value_start..value_end, REDACTED);
            // 下次搜索从 REDACTED 结尾之后开始，跳过已替换区域
            search_from = value_start + REDACTED.len();
        }
    }
    if s.chars().count() > MAX_LEN {
        s = s.chars().take(MAX_LEN).collect::<String>() + "…";
    }
    s
}

#[cfg(test)]
mod sanitize_error_raw_tests {
    use super::sanitize_error_raw;

    #[test]
    fn truncates_overlong_input() {
        let input = "x".repeat(600);
        let out = sanitize_error_raw(&input);
        assert!(out.chars().count() <= 501); // 500 + 省略号
        assert!(out.ends_with('…'));
    }

    #[test]
    fn redacts_token_query_param() {
        let input = "https://api.example.com/v1/chat?token=abc123&model=claude";
        let out = sanitize_error_raw(input);
        assert!(out.contains("token=REDACTED"));
        assert!(!out.contains("abc123"));
        assert!(
            out.contains("model=claude"),
            "non-sensitive params should be kept: {}",
            out
        );
    }

    #[test]
    fn redacts_api_key_camel_and_snake() {
        let input1 = "Authorization failed: api_key=sk-abc123";
        let out1 = sanitize_error_raw(input1);
        assert!(out1.contains("api_key=REDACTED"));
        assert!(!out1.contains("sk-abc123"));

        let input2 = "?apiKey=sk-xyz789&foo=bar";
        let out2 = sanitize_error_raw(input2);
        assert!(out2.contains("apiKey=REDACTED"));
        assert!(!out2.contains("sk-xyz789"));
    }

    #[test]
    fn keeps_normal_error_text_unchanged() {
        let input = "Chunk timeout (90s) after 10 retries";
        assert_eq!(sanitize_error_raw(input), input);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::chat::tool_round_driver::ToolRoundResult;
    use crate::runtime::chat::tool_round_types::{RuntimeToolCallOutcome, RuntimeToolCallRequest};
    use crate::runtime::events::RuntimeEventKind;
    use crate::runtime::identity::IdentityMapping;
    use crate::runtime::ids::{RunId, ToolCallId};

    #[test]
    fn chat_turn_request_pre_persisted_defaults_false() {
        let req = ChatTurnRequest::new("conv-x", "hello", vec![]);
        assert!(
            !req.pre_persisted,
            "ChatTurnRequest::new must default pre_persisted to false; dispatch path opts in explicitly"
        );
        assert!(
            req.skill_command.is_none(),
            "ChatTurnRequest::new must not imply a selected skill"
        );
    }

    #[test]
    fn v2_image_packaging_ignores_legacy_cloud_model_hint() {
        let settings = ResolvedLlmSettings {
            cloud_gateway_mode: crate::models::settings::CloudGatewayMode::V2,
            cloud_model: "deepseek-v4-pro".to_string(),
            ..ResolvedLlmSettings::default()
        };

        assert!(should_build_image_blocks_for_turn(&settings, false));
    }

    #[test]
    fn resume_turns_never_build_image_blocks() {
        let settings = ResolvedLlmSettings {
            cloud_gateway_mode: crate::models::settings::CloudGatewayMode::V2,
            cloud_model: "claude-sonnet-4-5".to_string(),
            ..ResolvedLlmSettings::default()
        };

        assert!(!should_build_image_blocks_for_turn(&settings, true));
    }

    #[test]
    fn legacy_image_packaging_still_uses_anthropic_vision_allowlist() {
        let deepseek_settings = ResolvedLlmSettings {
            cloud_gateway_mode: crate::models::settings::CloudGatewayMode::Legacy,
            cloud_model: "deepseek-v4-pro".to_string(),
            ..ResolvedLlmSettings::default()
        };
        let claude_settings = ResolvedLlmSettings {
            cloud_gateway_mode: crate::models::settings::CloudGatewayMode::Legacy,
            cloud_model: "claude-sonnet-4-5".to_string(),
            ..ResolvedLlmSettings::default()
        };

        assert!(!should_build_image_blocks_for_turn(
            &deepseek_settings,
            false
        ));
        assert!(should_build_image_blocks_for_turn(&claude_settings, false));
    }

    #[test]
    fn chat_turn_request_pre_persisted_round_trips() {
        let mut req = ChatTurnRequest::new("conv-y", "dispatch prompt body", vec![]);
        req.pre_persisted = true;
        req.skill_command = Some(SkillCommandRef {
            id: "dingtalk-workspace".to_string(),
            label: Some("玩转钉钉".to_string()),
            command: Some("/dingtalk-workspace".to_string()),
        });
        assert!(req.pre_persisted);
        let cloned = req.clone();
        assert!(
            cloned.pre_persisted,
            "Clone must preserve pre_persisted so spawn body sees the flag"
        );
        assert_eq!(
            cloned.skill_command.as_ref().map(|skill| skill.id.as_str()),
            Some("dingtalk-workspace")
        );
    }
    use crate::runtime::interaction::{
        InteractionId, InteractionKind, InteractionRequest, InteractionResolution,
        PendingInteractionControlPlane,
    };
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

    #[test]
    fn render_peer_messages_xml_includes_from_and_variant_and_escapes_body() {
        use crate::runtime::agent::inbox::{InboxItem, MessageSource};
        use crate::runtime::messaging::StructuredMessage;

        let items = vec![
            InboxItem::ChatMessage {
                message: StructuredMessage::text("调研完成 <see report>"),
                source: MessageSource::Teammate("小研".into()),
            },
            InboxItem::ChatMessage {
                message: StructuredMessage::text("ok"),
                source: MessageSource::Lead,
            },
        ];
        let xml = render_peer_messages_xml(&items);
        assert!(xml.starts_with("<peer-messages>"));
        assert!(xml.ends_with("</peer-messages>"));
        assert!(xml.contains("from=\"小研\""));
        assert!(xml.contains("from=\"team-lead\""));
        assert!(xml.contains("variant=\"text\""));
        // Body must be XML-escaped so the LLM doesn't think `<see report>`
        // is another tag and so the document stays well-formed.
        assert!(xml.contains("调研完成 &lt;see report&gt;"));
    }

    #[test]
    fn render_peer_messages_xml_skips_non_chat_items() {
        use crate::runtime::agent::inbox::{InboxItem, ShutdownRequest};

        let items = vec![InboxItem::Shutdown(ShutdownRequest {
            reason: "test".into(),
        })];
        let xml = render_peer_messages_xml(&items);
        // The wrapper is still there but the body is empty (no peer-message).
        assert!(xml.contains("<peer-messages>"));
        assert!(!xml.contains("<peer-message "));
    }

    struct RecordingPermissionControlPlane {
        inserted: Mutex<Vec<PendingPermissionRequest>>,
        resolution: PendingPermissionResolution,
    }

    struct BlockingPermissionControlPlane {
        inserted: Mutex<Vec<PendingPermissionRequest>>,
        senders: Mutex<Vec<oneshot::Sender<PendingPermissionResolution>>>,
        notify: tokio::sync::Notify,
    }

    struct RecordingInteractionControlPlane {
        inserted: Mutex<Vec<InteractionRequest>>,
        resolution: InteractionResolution,
    }

    impl RecordingInteractionControlPlane {
        fn new(resolution: InteractionResolution) -> Self {
            Self {
                inserted: Mutex::new(Vec::new()),
                resolution,
            }
        }
    }

    impl PendingInteractionControlPlane for RecordingInteractionControlPlane {
        fn insert_pending(
            &self,
            request: InteractionRequest,
        ) -> anyhow::Result<oneshot::Receiver<InteractionResolution>> {
            self.inserted.lock().unwrap().push(request);
            let (tx, rx) = oneshot::channel();
            let _ = tx.send(self.resolution.clone());
            Ok(rx)
        }

        fn resolve(
            &self,
            _interaction_id: &InteractionId,
            _resolution: InteractionResolution,
        ) -> anyhow::Result<()> {
            Ok(())
        }

        fn cancel_for_session(&self, _session_id: &str, _message: &str) -> usize {
            0
        }

        fn pending_count_for_session(&self, _session_id: &str) -> usize {
            0
        }

        fn pending_for_session(&self, _session_id: &str) -> Vec<InteractionRequest> {
            Vec::new()
        }

        fn get_pending(&self, interaction_id: &InteractionId) -> Option<InteractionRequest> {
            self.inserted
                .lock()
                .unwrap()
                .iter()
                .find(|request| request.interaction_id == *interaction_id)
                .cloned()
        }

        fn is_pending(&self, _interaction_id: &InteractionId) -> bool {
            false
        }
    }

    struct RecordingRunActivityController {
        calls: Mutex<Vec<String>>,
    }

    impl RecordingRunActivityController {
        fn new() -> Self {
            Self {
                calls: Mutex::new(Vec::new()),
            }
        }

        fn calls(&self) -> Vec<String> {
            self.calls.lock().unwrap().clone()
        }
    }

    #[async_trait]
    impl RunActivityController for RecordingRunActivityController {
        async fn suspend_for_user_interaction(
            &self,
            session_id: &SessionId,
            run_id: &RunId,
        ) -> anyhow::Result<()> {
            self.calls.lock().unwrap().push(format!(
                "suspend:{}:{}",
                session_id.as_str(),
                run_id.as_str()
            ));
            Ok(())
        }

        async fn resume_after_user_interaction(
            &self,
            session_id: &SessionId,
            run_id: &RunId,
            _cancel: &CancellationToken,
        ) -> anyhow::Result<()> {
            self.calls.lock().unwrap().push(format!(
                "resume:{}:{}",
                session_id.as_str(),
                run_id.as_str()
            ));
            Ok(())
        }
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

    impl BlockingPermissionControlPlane {
        fn new() -> Self {
            Self {
                inserted: Mutex::new(Vec::new()),
                senders: Mutex::new(Vec::new()),
                notify: tokio::sync::Notify::new(),
            }
        }

        fn inserted_requests(&self) -> Vec<PendingPermissionRequest> {
            self.inserted.lock().unwrap().clone()
        }

        async fn wait_for_inserted_count(&self, expected: usize) {
            loop {
                if self.inserted.lock().unwrap().len() >= expected {
                    return;
                }
                self.notify.notified().await;
            }
        }

        fn resolve_all(&self, resolution: PendingPermissionResolution) {
            for sender in self.senders.lock().unwrap().drain(..) {
                let _ = sender.send(resolution.clone());
            }
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

        fn is_pending(&self, _tool_call_id: &ToolCallId) -> bool {
            false
        }
    }

    impl PendingPermissionControlPlane for BlockingPermissionControlPlane {
        fn insert_pending_request(
            &self,
            request: PendingPermissionRequest,
        ) -> anyhow::Result<oneshot::Receiver<PendingPermissionResolution>> {
            self.inserted.lock().unwrap().push(request);
            let (tx, rx) = oneshot::channel();
            self.senders.lock().unwrap().push(tx);
            self.notify.notify_waiters();
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
            self.inserted.lock().unwrap().len()
        }

        fn is_pending(&self, _tool_call_id: &ToolCallId) -> bool {
            false
        }
    }

    #[test]
    fn chat_turn_request_new_defaults_permission_mode() {
        let request = ChatTurnRequest::new("conv-chat-mode", "hello", vec![]);

        assert_eq!(request.permission_mode, PermissionMode::Default);
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
    fn build_user_content_json_includes_skill_command() {
        let skill = SkillCommandRef {
            id: "dingtalk-workspace".to_string(),
            label: Some("玩转钉钉".to_string()),
            command: Some("/dingtalk-workspace".to_string()),
        };

        let content = build_user_content_json_with_skill("查今天日程", &[], Some(&skill));

        assert_eq!(content["text"].as_str(), Some("查今天日程"));
        assert_eq!(content["commandText"].as_str(), Some("/dingtalk-workspace"));
        assert_eq!(
            content["skillCommand"]["id"].as_str(),
            Some("dingtalk-workspace")
        );
        assert_eq!(content["skillCommand"]["label"].as_str(), Some("玩转钉钉"));
        assert_eq!(
            content["skillCommand"]["command"].as_str(),
            Some("/dingtalk-workspace")
        );
    }

    #[test]
    fn selected_skill_instruction_mentions_skill_tool_and_id() {
        let skill = SkillCommandRef {
            id: "dingtalk-workspace".to_string(),
            label: Some("玩转钉钉".to_string()),
            command: Some("/dingtalk-workspace".to_string()),
        };

        let instruction = selected_skill_instruction(Some(&skill)).expect("instruction");

        assert!(instruction.contains("dingtalk-workspace"));
        assert!(instruction.contains("玩转钉钉"));
        assert!(instruction.contains("Skill({ skill_id: \"dingtalk-workspace\" })"));
        assert!(instruction.contains("不要使用 label 作为 skill_id"));
        assert!(!instruction.contains("skill_id: \"玩转钉钉\""));
    }

    #[tokio::test]
    async fn resolve_permission_asks_uses_runtime_permission_mode_instead_of_decision_reason() {
        let bus = RuntimeEventBus::new();
        let control_plane = Arc::new(RecordingPermissionControlPlane::new(
            PendingPermissionResolution::Deny {
                message: "Denied by test".to_string(),
                remember: false,
                destination: None,
                path_auth_scope_override: None,
            },
        ));
        let driver = RuntimeChatTurnDriver {
            query_engine: QueryEngine::new(),
            event_bus: bus.clone(),
            llm_executor: None,
            pending_permission_control_plane: Some(control_plane.clone()),
            pending_interaction_control_plane: None,
            task_notification_queue: None,
            compact_client: None,
            turn_stage_path_resolver: None,
            run_activity_controller: None,
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
                path_auth_scope: None,
            },
        })];

        let _resolved = driver
            .resolve_permission_asks(&turn, &turn.cancellation(), round_results, None)
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

    #[tokio::test]
    async fn resolve_permission_asks_registers_same_round_asks_before_waiting() {
        let bus = RuntimeEventBus::new();
        let control_plane = Arc::new(BlockingPermissionControlPlane::new());
        let driver = RuntimeChatTurnDriver {
            query_engine: QueryEngine::new(),
            event_bus: bus.clone(),
            llm_executor: None,
            pending_permission_control_plane: Some(control_plane.clone()),
            pending_interaction_control_plane: None,
            task_notification_queue: None,
            compact_client: None,
            turn_stage_path_resolver: None,
            run_activity_controller: None,
        };
        let turn = TurnState::new(
            IdentityMapping::from_legacy_conversation_id("conv-batch-ask".to_string()),
            RunId::new("run-batch-ask"),
            "hello".to_string(),
        );
        let make_ask = |tool_call_id: &str, pattern: &str| {
            ToolRoundResult::Ok(RuntimeToolCallOutcome::AskRequired {
                tool_call_id: tool_call_id.to_string(),
                tool_name: "Glob".to_string(),
                capability_scopes: vec!["fs:read".to_string()],
                original_request: RuntimeToolCallRequest {
                    tool_call_id: tool_call_id.to_string(),
                    tool_name: "Glob".to_string(),
                    args: json!({"path":"/tmp", "pattern": pattern}),
                    purpose: None,
                },
                decision: PermissionDecision::Ask {
                    message: "该路径未授权，需要用户确认：路径=/private/tmp".to_string(),
                    suggestions: vec!["仅本次允许".to_string(), "拒绝".to_string()],
                    remember_options: vec![],
                    default_destination: None,
                    reason: PermissionReason::Other("test".to_string()),
                    path_auth_scope: Some("/private/tmp".to_string()),
                },
            })
        };
        let round_results = vec![
            make_ask("tc-batch-1", "*claw"),
            make_ask("tc-batch-2", "*opan"),
        ];

        let task = tokio::spawn(async move {
            driver
                .resolve_permission_asks(&turn, &turn.cancellation(), round_results, None)
                .await
        });

        tokio::time::timeout(
            std::time::Duration::from_millis(200),
            control_plane.wait_for_inserted_count(2),
        )
        .await
        .expect("all same-round permission asks should be registered before waiting for a reply");

        let inserted = control_plane.inserted_requests();
        assert_eq!(inserted.len(), 2);
        let ask_events = bus
            .recorded()
            .into_iter()
            .filter(|event| matches!(event.kind, RuntimeEventKind::PermissionAskRequired { .. }))
            .count();
        assert_eq!(ask_events, 2);

        control_plane.resolve_all(PendingPermissionResolution::Deny {
            message: "Denied by test".to_string(),
            remember: false,
            destination: None,
            path_auth_scope_override: None,
        });
        let _ = task.await.expect("permission task should join");
    }

    #[tokio::test]
    async fn resolve_permission_asks_suspends_active_run_while_waiting_for_user() {
        let bus = RuntimeEventBus::new();
        let control_plane = Arc::new(RecordingPermissionControlPlane::new(
            PendingPermissionResolution::Deny {
                message: "Denied by test".to_string(),
                remember: false,
                destination: None,
                path_auth_scope_override: None,
            },
        ));
        let activity = Arc::new(RecordingRunActivityController::new());
        let driver = RuntimeChatTurnDriver {
            query_engine: QueryEngine::new(),
            event_bus: bus,
            llm_executor: None,
            pending_permission_control_plane: Some(control_plane),
            pending_interaction_control_plane: None,
            task_notification_queue: None,
            compact_client: None,
            turn_stage_path_resolver: None,
            run_activity_controller: Some(activity.clone()),
        };
        let turn = TurnState::new(
            IdentityMapping::from_legacy_conversation_id("conv-wait".to_string()),
            RunId::new("run-wait"),
            "hello".to_string(),
        );
        let round_results = vec![ToolRoundResult::Ok(RuntimeToolCallOutcome::AskRequired {
            tool_call_id: "tc-wait".to_string(),
            tool_name: "Read".to_string(),
            capability_scopes: vec!["fs:read".to_string()],
            original_request: RuntimeToolCallRequest {
                tool_call_id: "tc-wait".to_string(),
                tool_name: "Read".to_string(),
                args: json!({"file_path":"/tmp/secret.txt"}),
                purpose: None,
            },
            decision: PermissionDecision::Ask {
                message: "need approval".to_string(),
                suggestions: vec![],
                remember_options: vec![],
                default_destination: None,
                reason: PermissionReason::Other("test".to_string()),
                path_auth_scope: None,
            },
        })];

        let _resolved = driver
            .resolve_permission_asks(&turn, &turn.cancellation(), round_results, None)
            .await
            .expect("permission ask should resolve");

        assert_eq!(
            activity.calls(),
            vec![
                "suspend:conv-wait:run-wait".to_string(),
                "resume:conv-wait:run-wait".to_string(),
            ]
        );
    }

    #[tokio::test]
    async fn resolve_interaction_requests_suspends_active_run_while_waiting_for_user() {
        let bus = RuntimeEventBus::new();
        let control_plane = Arc::new(RecordingInteractionControlPlane::new(
            InteractionResolution::Cancel {
                message: "Cancelled by test".to_string(),
            },
        ));
        let activity = Arc::new(RecordingRunActivityController::new());
        let driver = RuntimeChatTurnDriver {
            query_engine: QueryEngine::new(),
            event_bus: bus,
            llm_executor: None,
            pending_permission_control_plane: None,
            pending_interaction_control_plane: Some(control_plane),
            task_notification_queue: None,
            compact_client: None,
            turn_stage_path_resolver: None,
            run_activity_controller: Some(activity.clone()),
        };
        let turn = TurnState::new(
            IdentityMapping::from_legacy_conversation_id("conv-question".to_string()),
            RunId::new("run-question"),
            "hello".to_string(),
        );
        let original_request = RuntimeToolCallRequest {
            tool_call_id: "tool-question".to_string(),
            tool_name: "AskUserQuestion".to_string(),
            args: json!({"questions":[{"question":"看哪个文件？"}]}),
            purpose: None,
        };
        let round_results = vec![ToolRoundResult::Ok(
            RuntimeToolCallOutcome::InteractionRequired {
                tool_call_id: "tool-question".to_string(),
                tool_name: "AskUserQuestion".to_string(),
                original_request: original_request.clone(),
                interaction_request: InteractionRequest {
                    interaction_id: InteractionId::new("ask-question"),
                    session_id: turn.session_id().clone(),
                    run_id: turn.run_id().clone(),
                    tool_call_id: ToolCallId::new("tool-question"),
                    tool_name: "AskUserQuestion".to_string(),
                    kind: InteractionKind::AskUserQuestion,
                    payload: json!({"questions":[{"question":"看哪个文件？"}]}),
                    original_request,
                    turn_origin: turn.turn_origin().clone(),
                    output_binding: turn.output_binding().clone(),
                },
            },
        )];

        let _resolved = driver
            .resolve_interaction_requests(&turn, &turn.cancellation(), round_results)
            .await
            .expect("interaction should resolve");

        assert_eq!(
            activity.calls(),
            vec![
                "suspend:conv-question:run-question".to_string(),
                "resume:conv-question:run-question".to_string(),
            ]
        );
    }

    #[tokio::test]
    async fn resolve_permission_asks_persists_waiting_permission_stage() {
        let bus = RuntimeEventBus::new();
        let control_plane = Arc::new(RecordingPermissionControlPlane::new(
            PendingPermissionResolution::Deny {
                message: "Denied by test".to_string(),
                remember: false,
                destination: None,
                path_auth_scope_override: None,
            },
        ));
        let driver = RuntimeChatTurnDriver {
            query_engine: QueryEngine::new(),
            event_bus: bus.clone(),
            llm_executor: None,
            pending_permission_control_plane: Some(control_plane),
            pending_interaction_control_plane: None,
            task_notification_queue: None,
            compact_client: None,
            turn_stage_path_resolver: None,
            run_activity_controller: None,
        };
        let turn = TurnState::new(
            IdentityMapping::from_legacy_conversation_id("conv-stage".to_string()),
            RunId::new("run-stage"),
            "hello".to_string(),
        );
        let tmp = tempfile::tempdir().unwrap();
        let stage_path = tmp.path().join("turn_stages").join("conv-stage.json");
        let stage_emitter =
            TurnStageEmitter::new(bus, turn.session_id().clone(), turn.run_id().clone())
                .with_enabled(true)
                .with_persist_path(stage_path.clone());
        stage_emitter
            .tools_started(
                0,
                vec![RunningTool {
                    tool_name: "Read".to_string(),
                    tool_call_id: "tc-stage".to_string(),
                    started_at_ms: 1,
                }],
            )
            .await;

        let round_results = vec![ToolRoundResult::Ok(RuntimeToolCallOutcome::AskRequired {
            tool_call_id: "tc-stage".to_string(),
            tool_name: "Read".to_string(),
            capability_scopes: vec!["fs:read".to_string()],
            original_request: RuntimeToolCallRequest {
                tool_call_id: "tc-stage".to_string(),
                tool_name: "Read".to_string(),
                args: json!({"file_path":"/tmp/secret.txt"}),
                purpose: None,
            },
            decision: PermissionDecision::Ask {
                message: "need approval".to_string(),
                suggestions: vec!["Allow once".to_string()],
                remember_options: vec![],
                default_destination: None,
                reason: PermissionReason::Other("test".to_string()),
                path_auth_scope: None,
            },
        })];

        let _resolved = driver
            .resolve_permission_asks(
                &turn,
                &turn.cancellation(),
                round_results,
                Some(&stage_emitter),
            )
            .await
            .expect("ask resolution should succeed");

        let raw = std::fs::read(&stage_path).expect("turn stage should be persisted");
        let parsed: crate::runtime::chat::turn_stage::PersistedTurnStage =
            serde_json::from_slice(&raw).expect("parse turn stage");
        match parsed.stage {
            crate::runtime::events::TurnStage::WaitingPermission {
                tool_name,
                tool_call_id,
            } => {
                assert_eq!(tool_name, "Read");
                assert_eq!(tool_call_id, "tc-stage");
            }
            other => panic!("expected persisted waitingPermission, got {other:?}"),
        }
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
        override_system_prompt: Option<String>,
    }

    impl SnapshotPromptExecutor {
        fn new() -> Self {
            Self {
                legacy_calls: AtomicUsize::new(0),
                seen_system_prompts: Mutex::new(Vec::new()),
                seen_dynamic_contexts: Mutex::new(Vec::new()),
                skill_catalog: None,
                override_system_prompt: None,
            }
        }

        fn with_skill_catalog(skill_catalog: impl Into<String>) -> Self {
            Self {
                legacy_calls: AtomicUsize::new(0),
                seen_system_prompts: Mutex::new(Vec::new()),
                seen_dynamic_contexts: Mutex::new(Vec::new()),
                skill_catalog: Some(skill_catalog.into()),
                override_system_prompt: None,
            }
        }

        fn with_override_system_prompt(mut self, system_prompt: impl Into<String>) -> Self {
            self.override_system_prompt = Some(system_prompt.into());
            self
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
                thinking_blocks: Vec::new(),
                tokens_in: 0,
                tokens_out: 0,
                cache_creation_input_tokens: 0,
                cache_read_input_tokens: 0,
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
            _thinking_blocks: &[serde_json::Value],
            _error: Option<&crate::storage::file_store::types::MessageError>,
        ) -> Result<String, TurnError> {
            Ok("assistant-msg".to_string())
        }

        async fn persist_user_message(
            &self,
            _conversation_id: &str,
            _content: &str,
            _attachments: &[ChatAttachmentRef],
            _skill_command: Option<&SkillCommandRef>,
            _client_message_id: Option<&str>,
        ) -> Result<String, TurnError> {
            Ok("user-msg".to_string())
        }

        async fn build_prompt_snapshot(
            &self,
            _request: &ChatTurnRequest,
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

        async fn build_system_prompt(
            &self,
            _request: &ChatTurnRequest,
        ) -> Result<String, TurnError> {
            self.legacy_calls.fetch_add(1, Ordering::SeqCst);
            Ok("legacy prompt should not be used".to_string())
        }

        async fn load_workspace_path(&self) -> Result<PathBuf, TurnError> {
            Ok(std::env::temp_dir())
        }

        async fn get_skill_catalog(&self, _agent_id: Option<&str>) -> String {
            self.skill_catalog.clone().unwrap_or_default()
        }

        async fn load_turn_config_overrides(
            &self,
            _request: &ChatTurnRequest,
        ) -> Result<TurnConfigOverrides, TurnError> {
            Ok(TurnConfigOverrides {
                system_prompt: self.override_system_prompt.clone(),
                ..TurnConfigOverrides::default()
            })
        }

        async fn get_tool_defs(&self) -> Result<Vec<serde_json::Value>, TurnError> {
            Ok(vec![]) // 显式声明此 mock 不关心 tool_defs
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
    async fn driver_keeps_prompt_snapshot_when_system_prompt_override_is_set() {
        let executor =
            Arc::new(SnapshotPromptExecutor::new().with_override_system_prompt("override prompt"));
        let bus = RuntimeEventBus::new();
        let driver =
            RuntimeChatTurnDriver::with_llm_executor(QueryEngine::new(), bus, executor.clone());
        let mut turn = TurnState::new(
            IdentityMapping::from_legacy_conversation_id(
                "conv-driver-snapshot-override".to_string(),
            ),
            RunId::new("run-driver-snapshot-override"),
            "use snapshot".to_string(),
        );
        let request = ChatTurnRequest::new("conv-driver-snapshot-override", "use snapshot", vec![]);

        driver
            .run_chat_turn(&mut turn, &request)
            .await
            .expect("driver should run with prompt snapshot");

        let prompts = executor.seen_system_prompts.lock().unwrap().clone();
        assert_eq!(
            prompts,
            vec!["snapshot static\n\nsnapshot dynamic".to_string()]
        );
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

    #[tokio::test]
    async fn driver_injects_channel_context_into_dynamic_context() {
        let executor = Arc::new(SnapshotPromptExecutor::new());
        let bus = RuntimeEventBus::new();
        let driver =
            RuntimeChatTurnDriver::with_llm_executor(QueryEngine::new(), bus, executor.clone());
        let mut turn = TurnState::new(
            IdentityMapping::from_legacy_conversation_id("conv-driver-channel-context".to_string()),
            RunId::new("run-driver-channel-context"),
            "需要授权".to_string(),
        );
        let mut request = ChatTurnRequest::new("conv-driver-channel-context", "需要授权", vec![]);
        request.channel_context =
            Some("当前请求来自 IM/移动端渠道。请输出完整授权链接。".to_string());

        driver
            .run_chat_turn(&mut turn, &request)
            .await
            .expect("driver should run with channel context");

        let dynamic_contexts = executor.seen_dynamic_contexts.lock().unwrap().clone();
        assert_eq!(dynamic_contexts.len(), 1);
        assert!(dynamic_contexts[0].contains("IM/移动端渠道"));
        assert!(dynamic_contexts[0].contains("完整授权链接"));
    }

    #[tokio::test]
    async fn driver_injects_selected_skill_instruction_into_dynamic_context() {
        let executor = Arc::new(SnapshotPromptExecutor::with_skill_catalog(
            "## 可用专项技能\n- `dingtalk-workspace` — 玩转钉钉",
        ));
        let bus = RuntimeEventBus::new();
        let driver =
            RuntimeChatTurnDriver::with_llm_executor(QueryEngine::new(), bus, executor.clone());
        let mut turn = TurnState::new(
            IdentityMapping::from_legacy_conversation_id("conv-driver-selected-skill".to_string()),
            RunId::new("run-driver-selected-skill"),
            "查今天日程".to_string(),
        );
        let mut request = ChatTurnRequest::new("conv-driver-selected-skill", "查今天日程", vec![]);
        request.skill_command = Some(SkillCommandRef {
            id: "dingtalk-workspace".to_string(),
            label: Some("玩转钉钉".to_string()),
            command: Some("/dingtalk-workspace".to_string()),
        });

        driver
            .run_chat_turn(&mut turn, &request)
            .await
            .expect("driver should run with selected skill");

        let dynamic_contexts = executor.seen_dynamic_contexts.lock().unwrap().clone();
        assert_eq!(dynamic_contexts.len(), 1);
        assert!(dynamic_contexts[0].contains("dingtalk-workspace"));
        assert!(dynamic_contexts[0].contains("玩转钉钉"));
        assert!(dynamic_contexts[0].contains("Skill({ skill_id: \"dingtalk-workspace\" })"));
        assert!(!dynamic_contexts[0].contains("skill_id: \"玩转钉钉\""));
    }

    // ── LTR (B-gap1) Path A wiring tests ─────────────────────────────────────

    use crate::runtime::agent::{AgentNameRegistry, LeadIdleSupervisor};
    use crate::runtime::event_bus::RuntimeEventSubscriber;
    use crate::runtime::ids::AgentId;
    use crate::runtime::tools::builtin::team_tools::LEAD_NAME;

    /// Subscriber that captures every emitted event into a Vec.
    struct CapturingSubscriber {
        events: Mutex<Vec<RuntimeEventKind>>,
    }
    impl CapturingSubscriber {
        fn new() -> Arc<Self> {
            Arc::new(Self {
                events: Mutex::new(Vec::new()),
            })
        }
        fn snapshot(&self) -> Vec<RuntimeEventKind> {
            self.events.lock().unwrap().clone()
        }
    }
    #[async_trait]
    impl RuntimeEventSubscriber for CapturingSubscriber {
        async fn on_event(&self, event: &RuntimeEvent) -> anyhow::Result<()> {
            self.events.lock().unwrap().push(event.kind.clone());
            Ok(())
        }
    }

    /// Build a driver wired with a supervisor + name registry, plus a
    /// CapturingSubscriber on its bus, and pre-register the Lead.
    /// Returns `(driver, capture, lead_key)`.
    async fn build_driver_with_lead(
        session: &str,
        lead_id: &str,
    ) -> (
        RuntimeChatTurnDriver,
        Arc<CapturingSubscriber>,
        crate::runtime::agent::LeadKey,
    ) {
        let supervisor = LeadIdleSupervisor::new();
        let names = AgentNameRegistry::new();
        let session_id = SessionId::new(session);
        let lead_agent = AgentId::new(lead_id);
        names
            .register(&session_id, "", LEAD_NAME, lead_agent.clone())
            .await
            .expect("Lead registration should succeed in a fresh fixture");

        let bus = RuntimeEventBus::new();
        let capture = CapturingSubscriber::new();
        bus.subscribe(capture.clone());

        let qe = QueryEngine::new()
            .with_lead_idle(supervisor)
            .with_agent_names(names);

        let driver = RuntimeChatTurnDriver::new(qe, bus);
        let key = (session_id, lead_agent);
        (driver, capture, key)
    }

    /// Path A entry resolves the Lead key and flips the supervisor to Running.
    #[tokio::test]
    async fn path_a_entry_resolves_lead_key_and_marks_running() {
        let (driver, _capture, key) = build_driver_with_lead("conv-pa-entry", "lead-1").await;
        let turn = TurnState::new(
            IdentityMapping::from_legacy_conversation_id("conv-pa-entry".to_string()),
            RunId::new("run-pa-entry"),
            String::new(),
        );
        let resolved = driver.lead_key_and_mark_running(&turn).await;
        assert_eq!(resolved.as_ref(), Some(&key));
        let sup = driver
            .query_engine
            .lead_idle_supervisor()
            .expect("supervisor wired");
        assert_eq!(sup.state_of(&key).await, Some("running"));
    }

    /// When the session is not in Team mode (no `team-lead` registered),
    /// Path A entry returns None and never touches the supervisor.
    #[tokio::test]
    async fn path_a_entry_returns_none_when_no_lead_registered() {
        let supervisor = LeadIdleSupervisor::new();
        let names = AgentNameRegistry::new();
        let bus = RuntimeEventBus::new();
        let qe = QueryEngine::new()
            .with_lead_idle(supervisor)
            .with_agent_names(names);
        let driver = RuntimeChatTurnDriver::new(qe, bus);
        let turn = TurnState::new(
            IdentityMapping::from_legacy_conversation_id("conv-no-team".to_string()),
            RunId::new("run-no-team"),
            String::new(),
        );
        assert!(driver.lead_key_and_mark_running(&turn).await.is_none());
    }

    /// Path A exit emits LeadHasPendingMessages when supervisor reports
    /// pending == true (i.e. SendMessage arrived during the Running window).
    #[tokio::test]
    async fn path_a_exit_emits_pending_event_when_send_arrived_during_run() {
        let (driver, capture, key) = build_driver_with_lead("conv-pa-exit-pending", "lead-2").await;
        let sup = driver
            .query_engine
            .lead_idle_supervisor()
            .expect("supervisor wired")
            .clone();
        // Simulate a turn: mark_running, a Teammate enqueues a message,
        // then mark_idle reports pending=true.
        sup.mark_running(&key).await;
        sup.enqueue(&key, "default".to_string()).await;

        driver
            .mark_idle_and_maybe_emit_pending(&key.0, &RunId::new("run-pa-exit-1"), &key)
            .await
            .expect("emit must succeed");

        let kinds = capture.snapshot();
        assert!(
            matches!(
                kinds.first(),
                Some(RuntimeEventKind::LeadHasPendingMessages { agent_id }) if agent_id == &key.1
            ),
            "expected LeadHasPendingMessages first, got: {:?}",
            kinds
        );
    }

    /// Path A exit does NOT emit LeadHasPendingMessages when no SendMessage
    /// arrived during the Running window — the regular AgentIdle path
    /// proceeds normally.
    #[tokio::test]
    async fn path_a_exit_quiet_when_no_send_during_run() {
        let (driver, capture, key) = build_driver_with_lead("conv-pa-exit-clean", "lead-3").await;
        let sup = driver
            .query_engine
            .lead_idle_supervisor()
            .expect("supervisor wired")
            .clone();
        sup.mark_running(&key).await;
        // No enqueue.

        driver
            .mark_idle_and_maybe_emit_pending(&key.0, &RunId::new("run-pa-exit-2"), &key)
            .await
            .expect("emit must succeed");

        let kinds = capture.snapshot();
        assert!(
            !kinds
                .iter()
                .any(|k| matches!(k, RuntimeEventKind::LeadHasPendingMessages { .. })),
            "should NOT emit pending event, got: {:?}",
            kinds
        );
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
