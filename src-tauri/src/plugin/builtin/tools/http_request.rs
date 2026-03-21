//! http_request — send HTTP requests to connected internal systems.

use async_trait::async_trait;
use serde_json::{json, Value};

use crate::llm::tool_executor;
use crate::plugin::context::PluginContext;
use crate::plugin::tool_trait::{ToolError, ToolOutput, ToolPlugin};

pub struct HttpRequestTool;

#[async_trait]
impl ToolPlugin for HttpRequestTool {
    fn name(&self) -> &str { "http_request" }

    fn description(&self) -> &str {
        "Send HTTP requests to connected internal systems. Cookies are injected automatically. \
         Use this to fetch data or explore APIs of enterprise internal systems."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "app_name": {
                    "type": "string",
                    "description": "Name of the internal system"
                },
                "method": {
                    "type": "string",
                    "enum": ["GET", "POST", "PUT", "DELETE"],
                    "description": "HTTP method"
                },
                "url": {
                    "type": "string",
                    "description": "Full URL or relative path (base_url is auto-prepended for relative paths)"
                },
                "headers": {
                    "type": "object",
                    "description": "Extra request headers (optional)"
                },
                "body": {
                    "type": "object",
                    "description": "Request body (optional, for POST/PUT)"
                }
            },
            "required": ["app_name", "method", "url"]
        })
    }

    async fn execute(&self, ctx: &PluginContext, input: Value) -> Result<ToolOutput, ToolError> {
        match tool_executor::handle_http_request(ctx, &input).await {
            Ok(content) => Ok(ToolOutput::success(content)),
            Err(e) => Err(ToolError::Other(e)),
        }
    }
}
