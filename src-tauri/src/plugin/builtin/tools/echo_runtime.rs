use anyhow::Result;
use async_trait::async_trait;
use serde_json::Value;

use crate::runtime::tools::{
    RuntimeTool, ToolDefinition, ToolError, ToolExecutionContext, ToolResult,
};

#[derive(Default)]
pub struct EchoRuntimeTool;

#[async_trait]
impl RuntimeTool for EchoRuntimeTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition::new("echo_runtime", "Echo runtime migration sample")
    }

    async fn execute(
        &self,
        input: Value,
        _ctx: ToolExecutionContext,
    ) -> Result<ToolResult, ToolError> {
        let text = input
            .get("text")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        Ok(ToolResult {
            tool_name: "echo_runtime".to_string(),
            content: text,
            data: None,
            file_meta: None,
            is_degraded: false,
            degradation_notice: None,
        })
    }
}
