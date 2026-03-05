//! generate_slides — create professional PPTX presentations using python-pptx.

use async_trait::async_trait;
use serde_json::{json, Value};

use crate::llm::tool_executor;
use crate::plugin::context::PluginContext;
use crate::plugin::tool_trait::{ToolError, ToolOutput, ToolPlugin};

pub struct SlidesGenTool;

#[async_trait]
impl ToolPlugin for SlidesGenTool {
    fn name(&self) -> &str { "generate_slides" }

    fn description(&self) -> &str {
        "Generate a professional PPTX presentation file. \
         Creates a 16:9 widescreen slide deck with title slide, content slides, \
         section headers, and speaker notes. Supports light and dark themes."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "title": {
                    "type": "string",
                    "description": "Presentation title — REQUIRED. Example: '薪酬公平性分析报告'. Must be a non-empty string."
                },
                "slides": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "properties": {
                            "title": { "type": "string", "description": "Slide title" },
                            "bullets": {
                                "type": "array",
                                "items": { "type": "string" },
                                "description": "Bullet points for this slide"
                            },
                            "notes": { "type": "string", "description": "Speaker notes (optional)" },
                            "layout": {
                                "type": "string",
                                "enum": ["title_slide", "title_and_content", "section_header", "blank"],
                                "default": "title_and_content",
                                "description": "Slide layout type. title_slide for cover page, title_and_content for standard content (default), section_header for chapter dividers, blank for empty slides."
                            }
                        },
                        "required": ["title"]
                    },
                    "description": "Array of slide definitions — REQUIRED. Each slide has a title, optional bullets, notes, and layout."
                },
                "theme": {
                    "type": "string",
                    "enum": ["light", "dark"],
                    "default": "light",
                    "description": "Color theme: 'light' (white background, dark text) or 'dark' (dark background, light text). Default: light."
                }
            },
            "required": ["title", "slides"]
        })
    }

    async fn execute(&self, ctx: &PluginContext, input: Value) -> Result<ToolOutput, ToolError> {
        match tool_executor::handle_generate_slides(ctx, &input).await {
            Ok(result) => Ok(result.into()),
            Err(e) => Err(ToolError::Other(e)),
        }
    }
}
