#[test]
fn review_task_runtime_should_model_failed_terminal_state() {
    let source = include_str!("../src/runtime/task/task_models.rs");
    assert!(
        source.contains("Failed"),
        "task runtime currently cannot represent a failed terminal state, so task outcomes collapse into success/cancel only"
    );
}

#[test]
fn review_runtime_event_protocol_should_expose_task_terminal_notifications() {
    let source = include_str!("../src/runtime/events.rs");
    assert!(
        source.contains("TaskCompleted")
            || source.contains("TaskFailed")
            || source.contains("TaskStatusChanged"),
        "runtime event protocol has no task terminal notification event, so UI/transport cannot observe task completion/failure from the runtime"
    );
}
