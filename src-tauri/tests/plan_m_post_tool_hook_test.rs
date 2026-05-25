use std::sync::Arc;

use async_trait::async_trait;
use serde_json::{json, Value};

use app_lib::runtime::hooks::config::{HookConfig, HookEvent, HookRegistry};
use app_lib::runtime::tools::description_context::ToolDescriptionContext;
use app_lib::runtime::tools::dispatcher::{RuntimeTool, ToolDispatchOutcome};
use app_lib::runtime::tools::{
    AllowAllPermissionPipeline, ToolDefinition, ToolDispatcher, ToolError, ToolExecutionContext,
    ToolResult,
};

struct OkTool {
    name: String,
}

#[async_trait]
impl RuntimeTool for OkTool {
    fn id(&self) -> &str {
        &self.name
    }

    async fn definition(&self, _ctx: &ToolDescriptionContext) -> ToolDefinition {
        ToolDefinition::new(&self.name, "always ok")
    }

    async fn execute(
        &self,
        _input: Value,
        _ctx: ToolExecutionContext,
    ) -> Result<ToolResult, ToolError> {
        Ok(ToolResult::new(&self.name, "success output", None))
    }
}

#[tokio::test]
async fn post_tool_hook_executes_after_success() {
    let tmp_path = std::env::temp_dir().join("plan_m_post_hook_ran.txt");
    let _ = std::fs::remove_file(&tmp_path);
    let tmp_str = tmp_path.to_str().unwrap().to_string();

    let tool = Arc::new(OkTool {
        name: "bash_tool".to_string(),
    });
    let dispatcher = ToolDispatcher::new(Arc::new(AllowAllPermissionPipeline));
    dispatcher.register(tool);

    let mut registry = HookRegistry::new();
    registry.hooks.push(HookConfig {
        event: HookEvent::PostToolUse,
        command: format!("touch {} && echo '{{\"behavior\":\"allow\"}}'", tmp_str),
        tool_filter: None,
        timeout_secs: Some(10),
    });

    let ctx = ToolExecutionContext::for_test("conv-1", "run-1", "tc-1")
        .with_hook_registry(Arc::new(registry));

    let result = dispatcher.dispatch("bash_tool", json!({}), ctx).await;
    assert!(result.is_ok());
    assert!(tmp_path.exists());
    let _ = std::fs::remove_file(&tmp_path);
}

#[tokio::test]
async fn post_tool_hook_prevent_continuation_surfaced() {
    let tool = Arc::new(OkTool {
        name: "bash_tool".to_string(),
    });
    let dispatcher = ToolDispatcher::new(Arc::new(AllowAllPermissionPipeline));
    dispatcher.register(tool);

    let mut registry = HookRegistry::new();
    registry.hooks.push(HookConfig {
        event: HookEvent::PostToolUse,
        command: "echo '{\"behavior\":\"allow\",\"preventContinuation\":true,\"stopReason\":\"task done\"}'"
            .to_string(),
        tool_filter: None,
        timeout_secs: Some(10),
    });

    let ctx = ToolExecutionContext::for_test("conv-1", "run-1", "tc-1")
        .with_hook_registry(Arc::new(registry));

    let outcome = dispatcher
        .dispatch("bash_tool", json!({}), ctx)
        .await
        .unwrap();
    match outcome {
        ToolDispatchOutcome::Completed {
            prevent_continuation,
            stop_reason,
            ..
        } => {
            assert!(prevent_continuation);
            assert_eq!(stop_reason.as_deref(), Some("task done"));
        }
        _ => panic!("expected Completed outcome"),
    }
}

#[tokio::test]
async fn no_post_hook_no_prevent_continuation() {
    let tool = Arc::new(OkTool {
        name: "bash_tool".to_string(),
    });
    let dispatcher = ToolDispatcher::new(Arc::new(AllowAllPermissionPipeline));
    dispatcher.register(tool);

    let ctx = ToolExecutionContext::for_test("conv-1", "run-1", "tc-1");

    let outcome = dispatcher
        .dispatch("bash_tool", json!({}), ctx)
        .await
        .unwrap();
    match outcome {
        ToolDispatchOutcome::Completed {
            prevent_continuation,
            ..
        } => {
            assert!(!prevent_continuation);
        }
        _ => panic!("expected Completed"),
    }
}
