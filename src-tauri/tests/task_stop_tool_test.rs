//! Unit tests for TaskStopRuntimeTool.
//!
//! Covers the 6 behaviours specified in the test scope:
//! 1. cancels_running_task_and_marks_killed
//! 2. returns_error_for_unknown_task_id
//! 3. rejects_already_terminal_task
//! 4. rejects_missing_task_id_field
//! 5. rejects_empty_task_id_string
//! 6. response_data_shape

use std::path::PathBuf;
use std::sync::Arc;

use serde_json::json;

use app_lib::runtime::agent::async_task_store::{
    AsyncAgentTaskStore, AsyncTaskHandle, AsyncTaskState,
};
use app_lib::runtime::cancellation::{CancellationReason, CancellationToken};
use app_lib::runtime::ids::AgentId;
use app_lib::runtime::tools::builtin::task_stop::TaskStopRuntimeTool;
use app_lib::runtime::tools::context::ToolExecutionContext;
use app_lib::runtime::tools::executor::ToolError;
use app_lib::runtime::tools::RuntimeTool;

// ─── helpers ──────────────────────────────────────────────────────────────────

fn make_handle(agent_id: &str, state: AsyncTaskState) -> AsyncTaskHandle {
    AsyncTaskHandle {
        agent_id: AgentId::new(agent_id),
        state,
        output_file: PathBuf::from(format!("/tmp/{agent_id}.out")),
        description: format!("task {agent_id}"),
        cancel_token: CancellationToken::new(),
    }
}

fn make_handle_with_token(
    agent_id: &str,
    state: AsyncTaskState,
    token: CancellationToken,
) -> AsyncTaskHandle {
    AsyncTaskHandle {
        agent_id: AgentId::new(agent_id),
        state,
        output_file: PathBuf::from(format!("/tmp/{agent_id}.out")),
        description: format!("task {agent_id}"),
        cancel_token: token,
    }
}

fn ctx() -> ToolExecutionContext {
    ToolExecutionContext::for_test("test-sess", "test-run", "tc-1")
}

fn tool(store: Arc<AsyncAgentTaskStore>) -> TaskStopRuntimeTool {
    TaskStopRuntimeTool { store }
}

// ─── Test 1 ──────────────────────────────────────────────────────────────────

#[tokio::test]
async fn cancels_running_task_and_marks_killed() {
    let store = Arc::new(AsyncAgentTaskStore::new());
    let token = CancellationToken::new();
    store.register_anonymous(make_handle_with_token(
        "task-running-001",
        AsyncTaskState::Running,
        token.clone(),
    ));

    let result = tool(store.clone())
        .execute(json!({"task_id": "task-running-001"}), ctx())
        .await;

    assert!(result.is_ok(), "TaskStop should succeed: {:?}", result);

    // Cancel token must have been triggered with BackgroundStop reason
    assert!(token.is_cancelled(), "cancel token must be cancelled after TaskStop");
    assert_eq!(
        token.reason(),
        Some(CancellationReason::BackgroundStop),
        "reason must be BackgroundStop"
    );

    // State must have been updated to Killed
    let handle = store.find_by_id(&AgentId::new("task-running-001")).unwrap();
    assert_eq!(handle.state, AsyncTaskState::Killed, "state must be Killed");
}

// ─── Test 2 ──────────────────────────────────────────────────────────────────

#[tokio::test]
async fn returns_error_for_unknown_task_id() {
    let store = Arc::new(AsyncAgentTaskStore::new());
    let result = tool(store)
        .execute(json!({"task_id": "ghost-task-not-registered"}), ctx())
        .await;

    assert!(result.is_err(), "should return Err for unknown task id");
    match result.unwrap_err() {
        ToolError::ExecutionFailed(msg) => {
            assert!(
                msg.contains("ghost-task-not-registered"),
                "error message should mention the unknown id, got: {msg}"
            );
        }
        other => panic!("expected ExecutionFailed, got: {:?}", other),
    }
}

// ─── Test 3 ──────────────────────────────────────────────────────────────────

#[tokio::test]
async fn rejects_already_terminal_task() {
    let store = Arc::new(AsyncAgentTaskStore::new());
    // Register a task that is already Completed (terminal state)
    store.register_anonymous(make_handle("task-completed-001", AsyncTaskState::Completed));

    let result = tool(store)
        .execute(json!({"task_id": "task-completed-001"}), ctx())
        .await;

    assert!(result.is_err(), "should return Err for terminal task");
    match result.unwrap_err() {
        ToolError::ExecutionFailed(msg) => {
            assert!(
                msg.contains("task-completed-001"),
                "error should mention the task id, got: {msg}"
            );
        }
        other => panic!("expected ExecutionFailed, got: {:?}", other),
    }
}

// ─── Test 4 ──────────────────────────────────────────────────────────────────

#[tokio::test]
async fn rejects_missing_task_id_field() {
    let store = Arc::new(AsyncAgentTaskStore::new());
    // Input has no task_id key at all
    let result = tool(store)
        .execute(json!({}), ctx())
        .await;

    assert!(result.is_err(), "should return Err when task_id field is missing");
    match result.unwrap_err() {
        ToolError::InputValidationError { tool_name, message } => {
            assert_eq!(tool_name, "TaskStop");
            assert!(
                message.contains("task_id"),
                "message should mention task_id, got: {message}"
            );
        }
        other => panic!("expected InputValidationError, got: {:?}", other),
    }
}

// ─── Test 5 ──────────────────────────────────────────────────────────────────

#[tokio::test]
async fn rejects_empty_task_id_string() {
    let store = Arc::new(AsyncAgentTaskStore::new());
    // Input has task_id = "" (empty string, should be rejected)
    let result = tool(store)
        .execute(json!({"task_id": ""}), ctx())
        .await;

    assert!(result.is_err(), "should return Err for empty task_id string");
    match result.unwrap_err() {
        ToolError::InputValidationError { tool_name, message } => {
            assert_eq!(tool_name, "TaskStop");
            assert!(
                message.contains("task_id"),
                "message should mention task_id, got: {message}"
            );
        }
        other => panic!("expected InputValidationError, got: {:?}", other),
    }
}

// ─── Test 6 ──────────────────────────────────────────────────────────────────

#[tokio::test]
async fn response_data_shape() {
    let store = Arc::new(AsyncAgentTaskStore::new());
    store.register_anonymous(make_handle("task-shape-001", AsyncTaskState::Running));

    let result = tool(store)
        .execute(json!({"task_id": "task-shape-001"}), ctx())
        .await
        .expect("TaskStop should succeed");

    let data = result.data.expect("response must have a data field");

    // Verify all expected fields are present with correct values
    assert_eq!(
        data.get("task_id").and_then(|v| v.as_str()),
        Some("task-shape-001"),
        "data.task_id must match input"
    );
    assert_eq!(
        data.get("task_type").and_then(|v| v.as_str()),
        Some("local_agent"),
        "data.task_type must be 'local_agent'"
    );
    assert!(
        data.get("message").and_then(|v| v.as_str()).is_some(),
        "data.message must be a string"
    );
    assert!(
        data.get("command").and_then(|v| v.as_str()).is_some(),
        "data.command must be a string"
    );
}
