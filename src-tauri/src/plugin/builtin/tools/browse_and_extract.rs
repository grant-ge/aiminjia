//! browse_and_extract — navigate to a URL and extract all data in one operation.
//! Supports both page mode (HTML pages) and API mode (REST endpoints).

use async_trait::async_trait;
use serde_json::{json, Value};

use crate::llm::tool_executor;
use crate::plugin::context::PluginContext;
use crate::plugin::tool_trait::{ToolError, ToolOutput, ToolPlugin};

pub struct BrowseAndExtractTool;

#[async_trait]
impl ToolPlugin for BrowseAndExtractTool {
    fn name(&self) -> &str { "browse_and_extract" }

    fn description(&self) -> &str {
        "Navigate to a URL and extract all data in one operation. Much more efficient than \
         calling browse_navigate + read_page_content separately.\n\
         \n\
         For web pages: navigates, extracts tables, text, navigation links, forms, and \
         discovers API endpoints the page calls (XHR/fetch interception).\n\
         \n\
         For REST APIs: set method/body/headers to execute fetch() in the browser context \
         (automatically includes cookies/session). Returns parsed JSON or HTML table data.\n\
         \n\
         Use this as your primary tool for browsing and data extraction. Fall back to \
         page_execute_js only for complex interactions (multi-step clicks, form filling)."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "url": {
                    "type": "string",
                    "description": "Full URL to navigate to (page) or API endpoint URL (REST)"
                },
                "extract_script": {
                    "type": "string",
                    "description": "Optional custom JS extraction function (replaces default extractor)"
                },
                "method": {
                    "type": "string",
                    "description": "HTTP method for REST API mode (GET/POST/PUT/DELETE). Default: GET. Setting POST/PUT/DELETE forces API mode."
                },
                "body": {
                    "type": "string",
                    "description": "Request body as JSON string for POST/PUT requests"
                },
                "headers": {
                    "type": "string",
                    "description": "Extra request headers as JSON object string, e.g. {\"X-Custom\": \"value\"}"
                }
            },
            "required": ["url"]
        })
    }

    async fn execute(&self, ctx: &PluginContext, input: Value) -> Result<ToolOutput, ToolError> {
        match tool_executor::handle_browse_and_extract(ctx, &input).await {
            Ok(content) => Ok(ToolOutput::success(content)),
            Err(e) => Err(ToolError::Other(e)),
        }
    }
}
