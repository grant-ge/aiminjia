//! Tests for TaskClaimRuntimeTool.
//!
//! Covers 3 cases:
//! 1. owner None → claim succeeds; task.owner == agent name
//! 2. owner already set to another agent → ExecutionFailed with "already claimed"
//! 3. owner "*" → claim succeeds (open-claim slot)

use serde_json::json;
use tempfile::TempDir;

use app_lib::runtime::cancellation::CancellationToken;
use app_lib::runtime::ids::{AgentId, RunId, SessionId};
use app_lib::runtime::task::task_models::{TaskRecord, TaskStatus};
use app_lib::runtime::task::FileTaskV2Store;
use app_lib::runtime::tools::builtin::task_tools::{
    TaskClaimRuntimeTool, TaskCreateRuntimeTool,
};
use app_lib::runtime::tools::context::ToolExecutionContext;
use app_lib::runtime::tools::executor::ToolError;
use app_lib::runtime::tools::RuntimeTool;

const CONV_ID: &str = "sess-claim-test";

fn ctx(root: &TempDir) -> ToolExecutionContext {
    ctx_with_agent(root, None)
}

fn ctx_with_agent(root: &TempDir, agent_id: Option<&str>) -> ToolExecutionContext {
    let mut c = ToolExecutionContext::new(
        SessionId::new(CONV_ID),
        RunId::new("run-claim-test"),
        agent_id.map(|id| AgentId::new(id.to_string())),
        "tc-claim-test",
        CancellationToken::new(),
    );
    c.task_store_root = Some(root.path().to_path_buf());
    c
}

/// Seed a task directly into the per-conversation tasks dir (same path as store_for()).
fn seed_task(root: &TempDir, task_id: &str, owner: Option<&str>) {
    let tasks_root = root
        .path()
        .join("conversations")
        .join(CONV_ID)
        .join("tasks");
    let store = FileTaskV2Store::new(tasks_root);
    store
        .create(
            "",
            &TaskRecord {
                id: task_id.to_string(),
                subject: format!("Task {}", task_id),
                description: "seeded for claim test".to_string(),
                active_form: None,
                owner: owner.map(str::to_string),
                status: TaskStatus::Pending,
                blocks: vec![],
                blocked_by: vec![],
                metadata: None,
                session_id: session_id.clone(),
                parent_run_id: RunId::new("run-seed"),
                owner_agent_id: None,
            },
        )
        .unwrap();
}

// ─── Case 1: owner None → claim succeeds ────────────────────────────────────

#[tokio::test]
async fn claim_unclaimed_task_succeeds() {
    let root = TempDir::new().unwrap();
    seed_task(&root, "1", None);

    let tool = TaskClaimRuntimeTool;
    let result = tool
        .execute(
            json!({ "taskId": "1" }),
            ctx_with_agent(&root, Some("agent-alfa")),
        )
        .await
        .expect("claim of unclaimed task should succeed");

    assert!(
        result.content.contains("claimed by agent-alfa"),
        "content should mention claimant, got: {}",
        result.content
    );
    let data = result.data.expect("data must be present");
    let owner = data["task"]["owner"]
        .as_str()
        .expect("task.owner must be a string");
    assert_eq!(owner, "agent-alfa", "task.owner must be set to claimant");
}

// ─── Case 2: already claimed by another → ExecutionFailed ──────────────────

#[tokio::test]
async fn claim_already_owned_task_fails() {
    let root = TempDir::new().unwrap();
    seed_task(&root, "2", Some("agent-bravo"));

    let tool = TaskClaimRuntimeTool;
    let err = tool
        .execute(
            json!({ "taskId": "2" }),
            ctx_with_agent(&root, Some("agent-charlie")),
        )
        .await
        .expect_err("claim of already-owned task should fail");

    match err {
        ToolError::ExecutionFailed(msg) => {
            assert!(
                msg.contains("already claimed"),
                "error must say 'already claimed', got: {msg}"
            );
            assert!(
                msg.contains("agent-bravo"),
                "error must name the existing owner, got: {msg}"
            );
        }
        other => panic!("expected ExecutionFailed, got: {:?}", other),
    }
}

// ─── Case 3: owner "*" → open-claim slot, succeeds ──────────────────────────

#[tokio::test]
async fn claim_open_slot_task_succeeds() {
    let root = TempDir::new().unwrap();
    seed_task(&root, "3", Some("*"));

    let tool = TaskClaimRuntimeTool;
    let result = tool
        .execute(
            json!({ "taskId": "3" }),
            ctx_with_agent(&root, Some("agent-delta")),
        )
        .await
        .expect("claim of open-slot task should succeed");

    let data = result.data.expect("data must be present");
    let owner = data["task"]["owner"]
        .as_str()
        .expect("task.owner must be a string after claiming open slot");
    assert_eq!(
        owner, "agent-delta",
        "open-slot task owner must be set to claimant"
    );
}

// ─── Case 4: idempotent — claimant already owns the task ────────────────────

#[tokio::test]
async fn claim_own_task_is_idempotent() {
    let root = TempDir::new().unwrap();
    seed_task(&root, "4", Some("agent-echo"));

    let tool = TaskClaimRuntimeTool;
    let result = tool
        .execute(
            json!({ "taskId": "4" }),
            ctx_with_agent(&root, Some("agent-echo")),
        )
        .await
        .expect("re-claiming own task should succeed idempotently");

    // Data must confirm ownership
    let data = result.data.expect("data must be present");
    assert_eq!(data["success"], json!(true));
    assert_eq!(data["task"]["owner"], json!("agent-echo"));
}
