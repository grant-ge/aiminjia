use std::sync::Arc;

use app_lib::runtime::store::permission_store::PermissionStore;
use app_lib::runtime::tools::capability::CapabilityContext;
use app_lib::runtime::tools::permission::{
    CapabilityPermissionPipeline, PermissionDecision, PermissionPipeline, PermissionReason,
    StorePolicyPipeline,
};
use app_lib::runtime::tools::{ToolDefinition, ToolExecutionContext};
use serde_json::json;
use tempfile::TempDir;

fn def_with_scope(id: &str, scopes: &[&str]) -> ToolDefinition {
    ToolDefinition::new(id, "review permission scope dedup")
        .with_capability_scope(scopes.iter().copied())
}

fn ctx_without_capability() -> ToolExecutionContext {
    ToolExecutionContext::for_test("conv", "run", "tool-call")
}

fn ctx_with_workspace() -> ToolExecutionContext {
    let tmp = TempDir::new().expect("tempdir");
    let cap = CapabilityContext::with_workspace(tmp.path().to_path_buf(), "ws");
    ToolExecutionContext::for_test("conv", "run", "tool-call").with_capability(Arc::new(cap))
}

fn decision_kind(decision: &PermissionDecision) -> &'static str {
    match decision {
        PermissionDecision::Allow { .. } => "allow",
        PermissionDecision::Deny { .. } => "deny",
        PermissionDecision::Ask { .. } => "ask",
    }
}

#[test]
fn review_known_scopes_keep_aligned_outcomes_between_pipelines() {
    let capability_pipeline = CapabilityPermissionPipeline;
    let store_pipeline = StorePolicyPipeline::new(Arc::new(PermissionStore::in_memory()));

    let cases = [
        ("workspace:read", ctx_without_capability(), "deny"),
        ("workspace:read", ctx_with_workspace(), "allow"),
        ("workspace:write", ctx_without_capability(), "deny"),
        ("workspace:write", ctx_with_workspace(), "allow"),
        ("python:exec", ctx_without_capability(), "deny"),
        ("python:exec", ctx_with_workspace(), "allow"),
        ("browser", ctx_without_capability(), "deny"),
        ("network", ctx_without_capability(), "allow"),
    ];

    for (scope, ctx, expected_kind) in cases {
        let definition = def_with_scope("test_tool", &[scope]);
        let capability_decision = capability_pipeline.authorize(&definition, &json!({}), &ctx);
        let store_decision = store_pipeline.authorize(&definition, &json!({}), &ctx);

        assert_eq!(
            decision_kind(&capability_decision),
            expected_kind,
            "CapabilityPermissionPipeline mismatch for scope {scope}"
        );
        assert_eq!(
            decision_kind(&store_decision),
            expected_kind,
            "StorePolicyPipeline mismatch for scope {scope}"
        );
    }
}

#[test]
fn review_unknown_scope_semantics_remain_split_between_pipelines() {
    let definition = def_with_scope("test_tool", &["custom:unknown"]);
    let ctx = ctx_without_capability();
    let capability_pipeline = CapabilityPermissionPipeline;
    let store_pipeline = StorePolicyPipeline::new(Arc::new(PermissionStore::in_memory()));

    let capability_decision = capability_pipeline.authorize(&definition, &json!({}), &ctx);
    assert!(matches!(
        capability_decision,
        PermissionDecision::Deny {
            reason: PermissionReason::UnknownScope,
            ..
        }
    ));

    let store_decision = store_pipeline.authorize(&definition, &json!({}), &ctx);
    assert!(matches!(
        store_decision,
        PermissionDecision::Ask {
            reason: PermissionReason::UnknownScope,
            ..
        }
    ));
}
