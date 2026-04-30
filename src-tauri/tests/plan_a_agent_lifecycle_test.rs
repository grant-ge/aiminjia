use app_lib::runtime::agent::agent_runtime::AgentRuntime;
use app_lib::runtime::agent::invocation::SpawnChildRunRequest;
use app_lib::runtime::ids::RunId;

#[tokio::test]
async fn a2_fail_run_sets_status_to_failed() {
    let runtime = AgentRuntime::for_test();
    let handle = runtime
        .spawn_child_run(SpawnChildRunRequest {
            parent_run_id: RunId::new("parent-run"),
            background: false,
            allowed_tools: vec![],
        })
        .await
        .unwrap();
    let child_run_id = handle.child_run_id().clone();

    runtime.fail_run(&child_run_id).await.unwrap();

    let status = runtime.status(&child_run_id).await.unwrap();
    assert_eq!(status, "failed", "fail_run must set status to Failed");
}

#[tokio::test]
async fn a2_status_returns_failed_string() {
    let runtime = AgentRuntime::for_test();
    let handle = runtime
        .spawn_child_run(SpawnChildRunRequest {
            parent_run_id: RunId::new("parent-run-2"),
            background: false,
            allowed_tools: vec![],
        })
        .await
        .unwrap();
    let child_run_id = handle.child_run_id().clone();

    runtime.fail_run(&child_run_id).await.unwrap();
    assert_eq!(runtime.status(&child_run_id).await.unwrap(), "failed");

    let handle2 = runtime
        .spawn_child_run(SpawnChildRunRequest {
            parent_run_id: RunId::new("parent-run-3"),
            background: false,
            allowed_tools: vec![],
        })
        .await
        .unwrap();
    let child_run_id2 = handle2.child_run_id().clone();
    runtime.complete_run(&child_run_id2).await.unwrap();
    assert_eq!(runtime.status(&child_run_id2).await.unwrap(), "completed");
}

#[tokio::test]
async fn a2_fail_background_run_sets_failed_status_and_emits_task_status_changed() {
    let runtime = AgentRuntime::for_test();
    let bus = app_lib::runtime::event_bus::RuntimeEventBus::new();
    let parent_run_id = RunId::new("parent-run-bg");
    let handle = runtime
        .spawn_child_run(SpawnChildRunRequest {
            parent_run_id: parent_run_id.clone(),
            background: true,
            allowed_tools: vec![],
        })
        .await
        .unwrap();
    let child_run_id = handle.child_run_id().clone();

    runtime
        .fail_background_run(
            &child_run_id,
            Some("llm exploded"),
            "session-bg".into(),
            parent_run_id,
            bus.clone(),
        )
        .await
        .unwrap();

    assert_eq!(runtime.status(&child_run_id).await.unwrap(), "failed");
    assert_eq!(
        runtime.get_summary(&child_run_id).await.unwrap().as_deref(),
        Some("llm exploded")
    );

    let events = bus.recorded();
    assert!(events.iter().any(|event| matches!(
        &event.kind,
        app_lib::runtime::events::RuntimeEventKind::TaskStatusChanged { task_id, status, .. }
        if task_id.as_str() == child_run_id.as_str() && status == "failed"
    )));
}

#[test]
fn a2_agent_status_failed_serde_roundtrip() {
    let status = app_lib::runtime::agent::invocation::AgentStatus::Failed;
    let serialized = serde_json::to_string(&status).unwrap();
    assert_eq!(serialized, r#""Failed""#);
    let deserialized: app_lib::runtime::agent::invocation::AgentStatus =
        serde_json::from_str(&serialized).unwrap();
    assert_eq!(
        deserialized,
        app_lib::runtime::agent::invocation::AgentStatus::Failed
    );
}

#[tokio::test]
async fn a2_status_string_for_failed_is_lowercase() {
    let runtime = AgentRuntime::for_test();
    let handle = runtime
        .spawn_child_run(SpawnChildRunRequest {
            parent_run_id: RunId::new("p-run"),
            background: false,
            allowed_tools: vec![],
        })
        .await
        .unwrap();
    let cid = handle.child_run_id().clone();
    runtime.fail_run(&cid).await.unwrap();

    let status_str = runtime.status(&cid).await.unwrap();
    assert_eq!(status_str, "failed");
}
