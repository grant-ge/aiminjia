//! 权限策略管线集成测试。

use app_lib::runtime::store::permission_store::{PermissionStore, PolicyDecision};
use app_lib::runtime::tools::permission::{StorePolicyPipeline, PermissionPipeline};
use app_lib::runtime::tools::context::ToolExecutionContext;
use app_lib::runtime::tools::catalog::TOOL_CATALOG;
use app_lib::runtime::tools::definition::ToolDefinition;
use app_lib::runtime::ids::{SessionId, RunId};
use app_lib::runtime::cancellation::CancellationToken;
use std::sync::Arc;

fn make_ctx() -> ToolExecutionContext {
    ToolExecutionContext::new(
        SessionId::new("test"),
        RunId::new("test-run"),
        None,
        "test-tool-call",
        CancellationToken::new(),
    )
}

#[test]
fn test_always_allow_bypasses_capability_check() {
    let store = Arc::new(PermissionStore::in_memory());
    store.record("execute_python:python:exec".to_string(), PolicyDecision::AlwaysAllow);
    store.record("execute_python:workspace:write".to_string(), PolicyDecision::AlwaysAllow);

    let pipeline = StorePolicyPipeline::new(store);
    let def = TOOL_CATALOG.get("execute_python").unwrap();
    let ctx = make_ctx(); // 无 capability
    assert!(pipeline.authorize(def, &serde_json::json!({}), &ctx).is_ok());
}

#[test]
fn test_always_deny_blocks_tool() {
    let store = Arc::new(PermissionStore::in_memory());
    store.record("web_search:network".to_string(), PolicyDecision::AlwaysDeny);

    let pipeline = StorePolicyPipeline::new(store);
    let def = TOOL_CATALOG.get("web_search").unwrap();
    let ctx = make_ctx();
    assert!(pipeline.authorize(def, &serde_json::json!({}), &ctx).is_err());
}

#[test]
fn test_unknown_scope_fail_closed() {
    let def = ToolDefinition::new("fake_tool", "test")
        .with_capability_scope(["unknown_scope"]);
    let store = Arc::new(PermissionStore::in_memory());
    let pipeline = StorePolicyPipeline::new(store);
    let ctx = make_ctx();
    let result = pipeline.authorize(&def, &serde_json::json!({}), &ctx);
    assert!(result.is_err(), "unknown scope should fail-closed");
}
