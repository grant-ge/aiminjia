//! save_api_knowledge — persist validated API call patterns for future reuse.

use async_trait::async_trait;
use serde_json::{json, Value};

use crate::llm::tool_executor;
use crate::plugin::context::PluginContext;
use crate::plugin::tool_trait::{ToolError, ToolOutput, ToolPlugin};

pub struct SaveApiKnowledgeTool;

#[async_trait]
impl ToolPlugin for SaveApiKnowledgeTool {
    fn name(&self) -> &str { "save_api_knowledge" }

    fn description(&self) -> &str {
        "Save a successfully verified API call pattern so it can be reused next time. \
         Call this after successfully fetching data from an internal system."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "app_name": {
                    "type": "string",
                    "description": "Name of the internal system"
                },
                "name": {
                    "type": "string",
                    "description": "Descriptive name for this API pattern (e.g., 'Get employee list')"
                },
                "method": {
                    "type": "string",
                    "description": "HTTP method (GET, POST, etc.)"
                },
                "path": {
                    "type": "string",
                    "description": "API path (relative to base_url)"
                },
                "params_doc": {
                    "type": "string",
                    "description": "Documentation for parameters"
                },
                "response_doc": {
                    "type": "string",
                    "description": "Documentation for response structure"
                },
                "notes": {
                    "type": "string",
                    "description": "Additional notes or caveats"
                }
            },
            "required": ["app_name", "name", "method", "path"]
        })
    }

    async fn execute(&self, ctx: &PluginContext, input: Value) -> Result<ToolOutput, ToolError> {
        match tool_executor::handle_save_api_knowledge(ctx, &input).await {
            Ok(content) => Ok(ToolOutput::success(content)),
            Err(e) => Err(ToolError::Other(e)),
        }
    }
}
