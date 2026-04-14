use std::path::PathBuf;
use std::sync::Mutex;

use anyhow::Result;

use crate::runtime::ids::AgentId;
use crate::runtime::agent::invocation::AgentStatus;
use crate::runtime::store::{AgentInvocationRecord, AgentInvocationStore};

/// File-backed implementation of [`AgentInvocationStore`].
///
/// Records are persisted as a JSON array in a single file at `store_path`.
/// An in-memory cache avoids redundant disk reads; every mutating operation
/// flushes the cache back to disk before returning.
pub struct FileAgentInvocationStore {
    store_path: PathBuf,
    cache: Mutex<Vec<AgentInvocationRecord>>,
}

impl FileAgentInvocationStore {
    /// Create (or load) a store at `store_path`.
    ///
    /// If the file already exists its contents are deserialized into the
    /// in-memory cache.  If the file does not exist an empty cache is used
    /// and the file will be created on the first write.
    pub fn new(store_path: PathBuf) -> Result<Self> {
        let cache = if store_path.exists() {
            let raw = std::fs::read_to_string(&store_path)?;
            serde_json::from_str::<Vec<AgentInvocationRecord>>(&raw)?
        } else {
            Vec::new()
        };
        Ok(Self {
            store_path,
            cache: Mutex::new(cache),
        })
    }

    /// Persist the current cache to disk.
    fn flush(&self, cache: &[AgentInvocationRecord]) -> Result<()> {
        if let Some(parent) = self.store_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let json = serde_json::to_string_pretty(cache)?;
        std::fs::write(&self.store_path, json)?;
        Ok(())
    }
}

impl AgentInvocationStore for FileAgentInvocationStore {
    fn create_invocation(&self, record: AgentInvocationRecord) -> Result<()> {
        let mut cache = self.cache.lock().unwrap();
        cache.push(record);
        self.flush(&cache)
    }

    fn get_invocation(&self, agent_id: &AgentId) -> Result<Option<AgentInvocationRecord>> {
        let cache = self.cache.lock().unwrap();
        Ok(cache
            .iter()
            .find(|r| r.agent_id == *agent_id)
            .cloned())
    }

    fn list_invocations(&self) -> Result<Vec<AgentInvocationRecord>> {
        Ok(self.cache.lock().unwrap().clone())
    }

    fn update_invocation_status(&self, agent_id: &AgentId, status: AgentStatus) -> Result<()> {
        let mut cache = self.cache.lock().unwrap();
        for record in cache.iter_mut() {
            if record.agent_id == *agent_id {
                record.status = status;
                break;
            }
        }
        self.flush(&cache)
    }

    fn update_invocation_summary(
        &self,
        agent_id: &AgentId,
        summary: Option<String>,
    ) -> Result<()> {
        let mut cache = self.cache.lock().unwrap();
        for record in cache.iter_mut() {
            if record.agent_id == *agent_id {
                record.summary_or_output_ref = summary;
                break;
            }
        }
        self.flush(&cache)
    }
}
