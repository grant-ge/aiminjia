//! Integration tests for WriteFileRuntimeTool.

use app_lib::runtime::tools::builtin::workspace::WriteFileRuntimeTool;
use app_lib::runtime::tools::capability::{CapabilityContext, FileStateCache};
use app_lib::runtime::tools::{RuntimeTool, ToolExecutionContext};
use serde_json::json;
use std::sync::Arc;
use tempfile::TempDir;

fn make_ctx_with_workspace(tmp: &TempDir) -> ToolExecutionContext {
    let cap = Arc::new(
        CapabilityContext::with_workspace(tmp.path().to_path_buf(), "test-ws")
            .with_read_file_state(Arc::new(FileStateCache::new())),
    );
    ToolExecutionContext::for_test("conv-1", "run-1", "tc-1").with_capability(cap)
}

#[tokio::test]
async fn write_file_creates_new_file() {
    let tmp = TempDir::new().unwrap();
    let ctx = make_ctx_with_workspace(&tmp);

    let tool = WriteFileRuntimeTool;
    let result = tool
        .execute(
            json!({ "file_path": "hello.txt", "content": "hello world" }),
            ctx,
        )
        .await
        .unwrap();

    assert!(result.content.contains("hello.txt"));
    let written = std::fs::read_to_string(tmp.path().join("hello.txt")).unwrap();
    assert_eq!(written, "hello world");
}

#[tokio::test]
async fn write_file_overwrites_existing_file() {
    let tmp = TempDir::new().unwrap();
    std::fs::write(tmp.path().join("existing.txt"), b"old content").unwrap();
    let ctx = make_ctx_with_workspace(&tmp);

    let tool = WriteFileRuntimeTool;
    tool.execute(
        json!({ "file_path": "existing.txt", "content": "new content" }),
        ctx,
    )
    .await
    .unwrap();

    let written = std::fs::read_to_string(tmp.path().join("existing.txt")).unwrap();
    assert_eq!(written, "new content");
}

#[tokio::test]
async fn write_file_creates_parent_dirs() {
    let tmp = TempDir::new().unwrap();
    let ctx = make_ctx_with_workspace(&tmp);

    let tool = WriteFileRuntimeTool;
    tool.execute(
        json!({ "file_path": "subdir/nested/file.txt", "content": "nested" }),
        ctx,
    )
    .await
    .unwrap();

    assert!(tmp.path().join("subdir/nested/file.txt").exists());
}

#[tokio::test]
async fn write_file_rejects_path_traversal() {
    let tmp = TempDir::new().unwrap();
    let ctx = make_ctx_with_workspace(&tmp);

    let tool = WriteFileRuntimeTool;
    let result = tool
        .execute(
            json!({ "file_path": "../escape.txt", "content": "evil" }),
            ctx,
        )
        .await;

    assert!(result.is_err(), "Path traversal should be rejected");
}

#[tokio::test]
async fn write_file_updates_file_state_cache() {
    let tmp = TempDir::new().unwrap();
    let cache = Arc::new(FileStateCache::new());
    let cap = Arc::new(
        CapabilityContext::with_workspace(tmp.path().to_path_buf(), "test-ws")
            .with_read_file_state(cache.clone()),
    );
    let ctx = ToolExecutionContext::for_test("conv-1", "run-1", "tc-1").with_capability(cap);

    let tool = WriteFileRuntimeTool;
    tool.execute(
        json!({ "file_path": "cached.txt", "content": "cached content" }),
        ctx,
    )
    .await
    .unwrap();

    let resolved = std::fs::canonicalize(tmp.path())
        .unwrap()
        .join("cached.txt");
    let state = cache.get(&resolved);
    assert!(
        state.is_some(),
        "FileStateCache should be updated after write"
    );
    assert_eq!(state.unwrap().content, "cached content");
}

#[tokio::test]
async fn write_file_missing_path_returns_error() {
    let tmp = TempDir::new().unwrap();
    let ctx = make_ctx_with_workspace(&tmp);

    let tool = WriteFileRuntimeTool;
    let result = tool
        .execute(json!({ "content": "no path given" }), ctx)
        .await;
    assert!(result.is_err());
}
