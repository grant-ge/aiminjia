use std::sync::Arc;

use app_lib::runtime::store::permission_store::{
    PermissionRule, PermissionScope, PermissionSource, PermissionStore, PolicyDecision,
};
use app_lib::runtime::tools::capability::CapabilityContext;
use app_lib::runtime::tools::definition::ToolDefinition;
use app_lib::runtime::tools::permission::{
    apply_permission_mode, CapabilityPermissionPipeline, PermissionDecision, PermissionDestination,
    PermissionMode, PermissionPipeline, StorePolicyPipeline,
};
use app_lib::runtime::tools::ToolExecutionContext;
use serde_json::json;
use tempfile::TempDir;

fn def(id: &str, scopes: &[&str]) -> ToolDefinition {
    ToolDefinition::new(id, "test tool").with_capability_scope(scopes.iter().copied())
}

fn ctx_no_capability() -> ToolExecutionContext {
    ToolExecutionContext::for_test("conv-permission-pipeline", "run-permission-pipeline", "tc")
}

fn ctx_with_workspace(tmp: &TempDir) -> ToolExecutionContext {
    let cap = CapabilityContext::with_workspace(tmp.path().to_path_buf(), "ws");
    ToolExecutionContext::for_test("conv-permission-pipeline", "run-permission-pipeline", "tc")
        .with_capability(Arc::new(cap))
}

fn assert_allow(decision: &PermissionDecision) {
    assert!(
        matches!(decision, PermissionDecision::Allow { .. }),
        "expected Allow, got {decision:?}"
    );
}

fn deny_message(decision: &PermissionDecision) -> &str {
    match decision {
        PermissionDecision::Deny { message, .. } => message,
        other => panic!("expected Deny, got {other:?}"),
    }
}

#[test]
fn no_capability_scope_tool_is_allowed_with_or_without_capability_context() {
    let pipeline = CapabilityPermissionPipeline;
    let def = ToolDefinition::new("write_memory", "memory helper");
    let no_capability = ctx_no_capability();
    let tmp = TempDir::new().expect("tempdir");
    let with_workspace = ctx_with_workspace(&tmp);

    assert_allow(&pipeline.authorize(&def, &json!({}), &no_capability));
    assert_allow(&pipeline.authorize(&def, &json!({}), &with_workspace));
}

#[test]
fn workspace_write_tool_is_denied_without_workspace_capability() {
    let pipeline = CapabilityPermissionPipeline;
    let result = pipeline.authorize(
        &def("file_write", &["workspace:write"]),
        &json!({}),
        &ctx_no_capability(),
    );

    let message = deny_message(&result);
    assert!(
        message.contains("workspace"),
        "message should mention workspace: {message}"
    );
    assert!(
        message.contains("file_write"),
        "message should mention tool id: {message}"
    );
}

#[test]
fn workspace_write_tool_is_allowed_with_workspace_capability() {
    let pipeline = CapabilityPermissionPipeline;
    let tmp = TempDir::new().expect("tempdir");
    let result = pipeline.authorize(
        &def("file_write", &["workspace:write"]),
        &json!({}),
        &ctx_with_workspace(&tmp),
    );

    assert_allow(&result);
}

#[test]
fn workspace_write_scope_is_denied_without_workspace_capability() {
    let pipeline = CapabilityPermissionPipeline;
    let result = pipeline.authorize(
        &def("bash", &["workspace:write"]),
        &json!({}),
        &ctx_no_capability(),
    );

    let message = deny_message(&result);
    assert!(
        message.contains("workspace"),
        "message should mention workspace: {message}"
    );
}

#[test]
fn browser_scope_is_denied_without_browser_capability() {
    let pipeline = CapabilityPermissionPipeline;
    let result = pipeline.authorize(
        &def("browse_page", &["browser"]),
        &json!({}),
        &ctx_no_capability(),
    );

    let message = deny_message(&result);
    assert!(
        message.contains("browser"),
        "message should mention browser: {message}"
    );
    assert!(
        message.contains("browse_page"),
        "message should mention tool id: {message}"
    );
}

#[test]
fn network_scope_is_allowed_without_local_capability_context() {
    let pipeline = CapabilityPermissionPipeline;
    let result = pipeline.authorize(
        &def("fetch_url", &["network"]),
        &json!({}),
        &ctx_no_capability(),
    );

    assert_allow(&result);
}

#[test]
fn unknown_scope_is_denied_by_capability_pipeline_fail_closed() {
    let pipeline = CapabilityPermissionPipeline;
    let result = pipeline.authorize(
        &def("custom_tool", &["custom:unknown"]),
        &json!({}),
        &ctx_no_capability(),
    );

    let message = deny_message(&result);
    assert!(
        message.contains("custom:unknown"),
        "message should mention unknown scope: {message}"
    );
}

#[test]
fn mcp_scope_without_stored_policy_becomes_ask_in_store_policy_pipeline() {
    let store = Arc::new(PermissionStore::in_memory());
    let pipeline = StorePolicyPipeline::new(store);
    let result = pipeline.authorize(
        &def("mcp__demo__action", &["mcp"]),
        &json!({}),
        &ctx_no_capability(),
    );

    let PermissionDecision::Ask {
        message,
        suggestions,
        remember_options,
        default_destination,
        ..
    } = result
    else {
        panic!("mcp scope without stored policy should Ask");
    };

    assert!(
        message.contains("mcp__demo__action"),
        "message should mention tool id: {message}"
    );
    assert!(
        message.contains("external server") || message.contains("MCP"),
        "message should mention external server/MCP: {message}"
    );
    assert!(suggestions.contains(&"Allow once".to_string()));
    assert!(suggestions.contains(&"Deny".to_string()));
    assert!(remember_options.contains(&PermissionDestination::Session));
    assert!(remember_options.contains(&PermissionDestination::Workspace));
    assert!(remember_options.contains(&PermissionDestination::User));
    assert_eq!(default_destination, Some(PermissionDestination::Session));
}

#[test]
fn stored_allow_in_store_policy_pipeline_bypasses_missing_capability() {
    let store = Arc::new(PermissionStore::in_memory());
    store.record_to(
        PermissionDestination::Workspace,
        PermissionRule::simple(
            "file_write",
            PermissionScope::Scope("workspace:write".to_string()),
            PolicyDecision::AlwaysAllow,
            PermissionSource::Workspace,
        ),
    );

    let store_result = StorePolicyPipeline::new(store).authorize(
        &def("file_write", &["workspace:write"]),
        &json!({}),
        &ctx_no_capability(),
    );
    let capability_result = CapabilityPermissionPipeline.authorize(
        &def("file_write", &["workspace:write"]),
        &json!({}),
        &ctx_no_capability(),
    );

    assert_allow(&store_result);
    assert!(matches!(capability_result, PermissionDecision::Deny { .. }));
}

#[test]
fn stored_deny_in_store_policy_pipeline_denies_without_asking() {
    let store = Arc::new(PermissionStore::in_memory());
    store.record_to(
        PermissionDestination::Workspace,
        PermissionRule::simple(
            "mcp__demo__action",
            PermissionScope::Scope("mcp".to_string()),
            PolicyDecision::AlwaysDeny,
            PermissionSource::Workspace,
        ),
    );

    let result = StorePolicyPipeline::new(store).authorize(
        &def("mcp__demo__action", &["mcp"]),
        &json!({}),
        &ctx_no_capability(),
    );

    let message = deny_message(&result);
    assert!(
        message.contains("mcp__demo__action"),
        "message should mention tool id: {message}"
    );
    assert!(
        message.contains("stored policy") || message.contains("denied by"),
        "message should mention stored denial: {message}"
    );
}

#[test]
fn dont_ask_mode_transforms_ask_into_deny_without_permission_prompt() {
    let ask = PermissionDecision::Ask {
        message: "requires permission".to_string(),
        suggestions: vec!["Allow once".to_string(), "Deny".to_string()],
        remember_options: vec![PermissionDestination::Session],
        default_destination: Some(PermissionDestination::Session),
        reason: app_lib::runtime::tools::permission::PermissionReason::UnknownScope,
    };

    let transformed = apply_permission_mode(ask, "mcp__demo__action", PermissionMode::DontAsk);
    let message = deny_message(&transformed);
    assert!(
        message.contains("dontAsk"),
        "message should mention dontAsk: {message}"
    );
    assert!(
        message.contains("requires permission"),
        "message should mention permission: {message}"
    );
}

#[test]
fn plan_mode_transforms_ask_into_read_only_deny() {
    let ask = PermissionDecision::Ask {
        message: "requires permission".to_string(),
        suggestions: vec!["Allow once".to_string(), "Deny".to_string()],
        remember_options: vec![PermissionDestination::Session],
        default_destination: Some(PermissionDestination::Session),
        reason: app_lib::runtime::tools::permission::PermissionReason::UnknownScope,
    };

    let transformed = apply_permission_mode(ask, "file_write", PermissionMode::Plan);
    let message = deny_message(&transformed);
    assert!(
        message.contains("plan"),
        "message should mention plan mode: {message}"
    );
    assert!(
        message.contains("read-only"),
        "message should mention read-only: {message}"
    );
}
