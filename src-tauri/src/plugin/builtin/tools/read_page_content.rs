//! read_page_content — extract structured data from the active page in the CDP browser.

// This builtin tool implements the deprecated ToolPlugin trait.
// It is intentionally in the legacy zone and will be migrated to RuntimeTool.
#![allow(deprecated)]

use async_trait::async_trait;
use serde_json::{json, Value};

use crate::llm::tool_executor;
use crate::plugin::context::PluginContext;
use crate::plugin::tool_trait::{ToolError, ToolOutput, ToolPlugin};

pub struct ReadPageContentTool;

#[async_trait]
impl ToolPlugin for ReadPageContentTool {
    fn name(&self) -> &str {
        "read_page_content"
    }

    fn description(&self) -> &str {
        "Read and extract structured data from the active page in the Chrome browser. \
         Returns tables and text content from the page DOM. \
         Use after browse_navigate or after the user has manually interacted with the browser window."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "extract_script": {
                    "type": "string",
                    "description": "Custom JS extraction script (optional). Must define a function __aijia_extract() that returns {url, title, tables, text}. Default script extracts all tables and main text."
                }
            },
            "required": []
        })
    }

    async fn execute(&self, ctx: &PluginContext, input: Value) -> Result<ToolOutput, ToolError> {
        match tool_executor::handle_read_page_content(ctx, &input).await {
            Ok(content) => Ok(ToolOutput::success(content)),
            Err(e) => Err(ToolError::Other(e)),
        }
    }
}
