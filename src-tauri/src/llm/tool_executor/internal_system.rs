//! internal_system handlers — http_request, browse_navigate, read_page_content,
//! page_execute_js, and save_api_knowledge tool executors.

use anyhow::{anyhow, Result};
use log::{info, warn};
use serde_json::Value;
use tauri::Manager;

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
        let final_path = result.url.to_lowercase();
        if final_path.contains("error") || final_path.contains("forbidden") || final_path.contains("no_resource") || final_path.contains("no_permission") || final_path.contains("/403") || final_path.contains("/404") {
            return Err(anyhow!(
                "ACCESS DENIED: Redirected to error page '{}'. The current user does not have permission to access the requested URL. STOP browsing this URL. Tell the user they lack permission and ask which page to try instead.",
                result.url
            ));
        } else {
            output.push_str("\n\n⚠️ The page was redirected (possibly to a login page). Please ask the user to log in in the Chrome browser window, then call browse_navigate again with the same URL.");
        }
    } else {
        // Include auto-explored page profile
        if let Some(ref profile) = result.page_profile {
            output.push_str("\n\n");
            output.push_str(&profile.format_detail());
        } else {
            output.push_str("\nUse read_page_content to extract data, or page_execute_js to interact.");
        }
    }

    // Include screenshot path if available
    if let Some(ref path) = result.screenshot_path {
        output.push_str(&format!("\n\nScreenshot saved: {}", path.display()));
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

    if !result.links.is_empty() {
        output.push_str("\n### Navigation & Actions\n");
        for link in &result.links {
            match link.link_type.as_str() {
                "menu" => {
                    if !link.href.is_empty() {
                        output.push_str(&format!("- [menu] {} → {}\n", link.label, link.href));
                    } else if !link.selector.is_empty() {
                        output.push_str(&format!("- [menu] {} (selector: {})\n", link.label, link.selector));
                    } else {
                        output.push_str(&format!("- [menu] {}\n", link.label));
                    }
                }
                "button" => {
                    output.push_str(&format!("- [button] {}\n", link.label));
                }
                _ => {
                    if !link.href.is_empty() {
                        output.push_str(&format!("- {} → {}\n", link.label, link.href));
                    }
                }
            }
        }
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

/// Handle browse_and_extract tool invocations.
/// Handle browse_data tool — delegate to browser sub-agent.
pub(crate) async fn handle_browse_data(ctx: &PluginContext, args: &Value) -> Result<String> {
    let task = require_str(args, "task")?;
    let url = optional_str(args, "url");

    let gateway = ctx.gateway.as_ref()
        .ok_or_else(|| anyhow!("LLM gateway not available for sub-agent"))?;
    let tool_registry = ctx.tool_registry.as_ref()
        .ok_or_else(|| anyhow!("Tool registry not available for sub-agent"))?;
    let app_settings = ctx.app_settings.as_ref()
        .ok_or_else(|| anyhow!("App settings not available for sub-agent"))?;

    info!("[CONNECTOR] browse_data: task='{}', url={:?}", task, url);

    // Load browser_agent prompt
    let prompt_path = ctx.app_handle.as_ref()
        .and_then(|h| h.path().resource_dir().ok())
        .map(|d: std::path::PathBuf| d.join("prompts/browser_agent.md"));
    let system_prompt = prompt_path
        .and_then(|p| std::fs::read_to_string(&p).ok())
        .unwrap_or_else(|| "你是数据提取专家。使用 browse_and_extract 工具从内部系统提取数据。".to_string());

    // Build dynamic context: site map from connector engine
    let mut dynamic_context = String::new();
    let mut has_known_apis = false;
    let mut has_known_tables = false;
    let mut target_page_hint = String::new();

    if let Some(ref engine) = ctx.connector_engine {
        let cdp = engine.cdp_browser_ref().await;
        if let Some(cdp) = cdp.as_ref() {
            if let Some(ctx_str) = cdp.get_site_map_context(None).await {
                dynamic_context = ctx_str;
            }
            // Check if target URL has cached profile with APIs or tables
            if let Some(target_url) = url {
                let url_path = url::Url::parse(target_url).ok()
                    .map(|u| u.path().to_string())
                    .unwrap_or_default();
                let maps = cdp.site_maps.lock().await;
                let origin = url::Url::parse(target_url).ok()
                    .map(|u| format!("{}://{}", u.scheme(), u.host_str().unwrap_or("")))
                    .unwrap_or_default();
                if let Some(site_map) = maps.get(&origin) {
                    if let Some(profile) = site_map.get_page(&url_path) {
                        has_known_apis = !profile.api_endpoints.is_empty();
                        has_known_tables = !profile.table_schemas.is_empty();
                        if has_known_tables {
                            let table_info: Vec<String> = profile.table_schemas.iter()
                                .map(|t| format!("{} ({} rows, cols: {})",
                                    if t.name.is_empty() { "table" } else { &t.name },
                                    t.row_count, t.headers.join(", ")))
                                .collect();
                            target_page_hint = format!(
                                "\n\n[已知页面信息: {}]\n表格: {}\nAPI端点: {}\n表单: {}",
                                url_path,
                                table_info.join("; "),
                                if has_known_apis { "有" } else { "无（该系统可能是传统SSR架构，数据直接嵌在HTML中）" },
                                if profile.forms.is_empty() { "无" } else { "有" },
                            );
                        }
                    }
                }
            }
        }
    }

    // Build task message with strategy hints based on page profile
    let mut task_msg = if let Some(url) = url {
        format!("{}\n\nTarget URL: {}", task, url)
    } else {
        task.to_string()
    };

    task_msg.push_str(&target_page_hint);

    // Add strategy based on what we know
    if has_known_tables && !has_known_apis {
        task_msg.push_str("\n\n**策略提示**: 该页面有表格数据但没有发现 JSON API 端点。这可能是传统服务端渲染系统。请按以下优先级操作：\n\
            1. 先用 browse_and_extract 打开页面，查看返回的表格数据\n\
            2. 如果表格数据不完整（被分页截断），用 page_execute_js 查找分页参数或导出按钮\n\
            3. 优先寻找「导出」「下载」「export」按钮直接导出全量数据\n\
            4. 如果没有导出按钮，尝试修改 URL 参数（如 pageSize=500）重新请求页面获取更多数据\n\
            5. **不要花时间猜 API 路径** — 如果 auto_explore 没发现 API，该系统大概率没有 REST API");
    } else if has_known_apis {
        task_msg.push_str("\n\n**策略提示**: 该页面有已知的 API 端点。直接用 browse_and_extract 的 API 模式调用即可。");
    }

    let config = crate::llm::sub_agent::SubAgentConfig {
        task: task_msg,
        system_prompt,
        allowed_tools: vec![
            "browse_and_extract".to_string(),
            "browse_navigate".to_string(),
            "read_page_content".to_string(),
            "page_execute_js".to_string(),
        ],
        max_iterations: 15,
        dynamic_context,
    };

    let result = crate::llm::sub_agent::run_sub_agent(
        gateway,
        tool_registry,
        ctx,
        config,
        app_settings,
    ).await.map_err(|e| {
        warn!("[CONNECTOR] browse_data sub-agent failed: {}", e);
        anyhow!("Browser agent failed: {}", e)
    })?;

    info!("[CONNECTOR] browse_data complete: iterations={}, files={}, output_len={}",
        result.iterations_used, result.files.len(), result.output.len());

    // Format result for main agent
    let mut output = format!("Browser agent completed in {} iterations.\n\n", result.iterations_used);

    if !result.files.is_empty() {
        output.push_str("### Extracted Data Files\n");
        for f in &result.files {
            // Try to register each file into the conversation
            let src = std::path::Path::new(f);
            if src.exists() {
                let file_name = src.file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_else(|| "data.json".to_string());
                if let Ok(content) = std::fs::read(src) {
                    match ctx.file_manager.write_file("generated", &file_name, &content) {
                        Ok(file_info) => {
                            let file_id = uuid::Uuid::new_v4().to_string();
                            let _ = ctx.storage.insert_generated_file(
                                &file_id,
                                &ctx.conversation_id,
                                None,
                                &file_info.file_name,
                                &file_info.stored_path,
                                "json",
                                file_info.file_size as i64,
                                "data",
                                Some("Browser agent extracted data"),
                                1, true, None, None, None,
                            );
                            let full = ctx.file_manager.full_path(&file_info.stored_path);
                            output.push_str(&format!("- {}\n", full.display()));
                            continue;
                        }
                        Err(e) => warn!("[CONNECTOR] Failed to register sub-agent file: {}", e),
                    }
                }
            }
            output.push_str(&format!("- {}\n", f));
        }
        output.push_str("\nUse `execute_python` to load these JSON files (e.g. `pd.read_json(path)` or `json.load`).\n\n");
    }

    if !result.output.is_empty() {
        output.push_str("### Agent Summary\n");
        // Truncate if too long (safe UTF-8 boundary)
        if result.output.len() > 2000 {
            let end = result.output.char_indices()
                .take_while(|(i, _)| *i < 2000)
                .last()
                .map(|(i, c)| i + c.len_utf8())
                .unwrap_or(0);
            output.push_str(&result.output[..end]);
            output.push_str("\n...(truncated)");
        } else {
            output.push_str(&result.output);
        }
    }

    Ok(output)
}

/// Handle browse_and_extract tool invocations.
///
/// Smart routing: page mode (navigate + full extraction) or API mode (in-page fetch).
pub(crate) async fn handle_browse_and_extract(ctx: &PluginContext, args: &Value) -> Result<String> {
    let url = require_str(args, "url")?;
    let extract_script = optional_str(args, "extract_script");
    let method = args["method"].as_str().unwrap_or("GET").to_uppercase();
    let body = optional_str(args, "body");
    let headers = optional_str(args, "headers");

    let engine = ctx.connector_engine.as_ref()
        .ok_or_else(|| anyhow!("Internal app connector not initialized"))?;

    // Smart routing: non-GET or has body → API mode
    let is_api_mode = method != "GET" || body.is_some();

    if is_api_mode {
        // ── API Mode ──
        info!("[CONNECTOR] browse_and_extract API mode: {} '{}'", method, url);

        let result = engine.browser_api_fetch(url, &method, body, headers).await
            .map_err(|e| {
                warn!("[CONNECTOR] browse_and_extract API failed: {}", e);
                anyhow!(e)
            })?;

        let mut output = format!("API Response: {} {}\nStatus: {}, Content-Type: {}\n\n",
            method, url, result.status, result.content_type);

        if let Some(ref path) = result.saved_file_path {
            // Copy to workspace and register in conversation file_index
            let file_name = path.file_name()
                .map(|f| f.to_string_lossy().to_string())
                .unwrap_or_else(|| "api_data.json".to_string());

            let registered_path = if let Ok(content) = std::fs::read(path) {
                // Write to conversation's generated dir via file_manager
                match ctx.file_manager.write_file("generated", &file_name, &content) {
                    Ok(file_info) => {
                        let file_id = uuid::Uuid::new_v4().to_string();
                        let _ = ctx.storage.insert_generated_file(
                            &file_id,
                            &ctx.conversation_id,
                            None,
                            &file_info.file_name,
                            &file_info.stored_path,
                            "json",
                            file_info.file_size as i64,
                            "data",
                            Some(&format!("API data: {} {}", method, url)),
                            1, true, None, None, None,
                        );
                        info!("[CONNECTOR] Registered API data file: {} ({})", file_info.stored_path, file_info.file_size);
                        // Use the workspace path for LLM
                        ctx.file_manager.full_path(&file_info.stored_path).display().to_string()
                    }
                    Err(e) => {
                        warn!("[CONNECTOR] Failed to copy API data to workspace: {}", e);
                        path.display().to_string()
                    }
                }
            } else {
                path.display().to_string()
            };

            // Large data saved to file — tell LLM to use Python to process it
            let total = result.total_rows.map(|t| format!("{} rows", t)).unwrap_or("unknown size".to_string());
            output.push_str(&format!("### Data saved to file ({}) \n", total));
            output.push_str(&format!("File: {}\n\n", registered_path));
            output.push_str("The full JSON data has been saved to the file above. ");
            output.push_str("Use `execute_python` to load and process this JSON file (e.g. pd.read_json or json.load). ");
            output.push_str("Do NOT use page_execute_js to re-fetch the data.\n\n");
            output.push_str("### Sample (first rows)\n");
        } else if let Some(total) = result.total_rows {
            output.push_str(&format!("### Data ({} rows)\n", total));
        }

        // Format JSON data compactly (safe UTF-8 truncation)
        let data_str = serde_json::to_string_pretty(&result.data)
            .unwrap_or_else(|_| format!("{}", result.data));
        if data_str.len() > 8000 {
            let end = data_str.char_indices()
                .take_while(|(i, _)| *i < 8000)
                .last()
                .map(|(i, c)| i + c.len_utf8())
                .unwrap_or(0);
            output.push_str(&data_str[..end]);
            output.push_str("\n...(truncated)\n");
        } else {
            output.push_str(&data_str);
            output.push('\n');
        }

        Ok(output)
    } else {
        // ── Page Mode ──
        info!("[CONNECTOR] browse_and_extract page mode: '{}'", url);

        let result = engine.browser_navigate_and_extract(url, extract_script).await
            .map_err(|e| {
                warn!("[CONNECTOR] browse_and_extract page failed: {}", e);
                anyhow!(e)
            })?;

        let mut output = format!("Page: {} ({})\n", result.navigate.title, result.navigate.url);

        if result.navigate.redirected_to_login {
            let final_path = result.navigate.url.to_lowercase();
            if final_path.contains("error") || final_path.contains("forbidden") || final_path.contains("no_resource") || final_path.contains("no_permission") || final_path.contains("/403") || final_path.contains("/404") {
                return Err(anyhow!(
                    "ACCESS DENIED: Redirected to error page '{}'. The current user does not have permission to access the requested URL. STOP browsing this URL. Tell the user they lack permission and ask which page to try instead.",
                    result.navigate.url
                ));
            } else {
                output.push_str("\n⚠️ Redirected to login page. Ask the user to log in in Chrome, then call again.\n");
            }
            return Ok(output);
        }

        // Tables
        if !result.content.tables.is_empty() {
            for (i, table) in result.content.tables.iter().enumerate() {
                output.push_str(&format!("\n### Table {} ({} rows)\n", i + 1, table.rows.len()));
                if !table.headers.is_empty() {
                    output.push_str(&format!("Columns: {}\n", table.headers.join(" | ")));
                }
                for row in &table.rows {
                    let cells: Vec<String> = if !table.headers.is_empty() {
                        table.headers.iter().map(|h| row.get(h).cloned().unwrap_or_default()).collect()
                    } else {
                        row.values().cloned().collect()
                    };
                    output.push_str(&format!("{}\n", cells.join(" | ")));
                }
            }
        }

        // Navigation & Actions (links)
        if !result.content.links.is_empty() {
            output.push_str("\n### Navigation & Actions\n");
            for link in &result.content.links {
                match link.link_type.as_str() {
                    "menu" => {
                        if !link.href.is_empty() {
                            output.push_str(&format!("- [menu] {} → {}\n", link.label, link.href));
                        } else if !link.selector.is_empty() {
                            output.push_str(&format!("- [menu] {} (selector: {})\n", link.label, link.selector));
                        } else {
                            output.push_str(&format!("- [menu] {}\n", link.label));
                        }
                    }
                    "button" => {
                        output.push_str(&format!("- [button] {}\n", link.label));
                    }
                    _ => {
                        if !link.href.is_empty() {
                            output.push_str(&format!("- {} → {}\n", link.label, link.href));
                        }
                    }
                }
            }
        }

        // Discovered API endpoints
        if !result.api_calls.is_empty() {
            output.push_str("\n### Discovered API Endpoints\n");
            for api in &result.api_calls {
                let size = if api.size_bytes > 1024 {
                    format!("{:.1}KB", api.size_bytes as f64 / 1024.0)
                } else {
                    format!("{}B", api.size_bytes)
                };
                let ct_short = if api.content_type.contains("json") { "JSON" }
                    else if api.content_type.contains("html") { "HTML" }
                    else { &api.content_type };
                output.push_str(&format!("- {} {} → {} ({} {})\n",
                    api.method, api.url, api.status, size, ct_short));
            }
            output.push_str("Tip: Use browse_and_extract with these API URLs to fetch data directly.\n");
        }

        // Forms
        if !result.forms.is_empty() {
            output.push_str("\n### Forms\n");
            for form in &result.forms {
                output.push_str(&format!("- Form#{}: {} {}\n", form.id, form.method, form.action));
                for field in &form.fields {
                    let val = if field.value.is_empty() { String::new() }
                        else { format!("={}", field.value) };
                    output.push_str(&format!("  - {} ({}{})\n", field.name, field.field_type, val));
                }
            }
        }

        // Page text (only if no tables)
        if !result.content.text.is_empty() && result.content.tables.is_empty() {
            output.push_str("\n### Page Text\n");
            output.push_str(&result.content.text);
            output.push('\n');
        }

        Ok(output)
    }
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
