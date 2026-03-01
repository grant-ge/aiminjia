//! Conversation export — generates styled HTML and optionally converts to PDF via Python.

use std::sync::Arc;
use tauri::State;
use crate::storage::file_store::AppStorage;
use crate::storage::file_manager::FileManager;

/// Export a conversation as HTML or PDF.
///
/// Flow:
/// 1. Fetch conversation metadata + all messages from DB
/// 2. Render messages into a self-contained HTML document
/// 3. For HTML: save directly to workspace/exports/
///    For PDF: write HTML to temp, run Python (weasyprint) to convert
/// 4. Record in generated_files table
/// 5. Return file info
#[tauri::command]
pub async fn export_conversation(
    db: State<'_, Arc<AppStorage>>,
    file_mgr: State<'_, Arc<FileManager>>,
    conversation_id: String,
    format: String,
) -> Result<serde_json::Value, String> {
    log::info!("export_conversation: conversation_id={}, format={}", conversation_id, format);

    // 1. Get conversation title
    let conversations = db.get_conversations().map_err(|e| e.to_string())?;
    let conv = conversations.iter()
        .find(|c| c.get("id").and_then(|v| v.as_str()) == Some(&conversation_id))
        .ok_or_else(|| format!("Conversation {} not found", conversation_id))?;
    let title = conv.get("title").and_then(|v| v.as_str()).unwrap_or("Conversation Export");
    let created_at = conv.get("createdAt").and_then(|v| v.as_str()).unwrap_or("");

    // 2. Get all messages
    let messages = db.get_messages(&conversation_id).map_err(|e| e.to_string())?;
    if messages.is_empty() {
        return Err("No messages to export".to_string());
    }

    // 3. Render HTML
    let html = render_conversation_html(title, &messages, created_at);

    // 4. Prepare workspace paths
    let workspace = file_mgr.workspace_path().to_path_buf();
    let temp_dir = workspace.join("temp");
    let exports_dir = workspace.join("exports");
    std::fs::create_dir_all(&temp_dir).map_err(|e| format!("Failed to create temp dir: {}", e))?;
    std::fs::create_dir_all(&exports_dir).map_err(|e| format!("Failed to create exports dir: {}", e))?;

    let file_id = uuid::Uuid::new_v4().to_string();
    let safe_title: String = title.chars()
        .filter(|c| c.is_alphanumeric() || *c == ' ' || *c == '-' || *c == '_')
        .take(30)
        .collect();
    let safe_title = if safe_title.is_empty() { "export".to_string() } else { safe_title };
    let timestamp = chrono::Local::now().format("%Y%m%d_%H%M%S").to_string();

    let html_temp_path = temp_dir.join(format!("export_{}.html", file_id));
    std::fs::write(&html_temp_path, &html)
        .map_err(|e| format!("Failed to write temp HTML: {}", e))?;

    let (output_ext, _output_mime) = match format.as_str() {
        "pdf" => ("pdf", "pdf"),
        "html" => ("html", "html"),
        _ => return Err(format!("Unsupported export format: {}", format)),
    };

    let output_filename = format!("{}_{}.{}", safe_title, timestamp, output_ext);
    let output_path = exports_dir.join(&output_filename);
    let stored_path = format!("exports/{}", output_filename);

    // For HTML format: save directly without Python conversion
    if format == "html" {
        std::fs::write(&output_path, &html)
            .map_err(|e| format!("Failed to write HTML export: {}", e))?;

        let file_size = std::fs::metadata(&output_path)
            .map(|m| m.len() as i64)
            .unwrap_or(0);

        db.insert_generated_file(
            &file_id,
            &conversation_id,
            None,
            &output_filename,
            &stored_path,
            "html",
            file_size,
            "report",
            Some(&format!("Conversation export ({})", format)),
            1,
            true,
            None,
            None,
            None,
        ).map_err(|e| format!("Failed to record export file: {}", e))?;

        log::info!("export_conversation OK (HTML direct): file_id={}, path={}", file_id, stored_path);

        return Ok(serde_json::json!({
            "fileId": file_id,
            "fileName": output_filename,
            "storedPath": stored_path,
            "fileSize": file_size,
        }));
    }

    // For PDF: write HTML to temp and run Python conversion
    let python_code = format!(
            r#"
import json, sys

html_path = {html_path}
pdf_path = {pdf_path}

try:
    from weasyprint import HTML
    HTML(filename=html_path).write_pdf(pdf_path)
    print(json.dumps({{"status": "success", "path": pdf_path}}))
except ImportError:
    # Fallback: copy the HTML to exports dir (user can open in browser and print to PDF)
    import shutil
    html_out = pdf_path.replace('.pdf', '.html')
    shutil.copy(html_path, html_out)
    print(json.dumps({{"status": "fallback_html", "path": html_out, "error": "weasyprint not installed, exported as HTML instead. Install with: pip install weasyprint"}}))
except Exception as e:
    print(json.dumps({{"status": "error", "error": str(e)}}))
"#,
            html_path = serde_json::to_string(&html_temp_path.to_string_lossy().to_string()).unwrap(),
            pdf_path = serde_json::to_string(&output_path.to_string_lossy().to_string()).unwrap(),
        );

    // Write the script to a temp file and run via python3 directly
    let script_path = temp_dir.join(format!("export_script_{}.py", file_id));
    std::fs::write(&script_path, &python_code)
        .map_err(|e| format!("Failed to write export script: {}", e))?;

    let py_output = tokio::process::Command::new("python3")
        .arg("-u")
        .arg(&script_path)
        .env("PYTHONIOENCODING", "utf-8")
        .output()
        .await
        .map_err(|e| format!("Failed to run python3: {}", e))?;

    // Cleanup script
    let _ = std::fs::remove_file(&script_path);

    let stdout = String::from_utf8_lossy(&py_output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&py_output.stderr).to_string();

    if !py_output.status.success() && stdout.trim().is_empty() {
        let _ = std::fs::remove_file(&html_temp_path);
        return Err(format!("Python export failed: {}", if stderr.is_empty() { "unknown error" } else { &stderr }));
    }

    // Parse Python output
    let python_output: serde_json::Value = serde_json::from_str(stdout.trim())
        .unwrap_or_else(|_| serde_json::json!({"status": "error", "error": stdout}));

    let status = python_output.get("status").and_then(|v| v.as_str()).unwrap_or("error");

    if status == "error" {
        let _ = std::fs::remove_file(&html_temp_path);
        let err_msg = python_output.get("error").and_then(|v| v.as_str()).unwrap_or("Unknown error");
        return Err(format!("Export failed: {}", err_msg));
    }

    // Handle fallback case (PDF → HTML fallback when weasyprint not available)
    let (final_path, final_stored_path, final_filename, final_ext) = if status == "fallback_html" {
        let html_filename = output_filename.replace(".pdf", ".html");
        let html_stored = format!("exports/{}", html_filename);
        let html_out = python_output.get("path").and_then(|v| v.as_str())
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| exports_dir.join(&html_filename));
        (html_out, html_stored, html_filename, "html")
    } else {
        (output_path.clone(), stored_path.clone(), output_filename.clone(), output_ext)
    };

    // Get file size
    let file_size = std::fs::metadata(&final_path)
        .map(|m| m.len() as i64)
        .unwrap_or(0);

    // 6. Record in generated_files table
    db.insert_generated_file(
        &file_id,
        &conversation_id,
        None,           // message_id
        &final_filename,
        &final_stored_path,
        final_ext,
        file_size,
        "report",       // category
        Some(&format!("Conversation export ({})", format)),
        1,              // version
        true,           // is_latest
        None,           // superseded_by
        None,           // created_by_step
        None,           // expires_at
    ).map_err(|e| format!("Failed to record export file: {}", e))?;

    // Cleanup temp HTML
    let _ = std::fs::remove_file(&html_temp_path);

    log::info!("export_conversation OK: file_id={}, path={}", file_id, final_stored_path);

    Ok(serde_json::json!({
        "fileId": file_id,
        "fileName": final_filename,
        "storedPath": final_stored_path,
        "fileSize": file_size,
    }))
}

/// Render a conversation into a self-contained styled HTML document.
///
/// The HTML uses inline styles (no external CSS) so it renders correctly
/// both standalone in a browser and when converted to PDF/Word.
fn render_conversation_html(
    title: &str,
    messages: &[serde_json::Value],
    created_at: &str,
) -> String {
    let mut html = String::with_capacity(32 * 1024);

    // Header
    html.push_str(&format!(r#"<!DOCTYPE html>
<html lang="zh-CN">
<head>
<meta charset="UTF-8">
<meta name="viewport" content="width=device-width, initial-scale=1.0">
<title>{title}</title>
<style>
  * {{ margin: 0; padding: 0; box-sizing: border-box; }}
  body {{
    font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", "PingFang SC", "Hiragino Sans GB", "Microsoft YaHei", sans-serif;
    font-size: 14px;
    line-height: 1.6;
    color: #1a1a2e;
    background: #fff;
    padding: 40px;
    max-width: 900px;
    margin: 0 auto;
  }}
  .header {{
    border-bottom: 2px solid #e8e8f0;
    padding-bottom: 20px;
    margin-bottom: 30px;
  }}
  .header h1 {{
    font-size: 22px;
    font-weight: 700;
    color: #1a1a2e;
    margin-bottom: 8px;
  }}
  .header .meta {{
    font-size: 12px;
    color: #8e8ea0;
  }}
  .message {{
    margin-bottom: 24px;
    padding: 16px;
    border-radius: 8px;
  }}
  .message-user {{
    background: #f0f4ff;
    border-left: 3px solid #6c5ce7;
  }}
  .message-assistant {{
    background: #fafafa;
    border-left: 3px solid #00b894;
  }}
  .message-role {{
    font-size: 12px;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.5px;
    margin-bottom: 8px;
  }}
  .role-user {{ color: #6c5ce7; }}
  .role-assistant {{ color: #00b894; }}
  .msg-text {{
    white-space: pre-wrap;
    word-wrap: break-word;
  }}
  .msg-text p {{ margin-bottom: 8px; }}
  table {{
    width: 100%;
    border-collapse: collapse;
    margin: 12px 0;
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
  }}
  .metric-grid {{
    display: flex;
    flex-wrap: wrap;
    gap: 12px;
    margin: 12px 0;
  }}
  .metric-card {{
    flex: 1;
    min-width: 140px;
    padding: 12px 16px;
    border-radius: 8px;
    border: 1px solid #e0e0e8;
  }}
  .metric-label {{ font-size: 12px; color: #8e8ea0; }}
  .metric-value {{ font-size: 20px; font-weight: 700; margin: 4px 0; }}
  .metric-subtitle {{ font-size: 11px; color: #8e8ea0; }}
  .metric-good {{ border-color: #00b894; }}
  .metric-good .metric-value {{ color: #00b894; }}
  .metric-warn {{ border-color: #fdcb6e; }}
  .metric-warn .metric-value {{ color: #e17055; }}
  .metric-bad {{ border-color: #ff7675; }}
  .metric-bad .metric-value {{ color: #d63031; }}
  pre {{
    background: #2d3436;
    color: #dfe6e9;
    padding: 12px 16px;
    border-radius: 6px;
    overflow-x: auto;
    font-size: 13px;
    line-height: 1.5;
    margin: 12px 0;
  }}
  .code-result {{
    background: #f8f9fa;
    border: 1px solid #e0e0e8;
    padding: 10px 14px;
    border-radius: 6px;
    font-family: monospace;
    font-size: 12px;
    white-space: pre-wrap;
    margin: 8px 0;
  }}
  .code-result.error {{ border-color: #ff7675; background: #fff5f5; }}
  .anomaly {{ padding: 8px 0; border-bottom: 1px solid #f0f0f5; }}
  .anomaly:last-child {{ border-bottom: none; }}
  .anomaly-title {{ font-weight: 600; }}
  .anomaly-high .anomaly-dot {{ color: #d63031; }}
  .anomaly-medium .anomaly-dot {{ color: #e17055; }}
  .anomaly-low .anomaly-dot {{ color: #00b894; }}
  .insight {{
    border-left: 3px solid #74b9ff;
    padding: 10px 14px;
    margin: 12px 0;
    background: #f0f7ff;
    border-radius: 0 6px 6px 0;
  }}
  .insight-title {{ font-weight: 600; margin-bottom: 4px; }}
  .root-cause {{
    border-left: 3px solid #ff7675;
    padding: 10px 14px;
    margin: 12px 0;
    background: #fff5f5;
    border-radius: 0 6px 6px 0;
  }}
  .confirm {{
    border: 1px solid #fdcb6e;
    padding: 12px;
    margin: 12px 0;
    border-radius: 8px;
    background: #fffef5;
  }}
  .search-sources {{
    border-left: 3px solid #a29bfe;
    padding: 10px 14px;
    margin: 12px 0;
    background: #f8f7ff;
    border-radius: 0 6px 6px 0;
  }}
  .search-sources a {{ color: #6c5ce7; text-decoration: none; }}
  .search-sources a:hover {{ text-decoration: underline; }}
  .exec-summary {{
    display: flex;
    flex-wrap: wrap;
    gap: 12px;
    margin: 12px 0;
  }}
  .exec-box {{
    flex: 1;
    min-width: 140px;
    padding: 12px;
    border-radius: 8px;
    border: 1px solid #e0e0e8;
    text-align: center;
  }}
  .file-ref {{
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 8px 12px;
    margin: 6px 0;
    border: 1px solid #e0e0e8;
    border-radius: 6px;
    font-size: 13px;
  }}
  .progress-indicator {{
    display: flex;
    gap: 8px;
    align-items: center;
    margin: 12px 0;
    flex-wrap: wrap;
  }}
  .progress-step {{
    padding: 4px 10px;
    border-radius: 12px;
    font-size: 12px;
    font-weight: 500;
  }}
  .step-done {{ background: #00b894; color: white; }}
  .step-active {{ background: #6c5ce7; color: white; }}
  .step-pending {{ background: #e0e0e8; color: #8e8ea0; }}
  .progress-arrow {{ color: #c0c0c8; }}
  .footer {{
    border-top: 1px solid #e8e8f0;
    padding-top: 16px;
    margin-top: 40px;
    font-size: 11px;
    color: #8e8ea0;
    text-align: center;
  }}
</style>
</head>
<body>
<div class="header">
  <h1>{title}</h1>
  <div class="meta">Created: {created_at} &nbsp;|&nbsp; Messages: {msg_count} &nbsp;|&nbsp; Exported: {export_time}</div>
</div>
"#,
        title = html_escape(title),
        created_at = created_at,
        msg_count = messages.len(),
        export_time = chrono::Local::now().format("%Y-%m-%d %H:%M:%S"),
    ));

    // Messages
    for msg in messages {
        let role = msg.get("role").and_then(|v| v.as_str()).unwrap_or("unknown");
        let empty_content = serde_json::json!({});
        let content = msg.get("content").unwrap_or(&empty_content);
        let role_label = match role {
            "user" => "User",
            "assistant" => "AI Assistant",
            _ => role,
        };
        let role_class = match role {
            "user" => "role-user",
            "assistant" => "role-assistant",
            _ => "",
        };
        let msg_class = match role {
            "user" => "message-user",
            "assistant" => "message-assistant",
            _ => "",
        };

        html.push_str(&format!(
            r#"<div class="message {}"><div class="message-role {}">💬 {}</div>"#,
            msg_class, role_class, role_label
        ));

        // Render content types in order
        render_content(&mut html, content);

        html.push_str("</div>\n");
    }

    // Footer
    html.push_str(&format!(
        r#"<div class="footer">Exported from AI小家 (组织专家，工作助手) — {}</div>
</body>
</html>"#,
        chrono::Local::now().format("%Y-%m-%d %H:%M:%S"),
    ));

    html
}

/// Render message content fields into HTML.
fn render_content(html: &mut String, content: &serde_json::Value) {
    // Progress
    if let Some(progress) = content.get("progress") {
        render_progress(html, progress);
    }

    // Text
    if let Some(text) = content.get("text").and_then(|v| v.as_str()) {
        if !text.trim().is_empty() {
            html.push_str("<div class=\"msg-text\">");
            html.push_str(&simple_markdown_to_html(text));
            html.push_str("</div>");
        }
    }

    // File attachments
    if let Some(files) = content.get("files").and_then(|v| v.as_array()) {
        for f in files {
            let name = f.get("fileName").and_then(|v| v.as_str()).unwrap_or("file");
            let ftype = f.get("fileType").and_then(|v| v.as_str()).unwrap_or("");
            html.push_str(&format!(
                r#"<div class="file-ref">📎 {} <span style="color:#8e8ea0;font-size:11px">({})</span></div>"#,
                html_escape(name), ftype
            ));
        }
    }

    // Code blocks
    if let Some(blocks) = content.get("codeBlocks").and_then(|v| v.as_array()) {
        for block in blocks {
            let lang = block.get("language").and_then(|v| v.as_str()).unwrap_or("");
            let code = block.get("code").and_then(|v| v.as_str()).unwrap_or("");
            let purpose = block.get("purpose").and_then(|v| v.as_str());
            if let Some(p) = purpose {
                html.push_str(&format!(r#"<div style="font-size:12px;color:#8e8ea0;margin-top:8px">📝 {}</div>"#, html_escape(p)));
            }
            html.push_str(&format!("<pre><code data-lang=\"{}\">", html_escape(lang)));
            html.push_str(&html_escape(code));
            html.push_str("</code></pre>");
        }
    }

    // Code results
    if let Some(results) = content.get("codeResults").and_then(|v| v.as_array()) {
        for r in results {
            let output = r.get("output").and_then(|v| v.as_str()).unwrap_or("");
            let is_error = r.get("isError").and_then(|v| v.as_bool()).unwrap_or(false);
            let class = if is_error { "code-result error" } else { "code-result" };
            html.push_str(&format!(r#"<div class="{}">📋 Output:
{}</div>"#, class, html_escape(output)));
        }
    }

    // Tables
    if let Some(tables) = content.get("tables").and_then(|v| v.as_array()) {
        for table in tables {
            render_table(html, table);
        }
    }

    // Metrics
    if let Some(metrics) = content.get("metrics").and_then(|v| v.as_array()) {
        html.push_str("<div class=\"metric-grid\">");
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
            html.push_str(&format!(
                r#"<div class="metric-card {}"><div class="metric-label">{}</div><div class="metric-value">{}</div><div class="metric-subtitle">{}</div></div>"#,
                state_class, html_escape(label), html_escape(value), html_escape(subtitle)
            ));
        }
        html.push_str("</div>");
    }

    // Options
    if let Some(option_groups) = content.get("options").and_then(|v| v.as_array()) {
        for group in option_groups {
            if let Some(options) = group.get("options").and_then(|v| v.as_array()) {
                html.push_str(r#"<div style="display:flex;flex-wrap:wrap;gap:10px;margin:12px 0">"#);
                for opt in options {
                    let title = opt.get("title").and_then(|v| v.as_str()).unwrap_or("");
                    let desc = opt.get("description").and_then(|v| v.as_str()).unwrap_or("");
                    html.push_str(&format!(
                        r#"<div style="flex:1;min-width:160px;padding:10px;border:1px solid #e0e0e8;border-radius:6px"><div style="font-weight:600;font-size:13px">{}</div><div style="font-size:12px;color:#8e8ea0;margin-top:4px">{}</div></div>"#,
                        html_escape(title), html_escape(desc)
                    ));
                }
                html.push_str("</div>");
            }
        }
    }

    // Anomalies
    if let Some(anomalies) = content.get("anomalies").and_then(|v| v.as_array()) {
        for a in anomalies {
            let priority = a.get("priority").and_then(|v| v.as_str()).unwrap_or("low");
            let title = a.get("title").and_then(|v| v.as_str()).unwrap_or("");
            let desc = a.get("description").and_then(|v| v.as_str()).unwrap_or("");
            let dot = match priority {
                "high" => "🔴",
                "medium" => "🟡",
                _ => "🟢",
            };
            html.push_str(&format!(
                r#"<div class="anomaly anomaly-{}"><span class="anomaly-dot">{}</span> <span class="anomaly-title">{}</span> — {}</div>"#,
                priority, dot, html_escape(title), html_escape(desc)
            ));
        }
    }

    // Insights
    if let Some(insights) = content.get("insights").and_then(|v| v.as_array()) {
        for i in insights {
            let title = i.get("title").and_then(|v| v.as_str()).unwrap_or("");
            let content_text = i.get("content").and_then(|v| v.as_str()).unwrap_or("");
            html.push_str(&format!(
                r#"<div class="insight"><div class="insight-title">💡 {}</div><div>{}</div></div>"#,
                html_escape(title), html_escape(content_text)
            ));
        }
    }

    // Root causes
    if let Some(root_causes) = content.get("rootCauses").and_then(|v| v.as_array()) {
        for rc in root_causes {
            let title = rc.get("title").and_then(|v| v.as_str()).unwrap_or("");
            html.push_str(&format!(r#"<div class="root-cause"><div style="font-weight:600;margin-bottom:6px">🔍 {}</div>"#, html_escape(title)));
            if let Some(items) = rc.get("items").and_then(|v| v.as_array()) {
                html.push_str("<ul style=\"margin:0;padding-left:20px\">");
                for item in items {
                    let label = item.get("label").and_then(|v| v.as_str()).unwrap_or("");
                    let detail = item.get("detail").and_then(|v| v.as_str()).unwrap_or("");
                    html.push_str(&format!("<li><strong>{}</strong>: {}</li>", html_escape(label), html_escape(detail)));
                }
                html.push_str("</ul>");
            }
            html.push_str("</div>");
        }
    }

    // Generated files
    if let Some(gen_files) = content.get("generatedFiles").and_then(|v| v.as_array()) {
        for gf in gen_files {
            let name = gf.get("fileName").and_then(|v| v.as_str()).unwrap_or("file");
            let desc = gf.get("description").and_then(|v| v.as_str()).unwrap_or("");
            html.push_str(&format!(
                r#"<div class="file-ref">📄 {} <span style="color:#8e8ea0;font-size:11px">— {}</span></div>"#,
                html_escape(name), html_escape(desc)
            ));
        }
    }

    // Reports
    if let Some(reports) = content.get("reports").and_then(|v| v.as_array()) {
        for rpt in reports {
            let title = rpt.get("title").and_then(|v| v.as_str()).unwrap_or("");
            let desc = rpt.get("description").and_then(|v| v.as_str()).unwrap_or("");
            html.push_str(&format!(
                r#"<div class="file-ref">📊 {} <span style="color:#8e8ea0;font-size:11px">— {}</span></div>"#,
                html_escape(title), html_escape(desc)
            ));
        }
    }

    // Search sources
    if let Some(search_sources) = content.get("searchSources").and_then(|v| v.as_array()) {
        for ss in search_sources {
            let title = ss.get("title").and_then(|v| v.as_str()).unwrap_or("References");
            html.push_str(&format!(r#"<div class="search-sources"><div style="font-weight:600;margin-bottom:6px">🔗 {}</div>"#, html_escape(title)));
            if let Some(items) = ss.get("items").and_then(|v| v.as_array()) {
                for item in items {
                    let source = item.get("source").and_then(|v| v.as_str()).unwrap_or("");
                    let snippet = item.get("snippet").and_then(|v| v.as_str()).unwrap_or("");
                    let url = item.get("url").and_then(|v| v.as_str());
                    if let Some(u) = url {
                        html.push_str(&format!(
                            r#"<div style="margin:4px 0"><a href="{}">{}</a> — <span style="font-size:12px;color:#666">{}</span></div>"#,
                            html_escape(u), html_escape(source), html_escape(snippet)
                        ));
                    } else {
                        html.push_str(&format!(
                            r#"<div style="margin:4px 0"><strong>{}</strong> — <span style="font-size:12px;color:#666">{}</span></div>"#,
                            html_escape(source), html_escape(snippet)
                        ));
                    }
                }
            }
            html.push_str("</div>");
        }
    }

    // Exec summary
    if let Some(exec) = content.get("execSummary") {
        let title = exec.get("title").and_then(|v| v.as_str()).unwrap_or("Summary");
        html.push_str(&format!(r#"<div style="margin:12px 0"><div style="font-weight:600;margin-bottom:8px">📋 {}</div>"#, html_escape(title)));
        if let Some(boxes) = exec.get("boxes").and_then(|v| v.as_array()) {
            html.push_str("<div class=\"exec-summary\">");
            for b in boxes {
                let label = b.get("label").and_then(|v| v.as_str()).unwrap_or("");
                let value = b.get("value").and_then(|v| v.as_str()).unwrap_or("");
                let subtitle = b.get("subtitle").and_then(|v| v.as_str()).unwrap_or("");
                html.push_str(&format!(
                    r#"<div class="exec-box"><div style="font-size:12px;color:#8e8ea0">{}</div><div style="font-size:20px;font-weight:700;margin:4px 0">{}</div><div style="font-size:11px;color:#8e8ea0">{}</div></div>"#,
                    html_escape(label), html_escape(value), html_escape(subtitle)
                ));
            }
            html.push_str("</div>");
        }
        html.push_str("</div>");
    }

    // Confirmations
    if let Some(confirmations) = content.get("confirmations").and_then(|v| v.as_array()) {
        for c in confirmations {
            let title = c.get("title").and_then(|v| v.as_str()).unwrap_or("");
            let status = c.get("status").and_then(|v| v.as_str()).unwrap_or("pending");
            let status_icon = match status {
                "confirmed" => "✅",
                "rejected" => "❌",
                _ => "⏳",
            };
            html.push_str(&format!(
                r#"<div class="confirm">{} <strong>{}</strong> <span style="font-size:12px;color:#8e8ea0">({})</span></div>"#,
                status_icon, html_escape(title), status
            ));
        }
    }
}

/// Render a data table.
fn render_table(html: &mut String, table: &serde_json::Value) {
    let title = table.get("title").and_then(|v| v.as_str());
    if let Some(t) = title {
        html.push_str(&format!(r#"<div style="font-weight:600;font-size:13px;margin-top:12px;margin-bottom:6px">{}</div>"#, html_escape(t)));
    }

    let columns = match table.get("columns").and_then(|v| v.as_array()) {
        Some(cols) => cols,
        None => return,
    };
    let rows = match table.get("rows").and_then(|v| v.as_array()) {
        Some(rows) => rows,
        None => return,
    };

    html.push_str("<table><thead><tr>");
    for col in columns {
        let label = col.get("label").and_then(|v| v.as_str()).unwrap_or("");
        html.push_str(&format!("<th>{}</th>", html_escape(label)));
    }
    html.push_str("</tr></thead><tbody>");

    for row in rows {
        html.push_str("<tr>");
        for col in columns {
            let key = col.get("key").and_then(|v| v.as_str()).unwrap_or("");
            let cell = row.get(key);
            let text = if let Some(cell) = cell {
                if let Some(t) = cell.get("text").and_then(|v| v.as_str()) {
                    t.to_string()
                } else if let Some(t) = cell.as_str() {
                    t.to_string()
                } else {
                    cell.to_string()
                }
            } else {
                String::new()
            };
            html.push_str(&format!("<td>{}</td>", html_escape(&text)));
        }
        html.push_str("</tr>");
    }
    html.push_str("</tbody></table>");
}

/// Render progress indicator.
fn render_progress(html: &mut String, progress: &serde_json::Value) {
    let title = progress.get("title").and_then(|v| v.as_str()).unwrap_or("Progress");
    html.push_str(&format!(r#"<div style="margin:12px 0"><div style="font-weight:600;font-size:13px;margin-bottom:6px">{}</div>"#, html_escape(title)));

    if let Some(steps) = progress.get("steps").and_then(|v| v.as_array()) {
        html.push_str("<div class=\"progress-indicator\">");
        for (i, step) in steps.iter().enumerate() {
            if i > 0 {
                html.push_str("<span class=\"progress-arrow\">→</span>");
            }
            let label = step.get("label").and_then(|v| v.as_str()).unwrap_or("");
            let status = step.get("status").and_then(|v| v.as_str()).unwrap_or("pending");
            let class = match status {
                "done" => "step-done",
                "active" => "step-active",
                _ => "step-pending",
            };
            html.push_str(&format!(r#"<span class="progress-step {}">{}</span>"#, class, html_escape(label)));
        }
        html.push_str("</div>");
    }
    html.push_str("</div>");
}

/// Simple markdown-to-HTML conversion for text content.
/// Handles: bold, italic, inline code, paragraphs, lists.
fn simple_markdown_to_html(text: &str) -> String {
    let mut result = String::with_capacity(text.len() * 2);
    let mut in_list = false;

    for line in text.lines() {
        let trimmed = line.trim();

        // List items
        if trimmed.starts_with("- ") || trimmed.starts_with("* ") {
            if !in_list {
                result.push_str("<ul>");
                in_list = true;
            }
            result.push_str("<li>");
            result.push_str(&inline_markdown(&html_escape(&trimmed[2..])));
            result.push_str("</li>");
            continue;
        }

        // Numbered lists
        if let Some(rest) = trimmed.strip_prefix(|c: char| c.is_ascii_digit()).and_then(|s| s.strip_prefix(". ")) {
            if !in_list {
                result.push_str("<ol>");
                in_list = true;
            }
            result.push_str("<li>");
            result.push_str(&inline_markdown(&html_escape(rest)));
            result.push_str("</li>");
            continue;
        }

        if in_list {
            // Check if current line format suggests we're still in a list context
            if trimmed.is_empty() {
                result.push_str("</ul>"); // Close the list on blank line
                in_list = false;
            }
        }

        // Headers
        if trimmed.starts_with("### ") {
            result.push_str(&format!("<h4>{}</h4>", inline_markdown(&html_escape(&trimmed[4..]))));
        } else if trimmed.starts_with("## ") {
            result.push_str(&format!("<h3>{}</h3>", inline_markdown(&html_escape(&trimmed[3..]))));
        } else if trimmed.starts_with("# ") {
            result.push_str(&format!("<h2>{}</h2>", inline_markdown(&html_escape(&trimmed[2..]))));
        } else if trimmed.is_empty() {
            result.push_str("<br>");
        } else {
            result.push_str("<p>");
            result.push_str(&inline_markdown(&html_escape(trimmed)));
            result.push_str("</p>");
        }
    }

    if in_list {
        result.push_str("</ul>");
    }

    result
}

/// Convert inline markdown (bold, italic, code) to HTML.
fn inline_markdown(text: &str) -> String {
    let mut result = text.to_string();
    // Bold: **text** or __text__
    while let Some(start) = result.find("**") {
        if let Some(end) = result[start + 2..].find("**") {
            let inner = &result[start + 2..start + 2 + end].to_string();
            result = format!("{}<strong>{}</strong>{}", &result[..start], inner, &result[start + 2 + end + 2..]);
        } else {
            break;
        }
    }
    // Inline code: `text`
    while let Some(start) = result.find('`') {
        if let Some(end) = result[start + 1..].find('`') {
            let inner = &result[start + 1..start + 1 + end].to_string();
            result = format!("{}<code>{}</code>{}", &result[..start], inner, &result[start + 1 + end + 1..]);
        } else {
            break;
        }
    }
    result
}

/// HTML-escape a string.
fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
     .replace('<', "&lt;")
     .replace('>', "&gt;")
     .replace('"', "&quot;")
}
