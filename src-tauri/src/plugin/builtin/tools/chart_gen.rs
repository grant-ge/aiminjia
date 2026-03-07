//! generate_chart — create interactive data visualization charts via Plotly (HTML).

use async_trait::async_trait;
use serde_json::{json, Value};

use crate::llm::tool_executor;
use crate::plugin::context::PluginContext;
use crate::plugin::tool_trait::{ToolError, ToolOutput, ToolPlugin};

pub struct ChartGenTool;

#[async_trait]
impl ToolPlugin for ChartGenTool {
    fn name(&self) -> &str { "generate_chart" }

    fn description(&self) -> &str {
        "Generate an interactive data visualization chart (Plotly HTML). \
         Supports bar, line, scatter, box, heatmap, pie, and histogram types."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "chart_type": {
                    "type": "string",
                    "enum": ["bar", "line", "scatter", "box", "heatmap", "pie", "histogram"]
                },
                "title": { "type": "string" },
                "data_file": {
                    "type": "string",
                    "description": "RECOMMENDED: Path to a JSON file containing chart data. \
                        Use execute_python to prepare data and write to a JSON file, \
                        then pass the file path here. This avoids large tool arguments. \
                        When provided, 'data' parameter is ignored."
                },
                "data": {
                    "type": "object",
                    "description": "Chart data with labels and values (inline). Prefer using 'data_file' for large datasets."
                },
                "options": {
                    "type": "object",
                    "description": "Additional chart configuration (width, height, bins, etc.)"
                }
            },
            "required": ["chart_type", "title"]
        })
    }

    async fn execute(&self, ctx: &PluginContext, input: Value) -> Result<ToolOutput, ToolError> {
        match tool_executor::handle_generate_chart(ctx, &input).await {
            Ok(result) => Ok(result.into()),
            Err(e) => Err(ToolError::Other(e)),
        }
    }
}
