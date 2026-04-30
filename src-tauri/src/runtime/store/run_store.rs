use std::collections::HashMap;
use std::sync::Mutex;

use anyhow::Result;

use crate::runtime::ids::{RunId, SessionId};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RunStatus {
    Pending,
    Running,
    Completed,
    Cancelled,
}

#[derive(Clone, Debug)]
pub struct RunRecord {
    pub run_id: RunId,
    pub session_id: SessionId,
    pub status: RunStatus,
}

pub trait RunStore: Send + Sync {
    fn create_run(&self, record: RunRecord) -> Result<()>;
    fn get_run(&self, run_id: &RunId) -> Result<Option<RunRecord>>;
    fn update_run_status(&self, run_id: &RunId, status: RunStatus) -> Result<()>;
}

#[derive(Default)]
pub struct InMemoryRunStore {
    runs: Mutex<HashMap<String, RunRecord>>,
}

impl InMemoryRunStore {
    pub fn new() -> Self {
        Self::default()
    }
}

impl RunStore for InMemoryRunStore {
    fn create_run(&self, record: RunRecord) -> Result<()> {
        self.runs
            .lock()
            .unwrap()
            .insert(record.run_id.as_str().to_string(), record);
        Ok(())
    }

    fn get_run(&self, run_id: &RunId) -> Result<Option<RunRecord>> {
        Ok(self.runs.lock().unwrap().get(run_id.as_str()).cloned())
    }

    fn update_run_status(&self, run_id: &RunId, status: RunStatus) -> Result<()> {
        let mut runs = self.runs.lock().unwrap();
        let record = runs
            .get_mut(run_id.as_str())
            .ok_or_else(|| anyhow::anyhow!("run not found: {}", run_id.as_str()))?;
        record.status = status;
        Ok(())
    }
}
