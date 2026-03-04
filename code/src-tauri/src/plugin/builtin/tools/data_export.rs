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
         Alternative modes: (1) source_file — path to an existing file to convert format; \
         (2) data — actual JSON record array (NOT variable names like '_df')."
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
                    "description": "Data to export. MUST be an array of record objects, e.g. [{\"name\":\"A\",\"salary\":5000},{\"name\":\"B\",\"salary\":6000}]. Do NOT pass Python expressions or method names — use execute_python first to compute data, then pass actual JSON records here. Ignored when source_file is provided."
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
