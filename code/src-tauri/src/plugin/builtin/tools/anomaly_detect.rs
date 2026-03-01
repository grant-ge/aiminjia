//! detect_anomalies — outlier detection in compensation data.

use async_trait::async_trait;
use serde_json::{json, Value};

use crate::llm::tool_executor::{self, ToolContext};
use crate::plugin::context::PluginContext;
use crate::plugin::tool_trait::{ToolError, ToolOutput, ToolPlugin};

pub struct AnomalyDetectTool;

#[async_trait]
impl ToolPlugin for AnomalyDetectTool {
    fn name(&self) -> &str { "detect_anomalies" }

    fn description(&self) -> &str {
        "Detect outliers and anomalies in compensation data using statistical \
         methods (Z-score, IQR, or Grubbs test)."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "column": {
                    "type": "string",
                    "description": "Column name to analyze"
                },
                "method": {
                    "type": "string",
                    "enum": ["zscore", "iqr", "grubbs"],
                    "default": "zscore"
                },
                "threshold": {
                    "type": "number",
                    "description": "Detection threshold (default from settings)"
                },
                "group_by": {
                    "type": "string",
                    "description": "Optional grouping column"
                }
            },
            "required": ["column"]
        })
    }

    async fn execute(&self, ctx: &PluginContext, input: Value) -> Result<ToolOutput, ToolError> {
        let tool_ctx = ToolContext::from_plugin_context(ctx);
        match tool_executor::handle_detect_anomalies(&tool_ctx, &input).await {
            Ok(content) => Ok(ToolOutput::success(content)),
            Err(e) => Err(ToolError::Other(e)),
        }
    }
}
