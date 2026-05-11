//! Per-Session `name → AgentId` registry.
//!
//! Required by SendMessage routing (P2) and by Team membership lookup.
//! Session-scoped so the same friendly name (e.g. "researcher") can be
//! reused across unrelated chats.  Separate from TeamRegistry because
//! async subagents (one-shot, no Team) also need to be name-addressable.

use crate::runtime::ids::{AgentId, SessionId};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;

#[derive(thiserror::Error, Debug)]
pub enum NameRegistryError {
    #[error("name `{0}` already registered in session")]
    Duplicate(String),
}

#[derive(Debug, Default)]
pub struct AgentNameRegistry {
    by_session: Mutex<HashMap<SessionId, HashMap<String, AgentId>>>,
}

impl AgentNameRegistry {
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    pub async fn register(
        &self,
        session: &SessionId,
        name: &str,
        id: AgentId,
    ) -> Result<(), NameRegistryError> {
        let mut g = self.by_session.lock().await;
        let m = g.entry(session.clone()).or_default();
        if m.contains_key(name) {
            return Err(NameRegistryError::Duplicate(name.into()));
        }
        m.insert(name.into(), id);
        Ok(())
    }

    pub async fn resolve(&self, session: &SessionId, name: &str) -> Option<AgentId> {
        self.by_session
            .lock()
            .await
            .get(session)
            .and_then(|m| m.get(name).cloned())
    }

    pub async fn unregister(&self, session: &SessionId, name: &str) {
        if let Some(m) = self.by_session.lock().await.get_mut(session) {
            m.remove(name);
        }
    }

    /// Session 结束时清整张表 — P1.8 cleanup hook 调用。
    pub async fn drop_session(&self, session: &SessionId) {
        self.by_session.lock().await.remove(session);
    }

    /// LTR (P1.8): drop **all** sessions.  Used by the app-close hook so a
    /// relaunch starts with an empty registry.
    pub async fn clear_all(&self) -> usize {
        let mut g = self.by_session.lock().await;
        let n = g.len();
        g.clear();
        n
    }

    pub async fn names_in_session(&self, session: &SessionId) -> Vec<String> {
        self.by_session
            .lock()
            .await
            .get(session)
            .map(|m| m.keys().cloned().collect())
            .unwrap_or_default()
    }
}
