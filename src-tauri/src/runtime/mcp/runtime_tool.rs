use async_trait::async_trait;
use serde_json::Value;

use crate::runtime::cancellation::wait_for_cancellation;
use crate::runtime::mcp::{McpToolDefinition, SharedMcpConnection};
use crate::runtime::tools::{
    RuntimeTool, ToolDefinition, ToolDescriptionContext, ToolError, ToolExecutionContext,
    ToolResult,
};

/// RuntimeTool adapter for one MCP server tool definition.
pub struct McpRuntimeTool {
    tool: McpToolDefinition,
    connection: SharedMcpConnection,
    /// Cached fully-qualified id (`mcp__<server>__<tool>`) so `RuntimeTool::id()`
    /// can return `&str` without per-call allocation.
    qualified_id: String,
}

impl McpRuntimeTool {
    pub fn new(tool: McpToolDefinition, connection: SharedMcpConnection) -> Self {
        let qualified_id = tool.qualified_name();
        Self {
            tool,
            connection,
            qualified_id,
        }
    }
}

#[async_trait]
impl RuntimeTool for McpRuntimeTool {
    fn id(&self) -> &str {
        &self.qualified_id
    }

    async fn definition(&self, _ctx: &ToolDescriptionContext) -> ToolDefinition {
        self.tool.to_tool_definition()
    }

    fn is_concurrency_safe(&self, _input: &Value) -> bool {
        false
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
                if let Err(err) = self.connection.disconnect_on_cancel().await {
                    log::warn!(
                        "[mcp] disconnect_on_cancel failed for server '{}': {err}",
                        self.connection.server_name()
                    );
                }
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

        Ok(ToolResult::new(
            self.qualified_id.clone(),
            content,
            Some(value),
        ))
    }
}
