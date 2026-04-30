//! Integration tests for GrepContentTool.

use app_lib::runtime::tools::builtin::grep::GrepContentTool;
use app_lib::runtime::tools::capability::CapabilityContext;
use app_lib::runtime::tools::{RuntimeTool, ToolExecutionContext};
use serde_json::json;
use std::sync::Arc;
use tempfile::TempDir;

fn make_ctx(tmp: &TempDir) -> ToolExecutionContext {
    let cap = Arc::new(CapabilityContext::with_workspace(
        tmp.path().to_path_buf(),
        "test-ws",
    ));
    ToolExecutionContext::for_test("conv-1", "run-1", "tc-1").with_capability(cap)
}

fn setup_test_files(tmp: &TempDir) {
    std::fs::write(
        tmp.path().join("a.rs"),
        "fn main() {\n    println!(\"hello\");\n}\n",
    )
    .unwrap();
    std::fs::write(tmp.path().join("b.rs"), "fn foo() {}\nfn bar() {}\n").unwrap();
    std::fs::write(tmp.path().join("c.txt"), "hello world\nno match here\n").unwrap();

    std::fs::create_dir_all(tmp.path().join("subdir")).unwrap();
    std::fs::write(
        tmp.path().join("subdir/d.rs"),
        "// hello from subdir\nfn baz() {}\n",
    )
    .unwrap();

    std::fs::create_dir_all(tmp.path().join(".git")).unwrap();
    std::fs::write(
        tmp.path().join(".git/ignored.txt"),
        "hello from hidden dir\n",
    )
    .unwrap();

    let large = "x".repeat(2 * 1024 * 1024 + 64);
    std::fs::write(tmp.path().join("large.log"), large).unwrap();

    #[cfg(unix)]
    std::os::unix::fs::symlink(tmp.path().join("c.txt"), tmp.path().join("link.txt")).unwrap();
}

#[tokio::test]
async fn grep_files_with_matches_mode_returns_sorted_relative_paths() {
    let tmp = TempDir::new().unwrap();
    setup_test_files(&tmp);
    let ctx = make_ctx(&tmp);

    let tool = GrepContentTool;
    let result = tool
        .execute(
            json!({ "pattern": "hello", "output_mode": "files_with_matches" }),
            ctx,
        )
        .await
        .unwrap();

    let data = result.data.unwrap();
    assert_eq!(data["mode"], json!("files_with_matches"));
    assert_eq!(data["num_files"], json!(3));
    assert_eq!(data["filenames"], json!(["a.rs", "c.txt", "subdir/d.rs"]));
    assert!(
        result.content.contains("a.rs") && result.content.contains("subdir/d.rs"),
        "content should include matched file list: {}",
        result.content
    );
}

#[tokio::test]
async fn grep_content_mode_returns_line_text() {
    let tmp = TempDir::new().unwrap();
    setup_test_files(&tmp);
    let ctx = make_ctx(&tmp);

    let tool = GrepContentTool;
    let result = tool
        .execute(
            json!({ "pattern": "fn main", "output_mode": "content" }),
            ctx,
        )
        .await
        .unwrap();

    let data = result.data.unwrap();
    assert_eq!(data["mode"], json!("content"));
    assert_eq!(data["num_lines"], json!(1));
    let content = data["content"]
        .as_str()
        .expect("content output should be text");
    assert!(
        content.contains("a.rs:1:fn main()"),
        "content mode should include relative path, line number, and line text: {content}"
    );
}

#[tokio::test]
async fn grep_count_mode_returns_counts_text() {
    let tmp = TempDir::new().unwrap();
    setup_test_files(&tmp);
    let ctx = make_ctx(&tmp);

    let tool = GrepContentTool;
    let result = tool
        .execute(json!({ "pattern": "fn ", "output_mode": "count" }), ctx)
        .await
        .unwrap();

    let data = result.data.unwrap();
    assert_eq!(data["mode"], json!("count"));
    assert_eq!(data["num_matches"], json!(4));
    let content = data["content"]
        .as_str()
        .expect("count output should be text");
    assert!(
        content.contains("b.rs:2"),
        "count mode should include per-file counts: {content}"
    );
}

#[tokio::test]
async fn grep_glob_filter_rs_files_only() {
    let tmp = TempDir::new().unwrap();
    setup_test_files(&tmp);
    let ctx = make_ctx(&tmp);

    let tool = GrepContentTool;
    let result = tool
        .execute(
            json!({ "pattern": "hello", "glob": "*.rs", "output_mode": "files_with_matches" }),
            ctx,
        )
        .await
        .unwrap();

    let data = result.data.unwrap();
    assert_eq!(data["filenames"], json!(["a.rs", "subdir/d.rs"]));
}

#[tokio::test]
async fn grep_no_matches_returns_empty_success() {
    let tmp = TempDir::new().unwrap();
    setup_test_files(&tmp);
    let ctx = make_ctx(&tmp);

    let tool = GrepContentTool;
    let result = tool
        .execute(
            json!({ "pattern": "ZZZZZNOMATCH", "output_mode": "files_with_matches" }),
            ctx,
        )
        .await
        .unwrap();

    let data = result.data.unwrap();
    assert_eq!(data["num_files"], json!(0));
    assert_eq!(data["filenames"], json!([]));
}

#[tokio::test]
async fn grep_invalid_regex_returns_error() {
    let tmp = TempDir::new().unwrap();
    let ctx = make_ctx(&tmp);

    let tool = GrepContentTool;
    let result = tool.execute(json!({ "pattern": "[invalid" }), ctx).await;

    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(
        err.contains("Invalid regex") || err.contains("regex"),
        "should mention regex: {err}"
    );
}

#[tokio::test]
async fn grep_rejects_path_traversal() {
    let tmp = TempDir::new().unwrap();
    let ctx = make_ctx(&tmp);

    let tool = GrepContentTool;
    let result = tool
        .execute(json!({ "pattern": "root", "path": "../.." }), ctx)
        .await;

    assert!(result.is_err(), "path traversal should be rejected");
}

#[tokio::test]
async fn grep_skips_hidden_dirs_large_files_and_symlinks() {
    let tmp = TempDir::new().unwrap();
    setup_test_files(&tmp);
    let ctx = make_ctx(&tmp);

    let tool = GrepContentTool;
    let result = tool
        .execute(
            json!({ "pattern": "hello", "output_mode": "files_with_matches" }),
            ctx,
        )
        .await
        .unwrap();

    let data = result.data.unwrap();
    let filenames = data["filenames"]
        .as_array()
        .expect("filenames should be array");
    assert!(
        !filenames.iter().any(|value| value == ".git/ignored.txt"),
        "hidden directories should be skipped: {:?}",
        filenames
    );
    assert!(
        !filenames.iter().any(|value| value == "large.log"),
        "large files should be skipped: {:?}",
        filenames
    );
    #[cfg(unix)]
    assert!(
        !filenames.iter().any(|value| value == "link.txt"),
        "symlinks should be skipped: {:?}",
        filenames
    );
}

#[test]
fn grep_is_read_only_and_concurrency_safe() {
    let tool = GrepContentTool;
    let input = json!({});
    assert!(tool.is_concurrency_safe(&input));
    assert!(tool.is_read_only(&input));
}
