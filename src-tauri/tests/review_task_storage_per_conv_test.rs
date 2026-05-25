//! Architecture constraint: Task V2 storage is scoped to the conversation directory.
//!
//! Asserts that tasks created through the runtime tools land under
//! `<home>/conversations/<conv_id>/tasks/` rather than the old global
//! `<home>/tasks/` root.  No migration code: this is a greenfield path.
//!
//! Refs: 2026-05-10 plan §8.4

use serde_json::json;
use tempfile::TempDir;

use app_lib::runtime::cancellation::CancellationToken;
use app_lib::runtime::ids::{RunId, SessionId};
use app_lib::runtime::tools::builtin::task_tools::TaskCreateRuntimeTool;
use app_lib::runtime::tools::context::ToolExecutionContext;
use app_lib::runtime::tools::RuntimeTool;

fn ctx_with_root(root: &TempDir, conv_id: &str) -> ToolExecutionContext {
    let mut c = ToolExecutionContext::new(
        SessionId::new(conv_id),
        RunId::new("run-per-conv-test"),
        None,
        "tc-per-conv-test",
        CancellationToken::new(),
    );
    c.task_store_root = Some(root.path().to_path_buf());
    c
}

/// TaskCreate must write the task file under
/// `<root>/conversations/<conv_id>/tasks/<task_id>.json` (flat, no inner conv_id).
/// The old global root `<root>/tasks/` must NOT be created.
#[tokio::test]
async fn task_create_writes_to_conversation_scoped_dir() {
    let tmp = TempDir::new().unwrap();
    let conv_id = "conv-1";

    let tool = TaskCreateRuntimeTool;
    let result = tool
        .execute(
            json!({
                "subject": "Per-conversation task",
                "description": "Stored under conversations/<conv_id>/tasks/"
            }),
            ctx_with_root(&tmp, conv_id),
        )
        .await
        .expect("TaskCreate should succeed");

    assert!(
        result.content.contains("Task #1 created"),
        "expected creation confirmation, got: {}",
        result.content
    );

    // The task file must be flat under the per-conversation tasks dir.
    let conv_tasks_dir = tmp.path().join("conversations").join(conv_id).join("tasks");
    assert!(
        conv_tasks_dir.exists(),
        "per-conversation tasks dir must exist: {}",
        conv_tasks_dir.display()
    );
    let task_file = conv_tasks_dir.join("1.json");
    assert!(
        task_file.exists(),
        "task file must exist at: {}",
        task_file.display()
    );

    // No spurious second-level conv_id directory.
    let nested = conv_tasks_dir.join(conv_id);
    assert!(
        !nested.exists(),
        "tasks must be flat under tasks/; spurious nested dir found: {}",
        nested.display()
    );

    // The old global tasks/ root must NOT have been created.
    let global_tasks_dir = tmp.path().join("tasks");
    assert!(
        !global_tasks_dir.exists(),
        "global tasks/ root must NOT exist after P1.5 migration; found: {}",
        global_tasks_dir.display()
    );
}

/// Tasks from different conversations must be isolated in separate directories.
#[tokio::test]
async fn tasks_from_different_conversations_are_isolated() {
    let tmp = TempDir::new().unwrap();
    let tool = TaskCreateRuntimeTool;

    tool.execute(
        json!({ "subject": "Convo A task", "description": "belongs to conv-a" }),
        ctx_with_root(&tmp, "conv-a"),
    )
    .await
    .expect("create conv-a task");

    tool.execute(
        json!({ "subject": "Convo B task", "description": "belongs to conv-b" }),
        ctx_with_root(&tmp, "conv-b"),
    )
    .await
    .expect("create conv-b task");

    let conv_a_dir = tmp
        .path()
        .join("conversations")
        .join("conv-a")
        .join("tasks");
    let conv_b_dir = tmp
        .path()
        .join("conversations")
        .join("conv-b")
        .join("tasks");

    assert!(conv_a_dir.exists(), "conv-a tasks dir must exist");
    assert!(conv_b_dir.exists(), "conv-b tasks dir must exist");

    // Files in conv-a must not appear in conv-b and vice-versa.
    let count_a = std::fs::read_dir(&conv_a_dir)
        .map(|d| d.count())
        .unwrap_or(0);
    let count_b = std::fs::read_dir(&conv_b_dir)
        .map(|d| d.count())
        .unwrap_or(0);
    assert_eq!(
        count_a, 2,
        "conv-a should have 1 task file + 1 highwatermark"
    );
    assert_eq!(
        count_b, 2,
        "conv-b should have 1 task file + 1 highwatermark"
    );
}
