//! 文件写工具应查询 PermissionStore PathGlob 规则，Deny 路径应被拒绝。

use std::sync::Arc;

use app_lib::runtime::store::permission_store::{
    PermissionRule, PermissionScope, PermissionSource, PermissionStore, PolicyDecision,
};
use app_lib::runtime::tools::builtin::workspace::{EditFileRuntimeTool, WriteFileRuntimeTool};
use app_lib::runtime::tools::capability::CapabilityContext;
use app_lib::runtime::tools::context::ToolExecutionContext;
use app_lib::runtime::tools::permission::{PermissionDecision, PermissionDestination};
use app_lib::runtime::tools::RuntimeTool;
use serde_json::json;
use tempfile::TempDir;

fn make_ctx(store: Arc<PermissionStore>, tmp: &TempDir) -> ToolExecutionContext {
    let cap = CapabilityContext::with_workspace(tmp.path().to_path_buf(), "ws");
    ToolExecutionContext::for_test("conv", "run", "tc")
        .with_capability(Arc::new(cap))
        .with_permission_store(store)
}

#[tokio::test]
async fn review_write_file_path_glob_deny_blocks_matching_path() {
    let tmp = TempDir::new().unwrap();
    let store = Arc::new(PermissionStore::in_memory());
    store.record_to(
        PermissionDestination::User,
        PermissionRule::simple(
            "Write",
            PermissionScope::PathGlob(format!("{}/blocked/**", tmp.path().display())),
            PolicyDecision::AlwaysDeny,
            PermissionSource::User,
        ),
    );
    let ctx = make_ctx(store, &tmp);
    let decision = WriteFileRuntimeTool
        .check_permissions(
            &json!({"path": "blocked/output.csv", "content": "a,b"}),
            &ctx,
        )
        .await;
    assert!(
        matches!(decision, Some(PermissionDecision::Deny { .. })),
        "PathGlob deny must block matching write_file path"
    );
}

#[tokio::test]
async fn review_write_file_no_matching_glob_returns_none() {
    let tmp = TempDir::new().unwrap();
    let store = Arc::new(PermissionStore::in_memory());
    let ctx = make_ctx(store, &tmp);
    let decision = WriteFileRuntimeTool
        .check_permissions(
            &json!({"path": "output/result.csv", "content": "a,b"}),
            &ctx,
        )
        .await;
    assert!(decision.is_none(), "No glob rule should return None");
}

#[tokio::test]
async fn review_edit_file_path_glob_deny_blocks_matching_path() {
    let tmp = TempDir::new().unwrap();
    let store = Arc::new(PermissionStore::in_memory());
    store.record_to(
        PermissionDestination::Session,
        PermissionRule::simple(
            "Edit",
            PermissionScope::PathGlob(format!("{}/secret/**", tmp.path().display())),
            PolicyDecision::AlwaysDeny,
            PermissionSource::Session,
        ),
    );
    let ctx = make_ctx(store, &tmp);
    let decision = EditFileRuntimeTool
        .check_permissions(
            &json!({"path": "secret/config.txt", "old_string": "a", "new_string": "b"}),
            &ctx,
        )
        .await;
    assert!(
        matches!(decision, Some(PermissionDecision::Deny { .. })),
        "PathGlob deny must block matching edit_file path"
    );
}
