//! skill_smith_create_draft — create/reuse a draft bound to the conversation.

use async_trait::async_trait;
use serde_json::{json, Value};

use crate::llm::tool_executor;
use crate::plugin::context::PluginContext;
use crate::plugin::tool_trait::{ToolError, ToolOutput, ToolPlugin};

pub struct SkillSmithCreateDraftTool;

#[async_trait]
impl ToolPlugin for SkillSmithCreateDraftTool {
    fn name(&self) -> &str {
        "skill_smith_create_draft"
    }

    fn description(&self) -> &str {
        "Create a new skill draft in the conversational skill-smith flow. \
         Returns the draft_id (12-char hex) to pass to subsequent write / \
         validate / dry-run / commit calls. Idempotent: if this conversation \
         already has a bound draft, returns the existing draft_id unless \
         force_new=true."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "force_new": {
                    "type": "boolean",
                    "description": "If true, orphan any existing bound draft and create a fresh one. Default false (idempotent reuse).",
                    "default": false
                }
            },
            "required": []
        })
    }

    async fn execute(&self, ctx: &PluginContext, input: Value) -> Result<ToolOutput, ToolError> {
        match tool_executor::skill_smith::handle_skill_smith_create_draft(ctx, &input).await {
            Ok(content) => Ok(ToolOutput::success(content)),
            Err(e) => Err(ToolError::Other(e)),
        }
    }
}
