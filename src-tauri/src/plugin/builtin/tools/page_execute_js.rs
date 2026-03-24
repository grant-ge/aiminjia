//! page_execute_js — execute JavaScript on the active page in the CDP browser.

use async_trait::async_trait;
use serde_json::{json, Value};

use crate::llm::tool_executor;
use crate::plugin::context::PluginContext;
use crate::plugin::tool_trait::{ToolError, ToolOutput, ToolPlugin};

pub struct PageExecuteJsTool;

#[async_trait]
impl ToolPlugin for PageExecuteJsTool {
    fn name(&self) -> &str { "page_execute_js" }

    fn description(&self) -> &str {
        "Execute JavaScript on the active page. Use for: clicking buttons, filling forms, \
         flipping pages (click next-page button), scrolling. Do NOT use this to extract \
         table data — use extract_table_data instead."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "script": {
                    "type": "string",
                    "description": "JavaScript code to execute on the page. Can be async. Return value will be captured."
                }
            },
            "required": ["script"]
        })
    }

    async fn execute(&self, ctx: &PluginContext, input: Value) -> Result<ToolOutput, ToolError> {
        match tool_executor::handle_page_execute_js(ctx, &input).await {
            Ok(content) => Ok(ToolOutput::success(content)),
            Err(e) => Err(ToolError::Other(e)),
        }
    }
}
