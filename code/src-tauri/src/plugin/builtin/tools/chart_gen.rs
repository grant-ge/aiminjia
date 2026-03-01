//! generate_chart — create data visualization charts via matplotlib.

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
        "Generate a data visualization chart. Supports bar, line, scatter, \
         box, and heatmap types."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "chart_type": {
                    "type": "string",
                    "enum": ["bar", "line", "scatter", "box", "heatmap"]
                },
                "title": { "type": "string" },
                "data": {
                    "type": "object",
                    "description": "Chart data with labels and values"
                },
                "options": {
                    "type": "object",
                    "description": "Additional chart configuration"
                }
            },
            "required": ["chart_type", "title", "data"]
        })
    }

    async fn execute(&self, ctx: &PluginContext, input: Value) -> Result<ToolOutput, ToolError> {
        match tool_executor::handle_generate_chart(ctx, &input).await {
            Ok(result) => Ok(result.into()),
            Err(e) => Err(ToolError::Other(e)),
        }
    }
}
