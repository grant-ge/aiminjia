use std::sync::Arc;

use app_lib::runtime::event_bus::RuntimeEventBus;
use app_lib::runtime::events::{RuntimeEvent, RuntimeEventKind};
use app_lib::runtime::ids::{RunId, SessionId, TaskId, ToolCallId};
use app_lib::runtime::store::InMemoryTaskStore;
use app_lib::runtime::task::{TaskRecord, TaskRuntime, TaskStatus};
use app_lib::transport::tauri_event_adapter::TauriEventAdapter;
use app_lib::transport::testing::RecordingRuntimeHost;

fn make_bus_with_host() -> (RuntimeEventBus, Arc<RecordingRuntimeHost>) {
    let host = RecordingRuntimeHost::new();
    let bus = RuntimeEventBus::new();
    bus.subscribe(Arc::new(TauriEventAdapter::new(host.clone())));
    (bus, host)
}

#[tokio::test]
async fn review_tool_executing_payload_includes_input() {
    let (bus, host) = make_bus_with_host();
    let session_id = SessionId::new("s1");
    let run_id = RunId::new("r1");

    bus.emit(RuntimeEvent::new(
        session_id.clone(),
        run_id.clone(),
        RuntimeEventKind::ToolCallExecuting {
            tool_call_id: ToolCallId::new("tc-1"),
            tool_name: "browse_navigate".to_string(),
            input: serde_json::json!({"url": "https://example.com"}),
        },
    ))
    .await
    .unwrap();

    let trace = host.trace();
    let event = trace
        .events
        .iter()
        .find(|e| e.name == "tool:executing")
        .expect("tool:executing must be emitted");

    assert_eq!(
        event.payload["toolName"].as_str(),
        Some("browse_navigate"),
    );
    assert_eq!(
        event.payload["input"]["url"].as_str(),
        Some("https://example.com"),
        "tool:executing payload must include input field"
    );
}

#[tokio::test]
async fn review_tool_completed_payload_is_full_message() {
    let (bus, host) = make_bus_with_host();
    let session_id = SessionId::new("s2");
    let run_id = RunId::new("r2");

    bus.emit(RuntimeEvent::new(
        session_id.clone(),
        run_id.clone(),
        RuntimeEventKind::ToolCallCompleted {
            tool_call_id: ToolCallId::new("tc-2"),
            tool_name: "browse_navigate".to_string(),
            is_error: false,
            content: "Page ready: https://example.com".to_string(),
            msg_id: "tool-abc-123".to_string(),
            duration_ms: Some(1200),
        },
    ))
    .await
    .unwrap();

    let trace = host.trace();
    let event = trace
        .events
        .iter()
        .find(|e| e.name == "tool:completed")
        .expect("tool:completed must be emitted");

    assert_eq!(
        event.payload["role"].as_str(),
        Some("tool"),
        "payload.role must be 'tool'"
    );
    assert_eq!(
        event.payload["id"].as_str(),
        Some("tool-abc-123"),
        "payload.id must match msg_id"
    );
    assert_eq!(
        event.payload["toolResult"]["toolCallId"].as_str(),
        Some("tc-2"),
        "payload.toolResult.toolCallId must be present"
    );
    assert_eq!(
        event.payload["toolResult"]["content"].as_str(),
        Some("Page ready: https://example.com"),
    );
    assert_eq!(
        event.payload["toolResult"]["isError"].as_bool(),
        Some(false),
    );
    assert_eq!(
        event.payload["toolResult"]["durationMs"].as_u64(),
        Some(1200),
    );
}

#[test]
fn review_task_status_changed_payload_includes_subject_and_active_form() {
    let host = RecordingRuntimeHost::new();
    let bus = RuntimeEventBus::new();
    bus.subscribe(Arc::new(TauriEventAdapter::new(host.clone())));

    let store = Arc::new(InMemoryTaskStore::new());
    let runtime = TaskRuntime::with_event_bus(store.clone(), bus);
    let task_id = TaskId::new("task-payload-1");

    runtime
        .create_task(TaskRecord {
            task_id: task_id.clone(),
            session_id: SessionId::new("session-payload"),
            parent_run_id: RunId::new("run-payload-1"),
            owner_agent_id: None,
            subject: "探索项目上下文".to_string(),
            status: TaskStatus::Pending,
            active_form: Some("探索中…".to_string()),
        })
        .unwrap();

    runtime.set_status(&task_id, TaskStatus::Running).unwrap();

    let trace = host.trace();
    let event = trace
        .events
        .iter()
        .find(|e| e.name == "task:status-changed")
        .expect("task:status-changed must be emitted");

    assert_eq!(
        event.payload["subject"].as_str(),
        Some("探索项目上下文"),
        "payload must include subject"
    );
    assert_eq!(
        event.payload["activeForm"].as_str(),
        Some("探索中…"),
        "payload must include activeForm"
    );
    assert_eq!(event.payload["status"].as_str(), Some("running"));
}
