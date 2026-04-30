use std::collections::HashMap;
use std::sync::Mutex;

use anyhow::Result;

use crate::runtime::ids::SessionId;

#[derive(Clone, Debug)]
pub struct SessionRecord {
    pub session_id: SessionId,
    pub title: Option<String>,
}

pub trait SessionStore: Send + Sync {
    fn load_session(&self, session_id: &SessionId) -> Result<SessionRecord>;
    fn save_session(&self, record: SessionRecord) -> Result<()>;
}

#[derive(Default)]
pub struct InMemorySessionStore {
    sessions: Mutex<HashMap<String, SessionRecord>>,
}

impl SessionStore for InMemorySessionStore {
    fn load_session(&self, session_id: &SessionId) -> Result<SessionRecord> {
        self.sessions
            .lock()
            .unwrap()
            .get(session_id.as_str())
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("session not found"))
    }

    fn save_session(&self, record: SessionRecord) -> Result<()> {
        self.sessions
            .lock()
            .unwrap()
            .insert(record.session_id.as_str().to_string(), record);
        Ok(())
    }
}
