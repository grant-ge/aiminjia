use serde::{Deserialize, Serialize};

use crate::runtime::chat::ChatTurnOutcome;
use crate::runtime::ids::{AgentId, RunId, SessionId, TaskId, ToolCallId};
use crate::runtime::tools::permission::{PermissionDestination, PermissionMode};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum AgentIdleScope {
    Primary,
    Child,
}

/// Why the stream is being retried. Drives frontend toast wording so we don't
/// blame the user's network for upstream 5xx / rate-limit failures.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum RetryReason {
    /// Upstream gateway returned 5xx — service-side problem, not the user's network.
    UpstreamBusy,
    /// Upstream returned 429 / rate limit.
    RateLimited,
    /// Local-side network issue: timeout, connection reset, broken pipe, chunk stall.
    #[default]
    NetworkFlap,
    /// Stream retries exhausted; switching to non-streaming send fallback (PR3).
    /// Frontend should show "切换备用通道" instead of "重连中".
    FallbackToNonStream,
}

/// One tool that the agent is currently executing as part of a `TurnStage::Tools`
/// batch.  `started_at_ms` lets the frontend display elapsed time per tool.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RunningTool {
    pub tool_name: String,
    pub tool_call_id: String,
    pub started_at_ms: u64,
}

/// The agent's current macro state inside a single turn.  Emitted via
/// `TurnStageChanged` on every transition so the UI never has to derive
/// "what is the agent doing right now" from disparate events.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum TurnStage {
    /// Turn just started; building prompt / loading history.
    Submitted,
    /// LLM request in flight, no first token yet.
    #[serde(rename_all = "camelCase")]
    WaitingLlm { iteration: u32 },
    /// LLM is streaming tokens.
    #[serde(rename_all = "camelCase")]
    Streaming { iteration: u32 },
    /// A batch of tool calls is being dispatched / executed.
    #[serde(rename_all = "camelCase")]
    Tools {
        iteration: u32,
        running: Vec<RunningTool>,
        completed_in_batch: u32,
    },
    /// Blocked waiting on a user permission decision.
    #[serde(rename_all = "camelCase")]
    WaitingPermission {
        tool_name: String,
        tool_call_id: String,
    },
    /// Blocked waiting on a user interaction (AskUserQuestion, etc).
    #[serde(rename_all = "camelCase")]
    WaitingInteraction {
        interaction_kind: String,
        interaction_id: String,
    },
    /// Context compaction is running.
    Compacting,
    /// Final persistence + post-processing before TurnCompleted.
    Completing,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum RuntimeEventKind {
    RunStarted,
    StreamStarted,
    StreamDelta {
        content: String,
    },
    StreamDone,
    StreamRetryReset {
        #[serde(default)]
        reason: RetryReason,
    },
    StreamError {
        error: String,
        raw_error: Option<String>,
    },
    ToolCallExecuting {
        tool_call_id: ToolCallId,
        tool_name: String,
        input: serde_json::Value,
    },
    ToolCallCompleted {
        tool_call_id: ToolCallId,
        tool_name: String,
        /// Whether the tool execution ended in an error.
        is_error: bool,
        /// Tool output text content.
        content: String,
        /// Message id, used by the frontend to upsert the tool result message.
        msg_id: String,
        /// Optional wall-clock duration of the tool execution in milliseconds.
        duration_ms: Option<u64>,
    },
    /// Live stdout/stderr tail emitted by long-running shell tools (Bash /
    /// PowerShell). Throttled to ~500ms so a chatty command doesn't saturate
    /// the IPC bus. The frontend renders this in-line under the running tool
    /// card so users can see progress instead of a silent spinner for the
    /// whole timeout window. Sent in coalesced "latest tail" form (watch
    /// channel semantics) — clients should treat each event as the current
    /// snapshot, not as an append.
    ToolProgress {
        tool_call_id: ToolCallId,
        /// Most recent N lines of merged stdout/stderr (UTF-8 safe truncation).
        stdout_tail: String,
        /// Total bytes captured so far (including bytes not in `stdout_tail`).
        /// Lets the UI show "已收到 12.4 KB" without keeping its own counter.
        total_bytes: u64,
    },
    PermissionAskRequired {
        tool_call_id: ToolCallId,
        tool_name: String,
        message: String,
        suggestions: Vec<String>,
        mode: PermissionMode,
        remember_options: Vec<PermissionDestination>,
        default_destination: Option<PermissionDestination>,
        primary_model: String,
    },
    UserInteractionRequired {
        interaction_id: crate::runtime::interaction::InteractionId,
        tool_call_id: ToolCallId,
        tool_name: String,
        kind: crate::runtime::interaction::InteractionKind,
        payload: serde_json::Value,
        primary_model: String,
    },
    UserInteractionResolved {
        interaction_id: crate::runtime::interaction::InteractionId,
    },
    AgentIdle {
        agent_id: AgentId,
        scope: AgentIdleScope,
    },
    /// LTR (B-gap1): emitted by chat_turn_driver Path A when the Lead's turn
    /// is about to end but the LeadIdleSupervisor reports `pending == true`
    /// — at least one Teammate sent a message during the just-finished
    /// Running window.  The transport layer (or front-end) is responsible
    /// for spawning a continuation turn (typically by re-sending the
    /// `__resume_from_task_notification__` sentinel).  The Path C (in-process
    /// auto-spawn) wiring lands in a follow-up commit.
    LeadHasPendingMessages {
        agent_id: AgentId,
    },
    TaskStatusChanged {
        task_id: TaskId,
        status: String,
        subject: String,
        active_form: Option<String>,
        owner_agent_id: Option<AgentId>,
    },
    StopHookPreventedContinuation {
        reason: Option<String>,
    },
    OrphanedPermissionDetected {
        count: usize,
    },
    MessagePersisted {
        message_id: String,
        role: String,
        content: serde_json::Value,
        client_message_id: Option<String>,
        /// Optional `toolCalls` array carried on assistant messages that issued
        /// tool calls.  When present the transport layer forwards it to the
        /// frontend so streaming UI can render tool-call inputs without waiting
        /// for the conversation history to be reloaded.
        tool_calls: Option<Vec<serde_json::Value>>,
        /// Optional structured error for assistant messages that surfaced a
        /// terminal error to the user (PR2). When present, the transport
        /// layer forwards it as the `error` field on `message:updated`, and
        /// `history.rs::build_chat_history` filters this message out before
        /// sending history back to the LLM.
        error: Option<crate::storage::file_store::types::MessageError>,
    },
    PendingSnapshot {
        items: Vec<crate::runtime::pending::PendingItem>,
    },
    PendingQueued {
        item: crate::runtime::pending::PendingItem,
    },
    PendingDrained {
        drained_ids: Vec<String>,
    },
    PendingRemoved {
        item_id: String,
    },
    TurnCompleted {
        outcome: ChatTurnOutcome,
        total_input_tokens: u64,
        total_output_tokens: u64,
        /// Anthropic-style accumulated prompt-cache write tokens for this turn.
        #[serde(default)]
        total_cache_creation_input_tokens: u64,
        /// Anthropic-style accumulated prompt-cache read tokens for this turn.
        #[serde(default)]
        total_cache_read_input_tokens: u64,
        total_cost_usd: Option<f64>,
        permission_denial_count: usize,
    },
    RunCancelled,
    RunCompleted,
    /// Auto-compact completed successfully.  Carries token savings so the
    /// frontend can show a compaction summary (e.g. "保存了 X 个 token").
    CompactCompleted {
        conversation_id: String,
        boundary_id: String,
        trigger: String,
        created_at: String,
        tail_message_id: Option<String>,
        pre_tokens: u64,
        post_tokens: u64,
        messages_summarized: usize,
    },
    /// Macro-state of the current turn changed.  Always emitted alongside the
    /// existing fine-grained events; UI uses this as the single source of truth
    /// for the "what is the agent doing right now" indicator (see
    /// `docs/superpowers/specs/2026-05-17-turn-stages.md`).
    TurnStageChanged {
        stage: TurnStage,
        stage_started_at_ms: u64,
    },
    /// Keep-alive while a turn is in progress.  Emitted every ~2s by the driver
    /// so the frontend can distinguish "silent but alive" from "stuck".
    TurnHeartbeat {
        stage_elapsed_ms: u64,
        turn_elapsed_ms: u64,
    },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RuntimeEvent {
    pub session_id: SessionId,
    pub run_id: RunId,
    pub agent_id: Option<AgentId>,
    pub tool_call_id: Option<ToolCallId>,
    pub kind: RuntimeEventKind,
}

impl RuntimeEvent {
    pub fn new(session_id: SessionId, run_id: RunId, kind: RuntimeEventKind) -> Self {
        let tool_call_id = match &kind {
            RuntimeEventKind::ToolCallExecuting { tool_call_id, .. }
            | RuntimeEventKind::ToolCallCompleted { tool_call_id, .. }
            | RuntimeEventKind::ToolProgress { tool_call_id, .. }
            | RuntimeEventKind::PermissionAskRequired { tool_call_id, .. } => {
                Some(tool_call_id.clone())
            }
            RuntimeEventKind::UserInteractionRequired { tool_call_id, .. } => {
                Some(tool_call_id.clone())
            }
            _ => None,
        };
        let agent_id = match &kind {
            RuntimeEventKind::AgentIdle { agent_id, .. } => Some(agent_id.clone()),
            RuntimeEventKind::LeadHasPendingMessages { agent_id } => Some(agent_id.clone()),
            _ => None,
        };
        Self {
            session_id,
            run_id,
            agent_id,
            tool_call_id,
            kind,
        }
    }

    pub fn stream_delta(session_id: SessionId, run_id: RunId, content: String) -> Self {
        Self::new(
            session_id,
            run_id,
            RuntimeEventKind::StreamDelta { content },
        )
    }

    pub fn message_persisted(
        session_id: SessionId,
        run_id: RunId,
        message_id: impl Into<String>,
        role: impl Into<String>,
        content: serde_json::Value,
    ) -> Self {
        Self::new(
            session_id,
            run_id,
            RuntimeEventKind::MessagePersisted {
                message_id: message_id.into(),
                role: role.into(),
                content,
                client_message_id: None,
                tool_calls: None,
                error: None,
            },
        )
    }

    /// 与 [`message_persisted`] 同模式，但携带结构化错误（PR2）。
    pub fn message_persisted_with_error(
        session_id: SessionId,
        run_id: RunId,
        message_id: impl Into<String>,
        role: impl Into<String>,
        content: serde_json::Value,
        error: crate::storage::file_store::types::MessageError,
    ) -> Self {
        Self::new(
            session_id,
            run_id,
            RuntimeEventKind::MessagePersisted {
                message_id: message_id.into(),
                role: role.into(),
                content,
                client_message_id: None,
                tool_calls: None,
                error: Some(error),
            },
        )
    }

    pub fn stream_done(session_id: SessionId, run_id: RunId) -> Self {
        Self::new(session_id, run_id, RuntimeEventKind::StreamDone)
    }

    pub fn turn_stage_changed(
        session_id: SessionId,
        run_id: RunId,
        stage: TurnStage,
        stage_started_at_ms: u64,
    ) -> Self {
        Self::new(
            session_id,
            run_id,
            RuntimeEventKind::TurnStageChanged {
                stage,
                stage_started_at_ms,
            },
        )
    }

    pub fn turn_heartbeat(
        session_id: SessionId,
        run_id: RunId,
        stage_elapsed_ms: u64,
        turn_elapsed_ms: u64,
    ) -> Self {
        Self::new(
            session_id,
            run_id,
            RuntimeEventKind::TurnHeartbeat {
                stage_elapsed_ms,
                turn_elapsed_ms,
            },
        )
    }

    pub fn compact_completed(
        session_id: SessionId,
        run_id: RunId,
        conversation_id: String,
        boundary_id: String,
        trigger: String,
        created_at: String,
        tail_message_id: Option<String>,
        pre_tokens: u64,
        post_tokens: u64,
        messages_summarized: usize,
    ) -> Self {
        Self::new(
            session_id,
            run_id,
            RuntimeEventKind::CompactCompleted {
                conversation_id,
                boundary_id,
                trigger,
                created_at,
                tail_message_id,
                pre_tokens,
                post_tokens,
                messages_summarized,
            },
        )
    }
}
