//! Integration tests for `TaskOutputRuntimeTool`.
//!
//! Verifies that the tool correctly reads async sub-agent transcripts
//! from JSONL files, respecting offset semantics.

use std::sync::Arc;

use serde_json::{json, Value};
use tempfile::TempDir;

use app_lib::runtime::agent::output_writer::{self, TranscriptLine};
use app_lib::runtime::tools::builtin::task_output::TaskOutputRuntimeTool;
use app_lib::runtime::tools::context::ToolExecutionContext;
use app_lib::runtime::tools::RuntimeTool;
use app_lib::storage::user_scoped_paths::{UserScopedPathResolver, UserScopedPaths};

// ─── TestResolver ─────────────────────────────────────────────────────────────

struct TestResolver {
    paths: UserScopedPaths,
}

impl UserScopedPathResolver for TestResolver {
    fn resolve_paths(&self) -> Option<UserScopedPaths> {
        Some(self.paths.clone())
    }
}

fn build_tool(tmp: &TempDir) -> TaskOutputRuntimeTool {
    let paths = UserScopedPaths::new(tmp.path(), "t_test__u_test");
    TaskOutputRuntimeTool::new(Arc::new(TestResolver { paths }))
}

// ─── Helper ───────────────────────────────────────────────────────────────────

/// Return the `subagent_transcripts_dir` for the test scope inside `tmp`.
fn transcripts_dir(tmp: &TempDir) -> std::path::PathBuf {
    UserScopedPaths::new(tmp.path(), "t_test__u_test").subagent_transcripts_dir()
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn returns_empty_for_nonexistent_task() {
    let tmp = TempDir::new().unwrap();
    let tool = build_tool(&tmp);
    let ctx = ToolExecutionContext::for_test("c", "r", "tc");

    let result = tool
        .execute(json!({"task_id": "never_existed"}), ctx)
        .await
        .expect("should not fail for missing file");

    let body: Value = serde_json::from_str(&result.content).unwrap();
    assert_eq!(
        body["lines"].as_array().unwrap().len(),
        0,
        "lines should be empty for nonexistent task"
    );
    assert_eq!(
        body["new_offset"].as_u64().unwrap(),
        0,
        "new_offset should be 0 for nonexistent task"
    );
}

#[tokio::test]
async fn reads_three_lines_with_offset_zero() {
    let tmp = TempDir::new().unwrap();
    let tool = build_tool(&tmp);

    let path = transcripts_dir(&tmp).join("agent-x.jsonl");
    for i in 0..3 {
        output_writer::append_line(&path, &TranscriptLine::assistant(format!("msg-{i}"))).unwrap();
    }

    let ctx = ToolExecutionContext::for_test("c", "r", "tc");
    let result = tool
        .execute(json!({"task_id": "agent-x", "offset": 0}), ctx)
        .await
        .expect("should succeed");

    let body: Value = serde_json::from_str(&result.content).unwrap();
    assert_eq!(
        body["lines"].as_array().unwrap().len(),
        3,
        "should return all 3 lines when offset=0"
    );
    assert_eq!(
        body["new_offset"].as_u64().unwrap(),
        3,
        "new_offset should equal total line count"
    );
}

#[tokio::test]
async fn reads_tail_with_offset() {
    let tmp = TempDir::new().unwrap();
    let tool = build_tool(&tmp);

    let path = transcripts_dir(&tmp).join("agent-x.jsonl");
    for i in 0..3 {
        output_writer::append_line(&path, &TranscriptLine::assistant(format!("msg-{i}"))).unwrap();
    }

    let ctx = ToolExecutionContext::for_test("c", "r", "tc");
    let result = tool
        .execute(json!({"task_id": "agent-x", "offset": 2}), ctx)
        .await
        .expect("should succeed");

    let body: Value = serde_json::from_str(&result.content).unwrap();
    let lines = body["lines"].as_array().unwrap();
    assert_eq!(lines.len(), 1, "only 1 line past offset=2");
    assert_eq!(
        body["new_offset"].as_u64().unwrap(),
        3,
        "new_offset should still be 3"
    );
    assert!(
        lines[0].as_str().unwrap().contains("msg-2"),
        "last line should contain msg-2, got: {}",
        lines[0]
    );
}

#[tokio::test]
async fn incremental_after_append() {
    let tmp = TempDir::new().unwrap();
    let tool = build_tool(&tmp);

    let path = transcripts_dir(&tmp).join("agent-x.jsonl");
    for i in 0..3 {
        output_writer::append_line(&path, &TranscriptLine::assistant(format!("msg-{i}"))).unwrap();
    }

    // First read: offset=0 → 3 lines
    let ctx1 = ToolExecutionContext::for_test("c", "r", "tc1");
    let r1 = tool
        .execute(json!({"task_id": "agent-x", "offset": 0}), ctx1)
        .await
        .expect("first read should succeed");
    let b1: Value = serde_json::from_str(&r1.content).unwrap();
    assert_eq!(b1["lines"].as_array().unwrap().len(), 3);
    assert_eq!(b1["new_offset"].as_u64().unwrap(), 3);

    // Append a 4th line
    output_writer::append_line(&path, &TranscriptLine::assistant("msg-3")).unwrap();

    // Second read: offset=3 → 1 new line
    let ctx2 = ToolExecutionContext::for_test("c", "r", "tc2");
    let r2 = tool
        .execute(json!({"task_id": "agent-x", "offset": 3}), ctx2)
        .await
        .expect("second read should succeed");
    let b2: Value = serde_json::from_str(&r2.content).unwrap();
    let lines2 = b2["lines"].as_array().unwrap();
    assert_eq!(lines2.len(), 1, "only 1 new line after offset=3");
    assert_eq!(
        b2["new_offset"].as_u64().unwrap(),
        4,
        "new_offset should be 4"
    );
    assert!(
        lines2[0].as_str().unwrap().contains("msg-3"),
        "new line should contain msg-3, got: {}",
        lines2[0]
    );
}

#[tokio::test]
async fn rejects_path_traversal_dotdot() {
    let tmp = TempDir::new().unwrap();
    let tool = build_tool(&tmp);
    let ctx = ToolExecutionContext::for_test("c", "r", "tc");
    let res = tool.execute(json!({"task_id": "../foo"}), ctx).await;
    assert!(res.is_err(), "../foo should be rejected: {res:?}");
    let msg = format!("{:?}", res.unwrap_err());
    assert!(
        msg.contains("invalid task_id"),
        "error msg should mention invalid task_id: {msg}"
    );
}

#[tokio::test]
async fn rejects_absolute_path_separator() {
    let tmp = TempDir::new().unwrap();
    let tool = build_tool(&tmp);
    let ctx = ToolExecutionContext::for_test("c", "r", "tc");
    let res = tool.execute(json!({"task_id": "/etc/passwd"}), ctx).await;
    assert!(res.is_err(), "/etc/passwd should be rejected: {res:?}");
    let msg = format!("{:?}", res.unwrap_err());
    assert!(
        msg.contains("invalid task_id"),
        "error msg should mention invalid task_id: {msg}"
    );
}

#[tokio::test]
async fn rejects_backslash_separator() {
    let tmp = TempDir::new().unwrap();
    let tool = build_tool(&tmp);
    let ctx = ToolExecutionContext::for_test("c", "r", "tc");
    let res = tool.execute(json!({"task_id": "..\\foo"}), ctx).await;
    assert!(res.is_err(), "..\\foo should be rejected: {res:?}");
    let msg = format!("{:?}", res.unwrap_err());
    assert!(
        msg.contains("invalid task_id"),
        "error msg should mention invalid task_id: {msg}"
    );
}

#[tokio::test]
async fn rejects_dotfile_task_id() {
    let tmp = TempDir::new().unwrap();
    let tool = build_tool(&tmp);
    let ctx = ToolExecutionContext::for_test("c", "r", "tc");
    let res = tool.execute(json!({"task_id": ".hidden"}), ctx).await;
    assert!(res.is_err(), ".hidden should be rejected: {res:?}");
    let msg = format!("{:?}", res.unwrap_err());
    assert!(
        msg.contains("invalid task_id"),
        "error msg should mention invalid task_id: {msg}"
    );
}
