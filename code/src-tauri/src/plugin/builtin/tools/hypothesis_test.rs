//! hypothesis_test — statistical hypothesis testing.

use async_trait::async_trait;
use serde_json::{json, Value};

use crate::llm::tool_executor;
use crate::plugin::context::PluginContext;
use crate::plugin::tool_trait::{ToolError, ToolOutput, ToolPlugin};

pub struct HypothesisTestTool;

#[async_trait]
impl ToolPlugin for HypothesisTestTool {
    fn name(&self) -> &str { "hypothesis_test" }

    fn description(&self) -> &str {
        "Run a statistical hypothesis test on compensation data. Supports \
         t-test, ANOVA, chi-square, Mann-Whitney, and regression analysis."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "test_type": {
                    "type": "string",
                    "enum": ["t_test", "anova", "chi_square", "regression", "mann_whitney"]
                },
                "groups": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "Column names to compare"
                },
                "data_source": {
                    "type": "string",
                    "description": "File ID or inline data reference"
                },
                "significance_level": {
                    "type": "number",
                    "default": 0.05
                }
            },
            "required": ["test_type", "groups"]
        })
    }

    async fn execute(&self, ctx: &PluginContext, input: Value) -> Result<ToolOutput, ToolError> {
        match tool_executor::handle_hypothesis_test(ctx, &input).await {
            Ok(content) => Ok(ToolOutput::success(content)),
            Err(e) => Err(ToolError::Other(e)),
        }
    }
}
