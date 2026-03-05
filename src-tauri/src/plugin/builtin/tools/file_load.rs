//! load_file — load an uploaded file into a variable for execute_python.
//!
//! The LLM only provides a file_id; path resolution and data loading are
//! fully system-managed.

use async_trait::async_trait;
use serde_json::{json, Value};

use crate::llm::tool_executor;
use crate::plugin::context::PluginContext;
use crate::plugin::tool_trait::{ToolError, ToolOutput, ToolPlugin};

pub struct FileLoadTool;

#[async_trait]
impl ToolPlugin for FileLoadTool {
    fn name(&self) -> &str { "load_file" }

    fn description(&self) -> &str {
        "Load an uploaded file so it can be used in execute_python. \
         After calling this tool, the file data is available as variable \
         _df (DataFrame for tabular data) or _text (string for text files) \
         in execute_python. You do NOT need to handle file paths — the system \
         manages path resolution automatically."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "file_id": {
                    "type": "string",
                    "description": "ID of the uploaded file to load"
                },
                "sheet": {
                    "type": "string",
                    "description": "Sheet name for Excel files (optional, defaults to first sheet)"
                },
                "nrows": {
                    "type": "integer",
                    "description": "Maximum number of rows to load (optional, loads all by default)"
                }
            },
            "required": ["file_id"]
        })
    }

    async fn execute(&self, ctx: &PluginContext, input: Value) -> Result<ToolOutput, ToolError> {
        match tool_executor::handle_load_file(ctx, &input).await {
            Ok(content) => Ok(ToolOutput::success(content)),
            Err(e) => Err(ToolError::Other(e)),
        }
    }
}
