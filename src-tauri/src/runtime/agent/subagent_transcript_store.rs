use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Mutex;

use anyhow::Result;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SubagentTranscriptEntryRecord {
    pub role: String,
    pub content: String,
    pub tool_call_id: Option<String>,
    pub tool_name: Option<String>,
}

pub trait SubagentTranscriptStore: Send + Sync {
    fn put(&self, transcript_ref: &str, entries: &[SubagentTranscriptEntryRecord]) -> Result<()>;
    fn get(&self, transcript_ref: &str) -> Result<Option<Vec<SubagentTranscriptEntryRecord>>>;
}

#[derive(Default)]
pub struct InMemorySubagentTranscriptStore {
    entries: Mutex<HashMap<String, Vec<SubagentTranscriptEntryRecord>>>,
}

impl InMemorySubagentTranscriptStore {
    pub fn new() -> Self {
        Self::default()
    }
}

impl SubagentTranscriptStore for InMemorySubagentTranscriptStore {
    fn put(&self, transcript_ref: &str, entries: &[SubagentTranscriptEntryRecord]) -> Result<()> {
        self.entries
            .lock()
            .unwrap()
            .insert(transcript_ref.to_string(), entries.to_vec());
        Ok(())
    }

    fn get(&self, transcript_ref: &str) -> Result<Option<Vec<SubagentTranscriptEntryRecord>>> {
        Ok(self.entries.lock().unwrap().get(transcript_ref).cloned())
    }
}

pub struct FileSubagentTranscriptStore {
    root_dir: PathBuf,
}

impl FileSubagentTranscriptStore {
    pub fn new(root_dir: PathBuf) -> Result<Self> {
        std::fs::create_dir_all(&root_dir)?;
        Ok(Self { root_dir })
    }

    fn path_for_ref(&self, transcript_ref: &str) -> PathBuf {
        let raw = transcript_ref
            .strip_prefix("subagent://")
            .unwrap_or(transcript_ref);
        let sanitized: String = raw
            .chars()
            .map(|ch| match ch {
                'A'..='Z' | 'a'..='z' | '0'..='9' | '-' | '_' => ch,
                _ => '_',
            })
            .collect();
        self.root_dir.join(format!("{sanitized}.json"))
    }
}

impl SubagentTranscriptStore for FileSubagentTranscriptStore {
    fn put(&self, transcript_ref: &str, entries: &[SubagentTranscriptEntryRecord]) -> Result<()> {
        let path = self.path_for_ref(transcript_ref);
        let payload = serde_json::to_string_pretty(entries)?;
        std::fs::write(path, payload)?;
        Ok(())
    }

    fn get(&self, transcript_ref: &str) -> Result<Option<Vec<SubagentTranscriptEntryRecord>>> {
        let path = self.path_for_ref(transcript_ref);
        if !path.exists() {
            return Ok(None);
        }

        let raw = std::fs::read_to_string(path)?;
        let entries = serde_json::from_str(&raw)?;
        Ok(Some(entries))
    }
}
