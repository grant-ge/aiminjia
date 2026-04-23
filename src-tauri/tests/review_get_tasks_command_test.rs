use app_lib::runtime::ids::{RunId, SessionId, TaskId};
use app_lib::runtime::store::{InMemoryTaskStore, TaskStore};
use app_lib::runtime::task::task_models::{TaskRecord, TaskStatus};

/// list_for_session 只返回指定 session 的 tasks，不泄漏其他 session 数据。
#[test]
fn review_list_for_session_filters_by_session_id() {
    let store = InMemoryTaskStore::new();

    store.create_task(TaskRecord {
        task_id: TaskId::new("t1"),
        session_id: SessionId::new("conv-abc"),
        parent_run_id: RunId::new("run-1"),
        owner_agent_id: None,
        subject: "Task in conv-abc".to_string(),
        status: TaskStatus::Pending,
        active_form: None,
    }).unwrap();

    store.create_task(TaskRecord {
        task_id: TaskId::new("t2"),
        session_id: SessionId::new("conv-xyz"),
        parent_run_id: RunId::new("run-2"),
        owner_agent_id: None,
        subject: "Task in conv-xyz".to_string(),
        status: TaskStatus::Running,
        active_form: Some("探索中…".to_string()),
    }).unwrap();

    let result = store.list_for_session(&SessionId::new("conv-abc")).unwrap();

    assert_eq!(result.len(), 1, "must only return tasks for conv-abc");
    assert_eq!(result[0].task_id.as_str(), "t1");
    assert_eq!(result[0].subject, "Task in conv-abc");
}

/// 空 session 返回空列表，不 panic。
#[test]
fn review_list_for_session_returns_empty_for_unknown_session() {
    let store = InMemoryTaskStore::new();
    let result = store.list_for_session(&SessionId::new("no-such-session")).unwrap();
    assert!(result.is_empty());
}

/// TaskRecordFrontend::from 正确序列化所有字段。
#[test]
fn review_task_record_frontend_serialization() {
    use app_lib::models::message::TaskRecordFrontend;

    let record = TaskRecord {
        task_id: TaskId::new("t3"),
        session_id: SessionId::new("conv-s"),
        parent_run_id: RunId::new("run-3"),
        owner_agent_id: None,
        subject: "导出数据".to_string(),
        status: TaskStatus::Completed,
        active_form: Some("导出中…".to_string()),
    };

    let frontend: TaskRecordFrontend = record.into();
    let json = serde_json::to_value(&frontend).unwrap();

    assert_eq!(json["taskId"], "t3");
    assert_eq!(json["sessionId"], "conv-s");
    assert_eq!(json["subject"], "导出数据");
    assert_eq!(json["status"], "completed");
    assert_eq!(json["activeForm"], "导出中…");
}
