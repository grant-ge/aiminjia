//! extract_all_pages — auto-paginate and extract ALL table data from the current page.
//! Handles pagination automatically (detects URL params, iterates pages, merges data).

use async_trait::async_trait;
use serde_json::{json, Value};

use crate::llm::tool_executor;
use crate::plugin::context::PluginContext;
use crate::plugin::tool_trait::{ToolError, ToolOutput, ToolPlugin};

pub struct ExtractAllPagesTool;

#[async_trait]
impl ToolPlugin for ExtractAllPagesTool {
    fn name(&self) -> &str { "extract_all_pages" }

    fn description(&self) -> &str {
        "Auto-paginate and extract ALL table data from the current browser page. \
         Automatically detects pagination (URL page param), iterates through all pages, \
         merges all table rows, and saves as a JSON file. Use this AFTER navigating to a \
         page that shows a data table with pagination. Returns the file path + row count.\n\
         \n\
         This is much faster than manually flipping pages — one call extracts everything."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "max_pages": {
                    "type": "integer",
                    "description": "Maximum pages to fetch (default 50)"
                },
                "page_size": {
                    "type": "integer",
                    "description": "Override pageSize parameter (default: auto-detect or 100)"
                }
            },
            "required": []
        })
    }

    async fn execute(&self, ctx: &PluginContext, input: Value) -> Result<ToolOutput, ToolError> {
        match tool_executor::handle_extract_all_pages(ctx, &input).await {
            Ok(content) => Ok(ToolOutput::success(content)),
            Err(e) => Err(ToolError::Other(e)),
        }
    }
}
