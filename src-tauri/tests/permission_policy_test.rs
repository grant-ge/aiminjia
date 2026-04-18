//! 权限策略管线集成测试。

use app_lib::runtime::store::permission_store::{PermissionStore, PolicyDecision};
use app_lib::runtime::tools::permission::{StorePolicyPipeline, PermissionDecision, PermissionPipeline};
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

fn is_allow(d: &PermissionDecision) -> bool {
    matches!(d, PermissionDecision::Allow { .. })
}

fn is_deny(d: &PermissionDecision) -> bool {
    matches!(d, PermissionDecision::Deny { .. })
}

fn is_ask(d: &PermissionDecision) -> bool {
    matches!(d, PermissionDecision::Ask { .. })
}

#[test]
fn test_always_allow_bypasses_capability_check() {
    let store = Arc::new(PermissionStore::in_memory());
    store.record("execute_python:python:exec".to_string(), PolicyDecision::AlwaysAllow);
    store.record("execute_python:workspace:write".to_string(), PolicyDecision::AlwaysAllow);

    let pipeline = StorePolicyPipeline::new(store);
    let def = TOOL_CATALOG.get("execute_python").unwrap();
    let ctx = make_ctx(); // 无 capability
    assert!(is_allow(&pipeline.authorize(&def, &serde_json::json!({}), &ctx)));
}

#[test]
fn test_always_deny_blocks_tool() {
    let store = Arc::new(PermissionStore::in_memory());
    store.record("web_search:network".to_string(), PolicyDecision::AlwaysDeny);

    let pipeline = StorePolicyPipeline::new(store);
    let def = TOOL_CATALOG.get("web_search").unwrap();
    let ctx = make_ctx();
    assert!(is_deny(&pipeline.authorize(&def, &serde_json::json!({}), &ctx)));
}

#[test]
fn test_unknown_scope_escalates_to_ask() {
    // StorePolicyPipeline escalates unknown scopes to Ask (not fail-closed Deny),
    // giving the user a chance to grant or deny persistently.
    let def = ToolDefinition::new("fake_tool", "test")
        .with_capability_scope(["unknown_scope"]);
    let store = Arc::new(PermissionStore::in_memory());
    let pipeline = StorePolicyPipeline::new(store);
    let ctx = make_ctx();
    let result = pipeline.authorize(&def, &serde_json::json!({}), &ctx);
    assert!(
        is_ask(&result),
        "StorePolicyPipeline: unknown scope with no stored decision should escalate to Ask, got: {:?}",
        result
    );
}

#[test]
fn test_unknown_scope_with_deny_policy_blocks_tool() {
    // If the user has previously denied the unknown scope, it should be Deny.
    let def = ToolDefinition::new("fake_tool", "test")
        .with_capability_scope(["unknown_scope"]);
    let store = Arc::new(PermissionStore::in_memory());
    store.record("fake_tool:unknown_scope".to_string(), PolicyDecision::Deny);
    let pipeline = StorePolicyPipeline::new(store);
    let ctx = make_ctx();
    let result = pipeline.authorize(&def, &serde_json::json!({}), &ctx);
    assert!(
        is_deny(&result),
        "Stored deny policy must produce Deny, got: {:?}",
        result
    );
}
