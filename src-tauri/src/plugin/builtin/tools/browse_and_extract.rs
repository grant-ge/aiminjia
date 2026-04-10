//! browse_and_extract — navigate to a URL and extract all data in one operation.
//! Supports both page mode (HTML pages) and API mode (REST endpoints).

// This builtin tool implements the deprecated ToolPlugin trait.
// It is intentionally in the legacy zone and will be migrated to RuntimeTool.
#![allow(deprecated)]

use async_trait::async_trait;
use serde_json::{json, Value};

use crate::llm::tool_executor;
use crate::plugin::context::PluginContext;
use crate::plugin::tool_trait::{ToolError, ToolOutput, ToolPlugin};

pub struct BrowseAndExtractTool;

#[async_trait]
impl ToolPlugin for BrowseAndExtractTool {
    fn name(&self) -> &str {
        "browse_and_extract"
    }

    fn description(&self) -> &str {
        "Navigate to a URL and extract all data in one step. For pages: extracts tables, text, \
         links, forms. For REST APIs: set method/body to execute fetch() with cookies. \
         Primary browsing tool — use instead of separate browse_navigate + read_page_content."
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
