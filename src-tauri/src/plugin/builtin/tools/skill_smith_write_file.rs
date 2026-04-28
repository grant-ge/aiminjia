//! skill_smith_write_file — write a file inside the active draft.

use async_trait::async_trait;
use serde_json::{json, Value};

use crate::llm::tool_executor;
use crate::plugin::context::PluginContext;
use crate::plugin::tool_trait::{ToolError, ToolOutput, ToolPlugin};

pub struct SkillSmithWriteFileTool;

#[async_trait]
impl ToolPlugin for SkillSmithWriteFileTool {
    fn name(&self) -> &str {
        "skill_smith_write_file"
    }

    fn description(&self) -> &str {
        "Write (create or overwrite) a file inside the active skill draft. \
         Typical paths: 'SKILL.md', 'scripts/foo.py', 'references/notes.md', \
         'assets/template.json'. SKILL.md must contain valid YAML frontmatter — \
         the follow-up skill_smith_validate call will report errors and you can \
         retry. File size limited to 1MB; path cannot contain '..' or \
         leading-dot components."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "draft_id": {
                    "type": "string",
                    "description": "12-char hex draft_id from skill_smith_create_draft. Optional if a draft is already bound to this conversation."
                },
                "relative_path": {
                    "type": "string",
                    "description": "Path relative to draft root. e.g. 'SKILL.md', 'scripts/foo.py'. Cannot start with '/' or contain '..'. Max 5 directory levels deep."
                },
                "content": {
                    "type": "string",
                    "description": "Full file content as UTF-8 text. Max 1MB."
                }
            },
            "required": ["relative_path", "content"]
        })
    }

    async fn execute(&self, ctx: &PluginContext, input: Value) -> Result<ToolOutput, ToolError> {
        match tool_executor::skill_smith::handle_skill_smith_write_file(ctx, &input).await {
            Ok(content) => Ok(ToolOutput::success(content)),
            Err(e) => Err(ToolError::Other(e)),
        }
    }
}
