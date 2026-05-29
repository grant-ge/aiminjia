//! `CancellationRegistry` — per-process map from `(SessionId, TeamName, AgentId)` to
//! the agent's `CancellationToken`.
//!
//! The team_name dimension was added in PR4 so that multiple teams in the same
//! session remain isolated.  Callers that pre-date team namespacing pass `""`
//! as team_name.
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
    /// session → team_name → agent_id → token
    by_session: Mutex<HashMap<SessionId, HashMap<String, HashMap<AgentId, CancellationToken>>>>,
}

impl CancellationRegistry {
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    pub async fn register(
        &self,
        session: &SessionId,
        team_name: &str,
        agent: AgentId,
        token: CancellationToken,
    ) {
        self.by_session
            .lock()
            .await
            .entry(session.clone())
            .or_default()
            .entry(team_name.to_string())
            .or_default()
            .insert(agent, token);
    }

    pub async fn get(
        &self,
        session: &SessionId,
        team_name: &str,
        agent: &AgentId,
    ) -> Option<CancellationToken> {
        self.by_session
            .lock()
            .await
            .get(session)
            .and_then(|by_team| by_team.get(team_name))
            .and_then(|m| m.get(agent).cloned())
    }

    pub async fn unregister(&self, session: &SessionId, team_name: &str, agent: &AgentId) {
        if let Some(by_team) = self.by_session.lock().await.get_mut(session) {
            if let Some(m) = by_team.get_mut(team_name) {
                m.remove(agent);
            }
        }
    }

    /// Cancel all tokens registered for a team, then remove them.
    /// Returns the number of tokens cancelled.
    pub async fn cancel_team(&self, session: &SessionId, team_name: &str) -> usize {
        let tokens: Vec<CancellationToken> = {
            let mut g = self.by_session.lock().await;
            if let Some(by_team) = g.get_mut(session) {
                if let Some(team_map) = by_team.remove(team_name) {
                    team_map.into_values().collect()
                } else {
                    vec![]
                }
            } else {
                vec![]
            }
        };
        let count = tokens.len();
        for tok in tokens {
            tok.cancel_with_reason(crate::runtime::cancellation::CancellationReason::UserCancel);
        }
        count
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
        let team = "alpha";
        let agent = AgentId::new("a1");
        let token = CancellationToken::new();
        reg.register(&session, team, agent.clone(), token.clone())
            .await;

        let resolved = reg
            .get(&session, team, &agent)
            .await
            .expect("token should resolve");
        assert!(
            !resolved.is_cancelled(),
            "fresh token should not be cancelled"
        );
        resolved.cancel_with_reason(CancellationReason::UserCancel);
        assert!(
            token.is_cancelled(),
            "tokens share state — original is cancelled"
        );
    }

    #[tokio::test]
    async fn unregister_removes_only_target() {
        let reg = CancellationRegistry::new();
        let s = SessionId::new("s");
        let team = "alpha";
        let a = AgentId::new("a");
        let b = AgentId::new("b");
        reg.register(&s, team, a.clone(), CancellationToken::new())
            .await;
        reg.register(&s, team, b.clone(), CancellationToken::new())
            .await;
        reg.unregister(&s, team, &a).await;
        assert!(reg.get(&s, team, &a).await.is_none());
        assert!(reg.get(&s, team, &b).await.is_some());
    }

    #[tokio::test]
    async fn cancel_team_fires_all_tokens() {
        let reg = CancellationRegistry::new();
        let s = SessionId::new("s");
        let tok_a = CancellationToken::new();
        let tok_b = CancellationToken::new();
        reg.register(&s, "team-x", AgentId::new("a"), tok_a.clone())
            .await;
        reg.register(&s, "team-x", AgentId::new("b"), tok_b.clone())
            .await;
        // A token in a different team — must not be cancelled.
        let tok_c = CancellationToken::new();
        reg.register(&s, "team-y", AgentId::new("c"), tok_c.clone())
            .await;

        let count = reg.cancel_team(&s, "team-x").await;
        assert_eq!(count, 2);
        assert!(tok_a.is_cancelled());
        assert!(tok_b.is_cancelled());
        assert!(!tok_c.is_cancelled(), "different team must be untouched");
    }
}
