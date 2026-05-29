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
//!   Lead was Idle the supervisor's atomic CAS flips it to Running and —
//!   when a `wake_fn` has been injected — synchronously invokes it so the
//!   continuation turn gets spawned without the caller having to know how
//!   to do that.  Only one concurrent caller per Idle->Running transition
//!   wins the CAS, so `wake_fn` runs at most once per wake window.
//!
//! `wake_fn` is *fire-and-forget*: it is called inline on the `enqueue`
//! caller's task and must do its own `tokio::spawn` (or equivalent) if the
//! actual continuation work is async.  Keeping `enqueue` non-blocking is
//! what lets SendMessage stay a fast tool.

use std::collections::HashMap;
use std::sync::{Arc, OnceLock};
use tokio::sync::Mutex;

use crate::runtime::ids::{AgentId, SessionId};
use crate::telemetry::{record_diagnostic, DiagnosticEvent, DiagnosticSource};

/// Key used to scope supervisor state.  Distinct sessions can run their own
/// Leads concurrently; within one session there is at most one Lead so the
/// `AgentId` portion identifies which agent is the Lead.
pub type LeadKey = (SessionId, AgentId);

/// Fire-and-forget callback type for Path C.  Receives the `LeadKey` that
/// just transitioned Idle → Running together with the `team_name` that the
/// triggering message originated from (per-team disk layout v2 §6); it is
/// expected to spawn (typically via `tokio::spawn`) the continuation work
/// without blocking the caller.  The `team_name` lets the continuation turn
/// pick up the right team context without re-reading `conv.json`.
pub type WakeFn = Arc<dyn Fn(LeadKey, String) + Send + Sync + 'static>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LeadState {
    Running,
    Idle { pending: bool },
}

#[derive(Default)]
pub struct LeadIdleSupervisor {
    state: Mutex<HashMap<LeadKey, LeadState>>,
    /// Sidecar: tracks whether at least one SendMessage arrived while the
    /// Lead was in the Running state.  Reset when a new Running window
    /// begins and consumed at `mark_idle`.
    pending_during_run: Mutex<HashMap<LeadKey, bool>>,
    /// LTR (B-gap1 Path C): callback invoked inline by `enqueue` whenever
    /// the supervisor's atomic CAS flips a Lead from Idle to Running, so the
    /// session runtime can spawn a continuation turn without SendMessage
    /// needing a SessionRuntime handle.  Set once at SessionRuntime startup
    /// via `set_wake_fn`; subsequent attempts are silently ignored.
    wake_fn: OnceLock<WakeFn>,
}

impl LeadIdleSupervisor {
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    /// Inject the Path C wake callback.  Idempotent: only the first call
    /// wins, later attempts are ignored (and logged at debug level) so a
    /// stray re-wire cannot redirect existing wake traffic.  Returns `true`
    /// if this call installed the callback, `false` if one was already
    /// present.
    pub fn set_wake_fn(&self, wake_fn: WakeFn) -> bool {
        match self.wake_fn.set(wake_fn) {
            Ok(()) => {
                log::debug!("[LeadIdleSupervisor] set_wake_fn installed");
                let ws = crate::telemetry::diagnostics_workspace();
                record_diagnostic(
                    &ws,
                    DiagnosticEvent::new("agent.lead_idle.set_wake_fn", DiagnosticSource::Backend)
                        .ok(true)
                        .payload(serde_json::json!({ "installed": true })),
                );
                true
            }
            Err(_) => {
                log::debug!("[LeadIdleSupervisor] set_wake_fn called twice; second call ignored");
                false
            }
        }
    }

    /// Mark the Lead as currently running a turn.  Resets the pending counter
    /// so this Running window starts fresh.  Idempotent.
    pub async fn mark_running(&self, k: &LeadKey) {
        let mut s = self.state.lock().await;
        s.insert(k.clone(), LeadState::Running);
        drop(s);
        self.pending_during_run
            .lock()
            .await
            .insert(k.clone(), false);
        log::info!(
            "[LeadIdleSupervisor] mark_running session={} agent={}",
            k.0.as_str(),
            k.1.as_str()
        );
        let ws = crate::telemetry::diagnostics_workspace();
        record_diagnostic(
            &ws,
            DiagnosticEvent::new("agent.lead_idle.mark_running", DiagnosticSource::Backend)
                .conversation_id(k.0.as_str())
                .agent_id(k.1.as_str())
                .ok(true)
                .payload(serde_json::json!({ "state": "running" })),
        );
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
        log::info!(
            "[LeadIdleSupervisor] mark_idle session={} agent={} pending={}",
            k.0.as_str(),
            k.1.as_str(),
            pending
        );
        let ws = crate::telemetry::diagnostics_workspace();
        record_diagnostic(
            &ws,
            DiagnosticEvent::new("agent.lead_idle.mark_idle", DiagnosticSource::Backend)
                .conversation_id(k.0.as_str())
                .agent_id(k.1.as_str())
                .ok(true)
                .payload(serde_json::json!({ "state": "idle", "pending": pending })),
        );
        pending
    }

    /// Record that a new message arrived for the Lead.
    ///
    /// When the Lead was Idle, atomically flips it to Running and — if a
    /// `wake_fn` is wired — invokes it inline so the caller doesn't need to
    /// know how to spawn a continuation.  `wake_fn` is fire-and-forget; it
    /// must internally `tokio::spawn` any async work.
    ///
    /// `team_name` is the team the triggering message originated from
    /// (per-team disk layout §6).  It is forwarded verbatim to `wake_fn` so
    /// the continuation turn can use it as the authoritative team context
    /// instead of re-reading `conv.json`.
    ///
    /// Returns `true` when this caller won the Idle→Running CAS (and the
    /// wake callback was invoked, if any).  Returns `false` if the Lead was
    /// already Running — the pending mark is recorded and Path A will catch
    /// it at turn end, so the wake callback is intentionally NOT invoked
    /// here (the running turn will pick up the work itself).
    pub async fn enqueue(&self, k: &LeadKey, team_name: String) -> bool {
        let mut s = self.state.lock().await;
        match s.get(k).copied() {
            None | Some(LeadState::Idle { .. }) => {
                s.insert(k.clone(), LeadState::Running);
                drop(s);
                self.pending_during_run
                    .lock()
                    .await
                    .insert(k.clone(), false);
                log::info!(
                    "[LeadIdleSupervisor] enqueue idle->running session={} agent={} team={}",
                    k.0.as_str(),
                    k.1.as_str(),
                    team_name
                );
                let ws = crate::telemetry::diagnostics_workspace();
                record_diagnostic(
                    &ws,
                    DiagnosticEvent::new("agent.lead_idle.enqueue", DiagnosticSource::Backend)
                        .conversation_id(k.0.as_str())
                        .agent_id(k.1.as_str())
                        .ok(true)
                        .payload(serde_json::json!({ "transition": "idle_to_running", "team": team_name.clone(), "wake_fn_fired": self.wake_fn.get().is_some() })),
                );
                if let Some(wake) = self.wake_fn.get() {
                    wake(k.clone(), team_name);
                }
                true
            }
            Some(LeadState::Running) => {
                drop(s);
                self.pending_during_run.lock().await.insert(k.clone(), true);
                log::info!(
                    "[LeadIdleSupervisor] enqueue already-running session={} agent={} team={} pending=true",
                    k.0.as_str(),
                    k.1.as_str(),
                    team_name
                );
                let ws = crate::telemetry::diagnostics_workspace();
                record_diagnostic(
                    &ws,
                    DiagnosticEvent::new("agent.lead_idle.enqueue", DiagnosticSource::Backend)
                        .conversation_id(k.0.as_str())
                        .agent_id(k.1.as_str())
                        .ok(true)
                        .payload(serde_json::json!({ "transition": "already_running_pending_recorded", "team": team_name })),
                );
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
        assert!(!sup.enqueue(&k, "default".to_string()).await);
        assert!(!sup.enqueue(&k, "default".to_string()).await);
        assert!(sup.mark_idle(&k).await);
    }

    #[tokio::test]
    async fn send_when_idle_returns_true_and_only_first_caller_wakes() {
        let sup = LeadIdleSupervisor::new();
        let k = key("s1", "lead-1");
        sup.mark_running(&k).await;
        sup.mark_idle(&k).await;

        let a = sup.enqueue(&k, "default".to_string()).await;
        let b = sup.enqueue(&k, "default".to_string()).await;
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
            assert!(!sup.enqueue(&k, "default".to_string()).await);
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
        assert!(sup.enqueue(&k, "default".to_string()).await);
        assert_eq!(sup.state_of(&k).await, Some("running"));
    }

    #[tokio::test]
    async fn distinct_keys_are_isolated() {
        let sup = LeadIdleSupervisor::new();
        let a = key("s1", "lead-a");
        let b = key("s2", "lead-b");
        sup.mark_running(&a).await;
        sup.enqueue(&a, "default".to_string()).await;
        assert_eq!(sup.state_of(&a).await, Some("running"));
        assert_eq!(sup.state_of(&b).await, None);
        assert!(sup.enqueue(&b, "default".to_string()).await);
        assert!(sup.mark_idle(&a).await, "a still has pending");
    }

    #[tokio::test]
    async fn wake_fn_receives_team_name_from_enqueue() {
        use std::sync::Mutex as StdMutex;
        let sup = LeadIdleSupervisor::new();
        let k = key("s1", "lead-x");
        let captured: Arc<StdMutex<Vec<(String, String, String)>>> =
            Arc::new(StdMutex::new(Vec::new()));
        let captured_for_fn = captured.clone();
        sup.set_wake_fn(Arc::new(move |key: LeadKey, team: String| {
            captured_for_fn.lock().unwrap().push((
                key.0.as_str().to_string(),
                key.1.as_str().to_string(),
                team,
            ));
        }));
        // Lead is fresh → first enqueue treated as Idle→Running and fires wake_fn.
        assert!(sup.enqueue(&k, "team-alpha".to_string()).await);
        let snapshot = captured.lock().unwrap().clone();
        assert_eq!(
            snapshot.len(),
            1,
            "wake_fn fires exactly once on Idle→Running"
        );
        assert_eq!(snapshot[0].0, "s1");
        assert_eq!(snapshot[0].1, "lead-x");
        assert_eq!(snapshot[0].2, "team-alpha", "team_name forwarded verbatim");

        // Subsequent enqueue while Running does NOT fire wake_fn (Path A picks it up).
        assert!(!sup.enqueue(&k, "team-beta".to_string()).await);
        assert_eq!(captured.lock().unwrap().len(), 1);
    }
}
