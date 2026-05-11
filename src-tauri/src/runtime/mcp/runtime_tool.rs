use async_trait::async_trait;
use serde_json::Value;

use crate::runtime::mcp::{McpToolDefinition, SharedMcpConnection};
use crate::runtime::tools::{
    RuntimeTool, ToolDefinition, ToolError, ToolExecutionContext, ToolResult,
};
use crate::runtime::tools::builtin::shell_common::wait_for_cancellation;

/// RuntimeTool adapter for one MCP server tool definition.
pub struct McpRuntimeTool {
    tool: McpToolDefinition,
    connection: SharedMcpConnection,
}

impl McpRuntimeTool {
    pub fn new(tool: McpToolDefinition, connection: SharedMcpConnection) -> Self {
        Self { tool, connection }
    }
}

#[async_trait]
impl RuntimeTool for McpRuntimeTool {
    fn definition(&self) -> ToolDefinition {
        self.tool.to_tool_definition()
    }

    fn is_concurrency_safe(&self, _input: &Value) -> bool {
        false
    }

    fn is_read_only(&self, _input: &Value) -> bool {
        self.definition().default_read_only
    }

    fn is_destructive(&self, _input: &Value) -> bool {
        self.definition().default_destructive
    }

    async fn execute(
        &self,
        input: Value,
        ctx: ToolExecutionContext,
    ) -> Result<ToolResult, ToolError> {
        if !self.connection.is_connected() {
            return Err(ToolError::ExecutionFailed(format!(
                "MCP server '{}' is not connected",
                self.connection.server_name()
            )));
        }

        // Dispatcher permission/catalog use the fully-qualified id, but the remote
        // server must receive the original MCP tool name.
        let call_fut = self.connection.call_tool(&self.tool.tool_name, input);
        let cancel = ctx.cancellation.clone();

        let result = tokio::select! {
            biased;
            _ = wait_for_cancellation(cancel) => {
                let _ = self.connection.disconnect_on_cancel().await;
                return Err(ToolError::ExecutionFailed(format!(
                    "MCP tool '{}' cancelled before completion",
                    self.tool.tool_name
                )));
            }
            r = call_fut => r,
        };

        let value = result.map_err(|err| ToolError::ExecutionFailed(err.to_string()))?;

        let content = value.as_str().map(ToOwned::to_owned).unwrap_or_else(|| {
            serde_json::to_string_pretty(&value).unwrap_or_else(|_| value.to_string())
        });

        Ok(ToolResult::new(self.definition().id, content, Some(value)))
    }
}
