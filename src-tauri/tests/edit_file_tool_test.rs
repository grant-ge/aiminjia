//! Integration tests for EditFileRuntimeTool.

use app_lib::runtime::tools::builtin::workspace::EditFileRuntimeTool;
use app_lib::runtime::tools::capability::{CapabilityContext, FileStateCache};
use app_lib::runtime::tools::{RuntimeTool, ToolExecutionContext};
use serde_json::json;
use std::sync::Arc;
use tempfile::TempDir;

fn make_ctx(tmp: &TempDir) -> ToolExecutionContext {
    let cap = Arc::new(
        CapabilityContext::with_workspace(tmp.path().to_path_buf(), "test-ws")
            .with_read_file_state(Arc::new(FileStateCache::new())),
    );
    ToolExecutionContext::for_test("conv-1", "run-1", "tc-1").with_capability(cap)
}

#[tokio::test]
async fn edit_file_replaces_unique_string() {
    let tmp = TempDir::new().unwrap();
    std::fs::write(tmp.path().join("file.txt"), "hello world").unwrap();
    let ctx = make_ctx(&tmp);

    let tool = EditFileRuntimeTool;
    tool.execute(
        json!({ "file_path": "file.txt", "old_string": "world", "new_string": "rust" }),
        ctx,
    )
    .await
    .unwrap();

    let content = std::fs::read_to_string(tmp.path().join("file.txt")).unwrap();
    assert_eq!(content, "hello rust");
}

#[tokio::test]
async fn edit_file_fails_when_old_string_not_found() {
    let tmp = TempDir::new().unwrap();
    std::fs::write(tmp.path().join("file.txt"), "hello world").unwrap();
    let ctx = make_ctx(&tmp);

    let tool = EditFileRuntimeTool;
    let result = tool
        .execute(
            json!({ "file_path": "file.txt", "old_string": "NONEXISTENT", "new_string": "x" }),
            ctx,
        )
        .await;
    assert!(result.is_err());
}

#[tokio::test]
async fn edit_file_fails_when_old_string_not_unique() {
    let tmp = TempDir::new().unwrap();
    std::fs::write(tmp.path().join("dup.txt"), "foo foo foo").unwrap();
    let ctx = make_ctx(&tmp);

    let tool = EditFileRuntimeTool;
    let result = tool
        .execute(
            json!({ "file_path": "dup.txt", "old_string": "foo", "new_string": "bar" }),
            ctx,
        )
        .await;
    assert!(result.is_err());
    let msg = result.unwrap_err().to_string();
    assert!(
        msg.contains("3 times") || msg.contains("times"),
        "error should mention count: {msg}"
    );
}

#[tokio::test]
async fn edit_file_fails_when_file_does_not_exist() {
    let tmp = TempDir::new().unwrap();
    let ctx = make_ctx(&tmp);

    let tool = EditFileRuntimeTool;
    let result = tool
        .execute(
            json!({ "file_path": "missing.txt", "old_string": "anything", "new_string": "x" }),
            ctx,
        )
        .await;
    assert!(result.is_err());
}

#[tokio::test]
async fn edit_file_creates_new_file_when_old_string_empty_and_file_missing() {
    let tmp = TempDir::new().unwrap();
    let ctx = make_ctx(&tmp);

    let tool = EditFileRuntimeTool;
    tool.execute(
        json!({ "file_path": "new.txt", "old_string": "", "new_string": "brand new" }),
        ctx,
    )
    .await
    .unwrap();

    let content = std::fs::read_to_string(tmp.path().join("new.txt")).unwrap();
    assert_eq!(content, "brand new");
}

#[tokio::test]
async fn edit_file_updates_cache_after_edit() {
    let tmp = TempDir::new().unwrap();
    std::fs::write(tmp.path().join("cache.txt"), "original text").unwrap();
    let cache = Arc::new(FileStateCache::new());
    let cap = Arc::new(
        CapabilityContext::with_workspace(tmp.path().to_path_buf(), "test-ws")
            .with_read_file_state(cache.clone()),
    );
    let ctx = ToolExecutionContext::for_test("conv-1", "run-1", "tc-1").with_capability(cap);

    let tool = EditFileRuntimeTool;
    tool.execute(
        json!({ "file_path": "cache.txt", "old_string": "original", "new_string": "updated" }),
        ctx,
    )
    .await
    .unwrap();

    let resolved = std::fs::canonicalize(tmp.path()).unwrap().join("cache.txt");
    let state = cache.get(&resolved).expect("cache should be populated");
    assert_eq!(state.content, "updated text");
}

#[tokio::test]
async fn edit_file_rejects_path_traversal() {
    let tmp = TempDir::new().unwrap();
    let ctx = make_ctx(&tmp);

    let tool = EditFileRuntimeTool;
    let result = tool
        .execute(
            json!({ "file_path": "../etc/passwd", "old_string": "root", "new_string": "evil" }),
            ctx,
        )
        .await;
    assert!(result.is_err());
}

#[tokio::test]
async fn edit_file_replaces_multiline_string() {
    let tmp = TempDir::new().unwrap();
    let original = "line one\nline two\nline three\n";
    std::fs::write(tmp.path().join("multi.txt"), original).unwrap();
    let ctx = make_ctx(&tmp);

    let tool = EditFileRuntimeTool;
    tool.execute(
        json!({ "file_path": "multi.txt", "old_string": "line two\n", "new_string": "LINE TWO\n" }),
        ctx,
    )
    .await
    .unwrap();

    let result = std::fs::read_to_string(tmp.path().join("multi.txt")).unwrap();
    assert_eq!(result, "line one\nLINE TWO\nline three\n");
}
