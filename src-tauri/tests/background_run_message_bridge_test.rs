// Phase 3 Task 2 Step 4: background run + message bridge
// Tests are written first (TDD); they will fail until the implementation is in place.

use app_lib::runtime::agent::{AgentRuntime, SpawnChildRunRequest};
use app_lib::runtime::event_bus::RuntimeEventBus;
use app_lib::runtime::events::RuntimeEventKind;
use app_lib::runtime::ids::{RunId, SessionId};

/// A background-flagged child run should start in Running status and not block
/// the caller (tokio::spawn semantics verified by the JoinHandle being returned).
#[tokio::test]
async fn background_child_run_starts_running() {
    let runtime = AgentRuntime::for_test();
    let mut req = SpawnChildRunRequest::for_test(RunId::new("run-parent"));
    req.background = true;

    let handle = runtime.spawn_child_run(req).await.unwrap();
    assert_eq!(
        runtime.status(handle.child_run_id()).await.unwrap(),
        "running"
    );
    assert!(handle.invocation().background);
}

/// Completing a background child run should persist a summary and emit
/// AgentIdle so the UI can consume the completion event.
#[tokio::test]
async fn complete_background_run_stores_summary_and_emits_agent_idle() {
    let bus = RuntimeEventBus::new();
    let session_id = SessionId::new("session-1");
    let parent_run_id = RunId::new("run-parent");

    let runtime = AgentRuntime::for_test();
    let mut req = SpawnChildRunRequest::for_test(parent_run_id.clone());
    req.background = true;

    let handle = runtime.spawn_child_run(req).await.unwrap();
    let child_run_id = handle.child_run_id().clone();
    let agent_id = handle.agent_id().clone();

    // Complete with a summary
    runtime
        .complete_background_run(
            &child_run_id,
            Some("analysis complete: 42 rows processed"),
            session_id.clone(),
            parent_run_id.clone(),
            bus.clone(),
        )
        .await
        .unwrap();

    // Summary persisted
    let summary = runtime.get_summary(&child_run_id).await.unwrap();
    assert_eq!(
        summary.as_deref(),
        Some("analysis complete: 42 rows processed")
    );

    // Status is Completed
    assert_eq!(
        runtime.status(&child_run_id).await.unwrap(),
        "completed"
    );

    // AgentIdle event emitted for the agent
    let events = bus.recorded();
    let idle_event = events
        .iter()
        .find(|e| matches!(&e.kind, RuntimeEventKind::AgentIdle { agent_id: aid } if aid == &agent_id));
    assert!(
        idle_event.is_some(),
        "AgentIdle event not emitted for agent {agent_id:?}"
    );
}

/// A non-background run should not use complete_background_run path;
/// plain complete_run should work and produce no AgentIdle event.
#[tokio::test]
async fn foreground_run_complete_does_not_emit_agent_idle() {
    let bus = RuntimeEventBus::new();
    let runtime = AgentRuntime::for_test();
    let req = SpawnChildRunRequest::for_test(RunId::new("run-parent"));

    let handle = runtime.spawn_child_run(req).await.unwrap();
    let child_run_id = handle.child_run_id().clone();

    runtime.complete_run(&child_run_id).await.unwrap();

    let events = bus.recorded();
    let has_idle = events
        .iter()
        .any(|e| matches!(&e.kind, RuntimeEventKind::AgentIdle { .. }));
    assert!(!has_idle, "foreground complete should not emit AgentIdle");
}
