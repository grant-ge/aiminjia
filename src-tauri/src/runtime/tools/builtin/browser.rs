//! Browser automation as RuntimeTool.
//!
//! Five primitive browser tools, each backed by `BrowserDeps` (injected at
//! construction time, no `PluginContext` involved).
//!
//! - `BrowseNavigateRuntimeTool`      — navigate to URL
//! - `ReadPageContentRuntimeTool`     — read page text / tables
//! - `PageExecuteJsRuntimeTool`       — execute JavaScript
//! - `ExtractTableDataRuntimeTool`    — extract table + pagination info
//! - `ExtractWithPaginationRuntimeTool` — auto-paginate extraction
//!
//! `ExtractTableDataRuntimeTool` and `ExtractWithPaginationRuntimeTool` write
//! JSON files and register them in storage, so `BrowserDeps` carries the
//! `file_manager` and `storage` fields they need in addition to the engine.

use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;
use log::{info, warn};
use serde_json::Value;

use crate::connector::ConnectorEngine;
use crate::runtime::tools::catalog::TOOL_CATALOG;
use crate::runtime::tools::context::ToolExecutionContext;
use crate::runtime::tools::definition::ToolDefinition;
use crate::runtime::tools::executor::{ToolError, ToolResult};
use crate::runtime::tools::RuntimeTool;
use crate::storage::file_manager::FileManager;
use crate::storage::file_store::AppStorage;

// ── BrowserDeps ──────────────────────────────────────────────────────────────

/// Narrow browser dependencies — injected at construction.
///
/// `connector_engine` is required for all browser operations.
/// `file_manager` and `storage` are required by `extract_table_data` and
/// `extract_with_pagination` (which write JSON files to the workspace).
/// `workspace_path` and `conversation_id` are needed for file registration.
pub struct BrowserDeps {
    pub connector_engine: Option<Arc<ConnectorEngine>>,
    /// Required by `ExtractTableDataRuntimeTool` and `ExtractWithPaginationRuntimeTool`.
    pub file_manager: Arc<FileManager>,
    /// Required by `ExtractTableDataRuntimeTool` and `ExtractWithPaginationRuntimeTool`.
    pub storage: Arc<AppStorage>,
    pub workspace_path: PathBuf,
    pub conversation_id: String,
}

// ── Helper ───────────────────────────────────────────────────────────────────

fn tool_result(tool_name: &str, content: String) -> ToolResult {
    ToolResult {
        tool_name: tool_name.to_string(),
        content: content.clone(),
        data: Some(Value::String(content)),
        file_meta: None,
        is_degraded: false,
        degradation_notice: None,
    }
}

fn connector_engine(deps: &BrowserDeps) -> Result<&Arc<ConnectorEngine>, ToolError> {
    deps.connector_engine
        .as_ref()
        .ok_or_else(|| ToolError::ExecutionFailed("Internal app connector not initialized".into()))
}

// ── BrowseNavigateRuntimeTool ─────────────────────────────────────────────────

pub struct BrowseNavigateRuntimeTool {
    deps: Arc<BrowserDeps>,
}

impl BrowseNavigateRuntimeTool {
    pub fn new(deps: BrowserDeps) -> Self {
        Self {
            deps: Arc::new(deps),
        }
    }
}

#[async_trait]
impl RuntimeTool for BrowseNavigateRuntimeTool {
    fn definition(&self) -> ToolDefinition {
        TOOL_CATALOG
            .get("browse_navigate")
            .unwrap_or_else(|| ToolDefinition::new("browse_navigate", "导航浏览器到指定 URL"))
    }

    async fn execute(
        &self,
        input: Value,
        _ctx: ToolExecutionContext,
    ) -> Result<ToolResult, ToolError> {
        let url = input
            .get("url")
            .and_then(Value::as_str)
            .ok_or_else(|| ToolError::ExecutionFailed("Missing required: url".into()))?;

        info!("[BROWSER] browse_navigate: url='{}'", url);

        let result = connector_engine(&self.deps)?
            .browser_navigate(url)
            .await
            .map_err(|e| {
                warn!(
                    "[BROWSER] browse_navigate failed: url='{}', error={}",
                    url, e
                );
                ToolError::ExecutionFailed(e)
            })?;

        let mut output = format!(
            "Page ready: {} ({})\nThe browser window is now showing this page.",
            result.title, result.url
        );

        if result.redirected_to_login {
            let final_path = result.url.to_lowercase();
            if final_path.contains("error")
                || final_path.contains("forbidden")
                || final_path.contains("no_resource")
                || final_path.contains("no_permission")
                || final_path.contains("/403")
                || final_path.contains("/404")
            {
                return Err(ToolError::PermissionDenied(format!(
                    "ACCESS DENIED: Redirected to error page '{}'. The current user does not have \
                     permission to access the requested URL.",
                    result.url
                )));
            } else {
                output.push_str(
                    "\n\n⚠️ The page was redirected (possibly to a login page). Please ask the \
                     user to log in in the Chrome browser window, then call browse_navigate again \
                     with the same URL.",
                );
            }
        } else if let Some(ref profile) = result.page_profile {
            output.push_str("\n\n");
            output.push_str(&profile.format_detail());
        } else {
            output.push_str(
                "\nUse read_page_content to extract data, or page_execute_js to interact.",
            );
        }

        if let Some(ref path) = result.screenshot_path {
            output.push_str(&format!("\n\nScreenshot saved: {}", path.display()));
        }

        Ok(tool_result("browse_navigate", output))
    }
}

// ── ReadPageContentRuntimeTool ────────────────────────────────────────────────

pub struct ReadPageContentRuntimeTool {
    deps: Arc<BrowserDeps>,
}

impl ReadPageContentRuntimeTool {
    pub fn new(deps: BrowserDeps) -> Self {
        Self {
            deps: Arc::new(deps),
        }
    }
}

#[async_trait]
impl RuntimeTool for ReadPageContentRuntimeTool {
    fn definition(&self) -> ToolDefinition {
        TOOL_CATALOG.get("read_page_content").unwrap_or_else(|| {
            ToolDefinition::new("read_page_content", "读取当前浏览器页面的文本内容")
        })
    }

    async fn execute(
        &self,
        input: Value,
        _ctx: ToolExecutionContext,
    ) -> Result<ToolResult, ToolError> {
        let extract_script = input.get("selector").and_then(Value::as_str);

        info!("[BROWSER] read_page_content");

        let result = connector_engine(&self.deps)?
            .browser_read_content(extract_script)
            .await
            .map_err(|e| {
                warn!("[BROWSER] read_page_content failed: error={}", e);
                ToolError::ExecutionFailed(e)
            })?;

        info!(
            "[BROWSER] read_page_content: tables={}, text_len={}",
            result.tables.len(),
            result.text.len()
        );

        let mut output = format!("Page: {} ({})\n\n", result.title, result.url);

        if !result.tables.is_empty() {
            for (i, table) in result.tables.iter().enumerate() {
                output.push_str(&format!(
                    "### Table {} ({} rows)\n",
                    i + 1,
                    table.rows.len()
                ));
                if !table.headers.is_empty() {
                    output.push_str(&format!("Columns: {}\n", table.headers.join(" | ")));
                }
                for row in &table.rows {
                    let cells: Vec<String> = if !table.headers.is_empty() {
                        table
                            .headers
                            .iter()
                            .map(|h| row.get(h).cloned().unwrap_or_default())
                            .collect()
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
                            output.push_str(&format!(
                                "- [menu] {} (selector: {})\n",
                                link.label, link.selector
                            ));
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

        Ok(tool_result("read_page_content", output))
    }
}

// ── PageExecuteJsRuntimeTool ──────────────────────────────────────────────────

pub struct PageExecuteJsRuntimeTool {
    deps: Arc<BrowserDeps>,
}

impl PageExecuteJsRuntimeTool {
    pub fn new(deps: BrowserDeps) -> Self {
        Self {
            deps: Arc::new(deps),
        }
    }
}

#[async_trait]
impl RuntimeTool for PageExecuteJsRuntimeTool {
    fn definition(&self) -> ToolDefinition {
        TOOL_CATALOG.get("page_execute_js").unwrap_or_else(|| {
            ToolDefinition::new("page_execute_js", "在当前浏览器页面执行 JavaScript")
        })
    }

    async fn execute(
        &self,
        input: Value,
        _ctx: ToolExecutionContext,
    ) -> Result<ToolResult, ToolError> {
        let script = input
            .get("code")
            .and_then(Value::as_str)
            .ok_or_else(|| ToolError::ExecutionFailed("Missing required: code".into()))?;

        info!("[BROWSER] page_execute_js: script_len={}", script.len());

        let result = connector_engine(&self.deps)?
            .browser_execute_js(script)
            .await
            .map_err(|e| {
                warn!("[BROWSER] page_execute_js failed: error={}", e);
                ToolError::ExecutionFailed(e)
            })?;

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

        Ok(tool_result("page_execute_js", output))
    }
}

// ── BrowseAndExtractRuntimeTool ──────────────────────────────────────────────

pub struct BrowseAndExtractRuntimeTool {
    deps: Arc<BrowserDeps>,
}

impl BrowseAndExtractRuntimeTool {
    pub fn new(deps: BrowserDeps) -> Self {
        Self {
            deps: Arc::new(deps),
        }
    }
}

#[async_trait]
impl RuntimeTool for BrowseAndExtractRuntimeTool {
    fn definition(&self) -> ToolDefinition {
        TOOL_CATALOG.get("browse_and_extract").unwrap_or_else(|| {
            ToolDefinition::new("browse_and_extract", "导航到 URL 并抽取结构化数据")
        })
    }

    async fn execute(
        &self,
        input: Value,
        _ctx: ToolExecutionContext,
    ) -> Result<ToolResult, ToolError> {
        let url = input
            .get("url")
            .and_then(Value::as_str)
            .ok_or_else(|| ToolError::ExecutionFailed("Missing required: url".into()))?;
        let method = input
            .get("method")
            .and_then(Value::as_str)
            .unwrap_or("GET")
            .to_uppercase();
        let body = input.get("body").and_then(Value::as_str);
        let headers = input.get("headers").and_then(Value::as_str);
        let extract_script = input
            .get("extract_script")
            .or_else(|| input.get("selector"))
            .and_then(Value::as_str);

        let engine = connector_engine(&self.deps)?;
        let is_api_mode = method != "GET" || body.is_some();

        if is_api_mode {
            info!(
                "[BROWSER] browse_and_extract API mode: {} '{}'",
                method, url
            );
            let result = engine
                .browser_api_fetch(url, &method, body, headers)
                .await
                .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;

            let mut output = format!(
                "API Response: {} {}\nStatus: {}, Content-Type: {}\n\n",
                method, url, result.status, result.content_type
            );

            if let Some(ref path) = result.saved_file_path {
                let file_name = path
                    .file_name()
                    .map(|f| f.to_string_lossy().to_string())
                    .unwrap_or_else(|| "api_data.json".to_string());

                let registered_path = if let Ok(content) = std::fs::read(path) {
                    match self
                        .deps
                        .file_manager
                        .write_file("generated", &file_name, &content)
                    {
                        Ok(file_info) => {
                            let file_id = uuid::Uuid::new_v4().to_string();
                            let _ = self.deps.storage.insert_generated_file(
                                &file_id,
                                &self.deps.conversation_id,
                                None,
                                &file_info.file_name,
                                &file_info.stored_path,
                                "json",
                                file_info.file_size as i64,
                                "data",
                                Some(&format!("API data: {} {}", method, url)),
                                1,
                                true,
                                None,
                                None,
                                None,
                            );
                            self.deps
                                .file_manager
                                .full_path(&file_info.stored_path)
                                .display()
                                .to_string()
                        }
                        Err(e) => {
                            warn!("[BROWSER] Failed to copy API data to workspace: {}", e);
                            path.display().to_string()
                        }
                    }
                } else {
                    path.display().to_string()
                };

                let total = result
                    .total_rows
                    .map(|t| format!("{} rows", t))
                    .unwrap_or("unknown size".to_string());
                output.push_str(&format!("### Data saved to file ({}) \n", total));
                output.push_str(&format!("File: {}\n\n", registered_path));
                output.push_str("The full JSON data has been saved to the file above. ");
                output.push_str("Use `execute_python` to load and process this JSON file (e.g. pd.read_json or json.load). ");
                output.push_str("Do NOT use page_execute_js to re-fetch the data.\n\n");
                output.push_str("### Sample (first rows)\n");
            } else if let Some(total) = result.total_rows {
                output.push_str(&format!("### Data ({} rows)\n", total));
            }

            let data_str = serde_json::to_string_pretty(&result.data)
                .unwrap_or_else(|_| format!("{}", result.data));
            if data_str.len() > 8000 {
                let end = data_str
                    .char_indices()
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

            return Ok(tool_result("browse_and_extract", output));
        }

        info!("[BROWSER] browse_and_extract page mode: '{}'", url);
        let result = engine
            .browser_navigate_and_extract(url, extract_script)
            .await
            .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;

        let mut output = format!(
            "Page: {} ({})\n",
            result.navigate.title, result.navigate.url
        );

        if result.navigate.redirected_to_login {
            let final_path = result.navigate.url.to_lowercase();
            if final_path.contains("error")
                || final_path.contains("forbidden")
                || final_path.contains("no_resource")
                || final_path.contains("no_permission")
                || final_path.contains("/403")
                || final_path.contains("/404")
            {
                return Err(ToolError::PermissionDenied(format!(
                    "ACCESS DENIED: Redirected to error page '{}'. The current user does not have permission to access the requested URL.",
                    result.navigate.url
                )));
            }
            output.push_str("\n⚠️ Redirected to login page. Ask the user to log in in Chrome, then call again.\n");
            return Ok(tool_result("browse_and_extract", output));
        }

        if !result.content.tables.is_empty() {
            for (i, table) in result.content.tables.iter().enumerate() {
                output.push_str(&format!(
                    "\n### Table {} ({} rows)\n",
                    i + 1,
                    table.rows.len()
                ));
                if !table.headers.is_empty() {
                    output.push_str(&format!("Columns: {}\n", table.headers.join(" | ")));
                }
                for row in &table.rows {
                    let cells: Vec<String> = if !table.headers.is_empty() {
                        table
                            .headers
                            .iter()
                            .map(|h| row.get(h).cloned().unwrap_or_default())
                            .collect()
                    } else {
                        row.values().cloned().collect()
                    };
                    output.push_str(&format!("{}\n", cells.join(" | ")));
                }
            }
        }

        if !result.content.links.is_empty() {
            output.push_str("\n### Navigation & Actions\n");
            for link in &result.content.links {
                match link.link_type.as_str() {
                    "menu" => {
                        if !link.href.is_empty() {
                            output.push_str(&format!("- [menu] {} -> {}\n", link.label, link.href));
                        } else if !link.selector.is_empty() {
                            output.push_str(&format!(
                                "- [menu] {} (selector: {})\n",
                                link.label, link.selector
                            ));
                        } else {
                            output.push_str(&format!("- [menu] {}\n", link.label));
                        }
                    }
                    "button" => output.push_str(&format!("- [button] {}\n", link.label)),
                    _ => {
                        if !link.href.is_empty() {
                            output.push_str(&format!("- {} -> {}\n", link.label, link.href));
                        }
                    }
                }
            }
        }

        if !result.api_calls.is_empty() {
            output.push_str("\n### Discovered API Endpoints\n");
            for api in &result.api_calls {
                let size = if api.size_bytes > 1024 {
                    format!("{:.1}KB", api.size_bytes as f64 / 1024.0)
                } else {
                    format!("{}B", api.size_bytes)
                };
                let ct_short = if api.content_type.contains("json") {
                    "JSON"
                } else if api.content_type.contains("html") {
                    "HTML"
                } else {
                    &api.content_type
                };
                output.push_str(&format!(
                    "- {} {} -> {} ({} {})\n",
                    api.method, api.url, api.status, size, ct_short
                ));
            }
            output.push_str(
                "Tip: Use browse_and_extract with these API URLs to fetch data directly.\n",
            );
        }

        if !result.forms.is_empty() {
            output.push_str("\n### Forms\n");
            for form in &result.forms {
                output.push_str(&format!(
                    "- Form#{}: {} {}\n",
                    form.id, form.method, form.action
                ));
                for field in &form.fields {
                    let val = if field.value.is_empty() {
                        String::new()
                    } else {
                        format!("={}", field.value)
                    };
                    output.push_str(&format!(
                        "  - {} ({}{})\n",
                        field.name, field.field_type, val
                    ));
                }
            }
        }

        if !result.content.text.is_empty() && result.content.tables.is_empty() {
            output.push_str("\n### Page Text\n");
            output.push_str(&result.content.text);
            output.push('\n');
        }

        Ok(tool_result("browse_and_extract", output))
    }
}

// ── ExtractTableDataRuntimeTool ───────────────────────────────────────────────

pub struct ExtractTableDataRuntimeTool {
    deps: Arc<BrowserDeps>,
}

impl ExtractTableDataRuntimeTool {
    pub fn new(deps: BrowserDeps) -> Self {
        Self {
            deps: Arc::new(deps),
        }
    }
}

#[async_trait]
impl RuntimeTool for ExtractTableDataRuntimeTool {
    fn definition(&self) -> ToolDefinition {
        TOOL_CATALOG.get("extract_table_data").unwrap_or_else(|| {
            ToolDefinition::new("extract_table_data", "从当前浏览器页面抽取表格数据")
        })
    }

    async fn execute(
        &self,
        _input: Value,
        _ctx: ToolExecutionContext,
    ) -> Result<ToolResult, ToolError> {
        let filename = format!(
            "table_data_{}.json",
            chrono::Utc::now().format("%Y%m%d_%H%M%S")
        );
        let file_info = self
            .deps
            .file_manager
            .write_file("generated", &filename, b"{}")
            .map_err(|e| {
                ToolError::ExecutionFailed(format!("Failed to create output file: {}", e))
            })?;
        let save_path = self.deps.file_manager.full_path(&file_info.stored_path);

        info!("[BROWSER] extract_table_data: save_path={:?}", save_path);

        let result = connector_engine(&self.deps)?
            .browser_extract_table_data(&save_path.to_string_lossy(), None, None)
            .await
            .map_err(|e| ToolError::ExecutionFailed(format!("extract_table_data failed: {}", e)))?;

        let rows_count = result["rows"].as_u64().unwrap_or(0);
        let total_saved = result["totalSaved"].as_u64().unwrap_or(0);
        let headers = result["headers"]
            .as_array()
            .map(|arr| {
                arr.iter()
                    .filter_map(|h| h.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            })
            .unwrap_or_default();
        let pagination = &result["pagination"];
        let has_next = pagination["hasNext"].as_bool().unwrap_or(false);
        let total = pagination["total"].as_u64().unwrap_or(0);
        let current_page = pagination["currentPage"].as_u64().unwrap_or(0);

        let file_size = std::fs::metadata(&save_path)
            .map(|m| m.len() as i64)
            .unwrap_or(0);
        if file_size > 10 {
            let file_id = uuid::Uuid::new_v4().to_string();
            let _ = self.deps.storage.insert_generated_file(
                &file_id,
                &self.deps.conversation_id,
                None,
                &filename,
                &file_info.stored_path,
                "json",
                file_size,
                "data",
                Some("Extracted table data"),
                1,
                true,
                None,
                None,
                None,
            );
        }

        let mut output = format!(
            "### Current page extracted\n\
             - **Rows on this page**: {}\n\
             - **Columns**: {}\n\
             - **File**: {}\n\
             - **Total saved so far**: {} rows\n",
            rows_count,
            headers,
            save_path.display(),
            total_saved,
        );
        output.push_str(&format!(
            "\n### Pagination\n\
             - Total records: {}\n\
             - Current page: {}\n\
             - Has next page: {}\n",
            if total > 0 {
                total.to_string()
            } else {
                "unknown".to_string()
            },
            if current_page > 0 {
                current_page.to_string()
            } else {
                "unknown".to_string()
            },
            has_next,
        ));
        if has_next || (total > 0 && total_saved < total) {
            output.push_str(&format!(
                "\n⚠️ MORE DATA AVAILABLE: {} total records but only {} saved. \
                 You MUST use `page_execute_js` to click the next-page button, \
                 then call `extract_table_data` again. Repeat until all pages are extracted.\n",
                total, total_saved,
            ));
        } else {
            output.push_str("\nAll data has been extracted.\n");
        }
        if let Some(sample) = result.get("sampleRows").and_then(|s| s.as_array()) {
            if !sample.is_empty() {
                output.push_str("\n### Sample\n```json\n");
                let sample_str = serde_json::to_string_pretty(sample).unwrap_or_default();
                if sample_str.len() > 2000 {
                    let end = sample_str
                        .char_indices()
                        .take_while(|(i, _)| *i < 2000)
                        .last()
                        .map(|(i, c)| i + c.len_utf8())
                        .unwrap_or(0);
                    output.push_str(&sample_str[..end]);
                    output.push_str("\n...");
                } else {
                    output.push_str(&sample_str);
                }
                output.push_str("\n```");
            }
        }

        Ok(tool_result("extract_table_data", output))
    }
}

// ── ExtractWithPaginationRuntimeTool ──────────────────────────────────────────

pub struct ExtractWithPaginationRuntimeTool {
    deps: Arc<BrowserDeps>,
}

impl ExtractWithPaginationRuntimeTool {
    pub fn new(deps: BrowserDeps) -> Self {
        Self {
            deps: Arc::new(deps),
        }
    }
}

#[async_trait]
impl RuntimeTool for ExtractWithPaginationRuntimeTool {
    fn definition(&self) -> ToolDefinition {
        TOOL_CATALOG
            .get("extract_with_pagination")
            .unwrap_or_else(|| {
                ToolDefinition::new("extract_with_pagination", "分页抽取浏览器表格数据")
            })
    }

    async fn execute(
        &self,
        input: Value,
        _ctx: ToolExecutionContext,
    ) -> Result<ToolResult, ToolError> {
        let pagination_js = input
            .get("next_selector")
            .and_then(Value::as_str)
            .unwrap_or("");
        let max_pages = input
            .get("max_pages")
            .and_then(Value::as_u64)
            .map(|v| v as u32);

        let filename = format!(
            "table_data_{}.json",
            chrono::Utc::now().format("%Y%m%d_%H%M%S")
        );
        let file_info = self
            .deps
            .file_manager
            .write_file("generated", &filename, b"{}")
            .map_err(|e| {
                ToolError::ExecutionFailed(format!("Failed to create output file: {}", e))
            })?;
        let save_path = self.deps.file_manager.full_path(&file_info.stored_path);

        info!(
            "[BROWSER] extract_with_pagination: save_path={:?}",
            save_path
        );

        let result = connector_engine(&self.deps)?
            .browser_extract_with_pagination(&save_path.to_string_lossy(), pagination_js, max_pages)
            .await
            .map_err(|e| {
                ToolError::ExecutionFailed(format!("extract_with_pagination failed: {}", e))
            })?;

        let total_rows = result["totalRows"].as_u64().unwrap_or(0);
        let total_pages = result["totalPages"].as_u64().unwrap_or(0);
        let file_size = result["fileSize"].as_u64().unwrap_or(0);
        let headers = result["headers"]
            .as_array()
            .map(|arr| {
                arr.iter()
                    .filter_map(|h| h.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            })
            .unwrap_or_default();

        let file_id = uuid::Uuid::new_v4().to_string();
        let _ = self.deps.storage.insert_generated_file(
            &file_id,
            &self.deps.conversation_id,
            None,
            &filename,
            &file_info.stored_path,
            "json",
            file_size as i64,
            "data",
            Some("Extracted table data (all pages)"),
            1,
            true,
            None,
            None,
            None,
        );

        let mut output = format!(
            "### Data extracted successfully\n\
             - **Total rows**: {}\n\
             - **Total pages**: {}\n\
             - **Columns**: {}\n\
             - **File**: {}\n\
             - **Size**: {:.1} KB\n\n\
             Use `execute_python` with `pd.read_json('{}')` to load.",
            total_rows,
            total_pages,
            headers,
            save_path.display(),
            file_size as f64 / 1024.0,
            save_path.display(),
        );

        if let Some(sample) = result.get("sampleRows").and_then(|s| s.as_array()) {
            if !sample.is_empty() {
                let s = serde_json::to_string_pretty(sample).unwrap_or_default();
                let end = s
                    .char_indices()
                    .take_while(|(i, _)| *i < 1500)
                    .last()
                    .map(|(i, c)| i + c.len_utf8())
                    .unwrap_or(s.len().min(1500));
                output.push_str("\n\n### Sample\n```json\n");
                output.push_str(&s[..end]);
                output.push_str("\n```");
            }
        }

        Ok(tool_result("extract_with_pagination", output))
    }
}
