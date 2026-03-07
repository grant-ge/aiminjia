//! generate_report — create professional reports in HTML/Markdown/PDF/DOCX.

use async_trait::async_trait;
use serde_json::{json, Value};

use crate::llm::tool_executor;
use crate::plugin::context::PluginContext;
use crate::plugin::tool_trait::{ToolError, ToolOutput, ToolPlugin};

pub struct ReportGenTool;

#[async_trait]
impl ToolPlugin for ReportGenTool {
    fn name(&self) -> &str { "generate_report" }

    fn description(&self) -> &str {
        "Generate a professional analysis report as a downloadable file. \
         Supports HTML (default), Markdown, PDF, and DOCX formats. \
         Supports rich content: text with markdown, structured tables, metric cards, \
         bullet lists, and highlighted callouts. PDF and DOCX are converted from HTML automatically."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "title": {
                    "type": "string",
                    "description": "Report title — REQUIRED. Example: '薪酬公平性分析报告'. Must be a non-empty string."
                },
                "sections": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "properties": {
                            "heading": { "type": "string", "description": "Section heading" },
                            "content": { "type": "string", "description": "Text content (supports markdown)" },
                            "metrics": {
                                "type": "array",
                                "items": {
                                    "type": "object",
                                    "properties": {
                                        "label": { "type": "string" },
                                        "value": { "type": "string" },
                                        "subtitle": { "type": "string" },
                                        "state": { "type": "string", "enum": ["good", "warn", "bad", "neutral"] }
                                    },
                                    "required": ["label", "value"]
                                },
                                "description": "Metric cards displayed as a grid"
                            },
                            "table": {
                                "type": "object",
                                "properties": {
                                    "title": { "type": "string" },
                                    "columns": { "type": "array", "items": { "type": "string" } },
                                    "rows": { "type": "array", "items": { "type": "array", "items": { "type": "string" } } }
                                },
                                "description": "Structured data table"
                            },
                            "items": { "type": "array", "items": { "type": "string" }, "description": "Bullet list items" },
                            "highlight": { "type": "string", "description": "Highlighted callout text" }
                        },
                        "required": ["heading"]
                    },
                    "description": "Report sections — PREFERRED. Pass sections directly in this parameter. \
                        Each section has a heading and optional content, metrics, table, items, highlight. \
                        This is the fastest approach: one tool call generates the report."
                },
                "source": {
                    "type": "string",
                    "description": "Alternative: path to a JSON file containing the sections array. \
                        Only use this when sections data is already in a file (e.g. from precompute). \
                        When provided, 'sections' parameter is ignored."
                },
                "format": {
                    "type": "string",
                    "enum": ["html", "markdown", "pdf", "docx"],
                    "default": "html",
                    "description": "Output format. PDF and DOCX are converted from HTML. If PDF conversion fails, HTML is returned as fallback."
                }
            },
            "required": ["title"]
        })
    }

    async fn execute(&self, ctx: &PluginContext, input: Value) -> Result<ToolOutput, ToolError> {
        match tool_executor::handle_generate_report(ctx, &input).await {
            Ok(result) => Ok(result.into()),
            Err(e) => Err(ToolError::Other(e)),
        }
    }
}
