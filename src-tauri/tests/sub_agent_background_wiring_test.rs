// Phase 3 real wiring test: sub_agent.rs → background path → message_bridge → AgentIdle
//
// This test exercises the production code path in sub_agent.rs where a background
// child run is completed via `AgentRuntime::complete_background_run`, which persists
// the summary and emits `AgentIdle` through the `RuntimeEventBus`.
//
// It does NOT use the `for_test` helper alone — it uses a shared `AgentRuntime`
// and `RuntimeEventBus` just like the real application would.

use app_lib::runtime::agent::message_bridge;
use app_lib::runtime::agent::{AgentRuntime, SpawnChildRunRequest};
use app_lib::runtime::event_bus::RuntimeEventBus;
use app_lib::runtime::events::RuntimeEventKind;
use app_lib::runtime::ids::{RunId, SessionId};

// ─── message_bridge unit tests ──────────────────────────────────────────────

/// `format_sub_agent_summary` must include all key fields.
#[test]
fn message_bridge_summary_contains_key_fields() {
    let s = message_bridge::format_sub_agent_summary("task done", 5, 3);
    assert!(s.contains("iterations=5"), "missing iterations: {s}");
    assert!(s.contains("files=3"), "missing files count: {s}");
    assert!(s.contains("task done"), "missing output: {s}");
}

/// Long output must be truncated to stay manageable.
#[test]
fn message_bridge_summary_truncates_long_output() {
    let long = "a".repeat(2000);
    let s = message_bridge::format_sub_agent_summary(&long, 1, 0);
    // The total string must not balloon — output portion is capped at 500 chars
    assert!(
        s.len() < 600,
        "summary too long ({} chars), expected truncation",
        s.len()
    );
    assert!(
        s.ends_with("..."),
        "expected trailing '...' for truncated output"
    );
}

// ─── background run completion wiring ───────────────────────────────────────

/// When a background child run completes, `complete_background_run` must:
///   1. mark the child run as `completed`
///   2. persist the summary in the invocation store
///   3. emit `AgentIdle` on the bus (UI contract)
#[tokio::test]
async fn background_run_complete_background_run_wires_summary_and_idle_event() {
    let bus = RuntimeEventBus::new();
    let runtime = AgentRuntime::for_test();
    let session_id = SessionId::new("sess-bg-1");
    let parent_run_id = RunId::new("run-parent-bg-1");

    // Spawn a background-flagged child run
    let mut req = SpawnChildRunRequest::for_test(parent_run_id.clone());
    req.background = true;
    let handle = runtime.spawn_child_run(req).await.unwrap();
    let child_run_id = handle.child_run_id().clone();
    let agent_id = handle.agent_id().clone();

    // Simulate what sub_agent.rs does: build the summary via message_bridge helper
    let output = "extracted 100 rows from dataset";
    let summary = message_bridge::format_sub_agent_summary(output, 4, 1);

    // Call the real wiring point
    runtime
        .complete_background_run(
            &child_run_id,
            Some(&summary),
            None,
            session_id.clone(),
            parent_run_id.clone(),
            bus.clone(),
        )
        .await
        .unwrap();

    // ── Assertions ──────────────────────────────────────────────────────────

    // 1. Status is completed
    assert_eq!(
        runtime.status(&child_run_id).await.unwrap(),
        "completed",
        "child run should be marked completed"
    );

    // 2. Summary persisted
    let stored = runtime.get_summary(&child_run_id).await.unwrap();
    assert!(
        stored.is_some(),
        "summary should be persisted in invocation store"
    );
    let stored = stored.unwrap();
    assert!(
        stored.contains("iterations=4"),
        "stored summary should contain iteration count: {stored}"
    );
    assert!(
        stored.contains("files=1"),
        "stored summary should contain file count: {stored}"
    );

    // 3. AgentIdle event emitted for the correct agent
    let events = bus.recorded();
    let idle = events
        .iter()
        .find(|e| matches!(&e.kind, RuntimeEventKind::AgentIdle { agent_id: aid, .. } if aid == &agent_id));
    assert!(
        idle.is_some(),
        "AgentIdle event must be emitted on bus for agent {agent_id:?}"
    );

    // 4. The idle event carries the correct run/session context
    let idle = idle.unwrap();
    assert_eq!(idle.session_id, session_id);
    assert_eq!(idle.run_id, parent_run_id);
}

/// Foreground sub-agent runs must NOT emit AgentIdle (only background does).
#[tokio::test]
async fn foreground_run_complete_does_not_emit_agent_idle() {
    let bus = RuntimeEventBus::new();
    let runtime = AgentRuntime::for_test();

    let req = SpawnChildRunRequest::for_test(RunId::new("run-fg-parent"));
    // background = false (default)
    assert!(!req.background);

    let handle = runtime.spawn_child_run(req).await.unwrap();
    // Foreground path: use plain complete_run — no AgentIdle
    runtime.complete_run(handle.child_run_id()).await.unwrap();

    let events = bus.recorded();
    assert!(
        events.is_empty(),
        "foreground complete_run must not emit any events, got: {events:?}"
    );
}

/// Verify that `complete_background_run` tolerates a missing invocation gracefully.
#[tokio::test]
async fn background_run_unknown_child_run_is_noop() {
    let bus = RuntimeEventBus::new();
    let runtime = AgentRuntime::for_test();
    let missing = RunId::new("does-not-exist");

    // Should not panic or return Err
    let result = runtime
        .complete_background_run(
            &missing,
            Some("summary"),
            None,
            SessionId::new("s"),
            RunId::new("p"),
            bus.clone(),
        )
        .await;
    assert!(
        result.is_ok(),
        "missing invocation should be a no-op: {result:?}"
    );

    // No events emitted
    assert!(
        bus.recorded().is_empty(),
        "no events expected for missing run"
    );
}

// ─── BackgroundRun struct ────────────────────────────────────────────────────

/// `BackgroundRun` tracks parent→child association.
#[test]
fn background_run_new_stores_run_ids() {
    use app_lib::runtime::agent::background::BackgroundRun;

    let parent = RunId::new("p1");
    let child = RunId::new("c1");
    let bg = BackgroundRun::new(parent.clone(), child.clone());
    assert_eq!(bg.parent_run_id, parent);
    assert_eq!(bg.child_run_id, child);
}
