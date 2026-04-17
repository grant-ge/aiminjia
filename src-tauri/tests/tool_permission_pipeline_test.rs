use app_lib::runtime::tools::{
    ToolExecutionContext,
};
use app_lib::runtime::tools::permission::{CapabilityPermissionPipeline, PermissionDecision, PermissionPipeline};
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

fn is_allow(d: &PermissionDecision) -> bool {
    matches!(d, PermissionDecision::Allow { .. })
}

fn is_deny(d: &PermissionDecision) -> bool {
    matches!(d, PermissionDecision::Deny { .. })
}

#[test]
fn tool_without_scope_is_always_allowed() {
    let pipeline = CapabilityPermissionPipeline;
    let def = ToolDefinition::new("echo", "no scope");
    let ctx = ctx_no_capability();
    assert!(is_allow(&pipeline.authorize(&def, &json!({}), &ctx)));
}

#[test]
fn workspace_read_tool_rejected_without_capability() {
    let pipeline = CapabilityPermissionPipeline;
    let def = def_with_scope("list_directory", &["workspace:read"]);
    let ctx = ctx_no_capability();
    let result = pipeline.authorize(&def, &json!({}), &ctx);
    assert!(is_deny(&result), "workspace:read tool must be rejected without capability");
    if let PermissionDecision::Deny { message, .. } = &result {
        assert!(
            message.contains("workspace") || message.contains("capability"),
            "Error should mention workspace/capability: {}", message
        );
    }
}

#[test]
fn workspace_read_tool_allowed_with_workspace_capability() {
    let tmp = TempDir::new().unwrap();
    let pipeline = CapabilityPermissionPipeline;
    let def = def_with_scope("list_directory", &["workspace:read"]);
    let ctx = ctx_with_workspace(&tmp);
    assert!(is_allow(&pipeline.authorize(&def, &json!({}), &ctx)));
}

#[test]
fn browser_tool_rejected_without_browser_capability() {
    let pipeline = CapabilityPermissionPipeline;
    let def = def_with_scope("browse_navigate", &["browser"]);
    let ctx = ctx_no_capability();
    let result = pipeline.authorize(&def, &json!({}), &ctx);
    assert!(is_deny(&result), "browser tool must be rejected without browser capability");
}

#[test]
fn browser_tool_allowed_with_browser_available_capability() {
    let tmp = TempDir::new().unwrap();
    let pipeline = CapabilityPermissionPipeline;
    let def = def_with_scope("browse_navigate", &["browser"]);
    // 构建带 browser_available=true 的 context
    let cap = CapabilityContext::with_workspace(tmp.path().to_path_buf(), "ws")
        .with_browser();
    let ctx = ToolExecutionContext::for_test("conv", "run", "tc")
        .with_capability(Arc::new(cap));
    assert!(
        is_allow(&pipeline.authorize(&def, &json!({}), &ctx)),
        "browser tool must be allowed when browser_available=true"
    );
}

#[test]
fn browser_tool_still_rejected_when_browser_available_false() {
    let tmp = TempDir::new().unwrap();
    let pipeline = CapabilityPermissionPipeline;
    let def = def_with_scope("browse_navigate", &["browser"]);
    // 普通 workspace context，不调用 with_browser()
    let cap = CapabilityContext::with_workspace(tmp.path().to_path_buf(), "ws");
    let ctx = ToolExecutionContext::for_test("conv", "run", "tc")
        .with_capability(Arc::new(cap));
    let result = pipeline.authorize(&def, &json!({}), &ctx);
    assert!(is_deny(&result), "browser tool must be rejected when browser_available=false");
}

// Task 3.1 tests

#[tokio::test]
async fn tool_check_permissions_overrides_pipeline_when_some() {
    use app_lib::runtime::tools::{
        AllowAllPermissionPipeline, RuntimeTool, ToolDispatcher, ToolError,
        ToolResult,
    };
    use app_lib::runtime::tools::permission::PermissionReason;
    use async_trait::async_trait;
    use serde_json::Value;

    struct AlwaysDenyTool;

    #[async_trait]
    impl RuntimeTool for AlwaysDenyTool {
        fn definition(&self) -> ToolDefinition {
            ToolDefinition::new("always_deny", "always deny")
        }

        async fn execute(
            &self,
            _input: Value,
            _ctx: ToolExecutionContext,
        ) -> Result<ToolResult, ToolError> {
            Ok(ToolResult::new("always_deny", "should not reach", None))
        }

        async fn check_permissions(
            &self,
            _input: &Value,
            _ctx: &ToolExecutionContext,
        ) -> Option<PermissionDecision> {
            Some(PermissionDecision::Deny {
                message: "tool-level deny".to_string(),
                reason: PermissionReason::Other("test".into()),
            })
        }
    }

    let dispatcher = ToolDispatcher::new(Arc::new(AllowAllPermissionPipeline));
    dispatcher.register(Arc::new(AlwaysDenyTool));

    let ctx = ToolExecutionContext::for_test("c", "r", "t1");
    let result = dispatcher.dispatch("always_deny", json!({}), ctx).await;

    assert!(
        matches!(result, Err(ToolError::PermissionDenied(message)) if message == "tool-level deny"),
        "tool-level deny should override allow_all pipeline"
    );
}

#[tokio::test]
async fn tool_check_permissions_falls_through_to_pipeline_when_none() {
    use app_lib::runtime::tools::{
        AllowAllPermissionPipeline, RuntimeTool, ToolDispatchOutcome, ToolDispatcher, ToolError,
        ToolResult,
    };
    use async_trait::async_trait;
    use serde_json::Value;

    struct PassthroughTool;

    #[async_trait]
    impl RuntimeTool for PassthroughTool {
        fn definition(&self) -> ToolDefinition {
            ToolDefinition::new("passthrough", "passthrough")
        }

        async fn execute(
            &self,
            _input: Value,
            _ctx: ToolExecutionContext,
        ) -> Result<ToolResult, ToolError> {
            Ok(ToolResult::new("passthrough", "executed", None))
        }
    }

    let dispatcher = ToolDispatcher::new(Arc::new(AllowAllPermissionPipeline));
    dispatcher.register(Arc::new(PassthroughTool));

    let ctx = ToolExecutionContext::for_test("c", "r", "t1");
    let result = dispatcher.dispatch("passthrough", json!({}), ctx).await;

    assert!(
        matches!(result, Ok(ToolDispatchOutcome::Completed { .. })),
        "None from check_permissions should fall through to pipeline"
    );
}
