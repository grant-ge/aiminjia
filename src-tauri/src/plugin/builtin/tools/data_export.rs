//! export_data — export processed data to CSV/Excel/JSON.

use async_trait::async_trait;
use serde_json::{json, Value};

use crate::llm::tool_executor;
use crate::plugin::context::PluginContext;
use crate::plugin::tool_trait::{ToolError, ToolOutput, ToolPlugin};

pub struct DataExportTool;

#[async_trait]
impl ToolPlugin for DataExportTool {
    fn name(&self) -> &str { "export_data" }

    fn description(&self) -> &str {
        "Export data to CSV/Excel/JSON. RECOMMENDED: use _export_detail(df, filename, format) inside execute_python for DataFrame export. \
         Alternative: source_file — path to an existing file to convert format. \
         The 'data' parameter (inline JSON records) is DEPRECATED — use execute_python to write data to a file, then pass source_file."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "source_file": {
                    "type": "string",
                    "description": "Path to an existing file (CSV/Excel/JSON) to convert to a different format. Relative to workspace root (e.g. 'exports/step1_data.xlsx'). When provided, 'data' parameter is ignored."
                },
                "data": {
                    "description": "DEPRECATED — avoid using this parameter. Large inline JSON arrays risk token truncation and JSON escaping issues. \
                        Instead, use execute_python to write data to a file, then pass the file path via source_file. \
                        If you must use this: MUST be an array of record objects, e.g. [{\"name\":\"A\",\"salary\":5000}]. \
                        Do NOT pass Python expressions or method names."
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
            "required": ["format", "filename"]
        })
    }

    async fn execute(&self, ctx: &PluginContext, input: Value) -> Result<ToolOutput, ToolError> {
        match tool_executor::handle_export_data(ctx, &input).await {
            Ok(result) => Ok(result.into()),
            Err(e) => Err(ToolError::Other(e)),
        }
    }
}
