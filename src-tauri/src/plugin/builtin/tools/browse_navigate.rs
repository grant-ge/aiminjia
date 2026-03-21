//! browse_navigate — open any URL in the CDP browser and wait for page load.

use async_trait::async_trait;
use serde_json::{json, Value};

use crate::llm::tool_executor;
use crate::plugin::context::PluginContext;
use crate::plugin::tool_trait::{ToolError, ToolOutput, ToolPlugin};

pub struct BrowseNavigateTool;

#[async_trait]
impl ToolPlugin for BrowseNavigateTool {
    fn name(&self) -> &str { "browse_navigate" }

    fn description(&self) -> &str {
        "Navigate the Chrome browser to any URL. The browser window becomes visible so you and \
         the user can both see the page. If the page redirects to a login page, tell the user \
         to log in in Chrome, then re-navigate. After the page loads, use read_page_content \
         to extract data or page_execute_js to interact."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "url": {
                    "type": "string",
                    "description": "Full URL to navigate to"
                }
            },
            "required": ["url"]
        })
    }

    async fn execute(&self, ctx: &PluginContext, input: Value) -> Result<ToolOutput, ToolError> {
        match tool_executor::handle_browse_navigate(ctx, &input).await {
            Ok(content) => Ok(ToolOutput::success(content)),
            Err(e) => Err(ToolError::Other(e)),
        }
    }
}
