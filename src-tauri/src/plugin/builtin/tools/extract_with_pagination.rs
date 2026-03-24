//! extract_with_pagination — extract ALL table data with LLM-provided pagination JS.
//! The LLM figures out how to flip pages, this tool handles the loop.

use async_trait::async_trait;
use serde_json::{json, Value};

use crate::llm::tool_executor;
use crate::plugin::context::PluginContext;
use crate::plugin::tool_trait::{ToolError, ToolOutput, ToolPlugin};

pub struct ExtractWithPaginationTool;

#[async_trait]
impl ToolPlugin for ExtractWithPaginationTool {
    fn name(&self) -> &str { "extract_with_pagination" }

    fn description(&self) -> &str {
        "Extract ALL table data from the current page by automatically looping through pages. \
         You provide the JavaScript code to click the next-page button, this tool handles the loop: \
         extract → run your JS → wait → extract → repeat until no more data. \
         Returns total rows + file path. Use AFTER navigating to a data page."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "pagination_js": {
                    "type": "string",
                    "description": "JavaScript code to click the next-page button. Example: document.querySelector('.layui-laypage-next').click()"
                },
                "max_pages": {
                    "type": "integer",
                    "description": "Maximum pages to extract (default 100)"
                }
            },
            "required": ["pagination_js"]
        })
    }

    async fn execute(&self, ctx: &PluginContext, input: Value) -> Result<ToolOutput, ToolError> {
        match tool_executor::handle_extract_with_pagination(ctx, &input).await {
            Ok(content) => Ok(ToolOutput::success(content)),
            Err(e) => Err(ToolError::Other(e)),
        }
    }
}
