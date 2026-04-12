#[test]
fn review_task_runtime_should_emit_task_status_events_not_only_write_store() {
    let source = include_str!("../src/runtime/task/task_runtime.rs");
    assert!(
        source.contains("TaskStatusChanged") || source.contains("RuntimeEventBus"),
        "task runtime still only mutates the store and does not publish task status events, so terminal task states remain invisible to transport/UI"
    );
}
