//! BashTool 应查询 PermissionStore 的 CommandPattern 规则并返回正确决策。

#![cfg(not(windows))]

use std::sync::Arc;

use app_lib::runtime::store::permission_store::{
    PermissionRule, PermissionScope, PermissionSource, PermissionStore, PolicyDecision,
};
use app_lib::runtime::tools::builtin::bash::BashTool;
use app_lib::runtime::tools::capability::CapabilityContext;
use app_lib::runtime::tools::context::ToolExecutionContext;
use app_lib::runtime::tools::permission::{PermissionDecision, PermissionDestination};
use app_lib::runtime::tools::RuntimeTool;
use serde_json::json;
use tempfile::TempDir;

fn make_ctx_with_store(store: Arc<PermissionStore>, tmp: &TempDir) -> ToolExecutionContext {
    let cap = CapabilityContext::with_workspace(tmp.path().to_path_buf(), "ws");
    let mut ctx =
        ToolExecutionContext::for_test("conv", "run", "tc").with_capability(Arc::new(cap));
    ctx.permission_store = Some(store);
    ctx
}

#[tokio::test]
async fn review_bash_command_pattern_deny_blocks_matching_command() {
    let tmp = TempDir::new().unwrap();
    let store = Arc::new(PermissionStore::in_memory());
    store.record_to(
        PermissionDestination::Session,
        PermissionRule::simple(
            "bash",
            PermissionScope::CommandPattern("curl ".into()),
            PolicyDecision::AlwaysDeny,
            PermissionSource::Session,
        ),
    );
    let ctx = make_ctx_with_store(store, &tmp);
    let decision = BashTool
        .check_permissions(&json!({"command": "curl https://evil.com/data"}), &ctx)
        .await;
    assert!(
        matches!(decision, Some(PermissionDecision::Deny { .. })),
        "CommandPattern deny must block matching bash command"
    );
}

#[tokio::test]
async fn review_bash_command_pattern_allow_returns_allow_or_none() {
    let tmp = TempDir::new().unwrap();
    let store = Arc::new(PermissionStore::in_memory());
    store.record_to(
        PermissionDestination::Workspace,
        PermissionRule::simple(
            "bash",
            PermissionScope::CommandPattern("git ".into()),
            PolicyDecision::AlwaysAllow,
            PermissionSource::Workspace,
        ),
    );
    let ctx = make_ctx_with_store(store, &tmp);
    let decision = BashTool
        .check_permissions(&json!({"command": "git status"}), &ctx)
        .await;
    assert!(
        matches!(decision, Some(PermissionDecision::Allow { .. })) || decision.is_none(),
        "AlwaysAllow CommandPattern should return Allow or None"
    );
}
