//! search_memory — search cognitive memories by keywords.

use async_trait::async_trait;
use serde_json::{json, Value};

use crate::llm::tool_executor;
use crate::plugin::context::PluginContext;
use crate::plugin::tool_trait::{ToolError, ToolOutput, ToolPlugin};

pub struct MemorySearchTool;

#[async_trait]
impl ToolPlugin for MemorySearchTool {
    fn name(&self) -> &str { "search_memory" }

    fn description(&self) -> &str {
        "Search cognitive memory for previously saved knowledge. Use when you need to recall \
         user preferences, enterprise facts, or past analysis insights. Returns up to 10 \
         matching results ranked by relevance."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "query": {
                    "type": "string",
                    "description": "Search keywords (space-separated)"
                },
                "category": {
                    "type": "string",
                    "enum": ["preference", "fact", "learning", "pattern", "observation"],
                    "description": "Filter by category (optional)"
                },
                "days": {
                    "type": "integer",
                    "description": "Search the last N days (default: 30)"
                }
            },
            "required": ["query"]
        })
    }

    async fn execute(&self, ctx: &PluginContext, input: Value) -> Result<ToolOutput, ToolError> {
        match tool_executor::handle_search_memory(ctx, &input).await {
            Ok(content) => Ok(ToolOutput::success(content)),
            Err(e) => Err(ToolError::Other(e)),
        }
    }
}
