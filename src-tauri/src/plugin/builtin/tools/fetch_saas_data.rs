//! fetch_saas_data — fetch data from connected enterprise SaaS systems.

use async_trait::async_trait;
use serde_json::{json, Value};

use crate::llm::tool_executor;
use crate::plugin::context::PluginContext;
use crate::plugin::tool_trait::{ToolError, ToolOutput, ToolPlugin};

pub struct FetchSaasDataTool;

#[async_trait]
impl ToolPlugin for FetchSaasDataTool {
    fn name(&self) -> &str { "fetch_saas_data" }

    fn description(&self) -> &str {
        "Fetch data from connected enterprise SaaS systems. \
         Use 'get_sample' to see the API template first, then 'execute' to make the actual request. \
         Use 'set_credential' if the user provides an API key or token in the conversation."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "action": {
                    "type": "string",
                    "enum": ["get_sample", "execute", "set_credential"],
                    "description": "get_sample: view API template. execute: make the API call. set_credential: save API key/token provided by user."
                },
                "app_name": {
                    "type": "string",
                    "description": "Name of the SaaS application"
                },
                "capability_name": {
                    "type": "string",
                    "description": "Name of the API capability (not needed for set_credential)"
                },
                "parameters": {
                    "type": "object",
                    "description": "Parameters to fill in the request template (only for execute action)"
                },
                "api_credential": {
                    "type": "string",
                    "description": "API key or token provided by the user (only for set_credential action)"
                }
            },
            "required": ["action", "app_name"]
        })
    }

    async fn execute(&self, ctx: &PluginContext, input: Value) -> Result<ToolOutput, ToolError> {
        match tool_executor::handle_fetch_saas_data(ctx, &input).await {
            Ok(content) => Ok(ToolOutput::success(content)),
            Err(e) => Err(ToolError::Other(e)),
        }
    }
}
