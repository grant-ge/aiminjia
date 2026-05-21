//! McpRuntimeTool 不能自行绕过 ToolDispatcher 的 permission pipeline。
//! 通过 ToolDispatcher 调用 MCP 工具时，若 pipeline 返回 Deny，execute 不应被调用。

use std::sync::Arc;

use app_lib::runtime::tools::context::ToolExecutionContext;
use app_lib::runtime::tools::definition::ToolDefinition;
use app_lib::runtime::tools::description_context::ToolDescriptionContext;
use app_lib::runtime::tools::executor::{ToolError, ToolResult};
use app_lib::runtime::tools::permission::{
    PermissionDecision, PermissionPipeline, PermissionReason,
};
use app_lib::runtime::tools::{RuntimeTool, ToolDispatcher};
use async_trait::async_trait;
use serde_json::{json, Value};

struct AlwaysDenyPipeline;

impl PermissionPipeline for AlwaysDenyPipeline {
    fn authorize(
        &self,
        _: &ToolDefinition,
        _: &Value,
        _: &ToolExecutionContext,
    ) -> PermissionDecision {
        PermissionDecision::Deny {
            message: "deny_all_for_test".into(),
            reason: PermissionReason::Other("test".into()),
        }
    }
}

struct PanickingMcpTool;

#[async_trait]
impl RuntimeTool for PanickingMcpTool {
    fn id(&self) -> &str {
        "mcp__srv__panic_tool"
    }

    async fn definition(&self, _ctx: &ToolDescriptionContext) -> ToolDefinition {
        ToolDefinition::new("mcp__srv__panic_tool", "should not be called")
            .with_capability_scope(["mcp"])
    }

    async fn execute(&self, _: Value, _: ToolExecutionContext) -> Result<ToolResult, ToolError> {
        panic!("execute() must not be called when permission is denied");
    }
}

#[tokio::test]
async fn review_mcp_tool_pipeline_deny_prevents_execute() {
    let dispatcher = ToolDispatcher::new(Arc::new(AlwaysDenyPipeline));
    dispatcher.register(Arc::new(PanickingMcpTool));

    let ctx = ToolExecutionContext::for_test("conv", "run", "tc");
    let result = dispatcher
        .dispatch("mcp__srv__panic_tool", json!({}), ctx)
        .await;

    assert!(
        matches!(result, Err(ToolError::PermissionDenied(_))),
        "Denied MCP tool must return PermissionDenied, not execute"
    );
}
