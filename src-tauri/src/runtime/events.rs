use serde::{Deserialize, Serialize};

use crate::runtime::chat::ChatTurnOutcome;
use crate::runtime::ids::{AgentId, RunId, SessionId, TaskId, ToolCallId};
use crate::runtime::tools::permission::{PermissionDestination, PermissionMode};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum AgentIdleScope {
    Primary,
    Child,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum RuntimeEventKind {
    RunStarted,
    StreamStarted,
    StreamDelta {
        content: String,
    },
    StreamDone,
    StreamRetryReset,
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
            },
        )
    }

    pub fn stream_done(session_id: SessionId, run_id: RunId) -> Self {
        Self::new(session_id, run_id, RuntimeEventKind::StreamDone)
    }
}
