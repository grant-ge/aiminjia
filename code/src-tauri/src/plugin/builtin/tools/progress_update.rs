//! update_progress — update analysis step progress indicator.

use async_trait::async_trait;
use serde_json::{json, Value};

use crate::llm::tool_executor::{self, ToolContext};
use crate::plugin::context::PluginContext;
use crate::plugin::tool_trait::{ToolError, ToolOutput, ToolPlugin};

pub struct ProgressUpdateTool;

#[async_trait]
impl ToolPlugin for ProgressUpdateTool {
    fn name(&self) -> &str { "update_progress" }

    fn description(&self) -> &str {
        "Update the analysis progress indicator. Call this when transitioning \
         between analysis steps."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "current_step": {
                    "type": "integer",
                    "minimum": 1,
                    "maximum": 5
                },
                "step_status": {
                    "type": "string",
                    "enum": ["active", "completed", "error"]
                },
                "summary": {
                    "type": "string",
                    "description": "Brief summary of step progress"
                }
            },
            "required": ["current_step", "step_status"]
        })
    }

    async fn execute(&self, ctx: &PluginContext, input: Value) -> Result<ToolOutput, ToolError> {
        let tool_ctx = ToolContext::from_plugin_context(ctx);
        match tool_executor::handle_update_progress(&tool_ctx, &input).await {
            Ok(content) => Ok(ToolOutput::success(content)),
            Err(e) => Err(ToolError::Other(e)),
        }
    }
}
