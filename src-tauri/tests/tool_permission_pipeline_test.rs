use app_lib::runtime::tools::{
    ToolExecutionContext,
};
use app_lib::runtime::tools::permission::{CapabilityPermissionPipeline, PermissionPipeline};
use app_lib::runtime::tools::definition::ToolDefinition;
use app_lib::runtime::tools::capability::CapabilityContext;
use serde_json::json;
use std::sync::Arc;
use tempfile::TempDir;

fn def_with_scope(id: &str, scopes: &[&str]) -> ToolDefinition {
    ToolDefinition::new(id, "test")
        .with_capability_scope(scopes.iter().copied())
}

fn ctx_no_capability() -> ToolExecutionContext {
    ToolExecutionContext::for_test("conv", "run", "tc")
}

fn ctx_with_workspace(tmp: &TempDir) -> ToolExecutionContext {
    let cap = CapabilityContext::with_workspace(tmp.path().to_path_buf(), "ws");
    ToolExecutionContext::for_test("conv", "run", "tc").with_capability(Arc::new(cap))
}

#[test]
fn tool_without_scope_is_always_allowed() {
    let pipeline = CapabilityPermissionPipeline;
    let def = ToolDefinition::new("echo", "no scope");
    let ctx = ctx_no_capability();
    assert!(pipeline.authorize(&def, &json!({}), &ctx).is_ok());
}

#[test]
fn workspace_read_tool_rejected_without_capability() {
    let pipeline = CapabilityPermissionPipeline;
    let def = def_with_scope("list_directory", &["workspace:read"]);
    let ctx = ctx_no_capability();
    let result = pipeline.authorize(&def, &json!({}), &ctx);
    assert!(result.is_err(), "workspace:read tool must be rejected without capability");
    let err = result.unwrap_err().to_string();
    assert!(
        err.contains("workspace") || err.contains("capability"),
        "Error should mention workspace/capability: {}", err
    );
}

#[test]
fn workspace_read_tool_allowed_with_workspace_capability() {
    let tmp = TempDir::new().unwrap();
    let pipeline = CapabilityPermissionPipeline;
    let def = def_with_scope("list_directory", &["workspace:read"]);
    let ctx = ctx_with_workspace(&tmp);
    assert!(pipeline.authorize(&def, &json!({}), &ctx).is_ok());
}

#[test]
fn browser_tool_rejected_without_browser_capability() {
    let pipeline = CapabilityPermissionPipeline;
    let def = def_with_scope("browse_navigate", &["browser"]);
    let ctx = ctx_no_capability();
    let result = pipeline.authorize(&def, &json!({}), &ctx);
    assert!(result.is_err(), "browser tool must be rejected without browser capability");
}
