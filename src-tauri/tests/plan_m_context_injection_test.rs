use std::sync::Arc;

use app_lib::runtime::hooks::config::{HookConfig, HookEvent, HookRegistry};
use app_lib::runtime::tools::context::ToolExecutionContext;

#[test]
fn tool_execution_context_default_no_hooks() {
    let ctx = ToolExecutionContext::for_test("conv-1", "run-1", "tc-1");
    assert!(ctx.hook_registry.is_none());
}

#[test]
fn tool_execution_context_with_hook_registry() {
    let mut registry = HookRegistry::new();
    registry.hooks.push(HookConfig {
        event: HookEvent::PreToolUse,
        command: "echo 'ok'".to_string(),
        tool_filter: None,
        timeout_secs: None,
    });
    let ctx = ToolExecutionContext::for_test("conv-1", "run-1", "tc-1")
        .with_hook_registry(Arc::new(registry));
    assert!(ctx.hook_registry.is_some());
    let reg = ctx.hook_registry.unwrap();
    assert_eq!(reg.hooks.len(), 1);
}

#[test]
fn tool_execution_context_clone_preserves_hook_registry() {
    let mut registry = HookRegistry::new();
    registry.hooks.push(HookConfig {
        event: HookEvent::PostToolUse,
        command: "echo 'post'".to_string(),
        tool_filter: Some("bash_tool".to_string()),
        timeout_secs: Some(5),
    });
    let ctx = ToolExecutionContext::for_test("conv-1", "run-1", "tc-1")
        .with_hook_registry(Arc::new(registry));
    let cloned = ctx.clone();
    assert!(cloned.hook_registry.is_some());
}
