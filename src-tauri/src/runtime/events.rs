use serde::{Deserialize, Serialize};

use crate::runtime::ids::{AgentId, RunId, SessionId, TaskId, ToolCallId};

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
    StreamError {
        error: String,
        raw_error: Option<String>,
    },
    ToolCallExecuting {
        tool_call_id: ToolCallId,
        tool_name: String,
    },
    ToolCallCompleted {
        tool_call_id: ToolCallId,
        tool_name: String,
        /// Whether the tool execution ended in an error.
        /// Carried through to the frontend `success` field in `tool:completed`.
        is_error: bool,
    },
    AgentIdle {
        agent_id: AgentId,
        scope: AgentIdleScope,
    },
    TaskStatusChanged {
        task_id: TaskId,
        status: String,
    },
    MessagePersisted {
        message_id: String,
        role: String,
        content: serde_json::Value,
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
            | RuntimeEventKind::ToolCallCompleted { tool_call_id, .. } => {
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
            },
        )
    }

    pub fn stream_done(session_id: SessionId, run_id: RunId) -> Self {
        Self::new(session_id, run_id, RuntimeEventKind::StreamDone)
    }
}
