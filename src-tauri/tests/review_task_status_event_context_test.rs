use std::sync::Arc;

use app_lib::runtime::ids::{RunId, SessionId, TaskId};
use app_lib::runtime::store::InMemoryTaskStore;
use app_lib::runtime::task::{TaskRecord, TaskRuntime, TaskStatus};
use app_lib::runtime::RuntimeEventBus;
use app_lib::transport::tauri_event_adapter::TauriEventAdapter;
use app_lib::transport::testing::RecordingRuntimeHost;

#[test]
fn review_task_terminal_notification_should_use_real_parent_run_context() {
    let host = RecordingRuntimeHost::new();
    let bus = RuntimeEventBus::new();
    let _adapter = Arc::new(TauriEventAdapter::new(host.clone()));
    bus.subscribe(_adapter.clone());

    let store = Arc::new(InMemoryTaskStore::new());
    let runtime = TaskRuntime::with_event_bus(store.clone(), bus);
    let task_id = TaskId::new("task-ctx-1");

    runtime
        .create_task(TaskRecord {
            id: task_id.as_str().to_string(),
            description: String::new(),
            owner: None,
            blocks: vec![],
            blocked_by: vec![],
            metadata: None,
            session_id: SessionId::new("test-session"),
            parent_run_id: RunId::new("run-parent-ctx"),
            owner_agent_id: None,
            subject: "review task".to_string(),
            status: TaskStatus::InProgress,
            active_form: None,
        })
        .unwrap();

    runtime.set_status(&task_id, TaskStatus::Completed).unwrap();

    let trace = host.trace();
    let event = trace
        .events
        .iter()
        .find(|event| event.name == "task:status-changed")
        .expect("task runtime should emit task:status-changed");

    assert_eq!(
        event.payload.get("taskId").and_then(|value| value.as_str()),
        Some("task-ctx-1")
    );
    assert_eq!(
        event.payload.get("runId").and_then(|value| value.as_str()),
        Some("run-parent-ctx"),
        "task terminal notifications should keep the owning parent run id so the host can attribute task completion to the correct run"
    );
}
