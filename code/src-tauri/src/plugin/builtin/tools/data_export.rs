//! export_data — export processed data to CSV/Excel/JSON.

use async_trait::async_trait;
use serde_json::{json, Value};

use crate::llm::tool_executor::{self, ToolContext};
use crate::plugin::context::PluginContext;
use crate::plugin::tool_trait::{ToolError, ToolOutput, ToolPlugin};

pub struct DataExportTool;

#[async_trait]
impl ToolPlugin for DataExportTool {
    fn name(&self) -> &str { "export_data" }

    fn description(&self) -> &str {
        "Export processed data to a file format (CSV, Excel, JSON)."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "data": {
                    "type": "object",
                    "description": "Data to export"
                },
                "format": {
                    "type": "string",
                    "enum": ["csv", "excel", "json"]
                },
                "filename": {
                    "type": "string",
                    "description": "Output filename"
                }
            },
            "required": ["data", "format", "filename"]
        })
    }

    async fn execute(&self, ctx: &PluginContext, input: Value) -> Result<ToolOutput, ToolError> {
        let tool_ctx = ToolContext::from_plugin_context(ctx);
        match tool_executor::handle_export_data(&tool_ctx, &input).await {
            Ok(content) => Ok(ToolOutput::success(content)),
            Err(e) => Err(ToolError::Other(e)),
        }
    }
}
