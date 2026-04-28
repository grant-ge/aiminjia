//! skill_smith_validate — run schema validation on the active draft.

use async_trait::async_trait;
use serde_json::{json, Value};

use crate::llm::tool_executor;
use crate::plugin::context::PluginContext;
use crate::plugin::tool_trait::{ToolError, ToolOutput, ToolPlugin};

pub struct SkillSmithValidateTool;

#[async_trait]
impl ToolPlugin for SkillSmithValidateTool {
    fn name(&self) -> &str {
        "skill_smith_validate"
    }

    fn description(&self) -> &str {
        "Validate the active skill draft against the SKILL.md schema (SKILL.md \
         frontmatter + scripts/ + references/ + assets/). Returns a structured \
         report. When errors are present, each entry includes `path`, `rule`, \
         `actual`, `message`, and an optional `fix_hint` you can use to correct \
         the offending field and retry. `warnings` are non-blocking. Call this \
         after every skill_smith_write_file that touches a schema-governed file."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "draft_id": {
                    "type": "string",
                    "description": "Optional — defaults to the draft bound to this conversation."
                }
            },
            "required": []
        })
    }

    async fn execute(&self, ctx: &PluginContext, input: Value) -> Result<ToolOutput, ToolError> {
        match tool_executor::skill_smith::handle_skill_smith_validate(ctx, &input).await {
            Ok(content) => Ok(ToolOutput::success(content)),
            Err(e) => Err(ToolError::Other(e)),
        }
    }
}
