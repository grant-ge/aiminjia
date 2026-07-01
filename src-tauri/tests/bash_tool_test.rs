//! Integration tests for BashTool.
//! 所有测试仅使用安全命令（echo, ls, cat, grep 等），不执行危险操作。

#![cfg(not(windows))]

use app_lib::runtime::cancellation::{CancellationReason, CancellationToken};
use app_lib::runtime::ids::{RunId, SessionId};
use app_lib::runtime::tools::builtin::bash::BashTool;
use app_lib::runtime::tools::capability::CapabilityContext;
use app_lib::runtime::tools::executor::ToolError;
use app_lib::runtime::tools::permission::PermissionDecision;
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

#[tokio::test]
async fn bash_executes_echo_command() {
    let tmp = TempDir::new().unwrap();
    let ctx = make_ctx(&tmp);

    let tool = BashTool::default();
    let result = tool
        .execute(json!({ "command": "echo hello" }), ctx)
        .await
        .unwrap();

    assert!(
        result.content.contains("hello"),
        "output should contain hello: {}",
        result.content
    );
}

#[tokio::test]
async fn bash_returns_error_for_nonzero_exit_code() {
    let tmp = TempDir::new().unwrap();
    let ctx = make_ctx(&tmp);

    let tool = BashTool::default();
    let result = tool.execute(json!({ "command": "exit 42" }), ctx).await;

    assert!(result.is_err(), "exit 42 should surface as tool error");
    let err = result.unwrap_err().to_string();
    assert!(
        err.contains("42") || err.contains("exit code"),
        "error should mention exit code: {err}"
    );
}

#[tokio::test]
async fn bash_surfaces_dws_pat_no_permission_as_ask_required() {
    let tmp = TempDir::new().unwrap();
    let ctx = make_ctx(&tmp);

    let tool = BashTool::default();
    let result = tool
        .execute(
            json!({
                "command": r#"dws() { return 0; }; dws chat message send; printf '%s\n' '{"success":false,"code":"PAT_HIGH_RISK_NO_PERMISSION","data":{"flowId":"flow-1","authorizationUrl":"https://example.test/auth?flow=flow-1","requiredScopes":["chat.message:send"]}}'; exit 4"#
            }),
            ctx,
        )
        .await;

    let Err(ToolError::AskRequired(PermissionDecision::Ask {
        message,
        suggestions,
        remember_options,
        default_destination,
        ..
    })) = result
    else {
        panic!("expected DWS PAT failure to surface as AskRequired, got: {result:?}");
    };

    assert!(message.contains("chat.message:send"));
    assert!(message.contains("https://example.test/auth?flow=flow-1"));
    assert!(message.contains("flow-1"));
    assert!(
        suggestions
            .iter()
            .any(|suggestion| suggestion.contains("重放原命令")),
        "suggestions should guide replay after auth: {suggestions:?}"
    );
    assert_eq!(
        remember_options,
        vec![app_lib::runtime::tools::permission::PermissionDestination::Session]
    );
    assert_eq!(
        default_destination,
        Some(app_lib::runtime::tools::permission::PermissionDestination::Session)
    );
}

#[tokio::test]
async fn bash_does_not_intercept_non_dws_pat_like_json() {
    let tmp = TempDir::new().unwrap();
    let ctx = make_ctx(&tmp);

    let tool = BashTool::default();
    let result = tool
        .execute(
            json!({
                "command": r#"printf '%s\n' '{"success":false,"code":"PAT_HIGH_RISK_NO_PERMISSION","data":{"authorizationUrl":"https://example.test/auth","requiredScopes":["chat.message:send"]}}'; exit 4"#
            }),
            ctx,
        )
        .await;

    let Err(ToolError::ExecutionFailed(message)) = result else {
        panic!("expected non-DWS PAT-like output to remain ExecutionFailed, got: {result:?}");
    };
    assert!(message.contains("PAT_HIGH_RISK_NO_PERMISSION"));
}

#[tokio::test]
async fn bash_allows_grep_exit_one_as_non_error() {
    let tmp = TempDir::new().unwrap();
    std::fs::write(tmp.path().join("sample.txt"), "hello world\n").unwrap();
    let ctx = make_ctx(&tmp);

    let tool = BashTool::default();
    let result = tool
        .execute(json!({ "command": "grep needle sample.txt" }), ctx)
        .await
        .unwrap();

    let data = result.data.expect("grep result should include data");
    assert_eq!(data["exit_code"], json!(1));
    assert_eq!(data["semantic_message"], json!("No matches found"));
}

#[tokio::test]
async fn bash_runs_in_workspace_root() {
    let tmp = TempDir::new().unwrap();
    std::fs::write(tmp.path().join("sentinel.txt"), b"marker").unwrap();
    let ctx = make_ctx(&tmp);

    let tool = BashTool::default();
    let result = tool
        .execute(json!({ "command": "ls sentinel.txt" }), ctx)
        .await
        .unwrap();

    assert!(
        result.content.contains("sentinel.txt"),
        "should see sentinel.txt in workspace: {}",
        result.content
    );
}

#[tokio::test]
async fn bash_merges_stdout_and_stderr() {
    let tmp = TempDir::new().unwrap();
    let ctx = make_ctx(&tmp);

    let tool = BashTool::default();
    let result = tool
        .execute(
            json!({
                "command": "printf 'stdout-1\\n'; printf 'stderr-1\\n' >&2; printf 'stdout-2\\n'; printf 'stderr-2\\n' >&2"
            }),
            ctx,
        )
        .await
        .unwrap();

    assert!(
        result.content.contains("stdout-1"),
        "should contain stdout: {}",
        result.content
    );
    assert!(
        result.content.contains("stderr-1"),
        "should contain stderr: {}",
        result.content
    );
    let expected = "stdout-1\nstderr-1\nstdout-2\nstderr-2";
    assert!(
        result.content.contains(expected),
        "stdout/stderr should keep merged ordering.\nexpected snippet: {expected}\nactual: {}",
        result.content
    );
}

#[tokio::test]
async fn bash_returns_error_on_timeout() {
    let tmp = TempDir::new().unwrap();
    let ctx = make_ctx(&tmp);

    let tool = BashTool::default();
    let result = tool
        .execute(json!({ "command": "sleep 10", "timeout": 1000 }), ctx)
        .await;

    assert!(result.is_err(), "timeout should surface as tool error");
    let err = result.unwrap_err().to_string();
    assert!(
        err.contains("timed out") || err.contains("timeout"),
        "should indicate timeout: {err}"
    );
    assert!(
        !err.contains("background"),
        "task 3 should not claim background semantics: {err}"
    );
}

#[tokio::test]
async fn bash_does_not_wait_for_inherited_pipe_after_parent_exits() {
    let tmp = TempDir::new().unwrap();
    let ctx = make_ctx(&tmp);

    let tool = BashTool::default();
    let result = tokio::time::timeout(
        std::time::Duration::from_millis(2500),
        tool.execute(
            json!({
                "command": "(sh -c 'sleep 5' &); printf 'aijia-inherited-pipe-parent\\n'",
                "timeout": 1000
            }),
            ctx,
        ),
    )
    .await;

    assert!(
        result.is_ok(),
        "BashTool should not wait for inherited stdout/stderr handles after the parent exits"
    );
    let result = result.unwrap().unwrap();
    assert!(
        result.content.contains("aijia-inherited-pipe-parent"),
        "parent output should be preserved: {}",
        result.content
    );
    let data = result.data.expect("Bash result should include data");
    assert_eq!(data["stream_timed_out"], json!(true));
    assert_eq!(data["reader_aborted"], json!(true));
}

#[tokio::test]
async fn bash_timeout_kills_descendant_processes() {
    let tmp = TempDir::new().unwrap();
    let ctx = make_ctx(&tmp);

    let tool = BashTool::default();
    let result = tool
        .execute(
            json!({
                "command": "sh -c 'sleep 2; echo orphan > timeout-child.txt' & wait",
                "timeout": 1000
            }),
            ctx,
        )
        .await;

    assert!(result.is_err(), "timeout should still return an error");
    tokio::time::sleep(std::time::Duration::from_millis(2300)).await;
    assert!(
        !tmp.path().join("timeout-child.txt").exists(),
        "timeout should kill descendant processes, but background child still wrote a file"
    );
}

#[tokio::test]
async fn bash_returns_error_when_cancelled() {
    let tmp = TempDir::new().unwrap();
    let token = CancellationToken::new();
    let cap = Arc::new(CapabilityContext::with_workspace(
        tmp.path().to_path_buf(),
        "test-ws",
    ));
    let ctx = ToolExecutionContext::new(
        SessionId::new("conv-1"),
        RunId::new("run-1"),
        None,
        "tc-1",
        token.clone(),
    )
    .with_capability(cap);

    let token_clone = token.clone();
    tokio::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        token_clone.cancel();
    });

    let tool = BashTool::default();
    let result = tool.execute(json!({ "command": "sleep 10" }), ctx).await;

    assert!(result.is_err(), "cancelled command should return error");
    let err = result.unwrap_err().to_string();
    assert!(
        err.contains("cancelled") || err.contains("cancel"),
        "error should mention cancellation: {err}"
    );
}

#[tokio::test]
async fn bash_cancel_kills_descendant_processes() {
    let tmp = TempDir::new().unwrap();
    let token = CancellationToken::new();
    let cap = Arc::new(CapabilityContext::with_workspace(
        tmp.path().to_path_buf(),
        "test-ws",
    ));
    let ctx = ToolExecutionContext::new(
        SessionId::new("conv-1"),
        RunId::new("run-1"),
        None,
        "tc-1",
        token.clone(),
    )
    .with_capability(cap);

    let token_clone = token.clone();
    tokio::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        token_clone.cancel();
    });

    let tool = BashTool::default();
    let result = tool
        .execute(
            json!({ "command": "sh -c 'sleep 2; echo orphan > cancel-child.txt' & wait" }),
            ctx,
        )
        .await;

    assert!(
        result.is_err(),
        "cancelled command should still return an error"
    );
    tokio::time::sleep(std::time::Duration::from_millis(2300)).await;
    assert!(
        !tmp.path().join("cancel-child.txt").exists(),
        "cancel should kill descendant processes, but background child still wrote a file"
    );
}

#[tokio::test]
async fn bash_cancel_does_not_report_background_stop_reason() {
    let tmp = TempDir::new().unwrap();
    let token = CancellationToken::new();
    let cap = Arc::new(CapabilityContext::with_workspace(
        tmp.path().to_path_buf(),
        "test-ws",
    ));
    let ctx = ToolExecutionContext::new(
        SessionId::new("conv-1"),
        RunId::new("run-1"),
        None,
        "tc-1",
        token.clone(),
    )
    .with_capability(cap);

    let token_clone = token.clone();
    tokio::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        token_clone.cancel_with_reason(CancellationReason::BackgroundStop);
    });

    let tool = BashTool::default();
    let result = tool.execute(json!({ "command": "sleep 10" }), ctx).await;

    assert!(result.is_err(), "cancelled command should return error");
    let err = result.unwrap_err().to_string();
    assert!(
        !err.contains("background"),
        "foreground bash tool must not surface background wording: {err}"
    );
}

#[tokio::test]
async fn bash_denies_rm_rf_slash() {
    let tmp = TempDir::new().unwrap();
    let ctx = make_ctx(&tmp);

    let tool = BashTool::default();
    let input = json!({ "command": "rm -rf /" });
    let decision = tool.check_permissions(&input, &ctx).await;

    assert!(
        matches!(decision, Some(PermissionDecision::Deny { .. })),
        "rm -rf / should be denied"
    );
}

#[tokio::test]
async fn bash_denies_write_to_etc() {
    let tmp = TempDir::new().unwrap();
    let ctx = make_ctx(&tmp);

    let tool = BashTool::default();
    let input = json!({ "command": "echo evil > /etc/passwd" });
    let decision = tool.check_permissions(&input, &ctx).await;

    assert!(
        matches!(decision, Some(PermissionDecision::Deny { .. })),
        "write to /etc should be denied"
    );
}

#[tokio::test]
async fn bash_fails_without_capability_context() {
    let tool = BashTool::default();
    let ctx = ToolExecutionContext::for_test("conv-1", "run-1", "tc-1");

    let result = tool.execute(json!({ "command": "echo hi" }), ctx).await;

    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(
        err.contains("permission") || err.contains("capability"),
        "should mention capability/permission: {err}"
    );
}
