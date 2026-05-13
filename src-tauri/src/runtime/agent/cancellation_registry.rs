//! `CancellationRegistry` — per-process map from `(SessionId, AgentId)` to
//! the agent's `CancellationToken`.
//!
//! Populated when a Teammate idle loop boots (spawn_subagent registers the
//! child cancel token used by `run_teammate_idle`); released when the
//! Teammate exits cleanup or its session is dropped.  Lets external tools
//! (notably `TeammateStop` in P2.7) cancel a Teammate by AgentId without
//! holding a direct handle to its tokio task.

use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;

use crate::runtime::cancellation::CancellationToken;
use crate::runtime::ids::{AgentId, SessionId};

#[derive(Debug, Default)]
pub struct CancellationRegistry {
    by_session: Mutex<HashMap<SessionId, HashMap<AgentId, CancellationToken>>>,
}

impl CancellationRegistry {
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    pub async fn register(&self, session: &SessionId, agent: AgentId, token: CancellationToken) {
        self.by_session
            .lock()
            .await
            .entry(session.clone())
            .or_default()
            .insert(agent, token);
    }

    pub async fn get(&self, session: &SessionId, agent: &AgentId) -> Option<CancellationToken> {
        self.by_session
            .lock()
            .await
            .get(session)
            .and_then(|m| m.get(agent).cloned())
    }

    pub async fn unregister(&self, session: &SessionId, agent: &AgentId) {
        if let Some(m) = self.by_session.lock().await.get_mut(session) {
            m.remove(agent);
        }
    }

    pub async fn drop_session(&self, session: &SessionId) {
        self.by_session.lock().await.remove(session);
    }

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
    use crate::runtime::cancellation::CancellationReason;

    #[tokio::test]
    async fn register_get_and_cancel_propagates() {
        let reg = CancellationRegistry::new();
        let session = SessionId::new("s1");
        let agent = AgentId::new("a1");
        let token = CancellationToken::new();
        reg.register(&session, agent.clone(), token.clone()).await;

        let resolved = reg.get(&session, &agent).await.expect("token should resolve");
        assert!(!resolved.is_cancelled(), "fresh token should not be cancelled");
        resolved.cancel_with_reason(CancellationReason::UserCancel);
        assert!(token.is_cancelled(), "tokens share state — original is cancelled");
    }

    #[tokio::test]
    async fn unregister_removes_only_target() {
        let reg = CancellationRegistry::new();
        let s = SessionId::new("s");
        let a = AgentId::new("a");
        let b = AgentId::new("b");
        reg.register(&s, a.clone(), CancellationToken::new()).await;
        reg.register(&s, b.clone(), CancellationToken::new()).await;
        reg.unregister(&s, &a).await;
        assert!(reg.get(&s, &a).await.is_none());
        assert!(reg.get(&s, &b).await.is_some());
    }
}
