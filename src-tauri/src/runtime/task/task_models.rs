use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::runtime::ids::{AgentId, RunId, SessionId, TaskId};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskStatus {
    Pending,
    InProgress,
    Completed,
    Failed,
    Cancelled,
}

impl TaskStatus {
    pub fn as_str(&self) -> &'static str {
        match self {
            TaskStatus::Pending => "pending",
            TaskStatus::InProgress => "in_progress",
            TaskStatus::Completed => "completed",
            TaskStatus::Failed => "failed",
            TaskStatus::Cancelled => "cancelled",
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskRecord {
    pub id: String,
    pub subject: String,
    pub description: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub active_form: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub owner: Option<String>,
    pub status: TaskStatus,
    pub blocks: Vec<String>,
    pub blocked_by: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<HashMap<String, serde_json::Value>>,

    // lotus runtime context fields; persisted for recovery and event correlation.
    pub session_id: SessionId,
    pub parent_run_id: RunId,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub owner_agent_id: Option<AgentId>,
}

impl TaskRecord {
    pub fn task_id(&self) -> TaskId {
        TaskId::new(self.id.clone())
    }
}
