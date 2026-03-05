//! update_plan — update the analysis plan for the current step.
//!
//! Maintains a `_plan.md` file in `analysis/{conversation_id}/` that
//! tracks sub-tasks and key findings within each analysis step. The LLM
//! reads the plan from dynamic context and updates it after each sub-task.

use async_trait::async_trait;
use serde_json::{json, Value};

use crate::plugin::context::PluginContext;
use crate::plugin::tool_trait::{ToolError, ToolOutput, ToolPlugin};

/// Maximum plan content size (chars) to prevent unbounded growth.
const MAX_PLAN_CHARS: usize = 4000;

pub struct PlanUpdateTool;

#[async_trait]
impl ToolPlugin for PlanUpdateTool {
    fn name(&self) -> &str { "update_plan" }

    fn description(&self) -> &str {
        "Update the analysis plan for the current step. Write a structured markdown plan \
         with completed items, pending items, and key findings. Use checkboxes: \
         [x] done, [ ] todo."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "plan_content": {
                    "type": "string",
                    "description": "Full plan content in markdown. Use checkboxes: [x] done, [ ] todo. Include key findings."
                }
            },
            "required": ["plan_content"]
        })
    }

    async fn execute(&self, ctx: &PluginContext, input: Value) -> Result<ToolOutput, ToolError> {
        let content = input["plan_content"].as_str().unwrap_or("").trim();
        if content.is_empty() {
            return Err(ToolError::InvalidArgument("plan_content cannot be empty".to_string()));
        }

        // Truncate to prevent unbounded growth
        let content = if content.len() > MAX_PLAN_CHARS {
            let boundary = content.floor_char_boundary(MAX_PLAN_CHARS);
            &content[..boundary]
        } else {
            content
        };

        let plan_dir = ctx.workspace_path
            .join("analysis")
            .join(&ctx.conversation_id);
        if let Err(e) = std::fs::create_dir_all(&plan_dir) {
            return Err(ToolError::ExecutionFailed(format!("Failed to create plan directory: {}", e)));
        }

        let plan_path = plan_dir.join("_plan.md");
        if let Err(e) = std::fs::write(&plan_path, content) {
            return Err(ToolError::ExecutionFailed(format!("Failed to write plan: {}", e)));
        }

        log::info!(
            "[update_plan] Saved plan ({} chars) for conversation {} at {:?}",
            content.len(), ctx.conversation_id, plan_path,
        );

        Ok(ToolOutput::success("Plan updated.".to_string()))
    }
}
