use std::sync::Arc;

use app_lib::runtime::store::permission_store::{PermissionStore, PolicyDecision};
use app_lib::runtime::tools::context::ToolExecutionContext;
use app_lib::runtime::tools::definition::ToolDefinition;
use app_lib::runtime::tools::permission::{
    CapabilityPermissionPipeline, PermissionDecision, PermissionPipeline, StorePolicyPipeline,
};
use serde_json::json;

fn def_with_scope(id: &str, scopes: &[&str]) -> ToolDefinition {
    ToolDefinition::new(id, "mcp permission review test")
        .with_capability_scope(scopes.iter().copied())
}

fn make_ctx() -> ToolExecutionContext {
    ToolExecutionContext::for_test("conv-mcp-review", "run-mcp-review", "tc-mcp-review")
}

#[test]
fn review_mcp_scope_triggers_ask_in_store_pipeline() {
    let store = Arc::new(PermissionStore::in_memory());
    let pipeline = StorePolicyPipeline::new(store);
    let def = def_with_scope("mcp__demo__search", &["mcp"]);

    let decision = pipeline.authorize(&def, &json!({"query": "hi"}), &make_ctx());

    match decision {
        PermissionDecision::Ask {
            message, reason, ..
        } => {
            assert!(message.contains("MCP") || message.contains("external server"));
            assert!(matches!(
                reason,
                app_lib::runtime::tools::permission::PermissionReason::UnknownScope
            ));
        }
        other => panic!(
            "expected Ask for MCP scope in store pipeline, got: {:?}",
            other
        ),
    }
}

#[test]
fn review_mcp_scope_denies_in_capability_pipeline() {
    let pipeline = CapabilityPermissionPipeline;
    let def = def_with_scope("mcp__demo__search", &["mcp"]);

    let decision = pipeline.authorize(&def, &json!({"query": "hi"}), &make_ctx());

    match decision {
        PermissionDecision::Deny { message, reason } => {
            assert!(message.contains("unknown capability") || message.contains("unknown"));
            assert!(matches!(
                reason,
                app_lib::runtime::tools::permission::PermissionReason::UnknownScope
            ));
        }
        other => panic!(
            "expected Deny for MCP scope in capability pipeline, got: {:?}",
            other
        ),
    }
}

#[test]
fn review_mcp_always_allow_bypasses_ask() {
    let store = Arc::new(PermissionStore::in_memory());
    store.record(
        "mcp__demo__search:mcp".to_string(),
        PolicyDecision::AlwaysAllow,
    );
    let pipeline = StorePolicyPipeline::new(store);
    let def = def_with_scope("mcp__demo__search", &["mcp"]);

    let decision = pipeline.authorize(&def, &json!({"query": "hi"}), &make_ctx());

    assert!(matches!(decision, PermissionDecision::Allow { .. }));
}

#[test]
fn review_network_scope_still_passes_without_ask() {
    let pipeline = CapabilityPermissionPipeline;
    let def = def_with_scope("web_search", &["network"]);

    let decision = pipeline.authorize(&def, &json!({"query": "hi"}), &make_ctx());

    assert!(matches!(decision, PermissionDecision::Allow { .. }));
}
