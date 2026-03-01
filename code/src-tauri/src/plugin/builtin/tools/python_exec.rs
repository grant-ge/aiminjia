//! execute_python — run Python code in sandboxed environment.

use async_trait::async_trait;
use serde_json::{json, Value};

use crate::llm::tool_executor::{self, ToolContext};
use crate::plugin::context::PluginContext;
use crate::plugin::tool_trait::{ToolError, ToolOutput, ToolPlugin};

pub struct PythonExecTool;

#[async_trait]
impl ToolPlugin for PythonExecTool {
    fn name(&self) -> &str { "execute_python" }

    fn description(&self) -> &str {
        "Execute Python code for data analysis, statistical computation, \
         and file processing. Has access to pandas, numpy, scipy, openpyxl. \
         Working directory contains uploaded files."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "code": {
                    "type": "string",
                    "description": "Python code to execute"
                },
                "purpose": {
                    "type": "string",
                    "description": "Brief description of what this code does"
                }
            },
            "required": ["code"]
        })
    }

    async fn execute(&self, ctx: &PluginContext, input: Value) -> Result<ToolOutput, ToolError> {
        let tool_ctx = ToolContext::from_plugin_context(ctx);
        match tool_executor::handle_execute_python(&tool_ctx, &input).await {
            Ok(content) => Ok(ToolOutput::success(content)),
            Err(e) => Err(ToolError::Other(e)),
        }
    }
}
