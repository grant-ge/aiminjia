//! skill_smith_install — install the active draft into ~/.renlijia/skills.

use async_trait::async_trait;
use serde_json::{json, Value};

use crate::llm::tool_executor;
use crate::plugin::context::PluginContext;
use crate::plugin::tool_trait::{ToolError, ToolOutput, ToolPlugin};

pub struct SkillSmithInstallTool;

#[async_trait]
impl ToolPlugin for SkillSmithInstallTool {
    fn name(&self) -> &str {
        "skill_smith_install"
    }

    fn description(&self) -> &str {
        "Install the active skill draft into ~/.renlijia/skills/ and trigger \
         hot-reload (skill is usable immediately, no app restart needed). \
         If a skill with the same id is already installed, returns \
         `status: \"conflict\"` without changes — ask the user whether to \
         overwrite (re-invoke with force=true) or rename (write a new \
         plugin.toml with a different id and retry). On success the draft \
         is cleaned up."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "draft_id": {
                    "type": "string",
                    "description": "Optional — defaults to the draft bound to this conversation."
                },
                "force": {
                    "type": "boolean",
                    "description": "If true, overwrite any existing skill with the same id. Default false.",
                    "default": false
                }
            },
            "required": []
        })
    }

    async fn execute(&self, ctx: &PluginContext, input: Value) -> Result<ToolOutput, ToolError> {
        match tool_executor::skill_smith::handle_skill_smith_install(ctx, &input).await {
            Ok(content) => Ok(ToolOutput::success(content)),
            Err(e) => Err(ToolError::Other(e)),
        }
    }
}
