use serde_json::json;
use tempfile::TempDir;

use app_lib::models::message::TaskRecordFrontend;
use app_lib::runtime::cancellation::CancellationToken;
use app_lib::runtime::ids::{RunId, SessionId};
use app_lib::runtime::task::task_models::{TaskRecord, TaskStatus};
use app_lib::runtime::task::FileTaskV2Store;
use app_lib::runtime::tools::builtin::task_tools::{
    TaskCreateRuntimeTool, TaskListRuntimeTool, TaskUpdateRuntimeTool,
};
use app_lib::runtime::tools::catalog::{DAILY_ALLOWED_TOOLS, TOOL_CATALOG};
use app_lib::runtime::tools::context::ToolExecutionContext;
use app_lib::runtime::tools::RuntimeTool;

fn ctx(root: &TempDir) -> ToolExecutionContext {
    let mut ctx = ToolExecutionContext::new(
        "sess-task-test".into(),
        "run-task-test".into(),
        None,
        "tc-task-test",
        CancellationToken::new(),
    );
    ctx.task_store_root = Some(root.path().to_path_buf());
    ctx
}

#[test]
fn task_tools_are_in_catalog_and_daily_allowed() {
    for name in ["TaskCreate", "TaskUpdate", "TaskList"] {
        assert!(
            TOOL_CATALOG.get_entry(name).is_some(),
            "{} catalog entry missing",
            name
        );
        assert!(
            DAILY_ALLOWED_TOOLS.contains(&name),
            "{} missing from DAILY_ALLOWED_TOOLS",
            name
        );
    }
}

#[tokio::test]
async fn task_create_persists_and_task_list_reads() {
    let root = TempDir::new().unwrap();
    let create = TaskCreateRuntimeTool;
    let list = TaskListRuntimeTool;

    let create_result = create
        .execute(
            json!({
                "subject": "Write test",
                "description": "Write a regression test",
                "activeForm": "Writing test"
            }),
            ctx(&root),
        )
        .await
        .unwrap();

    assert!(create_result.content.contains("Task #1 created"));

    let list_result = list.execute(json!({}), ctx(&root)).await.unwrap();
    assert!(list_result.content.contains("#1 [pending] Write test"));
}

#[tokio::test]
async fn task_update_changes_status_and_owner() {
    let root = TempDir::new().unwrap();
    let create = TaskCreateRuntimeTool;
    let update = TaskUpdateRuntimeTool;
    let list = TaskListRuntimeTool;

    create
        .execute(
            json!({
                "subject": "Implement feature",
                "description": "Implement feature details"
            }),
            ctx(&root),
        )
        .await
        .unwrap();

    let update_result = update
        .execute(
            json!({
                "taskId": "1",
                "status": "in_progress",
                "owner": "agent-a"
            }),
            ctx(&root),
        )
        .await
        .unwrap();

    assert!(update_result.content.contains("Updated task #1"));

    let list_result = list.execute(json!({}), ctx(&root)).await.unwrap();
    assert!(list_result
        .content
        .contains("#1 [in_progress] Implement feature (agent-a)"));
}

#[tokio::test]
async fn task_update_delete_removes_task() {
    let root = TempDir::new().unwrap();
    let create = TaskCreateRuntimeTool;
    let update = TaskUpdateRuntimeTool;
    let list = TaskListRuntimeTool;

    create
        .execute(
            json!({
                "subject": "Temporary task",
                "description": "Will be deleted"
            }),
            ctx(&root),
        )
        .await
        .unwrap();

    update
        .execute(
            json!({
                "taskId": "1",
                "status": "deleted"
            }),
            ctx(&root),
        )
        .await
        .unwrap();

    let list_result = list.execute(json!({}), ctx(&root)).await.unwrap();
    assert_eq!(list_result.content, "No tasks found");
}

#[test]
fn task_frontend_records_are_loaded_from_file_task_v2_store() {
    let root = TempDir::new().unwrap();
    let store = FileTaskV2Store::new(root.path().to_path_buf());
    let session_id = SessionId::new("sess-file-task-ui");

    store
        .create(
            session_id.as_str(),
            &TaskRecord {
                id: "1".to_string(),
                subject: "Fix task monitor".to_string(),
                description: "Read Task V2 files for the right panel".to_string(),
                active_form: Some("Fixing task monitor".to_string()),
                owner: Some("agent-a".to_string()),
                status: TaskStatus::InProgress,
                blocks: vec![],
                blocked_by: vec![],
                metadata: None,
                session_id: session_id.clone(),
                parent_run_id: RunId::new("run-file-task-ui"),
                owner_agent_id: None,
            },
        )
        .unwrap();

    let tasks =
        TaskRecordFrontend::list_from_task_v2_store(root.path(), session_id.as_str()).unwrap();

    assert_eq!(tasks.len(), 1);
    assert_eq!(tasks[0].task_id, "1");
    assert_eq!(tasks[0].run_id, "run-file-task-ui");
    assert_eq!(tasks[0].subject, "Fix task monitor");
    assert_eq!(tasks[0].status, "in_progress");
    assert_eq!(tasks[0].active_form.as_deref(), Some("Fixing task monitor"));
    assert_eq!(tasks[0].owner.as_deref(), Some("agent-a"));
}

#[test]
fn task_frontend_records_fall_back_to_legacy_root_tasks() {
    let legacy_root = TempDir::new().unwrap();
    let user_root = legacy_root.path().join("users").join("t_28__u_54");
    std::fs::create_dir_all(&user_root).unwrap();

    let store = FileTaskV2Store::new(legacy_root.path().to_path_buf());
    let session_id = SessionId::new("sess-legacy-task-ui");
    store
        .create(
            session_id.as_str(),
            &TaskRecord {
                id: "1".to_string(),
                subject: "Restore legacy root task".to_string(),
                description: "Read task files created before user-scoped storage".to_string(),
                active_form: Some("Restoring task".to_string()),
                owner: None,
                status: TaskStatus::Pending,
                blocks: vec![],
                blocked_by: vec![],
                metadata: None,
                session_id: session_id.clone(),
                parent_run_id: RunId::new("run-legacy-task-ui"),
                owner_agent_id: None,
            },
        )
        .unwrap();

    let tasks =
        TaskRecordFrontend::list_from_task_v2_store(&user_root, session_id.as_str()).unwrap();

    assert_eq!(tasks.len(), 1);
    assert_eq!(tasks[0].task_id, "1");
    assert_eq!(tasks[0].subject, "Restore legacy root task");
    assert_eq!(tasks[0].run_id, "run-legacy-task-ui");
}
