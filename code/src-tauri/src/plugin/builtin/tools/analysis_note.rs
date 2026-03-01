//! save_analysis_note — store intermediate analysis findings.

use async_trait::async_trait;
use serde_json::{json, Value};

use crate::llm::tool_executor;
use crate::plugin::context::PluginContext;
use crate::plugin::tool_trait::{ToolError, ToolOutput, ToolPlugin};

pub struct AnalysisNoteTool;

#[async_trait]
impl ToolPlugin for AnalysisNoteTool {
    fn name(&self) -> &str { "save_analysis_note" }

    fn description(&self) -> &str {
        "Save an intermediate analysis finding or decision to the conversation \
         context. This helps maintain continuity across the 5-step analysis."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "key": {
                    "type": "string",
                    "description": "Note identifier (e.g. 'job_normalization_confirmed')"
                },
                "content": {
                    "type": "string",
                    "description": "The analysis note content"
                },
                "step": {
                    "type": "integer",
                    "description": "Which analysis step (1-5) this belongs to"
                }
            },
            "required": ["key", "content"]
        })
    }

    async fn execute(&self, ctx: &PluginContext, input: Value) -> Result<ToolOutput, ToolError> {
        match tool_executor::handle_save_analysis_note(ctx, &input).await {
            Ok(content) => Ok(ToolOutput::success(content)),
            Err(e) => Err(ToolError::Other(e)),
        }
    }
}
