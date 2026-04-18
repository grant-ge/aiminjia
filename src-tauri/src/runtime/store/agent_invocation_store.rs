use std::collections::HashMap;
use std::sync::Mutex;

use anyhow::Result;
use serde::{Deserialize, Serialize};

use crate::runtime::agent::invocation::{AgentInvocation, AgentStatus};
use crate::runtime::ids::AgentId;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AgentInvocationRecord {
    pub agent_id: AgentId,
    pub parent_run_id: crate::runtime::ids::RunId,
    pub child_run_id: crate::runtime::ids::RunId,
    pub status: AgentStatus,
    pub background: bool,
    pub summary_or_output_ref: Option<String>,
    #[serde(default)]
    pub transcript_ref: Option<String>,
}

impl From<AgentInvocation> for AgentInvocationRecord {
    fn from(value: AgentInvocation) -> Self {
        Self {
            agent_id: value.agent_id,
            parent_run_id: value.parent_run_id,
            child_run_id: value.child_run_id,
            status: value.status,
            background: value.background,
            summary_or_output_ref: value.summary_or_output_ref,
            transcript_ref: value.transcript_ref,
        }
    }
}

impl From<AgentInvocationRecord> for AgentInvocation {
    fn from(value: AgentInvocationRecord) -> Self {
        Self {
            agent_id: value.agent_id,
            parent_run_id: value.parent_run_id,
            child_run_id: value.child_run_id,
            status: value.status,
            background: value.background,
            summary_or_output_ref: value.summary_or_output_ref,
            transcript_ref: value.transcript_ref,
        }
    }
}

pub trait AgentInvocationStore: Send + Sync {
    fn create_invocation(&self, record: AgentInvocationRecord) -> Result<()>;
    fn get_invocation(&self, agent_id: &AgentId) -> Result<Option<AgentInvocationRecord>>;
    fn list_invocations(&self) -> Result<Vec<AgentInvocationRecord>>;
    fn update_invocation_status(&self, agent_id: &AgentId, status: AgentStatus) -> Result<()>;
    fn update_invocation_result_metadata(
        &self,
        agent_id: &AgentId,
        summary: Option<String>,
        transcript_ref: Option<String>,
    ) -> Result<()>;
}

#[derive(Default)]
pub struct InMemoryAgentInvocationStore {
    invocations: Mutex<HashMap<String, AgentInvocationRecord>>,
}

impl InMemoryAgentInvocationStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_child_run(agent_id: &str, parent_run_id: &str, child_run_id: &str) -> Self {
        let store = Self::new();
        store
            .create_invocation(AgentInvocationRecord {
                agent_id: AgentId::new(agent_id),
                parent_run_id: crate::runtime::ids::RunId::new(parent_run_id),
                child_run_id: crate::runtime::ids::RunId::new(child_run_id),
                status: AgentStatus::Running,
                background: false,
                summary_or_output_ref: None,
                transcript_ref: None,
            })
            .expect("seed child run");
        store
    }
}

impl AgentInvocationStore for InMemoryAgentInvocationStore {
    fn create_invocation(&self, record: AgentInvocationRecord) -> Result<()> {
        self.invocations
            .lock()
            .unwrap()
            .insert(record.agent_id.as_str().to_string(), record);
        Ok(())
    }

    fn get_invocation(&self, agent_id: &AgentId) -> Result<Option<AgentInvocationRecord>> {
        Ok(self
            .invocations
            .lock()
            .unwrap()
            .get(agent_id.as_str())
            .cloned())
    }

    fn list_invocations(&self) -> Result<Vec<AgentInvocationRecord>> {
        Ok(self.invocations.lock().unwrap().values().cloned().collect())
    }

    fn update_invocation_status(&self, agent_id: &AgentId, status: AgentStatus) -> Result<()> {
        if let Some(record) = self.invocations.lock().unwrap().get_mut(agent_id.as_str()) {
            record.status = status;
        }
        Ok(())
    }

    fn update_invocation_result_metadata(
        &self,
        agent_id: &AgentId,
        summary: Option<String>,
        transcript_ref: Option<String>,
    ) -> Result<()> {
        if let Some(record) = self.invocations.lock().unwrap().get_mut(agent_id.as_str()) {
            record.summary_or_output_ref = summary;
            record.transcript_ref = transcript_ref;
        }
        Ok(())
    }
}
