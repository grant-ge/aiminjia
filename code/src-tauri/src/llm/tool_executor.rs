//! Central tool execution dispatch — accepts a `ToolCall`, routes to the
//! correct handler, and returns a `ToolResult`.
//!
//! Each of the 10 registered tools has a dedicated `handle_*` async function.
//! Handlers never panic; errors are captured and returned as `ToolResult`
//! with `is_error = true`.

use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{anyhow, Result};
use log::{error, info};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use uuid::Uuid;

use crate::llm::streaming::ToolCall;
use crate::python::parser;
use crate::python::runner::PythonRunner;
use crate::search::tavily::TavilyClient;
use crate::search::searxng::SearxngClient;
use crate::storage::file_store::AppStorage;
use crate::storage::file_manager::FileManager;
use tauri::Emitter;

// ─────────────────────────────────────────────────
// Public types
// ─────────────────────────────────────────────────

/// Result of executing a tool.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolResult {
    pub tool_use_id: String,
    pub content: String,
    pub is_error: bool,
}

/// Context needed for tool execution.
pub struct ToolContext {
    pub db: Arc<AppStorage>,
    pub file_manager: Arc<FileManager>,
    pub workspace_path: PathBuf,
    pub conversation_id: String,
    pub tavily_api_key: Option<String>,
    pub app_handle: Option<tauri::AppHandle>,
}

// ─────────────────────────────────────────────────
// Dispatch
// ─────────────────────────────────────────────────

/// Execute a tool call and return the result.
///
/// Dispatches to the correct handler based on `tool_call.name`.
/// All errors are caught and returned as `ToolResult { is_error: true }`.
pub async fn execute_tool(ctx: &ToolContext, tool_call: &ToolCall) -> ToolResult {
    info!("Executing tool: {} (id={})", tool_call.name, tool_call.id);

    let result = match tool_call.name.as_str() {
        "web_search" => handle_web_search(ctx, &tool_call.arguments).await,
        "execute_python" => handle_execute_python(ctx, &tool_call.arguments).await,
        "analyze_file" => handle_analyze_file(ctx, &tool_call.arguments).await,
        "generate_report" => handle_generate_report(ctx, &tool_call.arguments).await,
        "generate_chart" => handle_generate_chart(ctx, &tool_call.arguments).await,
        "hypothesis_test" => handle_hypothesis_test(ctx, &tool_call.arguments).await,
        "detect_anomalies" => handle_detect_anomalies(ctx, &tool_call.arguments).await,
        "save_analysis_note" => handle_save_analysis_note(ctx, &tool_call.arguments).await,
        "export_data" => handle_export_data(ctx, &tool_call.arguments).await,
        "update_progress" => handle_update_progress(ctx, &tool_call.arguments).await,
        unknown => Err(anyhow!("Unknown tool: {}", unknown)),
    };

    match result {
        Ok(content) => ToolResult {
            tool_use_id: tool_call.id.clone(),
            content,
            is_error: false,
        },
        Err(e) => {
            error!("Tool '{}' failed: {:#}", tool_call.name, e);
            ToolResult {
                tool_use_id: tool_call.id.clone(),
                content: format!("Error: {}", e),
                is_error: true,
            }
        }
    }
}

// ─────────────────────────────────────────────────
// Argument extraction helpers
// ─────────────────────────────────────────────────

/// Extract a required string argument from a JSON Value.
fn require_str<'a>(args: &'a Value, key: &str) -> Result<&'a str> {
    args.get(key)
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow!("Missing required string argument: {}", key))
}

/// Extract an optional string argument.
fn optional_str<'a>(args: &'a Value, key: &str) -> Option<&'a str> {
    args.get(key).and_then(|v| v.as_str())
}

/// Extract an optional integer argument with a default value.
fn optional_i64(args: &Value, key: &str, default: i64) -> i64 {
    args.get(key).and_then(|v| v.as_i64()).unwrap_or(default)
}

/// Extract an optional f64 argument with a default value.
fn optional_f64(args: &Value, key: &str, default: f64) -> f64 {
    args.get(key).and_then(|v| v.as_f64()).unwrap_or(default)
}

// ─────────────────────────────────────────────────
// Tool handlers
// ─────────────────────────────────────────────────

/// 1. web_search — search the web via Tavily (if key configured) or free SearXNG fallback.
async fn handle_web_search(ctx: &ToolContext, args: &Value) -> Result<String> {
    let query = require_str(args, "query")?;
    let max_results = optional_i64(args, "max_results", 5) as u32;

    // Try Tavily first if an API key is available
    if let Some(api_key) = ctx.tavily_api_key.as_deref() {
        let client = TavilyClient::new(api_key.to_string());
        match client.search(query, true, max_results).await {
            Ok(response) => {
                let mut output = String::new();
                if let Some(answer) = &response.answer {
                    output.push_str(&format!("**Summary:** {}\n\n", answer));
                }
                for (i, result) in response.results.iter().enumerate() {
                    output.push_str(&format!(
                        "{}. **{}**\n   URL: {}\n   {}\n\n",
                        i + 1, result.title, result.url, result.content
                    ));
                }
                if output.is_empty() {
                    output = "No search results found.".to_string();
                }
                return Ok(output);
            }
            Err(e) => {
                info!("Tavily search failed, falling back to SearXNG: {}", e);
            }
        }
    }

    // Fallback: use free SearXNG (no API key needed)
    let client = SearxngClient::new();
    match client.search(query, max_results).await {
        Ok(results) => {
            let mut output = String::new();
            for (i, result) in results.iter().enumerate() {
                output.push_str(&format!(
                    "{}. **{}**\n   URL: {}\n   {}\n\n",
                    i + 1, result.title, result.url, result.content
                ));
            }

            if output.is_empty() {
                output = "No search results found.".to_string();
            }

            Ok(output)
        }
        Err(e) => {
            info!("SearXNG search also failed: {}", e);
            // Return a non-error result so the LLM doesn't retry infinitely
            Ok("[搜索不可用] Tavily 和 SearXNG 搜索均失败。请基于已有知识回答，不要编造搜索结果。".to_string())
        }
    }
}

/// 2. execute_python — run arbitrary Python code.
async fn handle_execute_python(ctx: &ToolContext, args: &Value) -> Result<String> {
    let code = require_str(args, "code")?;
    let purpose = optional_str(args, "purpose").unwrap_or("code execution");

    info!("[TOOL:execute_python] purpose='{}' code_len={} workspace={:?}",
        purpose, code.len(), ctx.workspace_path);
    info!("[TOOL:execute_python] code:\n{}", code);

    let runner = PythonRunner::new(ctx.workspace_path.clone(), ctx.app_handle.as_ref());
    let result = runner.execute(code).await?;

    info!("[TOOL:execute_python] exit_code={} time={}ms timed_out={} stdout_len={} stderr_len={}",
        result.exit_code, result.execution_time_ms, result.timed_out,
        result.stdout.len(), result.stderr.len());

    if result.timed_out {
        return Ok(format!(
            "Execution timed out after {}ms.\nPartial stderr: {}",
            result.execution_time_ms, result.stderr
        ));
    }

    // Auto-register files created by _export_detail (detected via __GENERATED_FILE__ markers)
    let mut generated_files_info = Vec::new();
    let mut clean_stdout = String::new();
    for line in result.stdout.lines() {
        if let Some(json_str) = line.strip_prefix("__GENERATED_FILE__:") {
            if let Ok(file_meta) = serde_json::from_str::<Value>(json_str) {
                let rel_path = file_meta.get("path").and_then(|v| v.as_str()).unwrap_or("");
                let filename = file_meta.get("filename").and_then(|v| v.as_str()).unwrap_or("");
                let title = file_meta.get("title").and_then(|v| v.as_str()).unwrap_or("");
                let fmt = file_meta.get("format").and_then(|v| v.as_str()).unwrap_or("excel");

                let full_path = ctx.workspace_path.join(rel_path);
                let file_size = std::fs::metadata(&full_path).map(|m| m.len() as i64).unwrap_or(0);
                let file_id = Uuid::new_v4().to_string();

                if let Err(e) = ctx.db.insert_generated_file(
                    &file_id,
                    &ctx.conversation_id,
                    None,
                    filename,
                    rel_path,
                    fmt,
                    file_size,
                    "data",
                    Some(title),
                    1, true, None, None, None,
                ) {
                    error!("Failed to register generated file '{}': {}", filename, e);
                } else {
                    info!("[TOOL:execute_python] auto-registered file: {} ({})", filename, file_id);
                    generated_files_info.push(json!({
                        "fileId": file_id,
                        "fileName": filename,
                        "storedPath": rel_path,
                        "fileSize": file_size,
                    }));
                }
            }
            // Don't include the marker line in output
        } else {
            clean_stdout.push_str(line);
            clean_stdout.push('\n');
        }
    }

    let mut output = String::new();
    output.push_str(&format!("[Purpose: {}]\n", purpose));
    output.push_str(&format!("Exit code: {}\n", result.exit_code));
    output.push_str(&format!(
        "Execution time: {}ms\n",
        result.execution_time_ms
    ));

    if !clean_stdout.is_empty() {
        output.push_str(&format!("\n--- stdout ---\n{}\n", clean_stdout.trim_end()));
    }
    if !result.stderr.is_empty() {
        output.push_str(&format!("\n--- stderr ---\n{}\n", result.stderr));
    }

    // Append generated file info so the LLM knows about registered files
    if !generated_files_info.is_empty() {
        output.push_str("\n--- generated_files ---\n");
        for fi in &generated_files_info {
            output.push_str(&format!(
                "File registered: {} (fileId: {}, size: {} bytes)\n",
                fi["fileName"].as_str().unwrap_or(""),
                fi["fileId"].as_str().unwrap_or(""),
                fi["fileSize"].as_i64().unwrap_or(0),
            ));
        }
    }

    Ok(output)
}

/// 3. analyze_file — parse and describe an uploaded file.
async fn handle_analyze_file(ctx: &ToolContext, args: &Value) -> Result<String> {
    let file_id = require_str(args, "file_id")?;
    info!("[TOOL:analyze_file] file_id='{}' conversation_id='{}'", file_id, ctx.conversation_id);

    // Look up the file record in the database, verified against current conversation.
    let file_record = ctx
        .db
        .get_uploaded_file_for_conversation(file_id, &ctx.conversation_id)?
        .ok_or_else(|| anyhow!(
            "Uploaded file not found or does not belong to this conversation: {}",
            file_id
        ))?;

    let stored_path = file_record
        .get("storedPath")
        .and_then(|v| v.as_str())
        .ok_or_else(|| anyhow!("File record missing storedPath"))?;

    let original_name = file_record
        .get("originalName")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown");

    let full_path = ctx.file_manager.full_path(stored_path);

    let runner = PythonRunner::new(ctx.workspace_path.clone(), ctx.app_handle.as_ref());
    let parse_result = parser::parse_file(&runner, &full_path).await?;

    // Return both the relative stored_path (for DB references) and the
    // absolute path (for direct use in execute_python code).
    // originalName is included so the LLM can refer to files by their
    // user-facing name instead of the randomized stored filename.
    let output = serde_json::to_string_pretty(&json!({
        "originalName": original_name,
        "format": parse_result.format,
        "columns": parse_result.column_names,
        "rowCount": parse_result.row_count,
        "schemaSummary": parse_result.schema_summary,
        "sampleData": parse_result.sample_data,
        "filePath": full_path.to_string_lossy(),
        "storedPath": stored_path,
    }))?;

    Ok(output)
}

/// 4. generate_report — create an HTML report and save it.
async fn handle_generate_report(ctx: &ToolContext, args: &Value) -> Result<String> {
    let title = require_str(args, "title")?;
    let sections = args
        .get("sections")
        .and_then(|v| v.as_array())
        .ok_or_else(|| anyhow!("Missing required array argument: sections"))?;
    let format = optional_str(args, "format").unwrap_or("html");

    let content = if format == "markdown" {
        build_markdown_report(title, sections)
    } else {
        build_html_report(title, sections)
    };

    let extension = if format == "markdown" { "md" } else { "html" };
    let file_name = format!(
        "report_{}_{}.{}",
        slugify(title),
        Uuid::new_v4().to_string().split('-').next().unwrap_or("x"),
        extension,
    );

    let file_info = ctx
        .file_manager
        .write_file("reports", &file_name, content.as_bytes())?;

    // Record in the database.
    let file_id = Uuid::new_v4().to_string();
    ctx.db.insert_generated_file(
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
    )?;

    Ok(serde_json::to_string_pretty(&json!({
        "fileId": file_id,
        "fileName": file_info.file_name,
        "storedPath": file_info.stored_path,
        "fileSize": file_info.file_size,
        "format": format,
    }))?)
}

/// 5. generate_chart — create a matplotlib chart and save the PNG.
async fn handle_generate_chart(ctx: &ToolContext, args: &Value) -> Result<String> {
    let chart_type = require_str(args, "chart_type")?;
    let title = require_str(args, "title")?;
    let data = args
        .get("data")
        .ok_or_else(|| anyhow!("Missing required argument: data"))?;
    let options = args.get("options").cloned().unwrap_or(json!({}));

    let chart_filename = format!(
        "chart_{}.png",
        Uuid::new_v4().to_string().split('-').next().unwrap_or("x"),
    );
    let chart_dir = ctx.workspace_path.join("charts");
    std::fs::create_dir_all(&chart_dir)?;
    let output_path = chart_dir.join(&chart_filename);

    let python_code = build_chart_python(
        chart_type,
        title,
        data,
        &options,
        &output_path.to_string_lossy(),
    );

    let runner = PythonRunner::new(ctx.workspace_path.clone(), ctx.app_handle.as_ref());
    let result = runner.execute(&python_code).await?;

    if result.exit_code != 0 {
        return Err(anyhow!(
            "Chart generation failed (exit {}):\n{}",
            result.exit_code,
            if result.stderr.is_empty() {
                &result.stdout
            } else {
                &result.stderr
            }
        ));
    }

    // Write the file info (the Python script already saved the PNG).
    let stored_path = format!("charts/{}", chart_filename);
    let file_size = std::fs::metadata(&output_path)
        .map(|m| m.len())
        .unwrap_or(0);

    let file_id = Uuid::new_v4().to_string();
    ctx.db.insert_generated_file(
        &file_id,
        &ctx.conversation_id,
        None,
        &chart_filename,
        &stored_path,
        "png",
        file_size as i64,
        "chart",
        Some(title),
        1,
        true,
        None,
        None,
        None,
    )?;

    Ok(serde_json::to_string_pretty(&json!({
        "fileId": file_id,
        "fileName": chart_filename,
        "storedPath": stored_path,
        "fileSize": file_size,
        "chartType": chart_type,
    }))?)
}

/// 6. hypothesis_test — run a statistical hypothesis test via Python.
async fn handle_hypothesis_test(ctx: &ToolContext, args: &Value) -> Result<String> {
    let test_type = require_str(args, "test_type")?;
    let groups = args
        .get("groups")
        .and_then(|v| v.as_array())
        .ok_or_else(|| anyhow!("Missing required array argument: groups"))?;
    let data_source = optional_str(args, "data_source");
    let significance_level = optional_f64(args, "significance_level", 0.05);

    let group_names: Vec<&str> = groups.iter().filter_map(|v| v.as_str()).collect();

    let python_code =
        build_hypothesis_test_python(test_type, &group_names, data_source, significance_level)?;

    let runner = PythonRunner::new(ctx.workspace_path.clone(), ctx.app_handle.as_ref());
    let result = runner.execute(&python_code).await?;

    if result.exit_code != 0 {
        return Err(anyhow!(
            "Hypothesis test failed:\n{}",
            if result.stderr.is_empty() {
                &result.stdout
            } else {
                &result.stderr
            }
        ));
    }

    Ok(result.stdout)
}

/// 7. detect_anomalies — find outliers via Z-score, IQR, or Grubbs.
async fn handle_detect_anomalies(ctx: &ToolContext, args: &Value) -> Result<String> {
    let column = require_str(args, "column")?;
    let method = optional_str(args, "method").unwrap_or("zscore");
    let threshold = optional_f64(args, "threshold", 3.0);
    let group_by = optional_str(args, "group_by");

    let python_code = build_anomaly_detection_python(column, method, threshold, group_by)?;

    let runner = PythonRunner::new(ctx.workspace_path.clone(), ctx.app_handle.as_ref());
    let result = runner.execute(&python_code).await?;

    if result.exit_code != 0 {
        return Err(anyhow!(
            "Anomaly detection failed:\n{}",
            if result.stderr.is_empty() {
                &result.stdout
            } else {
                &result.stderr
            }
        ));
    }

    Ok(result.stdout)
}

/// 8. save_analysis_note — store an intermediate finding in enterprise memory.
async fn handle_save_analysis_note(ctx: &ToolContext, args: &Value) -> Result<String> {
    let key = require_str(args, "key")?;
    let content = require_str(args, "content")?;
    let step = optional_i64(args, "step", 0);

    // Prefix key with conversation id for scoping.
    let full_key = format!("note:{}:{}", ctx.conversation_id, key);
    let source = format!("analysis_step_{}", step);

    ctx.db.set_memory(&full_key, content, Some(&source))?;

    Ok(json!({
        "status": "saved",
        "key": key,
        "step": step,
    })
    .to_string())
}

/// 9. export_data — write data to CSV, Excel, or JSON.
async fn handle_export_data(ctx: &ToolContext, args: &Value) -> Result<String> {
    let data = args
        .get("data")
        .ok_or_else(|| anyhow!("Missing required argument: data"))?;
    let format = require_str(args, "format")?;
    let filename = require_str(args, "filename")?;

    // Ensure the filename has the correct extension.
    let filename = ensure_extension(filename, format);

    let python_code = build_export_python(data, format, &filename)?;

    let runner = PythonRunner::new(ctx.workspace_path.clone(), ctx.app_handle.as_ref());
    let result = runner.execute(&python_code).await?;

    if result.exit_code != 0 {
        return Err(anyhow!(
            "Export failed:\n{}",
            if result.stderr.is_empty() {
                &result.stdout
            } else {
                &result.stderr
            }
        ));
    }

    // Record the file in the database.
    let stored_path = format!("exports/{}", filename);
    let full_path = ctx.workspace_path.join(&stored_path);
    let file_size = std::fs::metadata(&full_path)
        .map(|m| m.len())
        .unwrap_or(0);

    let file_id = Uuid::new_v4().to_string();
    ctx.db.insert_generated_file(
        &file_id,
        &ctx.conversation_id,
        None,
        &filename,
        &stored_path,
        format,
        file_size as i64,
        "data",
        None,
        1,
        true,
        None,
        None,
        None,
    )?;

    Ok(serde_json::to_string_pretty(&json!({
        "fileId": file_id,
        "fileName": filename,
        "storedPath": stored_path,
        "fileSize": file_size,
        "format": format,
    }))?)
}

/// 10. update_progress — update the analysis progress state.
async fn handle_update_progress(ctx: &ToolContext, args: &Value) -> Result<String> {
    let current_step = optional_i64(args, "current_step", 1) as i32;
    let step_status = require_str(args, "step_status")?;
    let summary = optional_str(args, "summary").unwrap_or("");

    let state_data = json!({
        "summary": summary,
    })
    .to_string();

    let step_status_json = json!({
        format!("step_{}", current_step): step_status,
    })
    .to_string();

    ctx.db.upsert_analysis_state(
        &ctx.conversation_id,
        current_step,
        &step_status_json,
        &state_data,
    )?;

    if let Some(ref app) = ctx.app_handle {
        let _ = app.emit("analysis:step-changed", serde_json::json!({
            "step": current_step,
            "status": step_status,
        }));
    }

    Ok(json!({
        "status": "updated",
        "currentStep": current_step,
        "stepStatus": step_status,
        "summary": summary,
    })
    .to_string())
}

// ─────────────────────────────────────────────────
// Code generation helpers
// ─────────────────────────────────────────────────

/// Build a professional HTML report from a title and sections array.
///
/// Each section supports multiple content types:
/// - `content` (string) — text/markdown content, converted to HTML
/// - `table` (object) — structured table { columns: [...], rows: [[...], ...] }
/// - `metrics` (array) — metric cards [{ label, value, subtitle?, state? }]
/// - `items` (array) — bullet list of strings
/// - `highlight` (string) — highlighted callout box
/// - `chartPath` (string) — relative path to a chart image to embed
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

/// Escape a string for safe embedding in a Python single-quoted string literal.
/// Replaces `\` with `\\` and `'` with `\'` to prevent code injection.
fn py_escape(s: &str) -> String {
    s.replace('\\', "\\\\").replace('\'', "\\'")
}

/// Generate Python code for a matplotlib chart.
fn build_chart_python(
    chart_type: &str,
    title: &str,
    data: &Value,
    options: &Value,
    output_path: &str,
) -> String {
    let data_json = serde_json::to_string(data).unwrap_or_else(|_| "{}".to_string());
    let options_json = serde_json::to_string(options).unwrap_or_else(|_| "{}".to_string());

    // Use json.loads for data (already JSON-serialized) and py_escape for string literals
    let escaped_chart_type = py_escape(chart_type);
    let escaped_title = py_escape(title);
    let escaped_output_path = py_escape(output_path);

    format!(
        r#"
import matplotlib
matplotlib.use('Agg')
import matplotlib.pyplot as plt
import json
import numpy as np

data = json.loads('''{data_json}''')
options = json.loads('''{options_json}''')
chart_type = '{chart_type}'
title = '{title}'
output_path = r'{output_path}'

fig, ax = plt.subplots(figsize=options.get('figsize', (10, 6)))

labels = data.get('labels', [])
values = data.get('values', [])

if chart_type == 'bar':
    if isinstance(values[0], list) if values else False:
        x = np.arange(len(labels))
        width = 0.8 / len(values)
        for i, v in enumerate(values):
            ax.bar(x + i * width, v, width, label=data.get('series_names', [f'Series {{i+1}}'])[i] if i < len(data.get('series_names', [])) else f'Series {{i+1}}')
        ax.set_xticks(x + width * (len(values) - 1) / 2)
        ax.set_xticklabels(labels, rotation=45, ha='right')
        ax.legend()
    else:
        ax.bar(labels, values)
        plt.xticks(rotation=45, ha='right')

elif chart_type == 'line':
    if isinstance(values[0], list) if values else False:
        for i, v in enumerate(values):
            name = data.get('series_names', [f'Series {{i+1}}'])[i] if i < len(data.get('series_names', [])) else f'Series {{i+1}}'
            ax.plot(labels, v, marker='o', label=name)
        ax.legend()
    else:
        ax.plot(labels, values, marker='o')

elif chart_type == 'scatter':
    x_vals = data.get('x', [])
    y_vals = data.get('y', [])
    ax.scatter(x_vals, y_vals, alpha=0.7)
    ax.set_xlabel(data.get('x_label', 'X'))
    ax.set_ylabel(data.get('y_label', 'Y'))

elif chart_type == 'box':
    box_data = data.get('groups', [values])
    ax.boxplot(box_data, labels=labels if labels else None)

elif chart_type == 'heatmap':
    matrix = np.array(data.get('matrix', [[]]))
    im = ax.imshow(matrix, cmap='YlOrRd', aspect='auto')
    plt.colorbar(im, ax=ax)
    if labels:
        ax.set_xticks(range(len(labels)))
        ax.set_xticklabels(labels, rotation=45, ha='right')
    y_labels = data.get('y_labels', [])
    if y_labels:
        ax.set_yticks(range(len(y_labels)))
        ax.set_yticklabels(y_labels)

ax.set_title(title)
plt.tight_layout()
plt.savefig(output_path, dpi=150, bbox_inches='tight')
plt.close()

print(f"Chart saved to {{output_path}}")
"#,
        data_json = data_json,
        options_json = options_json,
        chart_type = escaped_chart_type,
        title = escaped_title,
        output_path = escaped_output_path,
    )
}

/// Generate Python code for a hypothesis test.
fn build_hypothesis_test_python(
    test_type: &str,
    groups: &[&str],
    data_source: Option<&str>,
    significance_level: f64,
) -> Result<String> {
    let groups_json =
        serde_json::to_string(groups).unwrap_or_else(|_| "[]".to_string());

    let load_data = if let Some(source) = data_source {
        let escaped_source = py_escape(source);
        format!(
            r#"
import pandas as pd
import os

# Try to load from source
source = r'{source}'
if os.path.isfile(source):
    df = _smart_read_data(source)
else:
    # Assume source is a file ID and look in uploads/
    import glob
    files = glob.glob(f'uploads/*')
    if files:
        f = files[0]
        df = _smart_read_data(f)
    else:
        raise FileNotFoundError(f"No data file found for source: {{source}}")
"#,
            source = escaped_source,
        )
    } else {
        r#"
import pandas as pd
import glob

# Auto-detect data file in uploads
files = glob.glob('uploads/*')
if not files:
    raise FileNotFoundError("No data files found in uploads/")
f = files[0]
df = pd.read_csv(f) if f.endswith('.csv') else pd.read_excel(f)
"#
        .to_string()
    };

    let test_code = match test_type {
        "t_test" => format!(
            r#"
from scipy import stats
group_cols = {groups}
if len(group_cols) < 2:
    raise ValueError("t-test requires at least 2 groups")
g1 = df[group_cols[0]].dropna()
g2 = df[group_cols[1]].dropna()
stat, p_value = stats.ttest_ind(g1, g2)
print(f"T-test: t-statistic={{stat:.4f}}, p-value={{p_value:.6f}}")
print(f"Significant at alpha={alpha}: {{p_value < {alpha}}}")
print(f"Group 1 ({{group_cols[0]}}): mean={{g1.mean():.4f}}, std={{g1.std():.4f}}, n={{len(g1)}}")
print(f"Group 2 ({{group_cols[1]}}): mean={{g2.mean():.4f}}, std={{g2.std():.4f}}, n={{len(g2)}}")
"#,
            groups = groups_json,
            alpha = significance_level,
        ),
        "anova" => format!(
            r#"
from scipy import stats
group_cols = {groups}
group_data = [df[col].dropna().values for col in group_cols]
stat, p_value = stats.f_oneway(*group_data)
print(f"ANOVA: F-statistic={{stat:.4f}}, p-value={{p_value:.6f}}")
print(f"Significant at alpha={alpha}: {{p_value < {alpha}}}")
for col in group_cols:
    vals = df[col].dropna()
    print(f"  {{col}}: mean={{vals.mean():.4f}}, std={{vals.std():.4f}}, n={{len(vals)}}")
"#,
            groups = groups_json,
            alpha = significance_level,
        ),
        "chi_square" => format!(
            r#"
from scipy import stats
import numpy as np
group_cols = {groups}
if len(group_cols) < 2:
    raise ValueError("Chi-square test requires at least 2 columns")
contingency = pd.crosstab(df[group_cols[0]], df[group_cols[1]])
stat, p_value, dof, expected = stats.chi2_contingency(contingency)
print(f"Chi-square test: statistic={{stat:.4f}}, p-value={{p_value:.6f}}, dof={{dof}}")
print(f"Significant at alpha={alpha}: {{p_value < {alpha}}}")
print(f"Contingency table:\n{{contingency}}")
"#,
            groups = groups_json,
            alpha = significance_level,
        ),
        "regression" => format!(
            r#"
from scipy import stats
group_cols = {groups}
if len(group_cols) < 2:
    raise ValueError("Regression requires at least 2 columns (x, y)")
x = df[group_cols[0]].dropna()
y = df[group_cols[1]].dropna()
# Align indices
common = x.index.intersection(y.index)
x, y = x[common], y[common]
slope, intercept, r_value, p_value, std_err = stats.linregress(x, y)
print(f"Linear Regression:")
print(f"  slope={{slope:.4f}}, intercept={{intercept:.4f}}")
print(f"  R-squared={{r_value**2:.4f}}")
print(f"  p-value={{p_value:.6f}}, std_err={{std_err:.4f}}")
print(f"  Significant at alpha={alpha}: {{p_value < {alpha}}}")
"#,
            groups = groups_json,
            alpha = significance_level,
        ),
        "mann_whitney" => format!(
            r#"
from scipy import stats
group_cols = {groups}
if len(group_cols) < 2:
    raise ValueError("Mann-Whitney test requires at least 2 groups")
g1 = df[group_cols[0]].dropna()
g2 = df[group_cols[1]].dropna()
stat, p_value = stats.mannwhitneyu(g1, g2, alternative='two-sided')
print(f"Mann-Whitney U test: U-statistic={{stat:.4f}}, p-value={{p_value:.6f}}")
print(f"Significant at alpha={alpha}: {{p_value < {alpha}}}")
print(f"Group 1 ({{group_cols[0]}}): median={{g1.median():.4f}}, n={{len(g1)}}")
print(f"Group 2 ({{group_cols[1]}}): median={{g2.median():.4f}}, n={{len(g2)}}")
"#,
            groups = groups_json,
            alpha = significance_level,
        ),
        other => {
            return Err(anyhow!(
                "Unsupported test type: {}. Supported: t_test, anova, chi_square, regression, mann_whitney",
                other
            ));
        }
    };

    Ok(format!(
        r#"
import json
import sys
import warnings
warnings.filterwarnings('ignore')

try:
{load_data}
{test_code}
except Exception as e:
    print(f"Error: {{e}}", file=sys.stderr)
    sys.exit(1)
"#,
        load_data = load_data,
        test_code = test_code,
    ))
}

/// Generate Python code for anomaly detection.
fn build_anomaly_detection_python(
    column: &str,
    method: &str,
    threshold: f64,
    group_by: Option<&str>,
) -> Result<String> {
    let escaped_column = py_escape(column);
    let detection_code = match method {
        "zscore" => format!(
            r#"
from scipy import stats
import numpy as np

def detect_zscore(series, threshold):
    z_scores = np.abs(stats.zscore(series.dropna()))
    mask = z_scores > threshold
    anomalies = series.dropna()[mask]
    return anomalies, z_scores

col = '{column}'
threshold = {threshold}

if group_by:
    for group_name, group_df in df.groupby(group_by):
        data = group_df[col].dropna()
        if len(data) < 3:
            continue
        anomalies, z_scores = detect_zscore(data, threshold)
        print(f"Group '{{group_name}}': {{len(anomalies)}} anomalies out of {{len(data)}} values")
        if len(anomalies) > 0:
            print(f"  Anomalous values: {{anomalies.tolist()}}")
else:
    data = df[col].dropna()
    anomalies, z_scores = detect_zscore(data, threshold)
    print(f"Column '{{col}}': {{len(anomalies)}} anomalies out of {{len(data)}} values (z-score > {{threshold}})")
    if len(anomalies) > 0:
        print(f"  Anomalous values: {{anomalies.tolist()}}")
    print(f"  Mean: {{data.mean():.4f}}, Std: {{data.std():.4f}}")
    print(f"  Min: {{data.min():.4f}}, Max: {{data.max():.4f}}")
"#,
            column = escaped_column,
            threshold = threshold,
        ),
        "iqr" => format!(
            r#"
import numpy as np

def detect_iqr(series, multiplier):
    q1 = series.quantile(0.25)
    q3 = series.quantile(0.75)
    iqr = q3 - q1
    lower = q1 - multiplier * iqr
    upper = q3 + multiplier * iqr
    anomalies = series[(series < lower) | (series > upper)]
    return anomalies, lower, upper

col = '{column}'
multiplier = {threshold}

if group_by:
    for group_name, group_df in df.groupby(group_by):
        data = group_df[col].dropna()
        if len(data) < 4:
            continue
        anomalies, lower, upper = detect_iqr(data, multiplier)
        print(f"Group '{{group_name}}': {{len(anomalies)}} anomalies, bounds=[{{lower:.4f}}, {{upper:.4f}}]")
        if len(anomalies) > 0:
            print(f"  Anomalous values: {{anomalies.tolist()}}")
else:
    data = df[col].dropna()
    anomalies, lower, upper = detect_iqr(data, multiplier)
    print(f"Column '{{col}}': {{len(anomalies)}} anomalies (IQR multiplier={{multiplier}})")
    print(f"  Bounds: [{{lower:.4f}}, {{upper:.4f}}]")
    if len(anomalies) > 0:
        print(f"  Anomalous values: {{anomalies.tolist()}}")
    print(f"  Q1: {{data.quantile(0.25):.4f}}, Q3: {{data.quantile(0.75):.4f}}")
"#,
            column = escaped_column,
            threshold = threshold,
        ),
        "grubbs" => format!(
            r#"
from scipy import stats
import numpy as np

def grubbs_test(data, alpha=0.05):
    n = len(data)
    mean = np.mean(data)
    std = np.std(data, ddof=1)
    if std == 0:
        return [], []
    abs_dev = np.abs(data - mean)
    max_idx = np.argmax(abs_dev)
    G = abs_dev[max_idx] / std
    t_crit = stats.t.ppf(1 - alpha / (2 * n), n - 2)
    G_crit = (n - 1) / np.sqrt(n) * np.sqrt(t_crit**2 / (n - 2 + t_crit**2))
    if G > G_crit:
        return [data.iloc[max_idx]], [max_idx]
    return [], []

col = '{column}'

if group_by:
    for group_name, group_df in df.groupby(group_by):
        data = group_df[col].dropna()
        if len(data) < 3:
            continue
        anomalies, indices = grubbs_test(data)
        print(f"Group '{{group_name}}': {{len(anomalies)}} anomalies detected by Grubbs test")
        if anomalies:
            print(f"  Anomalous values: {{anomalies}}")
else:
    data = df[col].dropna()
    anomalies, indices = grubbs_test(data)
    print(f"Column '{{col}}': {{len(anomalies)}} anomalies detected by Grubbs test")
    if anomalies:
        print(f"  Anomalous values: {{anomalies}}")
    print(f"  Mean: {{data.mean():.4f}}, Std: {{data.std():.4f}}, n={{len(data)}}")
"#,
            column = escaped_column,
        ),
        other => {
            return Err(anyhow!(
                "Unsupported anomaly detection method: {}. Supported: zscore, iqr, grubbs",
                other
            ));
        }
    };

    let group_by_code = if let Some(gb) = group_by {
        format!("group_by = '{}'", py_escape(gb))
    } else {
        "group_by = None".to_string()
    };

    Ok(format!(
        r#"
import pandas as pd
import json
import sys
import glob
import warnings
warnings.filterwarnings('ignore')

try:
    # Auto-detect data file
    files = glob.glob('uploads/*')
    if not files:
        raise FileNotFoundError("No data files found in uploads/")
    f = files[0]
    df = pd.read_csv(f) if f.endswith('.csv') else pd.read_excel(f)

    {group_by_code}
{detection_code}
except Exception as e:
    print(f"Error: {{e}}", file=sys.stderr)
    sys.exit(1)
"#,
        group_by_code = group_by_code,
        detection_code = detection_code,
    ))
}

/// Generate Python code for data export.
fn build_export_python(data: &Value, format: &str, filename: &str) -> Result<String> {
    let data_json = serde_json::to_string(data)?;
    let escaped_filename = py_escape(filename);

    let write_code = match format {
        "csv" => format!(
            r#"df.to_csv(output_path, index=False, encoding='utf-8-sig')
print(f"Exported {{len(df)}} rows to {{output_path}}")"#,
        ),
        "excel" => format!(
            r#"df.to_excel(output_path, index=False, engine='openpyxl')
print(f"Exported {{len(df)}} rows to {{output_path}}")"#,
        ),
        "json" => format!(
            r#"df.to_json(output_path, orient='records', force_ascii=False, indent=2)
print(f"Exported {{len(df)}} rows to {{output_path}}")"#,
        ),
        other => {
            return Err(anyhow!(
                "Unsupported export format: {}. Supported: csv, excel, json",
                other
            ));
        }
    };

    Ok(format!(
        r#"
import pandas as pd
import json
import os
import sys

try:
    data = json.loads('''{data_json}''')

    # Handle various data shapes
    if isinstance(data, list):
        df = pd.DataFrame(data)
    elif isinstance(data, dict):
        if 'columns' in data and 'rows' in data:
            df = pd.DataFrame(data['rows'], columns=data['columns'])
        elif 'records' in data:
            df = pd.DataFrame(data['records'])
        else:
            df = pd.DataFrame([data])
    else:
        df = pd.DataFrame(data)

    # Ensure exports directory exists
    os.makedirs('exports', exist_ok=True)
    output_path = os.path.join('exports', '{filename}')

    {write_code}
except Exception as e:
    print(f"Error: {{e}}", file=sys.stderr)
    sys.exit(1)
"#,
        data_json = data_json,
        filename = escaped_filename,
        write_code = write_code,
    ))
}

/// Create a URL-safe slug from a title string.
fn slugify(s: &str) -> String {
    s.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '_'
            }
        })
        .collect::<String>()
        .trim_matches('_')
        .to_string()
}

/// Ensure a filename has the correct extension for the given format.
fn ensure_extension(filename: &str, format: &str) -> String {
    let expected_ext = match format {
        "csv" => "csv",
        "excel" => "xlsx",
        "json" => "json",
        _ => format,
    };

    if filename.ends_with(&format!(".{}", expected_ext)) {
        filename.to_string()
    } else {
        // Strip any existing extension and add the correct one.
        let base = filename
            .rsplit_once('.')
            .map(|(base, _)| base)
            .unwrap_or(filename);
        format!("{}.{}", base, expected_ext)
    }
}

// ─────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── Argument extraction tests ────────────────

    #[test]
    fn test_require_str_present() {
        let args = json!({"name": "hello"});
        assert_eq!(require_str(&args, "name").unwrap(), "hello");
    }

    #[test]
    fn test_require_str_missing() {
        let args = json!({"other": 42});
        assert!(require_str(&args, "name").is_err());
    }

    #[test]
    fn test_require_str_wrong_type() {
        let args = json!({"name": 123});
        assert!(require_str(&args, "name").is_err());
    }

    #[test]
    fn test_optional_str_present() {
        let args = json!({"key": "value"});
        assert_eq!(optional_str(&args, "key"), Some("value"));
    }

    #[test]
    fn test_optional_str_missing() {
        let args = json!({});
        assert_eq!(optional_str(&args, "key"), None);
    }

    #[test]
    fn test_optional_i64_present() {
        let args = json!({"count": 10});
        assert_eq!(optional_i64(&args, "count", 5), 10);
    }

    #[test]
    fn test_optional_i64_missing() {
        let args = json!({});
        assert_eq!(optional_i64(&args, "count", 5), 5);
    }

    #[test]
    fn test_optional_f64_present() {
        let args = json!({"alpha": 0.01});
        assert!((optional_f64(&args, "alpha", 0.05) - 0.01).abs() < f64::EPSILON);
    }

    #[test]
    fn test_optional_f64_missing() {
        let args = json!({});
        assert!((optional_f64(&args, "alpha", 0.05) - 0.05).abs() < f64::EPSILON);
    }

    // ── save_analysis_note tests ─────────────────

    #[tokio::test]
    async fn test_handle_save_analysis_note() {
        let (db, _dir) = create_test_db();
        let ctx = create_test_context(db);

        let args = json!({
            "key": "salary_distribution",
            "content": "Salary follows a log-normal distribution",
            "step": 2
        });

        let result = handle_save_analysis_note(&ctx, &args).await.unwrap();
        let parsed: Value = serde_json::from_str(&result).unwrap();

        assert_eq!(parsed["status"], "saved");
        assert_eq!(parsed["key"], "salary_distribution");
        assert_eq!(parsed["step"], 2);

        // Verify the memory was actually stored.
        let full_key = format!("note:{}:salary_distribution", ctx.conversation_id);
        let stored = ctx.db.get_memory(&full_key).unwrap();
        assert_eq!(stored, Some("Salary follows a log-normal distribution".to_string()));
    }

    #[tokio::test]
    async fn test_handle_save_analysis_note_missing_key() {
        let (db, _dir) = create_test_db();
        let ctx = create_test_context(db);

        let args = json!({"content": "some note"});
        let result = handle_save_analysis_note(&ctx, &args).await;
        assert!(result.is_err());
    }

    // ── update_progress tests ────────────────────

    #[tokio::test]
    async fn test_handle_update_progress() {
        let (db, _dir) = create_test_db();
        let ctx = create_test_context(db);

        let args = json!({
            "current_step": 3,
            "step_status": "completed",
            "summary": "Statistical analysis done"
        });

        let result = handle_update_progress(&ctx, &args).await.unwrap();
        let parsed: Value = serde_json::from_str(&result).unwrap();

        assert_eq!(parsed["status"], "updated");
        assert_eq!(parsed["currentStep"], 3);
        assert_eq!(parsed["stepStatus"], "completed");

        // Verify via database.
        let state = ctx.db.get_analysis_state(&ctx.conversation_id).unwrap();
        assert!(state.is_some());
        assert_eq!(state.unwrap()["currentStep"], 3);
    }

    #[tokio::test]
    async fn test_handle_update_progress_missing_status() {
        let (db, _dir) = create_test_db();
        let ctx = create_test_context(db);

        let args = json!({"current_step": 1});
        let result = handle_update_progress(&ctx, &args).await;
        assert!(result.is_err());
    }

    // ── export_data code generation tests ────────

    #[test]
    fn test_build_export_python_csv() {
        let data = json!([{"name": "Alice", "salary": 100000}]);
        let code = build_export_python(&data, "csv", "output.csv").unwrap();
        assert!(code.contains("to_csv"));
        assert!(code.contains("output.csv"));
        assert!(code.contains("exports"));
    }

    #[test]
    fn test_build_export_python_excel() {
        let data = json!({"columns": ["a", "b"], "rows": [[1, 2]]});
        let code = build_export_python(&data, "excel", "data.xlsx").unwrap();
        assert!(code.contains("to_excel"));
        assert!(code.contains("openpyxl"));
    }

    #[test]
    fn test_build_export_python_json() {
        let data = json!([{"x": 1}]);
        let code = build_export_python(&data, "json", "out.json").unwrap();
        assert!(code.contains("to_json"));
        assert!(code.contains("orient='records'"));
    }

    #[test]
    fn test_build_export_python_unsupported() {
        let data = json!([]);
        let result = build_export_python(&data, "parquet", "out.parquet");
        assert!(result.is_err());
    }

    // ── ensure_extension tests ───────────────────

    #[test]
    fn test_ensure_extension_already_correct() {
        assert_eq!(ensure_extension("data.csv", "csv"), "data.csv");
        assert_eq!(ensure_extension("report.xlsx", "excel"), "report.xlsx");
    }

    #[test]
    fn test_ensure_extension_wrong_ext() {
        assert_eq!(ensure_extension("data.txt", "csv"), "data.csv");
        assert_eq!(ensure_extension("report.csv", "excel"), "report.xlsx");
    }

    #[test]
    fn test_ensure_extension_no_ext() {
        assert_eq!(ensure_extension("data", "csv"), "data.csv");
        assert_eq!(ensure_extension("report", "json"), "report.json");
    }

    // ── slugify tests ────────────────────────────

    #[test]
    fn test_slugify() {
        assert_eq!(slugify("Hello World"), "hello_world");
        assert_eq!(slugify("Report #1 (Final)"), "report__1__final");
    }

    // ── HTML report generation tests ─────────────

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

    // ── Markdown report tests ────────────────────

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

    // ── execute_tool dispatch tests ──────────────

    #[tokio::test]
    async fn test_execute_tool_unknown() {
        let (db, _dir) = create_test_db();
        let ctx = create_test_context(db);

        let tool_call = ToolCall {
            id: "call_99".to_string(),
            name: "nonexistent_tool".to_string(),
            arguments: json!({}),
        };

        let result = execute_tool(&ctx, &tool_call).await;
        assert!(result.is_error);
        assert!(result.content.contains("Unknown tool"));
        assert_eq!(result.tool_use_id, "call_99");
    }

    // ── Chart code generation tests ──────────────

    #[test]
    fn test_build_chart_python_bar() {
        let data = json!({"labels": ["A", "B"], "values": [10, 20]});
        let code = build_chart_python("bar", "My Chart", &data, &json!({}), "/tmp/chart.png");
        assert!(code.contains("matplotlib"));
        assert!(code.contains("chart_type = 'bar'"));
        assert!(code.contains("savefig"));
        assert!(code.contains("/tmp/chart.png"));
    }

    // ── Hypothesis test code generation ──────────

    #[test]
    fn test_build_hypothesis_test_python_ttest() {
        let code =
            build_hypothesis_test_python("t_test", &["col_a", "col_b"], None, 0.05).unwrap();
        assert!(code.contains("ttest_ind"));
        assert!(code.contains("col_a"));
        assert!(code.contains("col_b"));
    }

    #[test]
    fn test_build_hypothesis_test_python_unsupported() {
        let result = build_hypothesis_test_python("unknown_test", &["a"], None, 0.05);
        assert!(result.is_err());
    }

    // ── Anomaly detection code generation ────────

    #[test]
    fn test_build_anomaly_detection_zscore() {
        let code = build_anomaly_detection_python("salary", "zscore", 3.0, None).unwrap();
        assert!(code.contains("zscore"));
        assert!(code.contains("salary"));
    }

    #[test]
    fn test_build_anomaly_detection_iqr() {
        let code =
            build_anomaly_detection_python("salary", "iqr", 1.5, Some("department")).unwrap();
        assert!(code.contains("iqr"));
        assert!(code.contains("department"));
    }

    #[test]
    fn test_build_anomaly_detection_unsupported() {
        let result = build_anomaly_detection_python("salary", "invalid", 3.0, None);
        assert!(result.is_err());
    }

    // ── Test helpers ─────────────────────────────

    fn create_test_db() -> (Arc<AppStorage>, tempfile::TempDir) {
        let dir = tempfile::TempDir::new().unwrap();
        let db = Arc::new(AppStorage::new(dir.path()).unwrap());
        // Create a conversation for testing.
        db.create_conversation("test_conv_1", "Test Conversation")
            .unwrap();
        (db, dir)
    }

    fn create_test_context(db: Arc<AppStorage>) -> ToolContext {
        let workspace = std::env::temp_dir().join("tool_executor_test");
        std::fs::create_dir_all(&workspace).ok();
        ToolContext {
            db,
            file_manager: Arc::new(FileManager::new(&workspace)),
            workspace_path: workspace,
            conversation_id: "test_conv_1".to_string(),
            tavily_api_key: None,
            app_handle: None,
        }
    }
}
