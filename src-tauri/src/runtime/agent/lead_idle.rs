//! `LeadIdleSupervisor` — atomic state machine that arbitrates between
//! "Lead is currently inside a chat turn" and "Lead is idle, waiting".
//!
//! The supervisor exists to solve the dual-path wake problem in v4 §5.6:
//!
//! - **Path A** (turn-end self-check):  the chat turn driver, just before
//!   emitting `AgentIdle`, calls `mark_idle(key)`.  If the supervisor reports
//!   `pending == true`, the driver immediately loops back into another turn
//!   instead of returning to the user — picking up whatever messages landed
//!   in the inbox during the previous turn.
//!
//! - **Path C** (SendMessage kick):  when a Teammate's SendMessage delivers
//!   to the Lead's inbox, the SendMessage tool calls `enqueue(key)`.  If the
//!   supervisor reports `true` (Lead was Idle), the tool is responsible for
//!   actually spawning the next turn — only one concurrent caller will see
//!   `true` thanks to the atomic CAS in the supervisor.  `false` means the
//!   Lead is currently running; Path A will pick up the pending mark.
//!
//! This module ships the state machine + tests.  Wiring into
//! `chat_turn_driver` and `SendMessage` arrives in follow-up work — for now
//! the supervisor is exposed via `app.manage()` so future wiring sites can
//! `try_state::<Arc<LeadIdleSupervisor>>()` it.

use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;

use crate::runtime::ids::{AgentId, SessionId};

/// Key used to scope supervisor state.  Distinct sessions can run their own
/// Leads concurrently; within one session there is at most one Lead so the
/// `AgentId` portion identifies which agent is the Lead.
pub type LeadKey = (SessionId, AgentId);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LeadState {
    Running,
    Idle { pending: bool },
}

#[derive(Debug, Default)]
pub struct LeadIdleSupervisor {
    state: Mutex<HashMap<LeadKey, LeadState>>,
    /// Sidecar: tracks whether at least one SendMessage arrived while the
    /// Lead was in the Running state.  Reset when a new Running window
    /// begins and consumed at `mark_idle`.
    pending_during_run: Mutex<HashMap<LeadKey, bool>>,
}

impl LeadIdleSupervisor {
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    /// Mark the Lead as currently running a turn.  Resets the pending counter
    /// so this Running window starts fresh.  Idempotent.
    pub async fn mark_running(&self, k: &LeadKey) {
        let mut s = self.state.lock().await;
        s.insert(k.clone(), LeadState::Running);
        drop(s);
        self.pending_during_run.lock().await.insert(k.clone(), false);
    }

    /// Mark the Lead as idle and return whether work is queued.
    ///
    /// Returns `true` if a SendMessage arrived during the previous Running
    /// window — the caller (chat turn driver / Path A) must loop back into
    /// another turn instead of yielding control.
    pub async fn mark_idle(&self, k: &LeadKey) -> bool {
        let pending = self
            .pending_during_run
            .lock()
            .await
            .insert(k.clone(), false)
            .unwrap_or(false);
        let mut s = self.state.lock().await;
        s.insert(k.clone(), LeadState::Idle { pending });
        pending
    }

    /// Record that a new message arrived for the Lead.
    ///
    /// Returns `true` if the caller (SendMessage / Path C) should wake the
    /// Lead.  Only one caller per Idle->Running transition sees `true`.
    ///
    /// Returns `false` if the Lead was already Running — the pending mark
    /// is recorded and Path A will catch it at turn end.
    pub async fn enqueue(&self, k: &LeadKey) -> bool {
        let mut s = self.state.lock().await;
        match s.get(k).copied() {
            None | Some(LeadState::Idle { .. }) => {
                s.insert(k.clone(), LeadState::Running);
                drop(s);
                self.pending_during_run.lock().await.insert(k.clone(), false);
                true
            }
            Some(LeadState::Running) => {
                drop(s);
                self.pending_during_run.lock().await.insert(k.clone(), true);
                false
            }
        }
    }

    /// Inspect-only helper for tests / debug logs.  Returns `None` if the
    /// Lead has never been observed.
    pub async fn state_of(&self, k: &LeadKey) -> Option<&'static str> {
        let s = self.state.lock().await;
        s.get(k).map(|v| match v {
            LeadState::Running => "running",
            LeadState::Idle { pending: true } => "idle+pending",
            LeadState::Idle { pending: false } => "idle",
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(s: &str, a: &str) -> LeadKey {
        (SessionId::new(s), AgentId::new(a))
    }

    #[tokio::test]
    async fn turn_ending_with_no_messages_reports_no_pending() {
        let sup = LeadIdleSupervisor::new();
        let k = key("s1", "lead-1");
        sup.mark_running(&k).await;
        assert!(!sup.mark_idle(&k).await, "no SendMessage → no pending");
        assert_eq!(sup.state_of(&k).await, Some("idle"));
    }

    #[tokio::test]
    async fn send_during_running_window_yields_pending_at_turn_end() {
        let sup = LeadIdleSupervisor::new();
        let k = key("s1", "lead-1");
        sup.mark_running(&k).await;
        assert!(!sup.enqueue(&k).await);
        assert!(!sup.enqueue(&k).await);
        assert!(sup.mark_idle(&k).await);
    }

    #[tokio::test]
    async fn send_when_idle_returns_true_and_only_first_caller_wakes() {
        let sup = LeadIdleSupervisor::new();
        let k = key("s1", "lead-1");
        sup.mark_running(&k).await;
        sup.mark_idle(&k).await;

        let a = sup.enqueue(&k).await;
        let b = sup.enqueue(&k).await;
        assert!(a, "first caller wins CAS, told to wake");
        assert!(!b, "second caller sees Running, returns false");
        assert_eq!(sup.state_of(&k).await, Some("running"));
    }

    #[tokio::test]
    async fn ten_messages_during_running_window_collapse_into_single_followup() {
        let sup = LeadIdleSupervisor::new();
        let k = key("s1", "lead-1");
        sup.mark_running(&k).await;
        for _ in 0..10 {
            assert!(!sup.enqueue(&k).await);
        }
        assert!(sup.mark_idle(&k).await);
        // Path A loops back: new Running window resets pending.
        sup.mark_running(&k).await;
        assert!(!sup.mark_idle(&k).await);
    }

    #[tokio::test]
    async fn never_observed_lead_treats_first_enqueue_as_wake() {
        let sup = LeadIdleSupervisor::new();
        let k = key("s1", "lead-fresh");
        assert!(sup.enqueue(&k).await);
        assert_eq!(sup.state_of(&k).await, Some("running"));
    }

    #[tokio::test]
    async fn distinct_keys_are_isolated() {
        let sup = LeadIdleSupervisor::new();
        let a = key("s1", "lead-a");
        let b = key("s2", "lead-b");
        sup.mark_running(&a).await;
        sup.enqueue(&a).await;
        assert_eq!(sup.state_of(&a).await, Some("running"));
        assert_eq!(sup.state_of(&b).await, None);
        assert!(sup.enqueue(&b).await);
        assert!(sup.mark_idle(&a).await, "a still has pending");
    }
}
