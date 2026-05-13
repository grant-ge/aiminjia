//! Per-process registry mapping `AgentId → Arc<AgentInbox>` for SendMessage
//! routing.
//!
//! Populated when a Teammate idle loop boots (P1.6 -> P2.2 wiring), released
//! when the Teammate exits cleanup or its session is dropped.  Lookups are
//! by `AgentId` because the session-scoped name → id resolution happens
//! upstream in `AgentNameRegistry`.

use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;

use crate::runtime::agent::inbox::AgentInbox;
use crate::runtime::ids::{AgentId, SessionId};

/// Tracks `(session_id, agent_id)` → inbox so SendMessage can deliver across
/// the worker boundary.  We key by `(session, agent)` so that re-using an
/// agent_id across two sessions (in tests, mostly) doesn't cross-contaminate.
#[derive(Debug, Default)]
pub struct InboxRegistry {
    by_session: Mutex<HashMap<SessionId, HashMap<AgentId, Arc<AgentInbox>>>>,
}

impl InboxRegistry {
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    pub async fn register(
        &self,
        session: &SessionId,
        agent_id: AgentId,
        inbox: Arc<AgentInbox>,
    ) {
        let mut g = self.by_session.lock().await;
        g.entry(session.clone())
            .or_default()
            .insert(agent_id, inbox);
    }

    pub async fn get(&self, session: &SessionId, agent_id: &AgentId) -> Option<Arc<AgentInbox>> {
        self.by_session
            .lock()
            .await
            .get(session)
            .and_then(|m| m.get(agent_id).cloned())
    }

    pub async fn unregister(&self, session: &SessionId, agent_id: &AgentId) {
        if let Some(m) = self.by_session.lock().await.get_mut(session) {
            m.remove(agent_id);
        }
    }

    /// Drop all inboxes for `session_id`.  Called from cleanup hooks.
    pub async fn drop_session(&self, session: &SessionId) {
        self.by_session.lock().await.remove(session);
    }

    /// Drop everything.  Used by the app-close hook (P1.8) for symmetry with
    /// the other LTR registries.
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
        let id = AgentId::new("a1");
        let inbox = AgentInbox::new(4);
        reg.register(&session, id.clone(), inbox.clone()).await;

        let resolved = reg.get(&session, &id).await.expect("inbox should resolve");
        // Push via the resolved handle, drain via the original — they're the
        // same inbox.
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
        let id = AgentId::new("shared-id");
        let inbox1 = AgentInbox::new(4);
        let inbox2 = AgentInbox::new(4);
        reg.register(&s1, id.clone(), inbox1.clone()).await;
        reg.register(&s2, id.clone(), inbox2.clone()).await;

        assert!(reg.get(&s1, &id).await.is_some());
        assert!(reg.get(&s2, &id).await.is_some());

        reg.drop_session(&s1).await;
        assert!(reg.get(&s1, &id).await.is_none());
        // s2 untouched.
        assert!(reg.get(&s2, &id).await.is_some());
    }
}
