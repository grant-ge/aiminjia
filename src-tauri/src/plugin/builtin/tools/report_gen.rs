//! generate_report — create professional reports in HTML/Markdown/PDF/DOCX.

// This builtin tool implements the deprecated ToolPlugin trait.
// It is intentionally in the legacy zone and will be migrated to RuntimeTool.
#![allow(deprecated)]
#![allow(dead_code)]

use async_trait::async_trait;
use serde_json::{json, Value};

use crate::llm::tool_executor;
use crate::plugin::context::PluginContext;
use crate::plugin::tool_trait::{ToolError, ToolOutput, ToolPlugin};

pub struct ReportGenTool;

#[async_trait]
impl ToolPlugin for ReportGenTool {
    fn name(&self) -> &str {
        "generate_report"
    }

    fn description(&self) -> &str {
        "Generate a formatted document (Word .docx, PDF, HTML, or Markdown).\n\
         \n\
         ⚠️ IMPORTANT: ALWAYS use the two-step file-based pattern. \
         Inline sections are REJECTED when > 2KB (SSE streaming corrupts large args).\n\
         \n\
         Step 1 — call execute_python to write sections to a file:\n\
           path = _save_sections([{\"heading\": \"...\", \"content\": \"...\"}], 'report.json')\n\
         Step 2 — call this tool with the file path:\n\
           generate_report(source='report.json', format='docx', title='报告标题')\n\
         \n\
         FORMAT: docx (Word可编辑) / pdf (正式报告) / html (网页预览,默认) / markdown (纯文本)\n\
         \n\
         USE WHEN: 用户要求 整理/生成 Word 文档、导出 PDF、生成报告、写成文档."
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
                            "highlight": { "type": "string", "description": "Highlighted callout text" },
                            "chart": {
                                "type": "string",
                                "description": "OPTIONAL: embed an interactive chart in this section. \
                                    Pass the 'storedPath' returned by generate_chart (e.g. 'charts/chart_xxx.html'). \
                                    Renders as inline iframe in HTML reports, as a link in Markdown."
                            }
                        },
                        "required": ["heading"]
                    },
                    "description": "DO NOT USE for reports with tables or long text — will be rejected if > 2KB. \
                        Use `source` parameter with _save_sections() file path instead."
                },
                "source": {
                    "type": "string",
                    "description": "PREFERRED: path to a JSON file containing the sections array, \
                        typically produced by `_save_sections(sections, filename)` in execute_python. \
                        When provided, 'sections' parameter is ignored. This is the reliable pattern \
                        — inline large payloads may be truncated by SSE streaming."
                },
                "format": {
                    "type": "string",
                    "enum": ["html", "markdown", "pdf", "docx"],
                    "default": "html",
                    "description": "Output format. Pick based on user intent: docx (Word, 可编辑) / \
                        pdf (正式报告、打印) / html (网页预览，默认) / markdown (纯文本)."
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
