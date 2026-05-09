//! Unit tests for TaskGetRuntimeTool.
//!
//! Covers 4 behaviours:
//! 1. returns_full_task_for_known_id
//! 2. returns_null_for_missing_id
//! 3. rejects_missing_taskId_field
//! 4. is_read_only

use serde_json::json;
use tempfile::TempDir;

use app_lib::runtime::cancellation::CancellationToken;
use app_lib::runtime::ids::{RunId, SessionId};
use app_lib::runtime::task::task_models::{TaskRecord, TaskStatus};
use app_lib::runtime::task::FileTaskV2Store;
use app_lib::runtime::tools::builtin::task_tools::TaskGetRuntimeTool;
use app_lib::runtime::tools::context::ToolExecutionContext;
use app_lib::runtime::tools::executor::ToolError;
use app_lib::runtime::tools::RuntimeTool;

// ─── helpers ──────────────────────────────────────────────────────────────────

fn ctx(root: &TempDir) -> ToolExecutionContext {
    let mut c = ToolExecutionContext::new(
        SessionId::new("sess-get-test"),
        RunId::new("run-get-test"),
        None,
        "tc-get-test",
        CancellationToken::new(),
    );
    c.task_store_root = Some(root.path().to_path_buf());
    c
}

/// Insert a task record into the store for the session "sess-get-test"
/// (which task_list_id() derives from ctx.session_id.as_str()).
fn insert_task(root: &TempDir, id: &str, subject: &str) -> TaskRecord {
    let store = FileTaskV2Store::new(root.path().to_path_buf());
    let session_id = SessionId::new("sess-get-test");
    let task = TaskRecord {
        id: id.to_string(),
        subject: subject.to_string(),
        description: "test task".to_string(),
        active_form: None,
        owner: None,
        status: TaskStatus::Pending,
        blocks: vec![],
        blocked_by: vec![],
        metadata: None,
        session_id: session_id.clone(),
        parent_run_id: RunId::new("run-get-test"),
        owner_agent_id: None,
    };
    store.create(session_id.as_str(), &task).unwrap();
    task
}

// ─── Test 1 ──────────────────────────────────────────────────────────────────

#[tokio::test]
async fn returns_full_task_for_known_id() {
    let root = TempDir::new().unwrap();
    insert_task(&root, "1", "Do something important");

    let tool = TaskGetRuntimeTool;
    let result = tool
        .execute(json!({"taskId": "1"}), ctx(&root))
        .await
        .expect("TaskGet should succeed for known id");

    // Content should contain the task info
    assert!(
        result.content.contains("#1") && result.content.contains("Do something important"),
        "content should describe the task, got: {}",
        result.content
    );

    // Data must have a non-null task object with the expected fields
    let data = result.data.expect("data field must be present");
    let task_obj = data
        .get("task")
        .expect("data.task must be present");
    assert!(
        !task_obj.is_null(),
        "data.task must not be null for a known id"
    );
    assert_eq!(
        task_obj.get("id").and_then(|v| v.as_str()),
        Some("1"),
        "task.id must be '1'"
    );
    assert_eq!(
        task_obj.get("subject").and_then(|v| v.as_str()),
        Some("Do something important"),
        "task.subject must match"
    );
}

// ─── Test 2 ──────────────────────────────────────────────────────────────────

#[tokio::test]
async fn returns_null_for_missing_id() {
    let root = TempDir::new().unwrap();
    // Don't insert any task — the store is empty

    let tool = TaskGetRuntimeTool;
    let result = tool
        .execute(json!({"taskId": "999"}), ctx(&root))
        .await
        .expect("TaskGet should succeed (not Err) for missing id");

    // Should return null task
    let data = result.data.expect("data field must be present");
    let task_val = data.get("task").expect("data.task must be present");
    assert!(task_val.is_null(), "data.task must be null for missing id");
    assert!(
        data.get("error").is_some(),
        "data.error should be present for missing id"
    );
}

// ─── Test 3 ──────────────────────────────────────────────────────────────────

#[tokio::test]
async fn rejects_missing_taskId_field() {
    let root = TempDir::new().unwrap();
    let tool = TaskGetRuntimeTool;

    // Pass empty json — no taskId key
    let result = tool
        .execute(json!({}), ctx(&root))
        .await;

    assert!(result.is_err(), "should return Err when taskId is missing");
    match result.unwrap_err() {
        ToolError::InputValidationError { tool_name, message } => {
            assert_eq!(tool_name, "TaskGet");
            assert!(
                message.contains("taskId"),
                "message should mention taskId, got: {message}"
            );
        }
        other => panic!("expected InputValidationError, got: {:?}", other),
    }
}

// ─── Test 4 ──────────────────────────────────────────────────────────────────

#[test]
fn is_read_only_returns_true() {
    let tool = TaskGetRuntimeTool;
    assert!(
        tool.is_read_only(&json!({})),
        "TaskGetRuntimeTool must be read-only"
    );
}
