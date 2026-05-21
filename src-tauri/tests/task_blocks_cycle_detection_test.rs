//! Tests for cycle detection in addBlocks / addBlockedBy.
//!
//! Covers 3 cases:
//! 1. Linear A→B→C: no cycles, all updates succeed.
//! 2. Cycle A→B→C→A: final edge rejected with CyclicBlockingDependency message.
//! 3. Self-block: A.addBlocks=[A] rejected immediately.

use serde_json::json;
use tempfile::TempDir;

use app_lib::runtime::cancellation::CancellationToken;
use app_lib::runtime::ids::{RunId, SessionId};
use app_lib::runtime::tools::builtin::task_tools::{
    TaskCreateRuntimeTool, TaskUpdateRuntimeTool,
};
use app_lib::runtime::tools::context::ToolExecutionContext;
use app_lib::runtime::tools::executor::ToolError;
use app_lib::runtime::tools::RuntimeTool;

const CONV_ID: &str = "sess-cycle-test";

fn ctx(root: &TempDir) -> ToolExecutionContext {
    let mut c = ToolExecutionContext::new(
        SessionId::new(CONV_ID),
        RunId::new("run-cycle-test"),
        None,
        "tc-cycle-test",
        CancellationToken::new(),
    );
    c.task_store_root = Some(root.path().to_path_buf());
    c
}

/// Create three tasks (A=1, B=2, C=3) via TaskCreate, return their ids.
async fn create_abc(root: &TempDir) -> (String, String, String) {
    let tool = TaskCreateRuntimeTool;
    let r1 = tool
        .execute(
            json!({ "subject": "Task A", "description": "node A" }),
            ctx(root),
        )
        .await
        .unwrap();
    let r2 = tool
        .execute(
            json!({ "subject": "Task B", "description": "node B" }),
            ctx(root),
        )
        .await
        .unwrap();
    let r3 = tool
        .execute(
            json!({ "subject": "Task C", "description": "node C" }),
            ctx(root),
        )
        .await
        .unwrap();

    // IDs are sequential: 1, 2, 3
    let id_a = r1.data.as_ref().unwrap()["taskId"]
        .as_str()
        .unwrap()
        .to_string();
    let id_b = r2.data.as_ref().unwrap()["taskId"]
        .as_str()
        .unwrap()
        .to_string();
    let id_c = r3.data.as_ref().unwrap()["taskId"]
        .as_str()
        .unwrap()
        .to_string();
    (id_a, id_b, id_c)
}

// ─── Case 1: Linear A→B→C, no cycle ─────────────────────────────────────────

#[tokio::test]
async fn linear_blocks_chain_succeeds() {
    let root = TempDir::new().unwrap();
    let (a, b, c) = create_abc(&root).await;
    let update = TaskUpdateRuntimeTool;

    // A blocks B
    update
        .execute(
            json!({ "taskId": &a, "addBlocks": [&b] }),
            ctx(&root),
        )
        .await
        .expect("A blocks B should succeed");

    // B blocks C
    update
        .execute(
            json!({ "taskId": &b, "addBlocks": [&c] }),
            ctx(&root),
        )
        .await
        .expect("B blocks C should succeed");

    // C has no further blocks — no cycle
    update
        .execute(
            json!({ "taskId": &c, "addBlocks": [] }),
            ctx(&root),
        )
        .await
        .expect("no-op update on C should succeed");
}

// ─── Case 2: A→B, B→C, then C→A — must be rejected ─────────────────────────

#[tokio::test]
async fn cycle_abc_is_rejected() {
    let root = TempDir::new().unwrap();
    let (a, b, c) = create_abc(&root).await;
    let update = TaskUpdateRuntimeTool;

    // A blocks B — ok
    update
        .execute(json!({ "taskId": &a, "addBlocks": [&b] }), ctx(&root))
        .await
        .expect("A blocks B");

    // B blocks C — ok
    update
        .execute(json!({ "taskId": &b, "addBlocks": [&c] }), ctx(&root))
        .await
        .expect("B blocks C");

    // C blocks A — must fail with cycle detection
    let err = update
        .execute(json!({ "taskId": &c, "addBlocks": [&a] }), ctx(&root))
        .await
        .expect_err("C blocks A should be rejected as cyclic");

    match err {
        ToolError::ExecutionFailed(msg) => {
            assert!(
                msg.contains("cyclic blocking dependency"),
                "error must say 'cyclic blocking dependency', got: {msg}"
            );
            // The error path must mention all three task ids
            assert!(
                msg.contains(&a),
                "error must include task A id ({a}), got: {msg}"
            );
            assert!(
                msg.contains(&b),
                "error must include task B id ({b}), got: {msg}"
            );
            assert!(
                msg.contains(&c),
                "error must include task C id ({c}), got: {msg}"
            );
        }
        other => panic!("expected ExecutionFailed, got: {:?}", other),
    }
}

// ─── Case 3: Self-block A→A ──────────────────────────────────────────────────

#[tokio::test]
async fn self_block_is_rejected() {
    let root = TempDir::new().unwrap();
    let (a, _b, _c) = create_abc(&root).await;
    let update = TaskUpdateRuntimeTool;

    let err = update
        .execute(json!({ "taskId": &a, "addBlocks": [&a] }), ctx(&root))
        .await
        .expect_err("self-block A→A should be rejected");

    match err {
        ToolError::ExecutionFailed(msg) => {
            assert!(
                msg.contains("cyclic blocking dependency"),
                "error must say 'cyclic blocking dependency', got: {msg}"
            );
            assert!(
                msg.contains(&a),
                "error must include the task id ({a}), got: {msg}"
            );
        }
        other => panic!("expected ExecutionFailed, got: {:?}", other),
    }
}

// ─── Case 4: addBlockedBy also detects cycles ────────────────────────────────

#[tokio::test]
async fn cycle_via_add_blocked_by_is_rejected() {
    let root = TempDir::new().unwrap();
    let (a, b, _c) = create_abc(&root).await;
    let update = TaskUpdateRuntimeTool;

    // A blocks B (A→B)
    update
        .execute(json!({ "taskId": &a, "addBlocks": [&b] }), ctx(&root))
        .await
        .expect("A blocks B");

    // Now try: B.addBlockedBy = [A] is redundant (already implied by A→B) — no new edge,
    // should succeed idempotently.
    // Then try: A.addBlockedBy = [B] → edge B→A; combined with A→B gives cycle.
    let err = update
        .execute(json!({ "taskId": &a, "addBlockedBy": [&b] }), ctx(&root))
        .await
        .expect_err("A.addBlockedBy=[B] should be rejected: B→A combined with A→B is a cycle");

    match err {
        ToolError::ExecutionFailed(msg) => {
            assert!(
                msg.contains("cyclic blocking dependency"),
                "error must say 'cyclic blocking dependency', got: {msg}"
            );
        }
        other => panic!("expected ExecutionFailed, got: {:?}", other),
    }
}
