use app_lib::runtime::ids::{RunId, SessionId, TaskId, ToolCallId};
use app_lib::runtime::store::{
    InMemoryRunStore, InMemoryTaskStore, InMemoryToolCallStore, RunStatus, RunStore, TaskStore,
    ToolCallStatus, ToolCallStore,
};
use app_lib::runtime::task::{TaskRecord, TaskStatus};

#[test]
fn review_run_store_should_not_acknowledge_missing_status_updates() {
    let store = InMemoryRunStore::new();
    let result = store.update_run_status(&RunId::new("missing-run"), RunStatus::Completed);
    assert!(
        result.is_err(),
        "run store should error when a terminal status update targets a missing run; silent success breaks store truth source"
    );
}

#[test]
fn review_task_store_should_not_acknowledge_missing_status_updates() {
    let store = InMemoryTaskStore::new();
    let result = store.update_task_status(&TaskId::new("missing-task"), TaskStatus::Completed);
    assert!(
        result.is_err(),
        "task store should error when a terminal status update targets a missing task; silent success breaks task truth source"
    );
}

#[test]
fn review_tool_call_store_should_not_acknowledge_missing_status_updates() {
    let store = InMemoryToolCallStore::new();
    let result =
        store.update_tool_call_status(&ToolCallId::new("missing-call"), ToolCallStatus::Completed);
    assert!(
        result.is_err(),
        "tool call store should error when a terminal status update targets a missing call; silent success breaks tool-call truth source"
    );
}

#[test]
fn review_resume_related_store_records_should_roundtrip_when_present() {
    let run_store = InMemoryRunStore::new();
    run_store
        .create_run(app_lib::runtime::store::RunRecord {
            run_id: RunId::new("run-present"),
            session_id: SessionId::new("session-present"),
            status: RunStatus::Running,
        })
        .unwrap();
    assert_eq!(
        run_store
            .get_run(&RunId::new("run-present"))
            .unwrap()
            .unwrap()
            .status,
        RunStatus::Running
    );

    let task_store = InMemoryTaskStore::new();
    task_store
        .create_task(TaskRecord {
            task_id: TaskId::new("task-present"),
            parent_run_id: RunId::new("run-present"),
            owner_agent_id: None,
            subject: "resume probe".to_string(),
            status: TaskStatus::Running,
        })
        .unwrap();
    assert_eq!(
        task_store
            .get_task(&TaskId::new("task-present"))
            .unwrap()
            .unwrap()
            .status,
        TaskStatus::Running
    );
}
