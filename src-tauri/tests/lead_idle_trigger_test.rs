//! LTR P2.4 Step 5 — `lead_idle_trigger_test`.
//!
//! Five behavior cases the v4 §5.6 dual-path mechanism must guarantee:
//!
//! 1. Lead is running a turn, a Teammate calls SendMessage → no immediate
//!    wake fires; the supervisor's `mark_idle` reports `pending == true`
//!    so Path A (chat_turn_driver self-check) can pick it up at turn end.
//!
//! 2. Lead is idle (post-turn), a Teammate calls SendMessage → Path C
//!    fires immediately: `enqueue` returns true and the injected wake_fn
//!    is invoked.
//!
//! 3. Lead is idle, two Teammates concurrently call SendMessage → only
//!    ONE caller wins the Idle→Running CAS, only ONE wake_fn invocation.
//!
//! 4. Lead is running, 10 sequential SendMessage calls during the
//!    Running window → no wake_fn fires; one consolidated `pending = true`
//!    at `mark_idle` covers all 10.
//!
//! 5. `mark_idle` with no SendMessage in this Running window → returns
//!    `pending == false`; chat_turn_driver Path A does NOT emit
//!    LeadHasPendingMessages, the regular AgentIdle path proceeds.
//!
//! These tests use the `LeadIdleSupervisor` directly with a counting
//! wake_fn rather than a real `RuntimeChatTurnDriver`, because driving a
//! full turn end-to-end requires a mocked `RuntimeLlmExecutor` + tool
//! registry + permission store that materially exceeds the value of
//! covering this state machine.  The supervisor IS the contract — Path A
//! and Path C wiring is just two thin shims that we already cover in
//! `chat_turn_driver::tests::path_a_*` and the supervisor's own unit
//! tests.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use app_lib::runtime::agent::{LeadIdleSupervisor, LeadKey};
use app_lib::runtime::ids::{AgentId, SessionId};

fn key(session: &str, agent: &str) -> LeadKey {
    (SessionId::new(session), AgentId::new(agent))
}

/// Build a supervisor wired with a counting wake_fn.
/// Returns `(supervisor, wake_count)`.  Read `wake_count.load(Ordering::SeqCst)`
/// to assert how many times Path C fired.
fn supervisor_with_wake_counter() -> (Arc<LeadIdleSupervisor>, Arc<AtomicUsize>) {
    let sup = LeadIdleSupervisor::new();
    let counter = Arc::new(AtomicUsize::new(0));
    let counter_clone = counter.clone();
    let installed = sup.set_wake_fn(Arc::new(move |_k: LeadKey, _team: String| {
        counter_clone.fetch_add(1, Ordering::SeqCst);
    }));
    assert!(
        installed,
        "wake_fn install must succeed in a fresh supervisor"
    );
    (sup, counter)
}

// ─── Case 1 — Lead running, SendMessage during run, Path A picks up at end

#[tokio::test]
async fn case1_send_during_run_does_not_wake_then_path_a_sees_pending() {
    let (sup, wake_count) = supervisor_with_wake_counter();
    let k = key("conv-c1", "lead-1");

    sup.mark_running(&k).await;
    let woke = sup.enqueue(&k, "default".to_string()).await;

    assert!(!woke, "enqueue while Running must return false");
    assert_eq!(
        wake_count.load(Ordering::SeqCst),
        0,
        "Path C wake_fn must NOT fire while Lead is Running — Path A's job"
    );

    let pending = sup.mark_idle(&k).await;
    assert!(
        pending,
        "mark_idle must report pending=true so Path A continues the loop"
    );
}

// ─── Case 2 — Lead idle, SendMessage immediately wakes via Path C

#[tokio::test]
async fn case2_send_when_idle_immediately_triggers_path_c_wake() {
    let (sup, wake_count) = supervisor_with_wake_counter();
    let k = key("conv-c2", "lead-2");

    // Simulate a complete prior turn cycle leaving the Lead Idle.
    sup.mark_running(&k).await;
    sup.mark_idle(&k).await;
    assert_eq!(
        sup.state_of(&k).await,
        Some("idle"),
        "supervisor should be Idle before the Path C send"
    );

    let woke = sup.enqueue(&k, "default".to_string()).await;
    assert!(woke, "enqueue while Idle must return true (CAS won)");
    assert_eq!(
        wake_count.load(Ordering::SeqCst),
        1,
        "Path C wake_fn must fire exactly once on Idle→Running transition"
    );
    assert_eq!(
        sup.state_of(&k).await,
        Some("running"),
        "supervisor must transition to Running"
    );
}

// ─── Case 3 — concurrent enqueues, CAS guarantees single wake

#[tokio::test]
async fn case3_two_concurrent_sends_when_idle_only_wake_once() {
    let (sup, wake_count) = supervisor_with_wake_counter();
    let k = key("conv-c3", "lead-3");

    sup.mark_running(&k).await;
    sup.mark_idle(&k).await;

    // Spawn two SendMessage callers as concurrently as tokio allows.
    let s1 = sup.clone();
    let s2 = sup.clone();
    let k1 = k.clone();
    let k2 = k.clone();
    let (a, b) = tokio::join!(
        tokio::spawn(async move { s1.enqueue(&k1, "default".to_string()).await }),
        tokio::spawn(async move { s2.enqueue(&k2, "default".to_string()).await }),
    );
    let r1 = a.unwrap();
    let r2 = b.unwrap();

    let trues = (r1 as u8) + (r2 as u8);
    assert_eq!(
        trues, 1,
        "exactly one caller must see enqueue=true (CAS); got r1={r1} r2={r2}"
    );
    assert_eq!(
        wake_count.load(Ordering::SeqCst),
        1,
        "Path C wake_fn must fire exactly once even with concurrent enqueues"
    );
}

// ─── Case 4 — Lead running, 10 sends collapse into a single Path A continuation

#[tokio::test]
async fn case4_ten_sends_during_run_collapse_into_one_continuation() {
    let (sup, wake_count) = supervisor_with_wake_counter();
    let k = key("conv-c4", "lead-4");

    sup.mark_running(&k).await;
    for _ in 0..10 {
        let woke = sup.enqueue(&k, "default".to_string()).await;
        assert!(!woke, "enqueue during Running must always return false");
    }
    assert_eq!(
        wake_count.load(Ordering::SeqCst),
        0,
        "no Path C wake should fire while Lead is Running"
    );

    let pending = sup.mark_idle(&k).await;
    assert!(
        pending,
        "the 10 sends collapse into a single pending=true at mark_idle"
    );
    assert_eq!(
        sup.state_of(&k).await,
        Some("idle+pending"),
        "state should be Idle with the pending flag set"
    );
}

// ─── Case 5 — clean turn end, no pending, no Path A continuation

#[tokio::test]
async fn case5_no_send_during_run_yields_clean_idle() {
    let (sup, wake_count) = supervisor_with_wake_counter();
    let k = key("conv-c5", "lead-5");

    sup.mark_running(&k).await;
    let pending = sup.mark_idle(&k).await;

    assert!(
        !pending,
        "mark_idle with no SendMessage during the run must return false"
    );
    assert_eq!(
        wake_count.load(Ordering::SeqCst),
        0,
        "Path C wake_fn must NOT fire on a clean turn end"
    );
    assert_eq!(
        sup.state_of(&k).await,
        Some("idle"),
        "state should be plain Idle (no pending flag)"
    );
}
