//! skill_smith_dry_run — run the 6-check static plumbing report.

use async_trait::async_trait;
use serde_json::{json, Value};

use crate::llm::tool_executor;
use crate::plugin::context::PluginContext;
use crate::plugin::tool_trait::{ToolError, ToolOutput, ToolPlugin};

pub struct SkillSmithDryRunTool;

#[async_trait]
impl ToolPlugin for SkillSmithDryRunTool {
    fn name(&self) -> &str {
        "skill_smith_dry_run"
    }

    fn description(&self) -> &str {
        "Static dry-run of the active draft — runs 6 checks (schema, \
         prompts-reference, prompts-content, python-scripts, knowledge, \
         loadable) without invoking any LLM or Python. Returns a report \
         with `pass: bool` + per-check status (pass/fail/skip/warn). \
         Call this once the draft appears complete and before commit to \
         surface any remaining issues. Safe to call repeatedly; no side effects."
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
        match tool_executor::skill_smith::handle_skill_smith_dry_run(ctx, &input).await {
            Ok(content) => Ok(ToolOutput::success(content)),
            Err(e) => Err(ToolError::Other(e)),
        }
    }
}
