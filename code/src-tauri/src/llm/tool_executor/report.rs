//! generate_report handler and HTML/PDF/DOCX report construction.

use anyhow::Result;
use serde_json::{json, Value};
use uuid::Uuid;

use crate::plugin::context::PluginContext;
use crate::plugin::tool_trait::FileMeta;
use crate::python::runner::PythonRunner;

use super::FileGenResult;
use super::file_load::{get_pii_unmask_map, unmask_text};
use super::{optional_str, require_str};
use super::util::{py_escape, slugify};

/// 4. generate_report — build a structured HTML/PDF/DOCX/Markdown report.
pub(crate) async fn handle_generate_report(ctx: &PluginContext, args: &Value) -> Result<FileGenResult> {
    let title = require_str(args, "title")?;
    let sections = args
        .get("sections")
        .and_then(|v| v.as_array())
        .ok_or_else(|| anyhow::anyhow!("Missing required array argument: sections"))?;
    let format = optional_str(args, "format").unwrap_or("html");

    // Collect PII unmask mapping for this conversation
    let unmask_map = get_pii_unmask_map(&ctx.storage, &ctx.conversation_id);

    // Always generate HTML first (it's the universal intermediate format)
    let html_content = build_html_report(title, sections);
    // Unmask PII placeholders in report content so users see real values
    let html_content = unmask_text(&html_content, &unmask_map);

    let (final_content, extension, actual_format) = match format {
        "markdown" => {
            let md = build_markdown_report(title, sections);
            let md = unmask_text(&md, &unmask_map);
            (md.into_bytes(), "md", "markdown")
        }
        "pdf" => {
            // HTML → PDF via Python (weasyprint)
            match convert_html_to_pdf(ctx, &html_content).await {
                Ok(pdf_bytes) => (pdf_bytes, "pdf", "pdf"),
                Err(e) => {
                    log::warn!("[generate_report] PDF conversion failed: {}. Falling back to HTML.", e);
                    (html_content.into_bytes(), "html", "html_fallback_from_pdf")
                }
            }
        }
        "docx" => {
            // HTML → DOCX via Python (htmldocx)
            match convert_html_to_docx(ctx, &html_content).await {
                Ok(docx_bytes) => (docx_bytes, "docx", "docx"),
                Err(e) => {
                    log::warn!("[generate_report] DOCX conversion failed: {}. Falling back to HTML.", e);
                    (html_content.into_bytes(), "html", "html_fallback_from_docx")
                }
            }
        }
        _ => {
            // Default: HTML
            (html_content.into_bytes(), "html", "html")
        }
    };

    let file_name = format!(
        "report_{}_{}.{}",
        slugify(title),
        Uuid::new_v4().to_string().split('-').next().unwrap_or("x"),
        extension,
    );

    let file_info = ctx
        .file_manager
        .write_file("reports", &file_name, &final_content)?;

    // Record in the database. If this fails, clean up the orphaned physical file.
    let file_id = Uuid::new_v4().to_string();
    if let Err(e) = ctx.storage.insert_generated_file(
        &file_id,
        &ctx.conversation_id,
        None,         // message_id
        &file_info.file_name,
        &file_info.stored_path,
        &file_info.file_type,
        file_info.file_size as i64,
        "report",     // category
        Some(title),  // description
        1,            // version
        true,         // is_latest
        None,         // superseded_by
        None,         // created_by_step
        None,         // expires_at
    ) {
        let _ = std::fs::remove_file(ctx.file_manager.full_path(&file_info.stored_path));
        return Err(e.into());
    }

    let is_degraded = actual_format.contains("fallback");
    let requested_format_label = format.to_string();
    let actual_format_label = if is_degraded { "html".to_string() } else { format.to_string() };

    let mut result = json!({
        "fileId": file_id,
        "fileName": file_info.file_name,
        "storedPath": file_info.stored_path,
        "fileSize": file_info.file_size,
        "format": actual_format,
    });

    let (content_str, degradation_notice) = if is_degraded {
        let requested = if actual_format.contains("pdf") { "PDF" } else { "DOCX" };
        let notice = format!(
            "⚠️ {} 转换失败，已保存为 HTML 格式（{}）。请告知用户实际生成的是 HTML 而非 {}，可在浏览器中打开后通过 Ctrl/Cmd+P 打印为 {}。",
            requested, file_info.file_name, requested, requested
        );
        result["notice"] = json!(&notice);
        let json_str = serde_json::to_string_pretty(&result)?;
        (format!("{}\n{}", notice, json_str), Some(notice))
    } else {
        (serde_json::to_string_pretty(&result)?, None)
    };

    let file_meta = FileMeta {
        file_id,
        file_name: file_info.file_name.clone(),
        requested_format: requested_format_label,
        actual_format: actual_format_label,
        file_size: file_info.file_size,
        stored_path: file_info.stored_path.clone(),
        category: "report".to_string(),
    };

    Ok(FileGenResult {
        content: content_str,
        file_meta,
        is_degraded,
        degradation_notice,
    })
}

/// Convert HTML content to PDF using Python weasyprint.
///
/// Uses temp-file protocol: writes HTML to a temp file, Python reads it.
/// This avoids string interpolation injection via triple-quote boundary breaking.
async fn convert_html_to_pdf(ctx: &PluginContext, html: &str) -> Result<Vec<u8>> {
    let runner = PythonRunner::new(ctx.workspace_path.clone(), ctx.app_handle.as_ref());

    let temp_dir = ctx.workspace_path.join("temp");
    std::fs::create_dir_all(&temp_dir)?;

    // Write HTML to temp file (safe: no string interpolation)
    let html_temp = temp_dir.join(format!(
        "html_{}.tmp",
        Uuid::new_v4().to_string().split('-').next().unwrap_or("x"),
    ));
    std::fs::write(&html_temp, html)?;

    let output_path = temp_dir.join(format!(
        "report_{}.pdf",
        Uuid::new_v4().to_string().split('-').next().unwrap_or("x"),
    ));

    let html_temp_str = py_escape(&html_temp.to_string_lossy());
    let output_path_str = py_escape(&output_path.to_string_lossy());

    let python_code = format!(r#"
import sys
import os

html_path = '{html_temp_str}'
output_path = '{output_path_str}'

with open(html_path, 'r', encoding='utf-8') as f:
    html_content = f.read()
os.remove(html_path)

try:
    from weasyprint import HTML
    HTML(string=html_content).write_pdf(output_path)
    print("OK:" + output_path)
except ImportError:
    try:
        import pdfkit
        pdfkit.from_string(html_content, output_path)
        print("OK:" + output_path)
    except ImportError:
        print("ERROR:no_pdf_library")
        sys.exit(1)
except Exception as exc:
    print("ERROR:" + str(exc))
    sys.exit(1)
"#);

    let result = runner.execute(&python_code).await?;

    // Clean up HTML temp file if Python didn't (e.g., on error before os.remove)
    let _ = std::fs::remove_file(&html_temp);

    if result.exit_code != 0 || result.stdout.trim().starts_with("ERROR:") {
        let err_msg = if result.stdout.contains("no_pdf_library") {
            "weasyprint/pdfkit not installed".to_string()
        } else {
            format!("exit_code={}, stdout={}, stderr={}", result.exit_code, result.stdout.trim(), result.stderr.trim())
        };
        anyhow::bail!("PDF conversion failed: {}", err_msg);
    }

    // Read the generated PDF file
    let pdf_bytes = std::fs::read(&output_path)?;
    // Clean up temp file
    let _ = std::fs::remove_file(&output_path);

    Ok(pdf_bytes)
}

/// Convert HTML content to DOCX using Python htmldocx.
///
/// Uses temp-file protocol: writes HTML to a temp file, Python reads it.
/// This avoids string interpolation injection via triple-quote boundary breaking.
async fn convert_html_to_docx(ctx: &PluginContext, html: &str) -> Result<Vec<u8>> {
    let runner = PythonRunner::new(ctx.workspace_path.clone(), ctx.app_handle.as_ref());

    let temp_dir = ctx.workspace_path.join("temp");
    std::fs::create_dir_all(&temp_dir)?;

    // Write HTML to temp file (safe: no string interpolation)
    let html_temp = temp_dir.join(format!(
        "html_{}.tmp",
        Uuid::new_v4().to_string().split('-').next().unwrap_or("x"),
    ));
    std::fs::write(&html_temp, html)?;

    let output_path = temp_dir.join(format!(
        "report_{}.docx",
        Uuid::new_v4().to_string().split('-').next().unwrap_or("x"),
    ));

    let html_temp_str = py_escape(&html_temp.to_string_lossy());
    let output_path_str = py_escape(&output_path.to_string_lossy());

    let python_code = format!(r#"
import sys
import os

html_path = '{html_temp_str}'
output_path = '{output_path_str}'

with open(html_path, 'r', encoding='utf-8') as f:
    html_content = f.read()
os.remove(html_path)

try:
    from htmldocx import HtmlToDocx
    from docx import Document

    doc = Document()
    parser = HtmlToDocx()
    parser.add_html_to_document(html_content, doc)
    doc.save(output_path)
    print("OK:" + output_path)
except ImportError as exc:
    print("ERROR:missing_library:" + str(exc))
    sys.exit(1)
except Exception as exc:
    print("ERROR:" + str(exc))
    sys.exit(1)
"#);

    let result = runner.execute(&python_code).await?;

    // Clean up HTML temp file if Python didn't
    let _ = std::fs::remove_file(&html_temp);

    if result.exit_code != 0 || result.stdout.trim().starts_with("ERROR:") {
        let err_msg = format!("exit_code={}, stdout={}, stderr={}", result.exit_code, result.stdout.trim(), result.stderr.trim());
        anyhow::bail!("DOCX conversion failed: {}", err_msg);
    }

    let docx_bytes = std::fs::read(&output_path)?;
    let _ = std::fs::remove_file(&output_path);

    Ok(docx_bytes)
}

/// Build a complete standalone HTML report from a title and sections array.
fn build_html_report(title: &str, sections: &[Value]) -> String {
    let mut body = String::new();

    for section in sections {
        let heading = section
            .get("heading")
            .and_then(|v| v.as_str())
            .unwrap_or("Untitled Section");

        body.push_str(&format!(
            "    <section>\n      <h2>{}</h2>\n",
            html_escape(heading),
        ));

        // Text content (with markdown → HTML conversion)
        if let Some(content) = section.get("content").and_then(|v| v.as_str()) {
            if !content.trim().is_empty() {
                body.push_str("      <div class=\"content\">");
                body.push_str(&report_markdown_to_html(content));
                body.push_str("</div>\n");
            }
        }

        // Metric cards
        if let Some(metrics) = section.get("metrics").and_then(|v| v.as_array()) {
            body.push_str("      <div class=\"metric-grid\">\n");
            for m in metrics {
                let label = m.get("label").and_then(|v| v.as_str()).unwrap_or("");
                let value = m.get("value").and_then(|v| v.as_str()).unwrap_or("");
                let subtitle = m.get("subtitle").and_then(|v| v.as_str()).unwrap_or("");
                let state = m.get("state").and_then(|v| v.as_str()).unwrap_or("neutral");
                let state_class = match state {
                    "good" => "metric-good",
                    "warn" => "metric-warn",
                    "bad" => "metric-bad",
                    _ => "",
                };
                body.push_str(&format!(
                    "        <div class=\"metric-card {}\"><div class=\"metric-label\">{}</div><div class=\"metric-value\">{}</div><div class=\"metric-sub\">{}</div></div>\n",
                    state_class, html_escape(label), html_escape(value), html_escape(subtitle),
                ));
            }
            body.push_str("      </div>\n");
        }

        // Structured table
        if let Some(table) = section.get("table") {
            render_report_table(&mut body, table);
        }

        // Bullet list
        if let Some(items) = section.get("items").and_then(|v| v.as_array()) {
            body.push_str("      <ul class=\"item-list\">\n");
            for item in items {
                if let Some(text) = item.as_str() {
                    body.push_str(&format!("        <li>{}</li>\n", report_inline_md(&html_escape(text))));
                }
            }
            body.push_str("      </ul>\n");
        }

        // Highlight callout
        if let Some(highlight) = section.get("highlight").and_then(|v| v.as_str()) {
            body.push_str(&format!(
                "      <div class=\"callout\">{}</div>\n",
                report_inline_md(&html_escape(highlight)),
            ));
        }

        body.push_str("    </section>\n");
    }

    let now = chrono::Local::now();

    format!(
        r##"<!DOCTYPE html>
<html lang="zh-CN">
<head>
<meta charset="UTF-8">
<meta name="viewport" content="width=device-width, initial-scale=1.0">
<title>{title}</title>
<style>
  @page {{ margin: 2cm; }}
  @media print {{
    body {{ padding: 0; }}
    .no-print {{ display: none; }}
    section {{ break-inside: avoid; }}
  }}
  * {{ margin: 0; padding: 0; box-sizing: border-box; }}
  body {{
    font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", "PingFang SC", "Hiragino Sans GB", "Microsoft YaHei", sans-serif;
    font-size: 14px;
    line-height: 1.7;
    color: #1a1a2e;
    background: #fff;
    padding: 48px 40px;
    max-width: 960px;
    margin: 0 auto;
  }}
  /* ── Header ── */
  .report-header {{
    border-bottom: 3px solid #6c5ce7;
    padding-bottom: 24px;
    margin-bottom: 36px;
  }}
  .report-header h1 {{
    font-size: 26px;
    font-weight: 700;
    color: #1a1a2e;
    margin-bottom: 8px;
  }}
  .report-header .meta {{
    font-size: 12px;
    color: #8e8ea0;
  }}
  /* ── Sections ── */
  section {{
    margin-bottom: 32px;
    page-break-inside: avoid;
  }}
  h2 {{
    font-size: 18px;
    font-weight: 700;
    color: #1a1a2e;
    padding-bottom: 8px;
    border-bottom: 1px solid #e8e8f0;
    margin-bottom: 16px;
  }}
  .content {{
    line-height: 1.7;
  }}
  .content p {{ margin-bottom: 10px; }}
  .content h3 {{ font-size: 15px; font-weight: 600; margin: 16px 0 8px; color: #2d3436; }}
  .content h4 {{ font-size: 14px; font-weight: 600; margin: 12px 0 6px; color: #2d3436; }}
  .content ul, .content ol {{ margin: 8px 0 12px 20px; }}
  .content li {{ margin-bottom: 4px; }}
  .content strong {{ color: #1a1a2e; }}
  .content code {{ background: #f0f0f5; padding: 2px 6px; border-radius: 3px; font-size: 13px; }}
  /* ── Metric Cards ── */
  .metric-grid {{
    display: flex;
    flex-wrap: wrap;
    gap: 14px;
    margin: 16px 0;
  }}
  .metric-card {{
    flex: 1;
    min-width: 160px;
    padding: 16px 20px;
    border-radius: 10px;
    border: 1px solid #e0e0e8;
    background: #fafafa;
  }}
  .metric-label {{ font-size: 12px; color: #8e8ea0; font-weight: 500; }}
  .metric-value {{ font-size: 24px; font-weight: 700; margin: 6px 0 4px; color: #1a1a2e; }}
  .metric-sub {{ font-size: 11px; color: #8e8ea0; }}
  .metric-good {{ border-color: #00b894; background: #f0faf7; }}
  .metric-good .metric-value {{ color: #00b894; }}
  .metric-warn {{ border-color: #fdcb6e; background: #fffef5; }}
  .metric-warn .metric-value {{ color: #e17055; }}
  .metric-bad {{ border-color: #ff7675; background: #fff5f5; }}
  .metric-bad .metric-value {{ color: #d63031; }}
  /* ── Tables ── */
  table {{
    width: 100%;
    border-collapse: collapse;
    margin: 14px 0;
    font-size: 13px;
  }}
  table th, table td {{
    border: 1px solid #e0e0e8;
    padding: 8px 12px;
    text-align: left;
  }}
  table th {{
    background: #f5f5fa;
    font-weight: 600;
    color: #444;
    font-size: 12px;
    text-transform: uppercase;
    letter-spacing: 0.3px;
  }}
  table tr:nth-child(even) {{ background: #fafafa; }}
  /* ── Callout ── */
  .callout {{
    border-left: 4px solid #6c5ce7;
    padding: 14px 18px;
    margin: 16px 0;
    background: #f8f7ff;
    border-radius: 0 8px 8px 0;
    font-size: 14px;
    line-height: 1.6;
  }}
  /* ── Item List ── */
  .item-list {{
    margin: 12px 0 16px 20px;
    line-height: 1.7;
  }}
  .item-list li {{ margin-bottom: 6px; }}
  /* ── Footer ── */
  .report-footer {{
    border-top: 1px solid #e8e8f0;
    padding-top: 16px;
    margin-top: 48px;
    font-size: 11px;
    color: #8e8ea0;
    text-align: center;
  }}
</style>
</head>
<body>
<div class="report-header">
  <h1>{title}</h1>
  <div class="meta">Generated: {timestamp} &nbsp;|&nbsp; AI小家 — 组织专家，工作助手</div>
</div>
{body}
<div class="report-footer">
  本报告由 AI小家（组织专家，工作助手）自动生成 — {timestamp}
</div>
</body>
</html>"##,
        title = html_escape(title),
        body = body,
        timestamp = now.format("%Y-%m-%d %H:%M"),
    )
}

/// Render a structured table for report HTML.
fn render_report_table(html: &mut String, table: &Value) {
    let title = table.get("title").and_then(|v| v.as_str());
    if let Some(t) = title {
        html.push_str(&format!("      <div style=\"font-weight:600;font-size:13px;margin:12px 0 6px\">{}</div>\n", html_escape(t)));
    }

    // Support both { columns: [str], rows: [[str]] } and { columns: [{label, key}], rows: [{key: val}] }
    let columns = match table.get("columns").and_then(|v| v.as_array()) {
        Some(cols) => cols,
        None => return,
    };
    let rows = match table.get("rows").and_then(|v| v.as_array()) {
        Some(rows) => rows,
        None => return,
    };

    html.push_str("      <table><thead><tr>\n");

    // Determine column labels and keys
    let col_info: Vec<(String, String)> = columns.iter().map(|col| {
        if let Some(label) = col.as_str() {
            (label.to_string(), label.to_string())
        } else {
            let label = col.get("label").and_then(|v| v.as_str()).unwrap_or("").to_string();
            let key = col.get("key").and_then(|v| v.as_str()).unwrap_or("").to_string();
            (label, key)
        }
    }).collect();

    for (label, _) in &col_info {
        html.push_str(&format!("        <th>{}</th>\n", html_escape(label)));
    }
    html.push_str("      </tr></thead><tbody>\n");

    for row in rows {
        html.push_str("      <tr>");
        if let Some(row_arr) = row.as_array() {
            // Row is an array of values
            for (i, _) in col_info.iter().enumerate() {
                let cell = row_arr.get(i).map(|v| {
                    v.as_str().map(|s| s.to_string()).unwrap_or_else(|| v.to_string())
                }).unwrap_or_default();
                html.push_str(&format!("<td>{}</td>", html_escape(&cell)));
            }
        } else {
            // Row is an object with keys
            for (_, key) in &col_info {
                let cell = row.get(key.as_str())
                    .map(|v| v.as_str().map(|s| s.to_string()).unwrap_or_else(|| v.to_string()))
                    .unwrap_or_default();
                html.push_str(&format!("<td>{}</td>", html_escape(&cell)));
            }
        }
        html.push_str("</tr>\n");
    }
    html.push_str("      </tbody></table>\n");
}

/// Simple markdown → HTML for report content blocks.
fn report_markdown_to_html(text: &str) -> String {
    let mut result = String::with_capacity(text.len() * 2);
    let mut in_list = false;
    let mut in_table = false;
    let mut table_header_done = false;

    for line in text.lines() {
        let trimmed = line.trim();

        // Table rows (| col | col |)
        if trimmed.starts_with('|') && trimmed.ends_with('|') {
            if trimmed.chars().all(|c| c == '|' || c == '-' || c == ' ' || c == ':') {
                // Separator line — skip but mark header done
                table_header_done = true;
                continue;
            }
            if !in_table {
                if in_list { result.push_str("</ul>"); in_list = false; }
                result.push_str("<table>");
                in_table = true;
                table_header_done = false;
            }
            let cells: Vec<&str> = trimmed.split('|')
                .filter(|s| !s.trim().is_empty())
                .collect();
            let tag = if !table_header_done { "th" } else { "td" };
            result.push_str("<tr>");
            for cell in &cells {
                result.push_str(&format!("<{}>{}</{}>", tag, report_inline_md(&html_escape(cell.trim())), tag));
            }
            result.push_str("</tr>");
            continue;
        }

        // Close table if we were in one
        if in_table {
            result.push_str("</table>");
            in_table = false;
            table_header_done = false;
        }

        // Unordered list items
        if trimmed.starts_with("- ") || trimmed.starts_with("* ") {
            if !in_list { result.push_str("<ul>"); in_list = true; }
            result.push_str(&format!("<li>{}</li>", report_inline_md(&html_escape(&trimmed[2..]))));
            continue;
        }

        // Ordered list items
        if let Some(rest) = trimmed.strip_prefix(|c: char| c.is_ascii_digit()).and_then(|s| s.strip_prefix(". ")) {
            if !in_list { result.push_str("<ol>"); in_list = true; }
            result.push_str(&format!("<li>{}</li>", report_inline_md(&html_escape(rest))));
            continue;
        }

        if in_list {
            if trimmed.is_empty() {
                result.push_str("</ul>");
                in_list = false;
            }
        }

        // Headers
        if trimmed.starts_with("### ") {
            result.push_str(&format!("<h4>{}</h4>", report_inline_md(&html_escape(&trimmed[4..]))));
        } else if trimmed.starts_with("## ") {
            result.push_str(&format!("<h3>{}</h3>", report_inline_md(&html_escape(&trimmed[3..]))));
        } else if trimmed.starts_with("# ") {
            result.push_str(&format!("<h3>{}</h3>", report_inline_md(&html_escape(&trimmed[2..]))));
        } else if trimmed.is_empty() {
            // Skip excessive blank lines
        } else {
            result.push_str(&format!("<p>{}</p>", report_inline_md(&html_escape(trimmed))));
        }
    }

    if in_list { result.push_str("</ul>"); }
    if in_table { result.push_str("</table>"); }
    result
}

/// Convert inline markdown (bold, code) to HTML for reports.
fn report_inline_md(text: &str) -> String {
    let mut result = text.to_string();
    // Bold: **text**
    while let Some(start) = result.find("**") {
        if let Some(end) = result[start + 2..].find("**") {
            let inner = result[start + 2..start + 2 + end].to_string();
            result = format!("{}<strong>{}</strong>{}", &result[..start], inner, &result[start + 2 + end + 2..]);
        } else { break; }
    }
    // Inline code: `text`
    while let Some(start) = result.find('`') {
        if let Some(end) = result[start + 1..].find('`') {
            let inner = result[start + 1..start + 1 + end].to_string();
            result = format!("{}<code>{}</code>{}", &result[..start], inner, &result[start + 1 + end + 1..]);
        } else { break; }
    }
    result
}

/// Build a Markdown report from a title and sections array.
fn build_markdown_report(title: &str, sections: &[Value]) -> String {
    let mut output = format!("# {}\n\n", title);
    for section in sections {
        let heading = section
            .get("heading")
            .and_then(|v| v.as_str())
            .unwrap_or("Untitled Section");
        let content = section
            .get("content")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        output.push_str(&format!("## {}\n\n{}\n\n", heading, content));
    }
    output
}

/// Minimal HTML escaping.
fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_html_report() {
        let sections = vec![
            json!({"heading": "Summary", "content": "Good results."}),
            json!({"heading": "Details", "content": "Line1\nLine2"}),
        ];
        let html = build_html_report("Test Report", &sections);
        assert!(html.contains("<title>Test Report</title>"));
        assert!(html.contains("<h2>Summary</h2>"));
        assert!(html.contains("Good results."));
        // Multi-line content is split into separate <p> tags
        assert!(html.contains("<p>Line1</p>"));
        assert!(html.contains("<p>Line2</p>"));
    }

    #[test]
    fn test_build_html_report_escapes_html() {
        let sections = vec![json!({"heading": "<script>alert(1)</script>", "content": "a & b"})];
        let html = build_html_report("Test", &sections);
        assert!(!html.contains("<script>"));
        assert!(html.contains("&lt;script&gt;"));
        assert!(html.contains("a &amp; b"));
    }

    #[test]
    fn test_build_markdown_report() {
        let sections = vec![
            json!({"heading": "Intro", "content": "Hello"}),
        ];
        let md = build_markdown_report("My Report", &sections);
        assert!(md.starts_with("# My Report\n"));
        assert!(md.contains("## Intro\n"));
        assert!(md.contains("Hello"));
    }
}
