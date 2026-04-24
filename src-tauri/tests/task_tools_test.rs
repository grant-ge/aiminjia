use tempfile::TempDir;
use serde_json::json;

use app_lib::runtime::cancellation::CancellationToken;
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
        assert!(TOOL_CATALOG.get_entry(name).is_some(), "{} catalog entry missing", name);
        assert!(DAILY_ALLOWED_TOOLS.contains(&name), "{} missing from DAILY_ALLOWED_TOOLS", name);
    }
}

#[tokio::test]
async fn task_create_persists_and_task_list_reads() {
    let root = TempDir::new().unwrap();
    let create = TaskCreateRuntimeTool;
    let list = TaskListRuntimeTool;

    let create_result = create.execute(json!({
        "subject": "Write test",
        "description": "Write a regression test",
        "activeForm": "Writing test"
    }), ctx(&root)).await.unwrap();

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

    create.execute(json!({
        "subject": "Implement feature",
        "description": "Implement feature details"
    }), ctx(&root)).await.unwrap();

    let update_result = update.execute(json!({
        "taskId": "1",
        "status": "in_progress",
        "owner": "agent-a"
    }), ctx(&root)).await.unwrap();

    assert!(update_result.content.contains("Updated task #1"));

    let list_result = list.execute(json!({}), ctx(&root)).await.unwrap();
    assert!(list_result.content.contains("#1 [in_progress] Implement feature (agent-a)"));
}

#[tokio::test]
async fn task_update_delete_removes_task() {
    let root = TempDir::new().unwrap();
    let create = TaskCreateRuntimeTool;
    let update = TaskUpdateRuntimeTool;
    let list = TaskListRuntimeTool;

    create.execute(json!({
        "subject": "Temporary task",
        "description": "Will be deleted"
    }), ctx(&root)).await.unwrap();

    update.execute(json!({
        "taskId": "1",
        "status": "deleted"
    }), ctx(&root)).await.unwrap();

    let list_result = list.execute(json!({}), ctx(&root)).await.unwrap();
    assert_eq!(list_result.content, "No tasks found");
}
