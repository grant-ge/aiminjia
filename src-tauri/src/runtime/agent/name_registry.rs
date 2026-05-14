//! Per-Session `(team_name, name) → AgentId` registry.
//!
//! Required by SendMessage routing (P2) and by Team membership lookup.
//! Keyed by `(SessionId, TeamName, name)` (PR4) so the same friendly name
//! (e.g. "researcher") can be reused in different teams within the same
//! session.  The `team_name` dimension was added in PR4; single-team callers
//! pass `""` for the legacy empty-team case.

use crate::runtime::ids::{AgentId, SessionId};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;

#[derive(thiserror::Error, Debug)]
pub enum NameRegistryError {
    #[error("name `{0}` already registered in session/team")]
    Duplicate(String),
}

#[derive(Debug, Default)]
pub struct AgentNameRegistry {
    /// session → team_name → name → AgentId
    by_session: Mutex<HashMap<SessionId, HashMap<String, HashMap<String, AgentId>>>>,
}

impl AgentNameRegistry {
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    pub async fn register(
        &self,
        session: &SessionId,
        team_name: &str,
        name: &str,
        id: AgentId,
    ) -> Result<(), NameRegistryError> {
        let mut g = self.by_session.lock().await;
        let m = g
            .entry(session.clone())
            .or_default()
            .entry(team_name.to_string())
            .or_default();
        if m.contains_key(name) {
            return Err(NameRegistryError::Duplicate(name.into()));
        }
        m.insert(name.into(), id);
        Ok(())
    }

    pub async fn resolve(&self, session: &SessionId, team_name: &str, name: &str) -> Option<AgentId> {
        self.by_session
            .lock()
            .await
            .get(session)
            .and_then(|by_team| by_team.get(team_name))
            .and_then(|m| m.get(name).cloned())
    }

    pub async fn unregister(&self, session: &SessionId, team_name: &str, name: &str) {
        if let Some(by_team) = self.by_session.lock().await.get_mut(session) {
            if let Some(m) = by_team.get_mut(team_name) {
                m.remove(name);
            }
        }
    }

    /// Remove all name→id mappings for a single team within a session.
    pub async fn unregister_team(&self, session: &SessionId, team_name: &str) {
        if let Some(by_team) = self.by_session.lock().await.get_mut(session) {
            by_team.remove(team_name);
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

    pub async fn names_in_team(
        &self,
        session: &SessionId,
        team_name: &str,
    ) -> Vec<String> {
        self.by_session
            .lock()
            .await
            .get(session)
            .and_then(|by_team| by_team.get(team_name))
            .map(|m| m.keys().cloned().collect())
            .unwrap_or_default()
    }

    /// Reverse lookup: find the registered name for an `AgentId` within a
    /// (session, team_name) pair.  Returns `None` if the agent has no name
    /// binding in that team.
    pub async fn name_for(
        &self,
        session: &SessionId,
        team_name: &str,
        id: &AgentId,
    ) -> Option<String> {
        self.by_session
            .lock()
            .await
            .get(session)
            .and_then(|by_team| by_team.get(team_name))
            .and_then(|m| {
                m.iter()
                    .find_map(|(name, aid)| (aid == id).then(|| name.clone()))
            })
    }
}
