//! skill_smith_export — pack the active draft into a .aijia-skill zip.

use async_trait::async_trait;
use serde_json::{json, Value};

use crate::llm::tool_executor;
use crate::plugin::context::PluginContext;
use crate::plugin::tool_trait::{ToolError, ToolOutput, ToolPlugin};

pub struct SkillSmithExportTool;

#[async_trait]
impl ToolPlugin for SkillSmithExportTool {
    fn name(&self) -> &str {
        "skill_smith_export"
    }

    fn description(&self) -> &str {
        "Package the active skill draft into a `.aijia-skill` zip file \
         (shareable; recipients install via Settings → Skills → Install from \
         folder). Draft is preserved after export — you can still commit or \
         re-export. If `output_dir` is not provided, defaults to the user's \
         workspace `exports/` subfolder."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "draft_id": {
                    "type": "string",
                    "description": "Optional — defaults to the draft bound to this conversation."
                },
                "output_dir": {
                    "type": "string",
                    "description": "Absolute directory path to write the .aijia-skill into. If omitted, defaults to {workspace}/exports/."
                }
            },
            "required": []
        })
    }

    async fn execute(&self, ctx: &PluginContext, input: Value) -> Result<ToolOutput, ToolError> {
        match tool_executor::skill_smith::handle_skill_smith_export(ctx, &input).await {
            Ok(content) => Ok(ToolOutput::success(content)),
            Err(e) => Err(ToolError::Other(e)),
        }
    }
}
