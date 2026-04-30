use app_lib::runtime::events::{RuntimeEvent, RuntimeEventKind};
use app_lib::runtime::ids::{RunId, SessionId, TaskId};
use app_lib::transport::tauri_event_adapter::map_runtime_event;

#[test]
fn review_task_terminal_runtime_event_should_not_be_dropped_by_legacy_adapter() {
    let event = RuntimeEvent::new(
        SessionId::new("session-task"),
        RunId::new("run-task"),
        RuntimeEventKind::TaskStatusChanged {
            task_id: TaskId::new("task-1"),
            status: "completed".to_string(),
            subject: String::new(),
            active_form: None,
            owner_agent_id: None,
        },
    );
    assert!(
        map_runtime_event(&event).is_some(),
        "task terminal runtime events are still dropped by the legacy adapter, so UI/transport cannot observe task completion"
    );
}
