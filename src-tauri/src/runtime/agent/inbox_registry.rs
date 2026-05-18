//! Per-process registry mapping `AgentId → Arc<AgentInbox>` for SendMessage
//! routing.
//!
//! Keyed by `(SessionId, TeamName, AgentId)` so that multiple teams in the
//! same session remain isolated and the same agent_id in different teams
//! doesn't cross-contaminate.  The team dimension was added in PR4.

use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;

use crate::runtime::agent::inbox::AgentInbox;
use crate::runtime::ids::{AgentId, SessionId};

/// Three-level nested map: `session → team_name → agent_id → inbox`.
#[derive(Debug, Default)]
pub struct InboxRegistry {
    by_session: Mutex<HashMap<SessionId, HashMap<String, HashMap<AgentId, Arc<AgentInbox>>>>>,
}

impl InboxRegistry {
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    pub async fn register(
        &self,
        session: &SessionId,
        team_name: &str,
        agent_id: AgentId,
        inbox: Arc<AgentInbox>,
    ) {
        let mut g = self.by_session.lock().await;
        g.entry(session.clone())
            .or_default()
            .entry(team_name.to_string())
            .or_default()
            .insert(agent_id, inbox);
    }

    pub async fn get(
        &self,
        session: &SessionId,
        team_name: &str,
        agent_id: &AgentId,
    ) -> Option<Arc<AgentInbox>> {
        self.by_session
            .lock()
            .await
            .get(session)
            .and_then(|m| m.get(team_name))
            .and_then(|m| m.get(agent_id).cloned())
    }

    pub async fn unregister(&self, session: &SessionId, team_name: &str, agent_id: &AgentId) {
        if let Some(by_team) = self.by_session.lock().await.get_mut(session) {
            if let Some(m) = by_team.get_mut(team_name) {
                m.remove(agent_id);
            }
        }
    }

    /// Remove all inboxes for a single team within a session (idempotent sweep).
    /// Returns the number of inboxes removed.
    pub async fn unregister_team(&self, session: &SessionId, team_name: &str) -> usize {
        let mut g = self.by_session.lock().await;
        if let Some(by_team) = g.get_mut(session) {
            let removed = by_team.remove(team_name).map(|m| m.len()).unwrap_or(0);
            removed
        } else {
            0
        }
    }

    /// Drop all inboxes for `session_id`.  Called from cleanup hooks.
    pub async fn drop_session(&self, session: &SessionId) {
        self.by_session.lock().await.remove(session);
    }

    /// Drop everything.  Used by the app-close hook for symmetry with the
    /// other LTR registries.
    pub async fn clear_all(&self) -> usize {
        let mut g = self.by_session.lock().await;
        let n = g.len();
        g.clear();
        n
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::agent::inbox::{InboxItem, MessageSource};
    use crate::runtime::messaging::StructuredMessage;

    #[tokio::test]
    async fn register_and_get_roundtrip() {
        let reg = InboxRegistry::new();
        let session = SessionId::new("s1");
        let team = "alpha";
        let id = AgentId::new("a1");
        let inbox = AgentInbox::new(4);
        reg.register(&session, team, id.clone(), inbox.clone()).await;

        let resolved = reg
            .get(&session, team, &id)
            .await
            .expect("inbox should resolve");
        resolved
            .send(InboxItem::ChatMessage {
                message: StructuredMessage::text("ping"),
                source: MessageSource::Lead,
            })
            .await
            .unwrap();
        match inbox.recv().await.unwrap() {
            InboxItem::ChatMessage { message, .. } => assert_eq!(message.as_text(), Some("ping")),
            other => panic!("unexpected item: {other:?}"),
        }
    }

    #[tokio::test]
    async fn cross_session_isolation() {
        let reg = InboxRegistry::new();
        let s1 = SessionId::new("s1");
        let s2 = SessionId::new("s2");
        let team = "alpha";
        let id = AgentId::new("shared-id");
        let inbox1 = AgentInbox::new(4);
        let inbox2 = AgentInbox::new(4);
        reg.register(&s1, team, id.clone(), inbox1.clone()).await;
        reg.register(&s2, team, id.clone(), inbox2.clone()).await;

        assert!(reg.get(&s1, team, &id).await.is_some());
        assert!(reg.get(&s2, team, &id).await.is_some());

        reg.drop_session(&s1).await;
        assert!(reg.get(&s1, team, &id).await.is_none());
        // s2 untouched.
        assert!(reg.get(&s2, team, &id).await.is_some());
    }

    #[tokio::test]
    async fn cross_team_isolation() {
        let reg = InboxRegistry::new();
        let s = SessionId::new("s1");
        let id = AgentId::new("a1");
        let inbox_a = AgentInbox::new(4);
        let inbox_b = AgentInbox::new(4);
        reg.register(&s, "team-alpha", id.clone(), inbox_a.clone())
            .await;
        reg.register(&s, "team-beta", id.clone(), inbox_b.clone())
            .await;

        assert!(reg.get(&s, "team-alpha", &id).await.is_some());
        assert!(reg.get(&s, "team-beta", &id).await.is_some());

        // Unregister alpha — beta untouched.
        reg.unregister_team(&s, "team-alpha").await;
        assert!(reg.get(&s, "team-alpha", &id).await.is_none());
        assert!(reg.get(&s, "team-beta", &id).await.is_some());
    }
}
