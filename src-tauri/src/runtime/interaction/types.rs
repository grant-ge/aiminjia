//! Interaction Runtime types.
//!
//! Separates user-facing interactive tools (AskUserQuestion, etc.) from the
//! permission/security pipeline (PermissionDecision::Ask).

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::runtime::chat::tool_round_types::RuntimeToolCallRequest;
use crate::runtime::ids::{RunId, SessionId, ToolCallId};

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct InteractionId(String);

impl InteractionId {
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for InteractionId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<String> for InteractionId {
    fn from(s: String) -> Self {
        Self(s)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum InteractionKind {
    AskUserQuestion,
}

#[derive(Clone, Debug)]
pub struct InteractionRequest {
    pub interaction_id: InteractionId,
    pub session_id: SessionId,
    pub run_id: RunId,
    pub tool_call_id: ToolCallId,
    pub tool_name: String,
    pub kind: InteractionKind,
    pub payload: Value,
    pub original_request: RuntimeToolCallRequest,
}

#[derive(Clone, Debug)]
pub enum InteractionResolution {
    Submit { value: Value },
    Cancel { message: String },
}
