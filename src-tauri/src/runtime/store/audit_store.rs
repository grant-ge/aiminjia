use std::sync::Mutex;

use anyhow::Result;

use crate::runtime::ids::RunId;

#[derive(Clone, Debug)]
pub struct AuditRecord {
    pub run_id: Option<RunId>,
    pub event: String,
}

pub trait AuditStore: Send + Sync {
    fn append_event(&self, run_id: &RunId, event: &str) -> Result<()>;
    fn list(&self) -> Vec<AuditRecord>;
}

#[derive(Default)]
pub struct InMemoryAuditStore {
    records: Mutex<Vec<AuditRecord>>,
}

impl AuditStore for InMemoryAuditStore {
    fn append_event(&self, run_id: &RunId, event: &str) -> Result<()> {
        self.records.lock().unwrap().push(AuditRecord {
            run_id: Some(run_id.clone()),
            event: event.to_string(),
        });
        Ok(())
    }

    fn list(&self) -> Vec<AuditRecord> {
        self.records.lock().unwrap().clone()
    }
}
