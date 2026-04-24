//! frame_inspect — scan all frames (including cross-origin iframes) for interactive elements.
//! Uses Playwright locator API at CDP level, bypassing same-origin policy.

use async_trait::async_trait;
use serde_json::{json, Value};

use crate::llm::tool_executor;
use crate::plugin::context::PluginContext;
use crate::plugin::tool_trait::{ToolError, ToolOutput, ToolPlugin};

pub struct FrameInspectTool;

#[async_trait]
impl ToolPlugin for FrameInspectTool {
    fn name(&self) -> &str { "frame_inspect" }

    fn description(&self) -> &str {
        "Scan all frames (including cross-origin iframes) for buttons, links, and text content. \
         Works on pages where read_page_content returns empty results (e.g. BI dashboards, \
         Canvas-rendered pages, cross-origin iframes). Uses Playwright locator API which \
         bypasses same-origin restrictions. Returns a list of frames with their interactive elements."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {},
            "required": []
        })
    }

    async fn execute(&self, ctx: &PluginContext, input: Value) -> Result<ToolOutput, ToolError> {
        match tool_executor::handle_frame_inspect(ctx, &input).await {
            Ok(content) => Ok(ToolOutput::success(content)),
            Err(e) => Err(ToolError::Other(e)),
        }
    }
}
