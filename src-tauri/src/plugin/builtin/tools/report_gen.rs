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
        "Generate a formatted document (Word .docx, PDF, HTML, or Markdown) from structured sections. \
         USE WHEN the user asks to 整理/生成 Word 文档、导出 PDF、生成报告、写成文档、创建 HTML 页面, \
         or any request to produce a downloadable, formatted document. \
         Prefer this over writing python-docx / reportlab code in execute_python yourself.\n\
         \n\
         FORMAT selection (pick based on user intent):\n\
         - format=\"docx\" → Word document (可编辑，发给同事协作)\n\
         - format=\"pdf\"  → PDF report (正式、打印、正式汇报)\n\
         - format=\"html\" → Web page (浏览器预览、分享链接) — default\n\
         - format=\"markdown\" → plain text (技术文档、Git 仓库)\n\
         \n\
         TWO-STEP CALL (required for reliability — large tool args get corrupted in SSE streaming):\n\
         \n\
         Step 1: write sections to a JSON file via execute_python using the built-in helper:\n\
           ```python\n\
           path = _save_sections([\n\
               {\"heading\": \"核心发现\", \"content\": \"...\", \"metrics\": [...]},\n\
               {\"heading\": \"建议\", \"items\": [\"...\", \"...\"]},\n\
           ], filename=\"report.json\")\n\
           ```\n\
         Step 2: call this tool with the returned path:\n\
           generate_report(source=\"report.json\", format=\"docx\", title=\"分析报告\")\n\
         \n\
         Inline `sections` parameter is accepted for VERY SHORT reports only (< ~2KB total). \
         For anything with tables, long text, or multiple sections: ALWAYS use the two-step pattern \
         — SSE streaming truncates large tool arguments unpredictably."
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
                    "description": "Report sections — INLINE ONLY FOR SHORT REPORTS (< ~2KB). \
                        For anything with tables or multi-paragraph text: prefer `source` (file path) \
                        written by _save_sections() in execute_python — SSE streaming will corrupt \
                        large inline arguments."
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
