//! internal_system handlers — http_request, browse_navigate, read_page_content,
//! page_execute_js, and save_api_knowledge tool executors.

use anyhow::{anyhow, Result};
use log::{info, warn};
use serde_json::Value;

use crate::plugin::context::PluginContext;
use super::{require_str, optional_str};

/// Handle http_request tool invocations.
pub(crate) async fn handle_http_request(ctx: &PluginContext, args: &Value) -> Result<String> {
    let app_name = require_str(args, "app_name")?;
    let method = require_str(args, "method")?;
    let url_raw = require_str(args, "url")?;

    let engine = ctx.connector_engine.as_ref()
        .ok_or_else(|| anyhow!("Internal app connector not initialized"))?;

    // Find app by name to get base_url and app_id
    let apps = engine.get_apps().await;
    let app = apps.iter()
        .find(|a| a.name == app_name)
        .ok_or_else(|| anyhow!(
            "App '{}' not found. Available: {}",
            app_name,
            apps.iter().map(|a| a.name.as_str()).collect::<Vec<_>>().join(", ")
        ))?;

    // If url is relative, prepend base_url
    let url = if url_raw.starts_with("http://") || url_raw.starts_with("https://") {
        url_raw.to_string()
    } else {
        format!("{}{}", app.base_url.trim_end_matches('/'), url_raw)
    };

    // Extract the path part (relative to base_url) for knowledge storage
    let path_for_knowledge = url.strip_prefix(app.base_url.trim_end_matches('/'))
        .unwrap_or(&url)
        .split('?').next()
        .unwrap_or("")
        .to_string();

    let headers = args.get("headers");
    let body = args.get("body");

    let response = engine.request(app.id, method, &url, headers, body).await
        .map_err(|e| anyhow!(e))?;

    // Detect HTML response — suggest browse_navigate instead
    let body_trimmed = response.body.trim_start();
    let body_lower = body_trimmed.get(..15).unwrap_or(body_trimmed).to_lowercase();
    if response.status == 200 && (body_lower.starts_with("<!doctype") || body_lower.starts_with("<html")) {
        info!("[CONNECTOR] {} {} returned HTML — suggesting browse_navigate", method, url);
        let mut result = format!("HTTP {} — Status: {}\n", method, response.status);
        result.push_str("(Response is HTML page, not JSON API. Use browse_navigate tool to open this page in the browser, then read_page_content to extract data.)");
        return Ok(result);
    }

    // Auto-save knowledge when request succeeds (HTTP 200) with actual data
    if response.status == 200 && response.body.len() > 10 && !path_for_knowledge.is_empty() {
        let knowledge = serde_json::json!({
            "name": format!("{} {}", method, path_for_knowledge),
            "method": method,
            "path": path_for_knowledge,
            "params_doc": url.split('?').nth(1).unwrap_or(""),
        });
        match engine.save_knowledge(app.id, &knowledge).await {
            Ok(_) => info!("[CONNECTOR] Auto-saved knowledge: {} {}", method, path_for_knowledge),
            Err(e) => info!("[CONNECTOR] Failed to auto-save knowledge (non-fatal): {}", e),
        }
    }

    // Format response for LLM
    let mut result = format!("HTTP {} — Status: {}\n", method, response.status);
    if response.truncated {
        result.push_str("(Response truncated to 8000 chars)\n");
    }
    result.push_str(&response.body);

    Ok(result)
}

/// Handle browse_navigate tool invocations (V4 — open browsing mode).
///
/// Opens any URL in the CDP browser. No app lookup, no pre-configuration needed.
pub(crate) async fn handle_browse_navigate(ctx: &PluginContext, args: &Value) -> Result<String> {
    let url = require_str(args, "url")?;

    let engine = ctx.connector_engine.as_ref()
        .ok_or_else(|| anyhow!("Internal app connector not initialized"))?;

    info!("[CONNECTOR] browse_navigate: url='{}'", url);

    let result = engine.browser_navigate(url).await
        .map_err(|e| {
            warn!("[CONNECTOR] browse_navigate failed: url='{}', error={}", url, e);
            anyhow!(e)
        })?;

    let mut output = format!(
        "Page ready: {} ({})\nThe browser window is now showing this page.",
        result.title, result.url
    );

    if result.redirected_to_login {
        output.push_str("\n\n⚠️ The page was redirected (possibly to a login page). Please ask the user to log in in the Chrome browser window, then call browse_navigate again with the same URL.");
    } else {
        output.push_str("\nUse read_page_content to extract data, or page_execute_js to interact.");
    }

    Ok(output)
}

/// Handle read_page_content tool invocations (V4 — open browsing mode).
///
/// Extracts tables + text from the active page in the CDP browser.
pub(crate) async fn handle_read_page_content(ctx: &PluginContext, args: &Value) -> Result<String> {
    let extract_script = optional_str(args, "extract_script");

    let engine = ctx.connector_engine.as_ref()
        .ok_or_else(|| anyhow!("Internal app connector not initialized"))?;

    info!("[CONNECTOR] read_page_content");

    let result = engine.browser_read_content(extract_script).await
        .map_err(|e| {
            warn!("[CONNECTOR] read_page_content failed: error={}", e);
            anyhow!(e)
        })?;

    info!(
        "[CONNECTOR] read_page_content complete: tables={}, text_len={}",
        result.tables.len(), result.text.len()
    );

    // Format result for LLM
    let mut output = format!("Page: {} ({})\n\n", result.title, result.url);

    if !result.tables.is_empty() {
        for (i, table) in result.tables.iter().enumerate() {
            output.push_str(&format!("### Table {} ({} rows)\n", i + 1, table.rows.len()));
            if !table.headers.is_empty() {
                output.push_str(&format!("Columns: {}\n", table.headers.join(" | ")));
            }
            for row in &table.rows {
                let cells: Vec<String> = if !table.headers.is_empty() {
                    table.headers.iter().map(|h| {
                        row.get(h).cloned().unwrap_or_default()
                    }).collect()
                } else {
                    row.values().cloned().collect()
                };
                output.push_str(&format!("{}\n", cells.join(" | ")));
            }
            output.push('\n');
        }
    }

    if !result.text.is_empty() && result.tables.is_empty() {
        output.push_str("### Page Text\n");
        output.push_str(&result.text);
        output.push('\n');
    }

    Ok(output)
}

/// Handle page_execute_js tool invocations (V4 — open browsing mode).
///
/// Executes custom JavaScript on the active page (click, fill, scroll, paginate).
pub(crate) async fn handle_page_execute_js(ctx: &PluginContext, args: &Value) -> Result<String> {
    let script = require_str(args, "script")?;

    let engine = ctx.connector_engine.as_ref()
        .ok_or_else(|| anyhow!("Internal app connector not initialized"))?;

    info!("[CONNECTOR] page_execute_js: script_len={}", script.len());

    let result = engine.browser_execute_js(script).await
        .map_err(|e| {
            warn!("[CONNECTOR] page_execute_js failed: error={}", e);
            anyhow!(e)
        })?;

    // Format result for LLM
    let mut output = String::new();

    if let Some(ref err) = result.error {
        output.push_str(&format!("Script error: {}\n", err));
    } else {
        output.push_str("Script executed successfully.\n");
        if !result.value.is_null() {
            output.push_str(&format!("Return value: {}\n", result.value));
        }
    }

    if let Some(ref new_url) = result.new_url {
        output.push_str(&format!("Current URL: {}\n", new_url));
    }
    if let Some(ref new_title) = result.new_title {
        output.push_str(&format!("Page title: {}\n", new_title));
    }

    output.push_str("Use read_page_content to see the updated page content.");

    Ok(output)
}

/// Handle save_api_knowledge tool invocations.
pub(crate) async fn handle_save_api_knowledge(ctx: &PluginContext, args: &Value) -> Result<String> {
    let app_name = require_str(args, "app_name")?;
    let name = require_str(args, "name")?;
    let method = require_str(args, "method")?;
    let path = require_str(args, "path")?;
    let params_doc = optional_str(args, "params_doc");
    let response_doc = optional_str(args, "response_doc");
    let notes = optional_str(args, "notes");

    let engine = ctx.connector_engine.as_ref()
        .ok_or_else(|| anyhow!("Internal app connector not initialized"))?;

    // Find app by name
    let apps = engine.get_apps().await;
    let app = apps.iter()
        .find(|a| a.name == app_name)
        .ok_or_else(|| anyhow!("App '{}' not found", app_name))?;

    let knowledge = serde_json::json!({
        "name": name,
        "method": method,
        "path": path,
        "params_doc": params_doc,
        "response_doc": response_doc,
        "notes": notes,
    });

    engine.save_knowledge(app.id, &knowledge).await
        .map_err(|e| anyhow!(e))?;

    Ok(format!("API knowledge '{}' saved for '{}'.", name, app_name))
}
