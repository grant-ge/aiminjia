//! generate_slides — create professional PPTX presentations using python-pptx.

// This builtin tool implements the deprecated ToolPlugin trait.
// It is intentionally in the legacy zone and will be migrated to RuntimeTool.
#![allow(deprecated)]

use async_trait::async_trait;
use serde_json::{json, Value};

use crate::llm::tool_executor;
use crate::plugin::context::PluginContext;
use crate::plugin::tool_trait::{ToolError, ToolOutput, ToolPlugin};

pub struct SlidesGenTool;

#[async_trait]
impl ToolPlugin for SlidesGenTool {
    fn name(&self) -> &str {
        "generate_slides"
    }

    fn description(&self) -> &str {
        "Generate a PowerPoint presentation (.pptx) — 16:9 widescreen. \
         USE WHEN the user asks to 生成 PPT、做幻灯片、写成演示文稿、导出 PPTX, \
         or any request for a slide deck. Prefer this over writing python-pptx code yourself.\n\
         \n\
         TWO-STEP CALL (required for reliability — large tool args get corrupted in SSE streaming):\n\
         \n\
         Step 1: write slides to a JSON file via execute_python using the built-in helper:\n\
           ```python\n\
           path = _save_slides([\n\
               {\"title\": \"封面\", \"layout\": \"title_slide\"},\n\
               {\"title\": \"核心发现\", \"bullets\": [\"...\", \"...\"]},\n\
               {\"title\": \"建议\", \"bullets\": [\"...\"], \"notes\": \"演讲稿备注\"},\n\
           ], filename=\"slides.json\")\n\
           ```\n\
         Step 2: call this tool with the returned path:\n\
           generate_slides(source=\"slides.json\", title=\"...\", theme=\"light\")\n\
         \n\
         Inline `slides` parameter is accepted for VERY SHORT decks only (< ~2KB). \
         For anything with multiple slides or long bullets: ALWAYS use the two-step pattern."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "title": {
                    "type": "string",
                    "description": "Presentation title — REQUIRED. Example: '薪酬公平性分析报告'. Must be a non-empty string."
                },
                "source": {
                    "type": "string",
                    "description": "PREFERRED: path to a JSON file containing the slides array, \
                        typically produced by `_save_slides(slides, filename)` in execute_python. \
                        When provided, 'slides' parameter is ignored."
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
                    "description": "Inline slides — ONLY FOR SHORT DECKS (< ~2KB). \
                        For multi-slide decks: prefer `source` (file path written by _save_slides()) \
                        — SSE streaming will truncate large inline arrays."
                },
                "theme": {
                    "type": "string",
                    "enum": ["light", "dark"],
                    "default": "light",
                    "description": "Color theme: 'light' (white background, dark text) or 'dark' (dark background, light text). Default: light."
                }
            },
            "required": ["title"]
        })
    }

    async fn execute(&self, ctx: &PluginContext, input: Value) -> Result<ToolOutput, ToolError> {
        match tool_executor::handle_generate_slides(ctx, &input).await {
            Ok(result) => Ok(result.into()),
            Err(e) => Err(ToolError::Other(e)),
        }
    }
}
