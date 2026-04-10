//! extract_with_pagination — extract ALL table data with LLM-provided pagination JS.
//! The LLM figures out how to flip pages, this tool handles the loop.

// This builtin tool implements the deprecated ToolPlugin trait.
// It is intentionally in the legacy zone and will be migrated to RuntimeTool.
#![allow(deprecated)]

use async_trait::async_trait;
use serde_json::{json, Value};

use crate::llm::tool_executor;
use crate::plugin::context::PluginContext;
use crate::plugin::tool_trait::{ToolError, ToolOutput, ToolPlugin};

pub struct ExtractWithPaginationTool;

#[async_trait]
impl ToolPlugin for ExtractWithPaginationTool {
    fn name(&self) -> &str {
        "extract_with_pagination"
    }

    fn description(&self) -> &str {
        "Extract ALL table data from the current page by automatically looping through pages. \
         Tries HTTP pagination first (fastest), falls back to page.goto with URL params. \
         No pagination_js needed — just call this after navigating to a data page. \
         Returns total rows + file path."
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
            "required": []
        })
    }

    async fn execute(&self, ctx: &PluginContext, input: Value) -> Result<ToolOutput, ToolError> {
        match tool_executor::handle_extract_with_pagination(ctx, &input).await {
            Ok(content) => Ok(ToolOutput::success(content)),
            Err(e) => Err(ToolError::Other(e)),
        }
    }
}
