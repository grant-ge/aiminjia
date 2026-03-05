//! distill_memories — promote high-hit memories to core, apply decay.

use async_trait::async_trait;
use serde_json::{json, Value};

use crate::llm::tool_executor;
use crate::plugin::context::PluginContext;
use crate::plugin::tool_trait::{ToolError, ToolOutput, ToolPlugin};

pub struct MemoryDistillTool;

#[async_trait]
impl ToolPlugin for MemoryDistillTool {
    fn name(&self) -> &str { "distill_memories" }

    fn description(&self) -> &str {
        "Distill daily memories: promote frequently-hit entries to core memory, apply decay \
         to stale entries, and archive old daily files. Run periodically to maintain \
         memory quality."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "days": {
                    "type": "integer",
                    "description": "Review the last N days (default: 7)"
                },
                "dry_run": {
                    "type": "boolean",
                    "description": "Preview only — don't actually modify anything"
                }
            }
        })
    }

    async fn execute(&self, ctx: &PluginContext, input: Value) -> Result<ToolOutput, ToolError> {
        match tool_executor::handle_distill_memories(ctx, &input).await {
            Ok(content) => Ok(ToolOutput::success(content)),
            Err(e) => Err(ToolError::Other(e)),
        }
    }
}
