//! skill_smith_read_file — read a file from the active draft.

use async_trait::async_trait;
use serde_json::{json, Value};

use crate::llm::tool_executor;
use crate::plugin::context::PluginContext;
use crate::plugin::tool_trait::{ToolError, ToolOutput, ToolPlugin};

pub struct SkillSmithReadFileTool;

#[async_trait]
impl ToolPlugin for SkillSmithReadFileTool {
    fn name(&self) -> &str {
        "skill_smith_read_file"
    }

    fn description(&self) -> &str {
        "Read a file from the active skill draft. Use this to review previously \
         generated files (e.g. 'plugin.toml', 'workflow.toml', 'prompts/step0.md') \
         so you can reference their content when generating subsequent files. \
         Returns the file content as UTF-8 text."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "draft_id": {
                    "type": "string",
                    "description": "12-char hex draft_id. Optional if a draft is already bound to this conversation."
                },
                "relative_path": {
                    "type": "string",
                    "description": "Path relative to draft root. e.g. 'plugin.toml', 'workflow.toml', 'prompts/step0.md'. Cannot start with '/' or contain '..'."
                }
            },
            "required": ["relative_path"]
        })
    }

    async fn execute(&self, ctx: &PluginContext, input: Value) -> Result<ToolOutput, ToolError> {
        match tool_executor::skill_smith::handle_skill_smith_read_file(ctx, &input).await {
            Ok(content) => Ok(ToolOutput::success(content)),
            Err(e) => Err(ToolError::Other(e)),
        }
    }
}
