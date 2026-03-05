//! load_core_memory — read the always-loaded core memory.

use async_trait::async_trait;
use serde_json::{json, Value};

use crate::llm::tool_executor;
use crate::plugin::context::PluginContext;
use crate::plugin::tool_trait::{ToolError, ToolOutput, ToolPlugin};

pub struct CoreMemoryTool;

#[async_trait]
impl ToolPlugin for CoreMemoryTool {
    fn name(&self) -> &str { "load_core_memory" }

    fn description(&self) -> &str {
        "Load the full core memory (mem.md). Core memory is automatically injected into context, \
         but use this tool to explicitly refresh or verify the latest version."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {}
        })
    }

    async fn execute(&self, ctx: &PluginContext, input: Value) -> Result<ToolOutput, ToolError> {
        match tool_executor::handle_load_core_memory(ctx, &input).await {
            Ok(content) => Ok(ToolOutput::success(content)),
            Err(e) => Err(ToolError::Other(e)),
        }
    }
}
