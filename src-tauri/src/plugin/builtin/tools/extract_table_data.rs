//! extract_table_data — extract table data from current page + report pagination info.
//! Does NOT auto-paginate. LLM decides how to flip pages using page_execute_js.

// This builtin tool implements the deprecated ToolPlugin trait.
// It is intentionally in the legacy zone and will be migrated to RuntimeTool.
#![allow(deprecated)]

use async_trait::async_trait;
use serde_json::{json, Value};

use crate::llm::tool_executor;
use crate::plugin::context::PluginContext;
use crate::plugin::tool_trait::{ToolError, ToolOutput, ToolPlugin};

pub struct ExtractTableDataTool;

#[async_trait]
impl ToolPlugin for ExtractTableDataTool {
    fn name(&self) -> &str {
        "extract_table_data"
    }

    fn description(&self) -> &str {
        "Extract table data from the current page and save to JSON file. \
         This is the ONLY correct way to extract table data — do NOT use page_execute_js \
         to read table rows. Returns: rows extracted, column names, pagination info \
         (total records, current page, has next page). If has_next=true, use page_execute_js \
         to click the next-page button, then call extract_table_data again to append more rows."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {},
            "required": []
        })
    }

    async fn execute(&self, ctx: &PluginContext, input: Value) -> Result<ToolOutput, ToolError> {
        match tool_executor::handle_extract_table_data(ctx, &input).await {
            Ok(content) => Ok(ToolOutput::success(content)),
            Err(e) => Err(ToolError::Other(e)),
        }
    }
}
