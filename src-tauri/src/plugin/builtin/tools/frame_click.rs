//! frame_click — click an element in a specific frame using Playwright locator.
//! Bypasses cross-origin restrictions. Supports text matching, CSS selector, or button index.

use async_trait::async_trait;
use serde_json::{json, Value};

use crate::llm::tool_executor;
use crate::plugin::context::PluginContext;
use crate::plugin::tool_trait::{ToolError, ToolOutput, ToolPlugin};

pub struct FrameClickTool;

#[async_trait]
impl ToolPlugin for FrameClickTool {
    fn name(&self) -> &str { "frame_click" }

    fn description(&self) -> &str {
        "Click an element in a specific frame, including cross-origin iframes. \
         Use after frame_inspect to identify which frame and element to click. \
         Supports matching by visible text (recommended), CSS selector, or button index. \
         Set wait_for_download=true when clicking export/download buttons to capture the file."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "frame_index": {
                    "type": "integer",
                    "description": "Frame index from frame_inspect results (0 = main frame)"
                },
                "text": {
                    "type": "string",
                    "description": "Text content to match (recommended — most reliable for cross-origin)"
                },
                "selector": {
                    "type": "string",
                    "description": "CSS selector to match"
                },
                "button_index": {
                    "type": "integer",
                    "description": "Index of button in the frame (from frame_inspect button list)"
                },
                "wait_for_download": {
                    "type": "boolean",
                    "description": "Set true when clicking export/download buttons. Waits up to 15s for download to complete and returns the file path."
                }
            },
            "required": []
        })
    }

    async fn execute(&self, ctx: &PluginContext, input: Value) -> Result<ToolOutput, ToolError> {
        match tool_executor::handle_frame_click(ctx, &input).await {
            Ok(content) => Ok(ToolOutput::success(content)),
            Err(e) => Err(ToolError::Other(e)),
        }
    }
}
