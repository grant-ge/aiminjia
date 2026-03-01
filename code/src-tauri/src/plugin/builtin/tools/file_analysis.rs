//! analyze_file — parse and analyze uploaded files.

use async_trait::async_trait;
use serde_json::{json, Value};

use crate::llm::tool_executor::{self, ToolContext};
use crate::plugin::context::PluginContext;
use crate::plugin::tool_trait::{ToolError, ToolOutput, ToolPlugin};

pub struct FileAnalysisTool;

#[async_trait]
impl ToolPlugin for FileAnalysisTool {
    fn name(&self) -> &str { "analyze_file" }

    fn description(&self) -> &str {
        "Parse and analyze an uploaded file. Returns column names, \
         data types, row count, and sample data."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "file_id": {
                    "type": "string",
                    "description": "ID of the uploaded file"
                }
            },
            "required": ["file_id"]
        })
    }

    async fn execute(&self, ctx: &PluginContext, input: Value) -> Result<ToolOutput, ToolError> {
        let tool_ctx = ToolContext::from_plugin_context(ctx);
        match tool_executor::handle_analyze_file(&tool_ctx, &input).await {
            Ok(content) => Ok(ToolOutput::success(content)),
            Err(e) => Err(ToolError::Other(e)),
        }
    }
}
