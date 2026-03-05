//! save_memory — persist a cognitive memory entry.

use async_trait::async_trait;
use serde_json::{json, Value};

use crate::llm::tool_executor;
use crate::plugin::context::PluginContext;
use crate::plugin::tool_trait::{ToolError, ToolOutput, ToolPlugin};

pub struct MemorySaveTool;

#[async_trait]
impl ToolPlugin for MemorySaveTool {
    fn name(&self) -> &str { "save_memory" }

    fn description(&self) -> &str {
        "Save a piece of knowledge to persistent cognitive memory. Use for user preferences, \
         enterprise facts, data patterns, and analytical observations that may be useful across \
         conversations. Do NOT save temporary or one-time facts."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "content": {
                    "type": "string",
                    "description": "The memory content — a concise statement (10-500 characters)"
                },
                "category": {
                    "type": "string",
                    "enum": ["preference", "fact", "learning", "pattern", "observation"],
                    "description": "Memory category: preference (user habits), fact (enterprise info), learning (data insights), pattern (analysis methods), observation (findings)"
                },
                "tags": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "2-5 search keywords for retrieval"
                },
                "to_core": {
                    "type": "boolean",
                    "description": "Write directly to core memory (only for critical preference/fact entries)"
                }
            },
            "required": ["content", "category"]
        })
    }

    async fn execute(&self, ctx: &PluginContext, input: Value) -> Result<ToolOutput, ToolError> {
        match tool_executor::handle_save_memory(ctx, &input).await {
            Ok(content) => Ok(ToolOutput::success(content)),
            Err(e) => Err(ToolError::Other(e)),
        }
    }
}
