use serde::{Deserialize, Serialize};

use crate::runtime::ids::{AgentId, RunId};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum AgentStatus {
    Pending,
    Running,
    Completed,
    Cancelled,
    Failed,
}

#[derive(Clone, Debug)]
pub struct AgentInvocation {
    pub agent_id: AgentId,
    pub parent_run_id: RunId,
    pub child_run_id: RunId,
    pub status: AgentStatus,
    pub background: bool,
    pub summary_or_output_ref: Option<String>,
    pub transcript_ref: Option<String>,
}

impl AgentInvocation {
    pub fn new(agent_id: AgentId, parent_run_id: RunId, child_run_id: RunId) -> Self {
        Self {
            agent_id,
            parent_run_id,
            child_run_id,
            status: AgentStatus::Pending,
            background: false,
            summary_or_output_ref: None,
            transcript_ref: None,
        }
    }

    pub fn child_run_id(&self) -> &RunId {
        &self.child_run_id
    }
}

#[derive(Clone, Debug)]
pub struct SpawnChildRunRequest {
    pub parent_run_id: RunId,
    pub background: bool,
    pub allowed_tools: Vec<String>,
}

impl SpawnChildRunRequest {
    pub fn for_test(parent_run_id: RunId) -> Self {
        Self {
            parent_run_id,
            background: false,
            allowed_tools: vec!["python_exec".to_string()],
        }
    }
}

#[derive(Clone, Debug)]
pub struct ChildRunHandle {
    invocation: AgentInvocation,
}

impl ChildRunHandle {
    pub fn new(invocation: AgentInvocation) -> Self {
        Self { invocation }
    }

    pub fn child_run_id(&self) -> &RunId {
        self.invocation.child_run_id()
    }

    pub fn agent_id(&self) -> &AgentId {
        &self.invocation.agent_id
    }

    pub fn invocation(&self) -> &AgentInvocation {
        &self.invocation
    }
}

#[derive(Clone, Debug)]
pub struct ResumeChildRunRequest {
    pub agent_id: AgentId,
}

impl ResumeChildRunRequest {
    pub fn new(agent_id: impl Into<String>) -> Self {
        Self {
            agent_id: AgentId::new(agent_id.into()),
        }
    }
}
