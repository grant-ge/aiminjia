use std::sync::Arc;

use anyhow::Result;

use crate::runtime::agent::invocation::{
    AgentInvocation, AgentStatus, ChildRunHandle, ResumeChildRunRequest, SpawnChildRunRequest,
};
use crate::runtime::agent::message_bridge;
use crate::runtime::agent::subagent_transcript_store::{
    FileSubagentTranscriptStore, InMemorySubagentTranscriptStore, SubagentTranscriptEntryRecord,
    SubagentTranscriptStore,
};
use crate::runtime::event_bus::RuntimeEventBus;
use crate::runtime::events::RuntimeEvent;
use crate::runtime::ids::{AgentId, RunId, SessionId};
use crate::runtime::store::{
    AgentInvocationRecord, AgentInvocationStore, InMemoryAgentInvocationStore,
};

pub struct AgentRuntime {
    invocation_store: Arc<dyn AgentInvocationStore>,
    transcript_store: Arc<dyn SubagentTranscriptStore>,
}

impl AgentRuntime {
    pub fn new(
        invocation_store: Arc<dyn AgentInvocationStore>,
        transcript_store: Arc<dyn SubagentTranscriptStore>,
    ) -> Self {
        Self {
            invocation_store,
            transcript_store,
        }
    }

    pub fn for_test() -> Self {
        Self::new(
            Arc::new(InMemoryAgentInvocationStore::new()),
            Arc::new(InMemorySubagentTranscriptStore::new()),
        )
    }

    pub fn for_resume_test(store: InMemoryAgentInvocationStore) -> Self {
        Self::new(
            Arc::new(store),
            Arc::new(InMemorySubagentTranscriptStore::new()),
        )
    }

    pub fn from_storage(
        store_path: std::path::PathBuf,
        transcript_store_path: std::path::PathBuf,
    ) -> anyhow::Result<Self> {
        let invocation_store =
            super::file_agent_invocation_store::FileAgentInvocationStore::new(store_path)?;
        let transcript_store = FileSubagentTranscriptStore::new(transcript_store_path)?;
        Ok(Self {
            invocation_store: Arc::new(invocation_store),
            transcript_store: Arc::new(transcript_store),
        })
    }

    pub async fn spawn_child_run(&self, request: SpawnChildRunRequest) -> Result<ChildRunHandle> {
        let agent_id = AgentId::new(uuid::Uuid::new_v4().to_string());
        let child_run_id = RunId::new(format!("child-{}", uuid::Uuid::new_v4()));
        let mut invocation =
            AgentInvocation::new(agent_id.clone(), request.parent_run_id, child_run_id);
        invocation.status = AgentStatus::Running;
        invocation.background = request.background;
        self.invocation_store
            .create_invocation(AgentInvocationRecord::from(invocation.clone()))?;
        Ok(ChildRunHandle::new(invocation))
    }

    pub async fn cancel_run(&self, child_run_id: RunId) -> Result<()> {
        for record in self.invocation_store.list_invocations()? {
            if record.child_run_id == child_run_id {
                self.invocation_store
                    .update_invocation_status(&record.agent_id, AgentStatus::Cancelled)?;
            }
        }
        Ok(())
    }

    pub async fn complete_run(&self, child_run_id: &RunId) -> Result<()> {
        for record in self.invocation_store.list_invocations()? {
            if &record.child_run_id == child_run_id {
                self.invocation_store
                    .update_invocation_status(&record.agent_id, AgentStatus::Completed)?;
            }
        }
        Ok(())
    }

    pub async fn fail_run(&self, child_run_id: &RunId) -> Result<()> {
        for record in self.invocation_store.list_invocations()? {
            if &record.child_run_id == child_run_id {
                self.invocation_store
                    .update_invocation_status(&record.agent_id, AgentStatus::Failed)?;
            }
        }
        Ok(())
    }

    pub async fn status(&self, child_run_id: &RunId) -> Result<String> {
        for record in self.invocation_store.list_invocations()? {
            if &record.child_run_id == child_run_id {
                return Ok(match record.status {
                    AgentStatus::Pending => "pending",
                    AgentStatus::Running => "running",
                    AgentStatus::Completed => "completed",
                    AgentStatus::Cancelled => "cancelled",
                    AgentStatus::Failed => "failed",
                }
                .to_string());
            }
        }
        Ok("missing".to_string())
    }

    pub async fn resume_child_run(&self, request: ResumeChildRunRequest) -> Result<ChildRunHandle> {
        let record = self
            .invocation_store
            .get_invocation(&request.agent_id)?
            .ok_or_else(|| anyhow::anyhow!("missing invocation"))?;
        Ok(ChildRunHandle::new(record.into()))
    }

    /// Complete a background child run, persist an optional summary, and emit
    /// an `AgentIdle` event so the UI knows the sub-agent has finished.
    pub async fn complete_background_run(
        &self,
        child_run_id: &RunId,
        summary: Option<&str>,
        transcript_ref: Option<&str>,
        session_id: SessionId,
        parent_run_id: RunId,
        bus: RuntimeEventBus,
    ) -> Result<()> {
        let mut target_agent_id: Option<AgentId> = None;
        for record in self.invocation_store.list_invocations()? {
            if &record.child_run_id == child_run_id {
                target_agent_id = Some(record.agent_id.clone());
                self.invocation_store
                    .update_invocation_status(&record.agent_id, AgentStatus::Completed)?;
                self.invocation_store.update_invocation_result_metadata(
                    &record.agent_id,
                    summary.map(str::to_owned),
                    transcript_ref.map(str::to_owned),
                )?;
            }
        }
        if let Some(agent_id) = target_agent_id {
            let kind = message_bridge::bridge_agent_summary(agent_id);
            let event = RuntimeEvent::new(session_id, parent_run_id, kind);
            bus.emit(event).await?;
        }
        Ok(())
    }

    pub async fn fail_background_run(
        &self,
        child_run_id: &RunId,
        error_summary: Option<&str>,
        session_id: SessionId,
        parent_run_id: RunId,
        bus: RuntimeEventBus,
    ) -> Result<()> {
        for record in self.invocation_store.list_invocations()? {
            if &record.child_run_id == child_run_id {
                self.invocation_store
                    .update_invocation_status(&record.agent_id, AgentStatus::Failed)?;
                if let Some(summary) = error_summary {
                    self.invocation_store.update_invocation_result_metadata(
                        &record.agent_id,
                        Some(summary.to_owned()),
                        None,
                    )?;
                }
            }
        }

        let event = RuntimeEvent::new(
            session_id,
            parent_run_id,
            crate::runtime::events::RuntimeEventKind::TaskStatusChanged {
                task_id: crate::runtime::ids::TaskId::new(child_run_id.as_str()),
                status: "failed".to_string(),
            },
        );
        bus.emit(event).await?;
        Ok(())
    }

    /// Return the stored summary/output-ref for a child run (if any).
    pub async fn get_summary(&self, child_run_id: &RunId) -> Result<Option<String>> {
        for record in self.invocation_store.list_invocations()? {
            if &record.child_run_id == child_run_id {
                return Ok(record.summary_or_output_ref);
            }
        }
        Ok(None)
    }

    pub async fn get_transcript_ref(&self, child_run_id: &RunId) -> Result<Option<String>> {
        for record in self.invocation_store.list_invocations()? {
            if &record.child_run_id == child_run_id {
                return Ok(record.transcript_ref.clone());
            }
        }
        Ok(None)
    }

    pub async fn load_transcript(
        &self,
        child_run_id: &RunId,
    ) -> Result<Option<Vec<SubagentTranscriptEntryRecord>>> {
        let Some(transcript_ref) = self.get_transcript_ref(child_run_id).await? else {
            return Ok(None);
        };
        self.transcript_store.get(&transcript_ref)
    }

    pub fn store_transcript(
        &self,
        transcript_ref: &str,
        entries: &[SubagentTranscriptEntryRecord],
    ) -> Result<()> {
        self.transcript_store.put(transcript_ref, entries)
    }

    pub fn transcript_store_get(
        &self,
        transcript_ref: &str,
    ) -> Result<Option<Vec<SubagentTranscriptEntryRecord>>> {
        self.transcript_store.get(transcript_ref)
    }
}
