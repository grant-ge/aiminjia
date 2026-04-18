//! internal_system handlers — browser automation tool executors.
//! browse_navigate, read_page_content, page_execute_js, browse_and_extract,
//! browse_data (sub-agent), extract_table_data.

use anyhow::{anyhow, Result};
use async_trait::async_trait;
use log::{info, warn};
use serde_json::Value;

use super::{optional_str, require_str};
use crate::plugin::context::PluginContext;
use crate::runtime::cancellation::CancellationToken;
use crate::runtime::tools::builtin::browse_data::{
    BrowseDataLaunchContext, BrowseDataLaunchRequest, BrowseDataLauncher,
};

#[derive(Clone)]
pub(crate) struct DefaultBrowseDataLauncher {
    base_ctx: PluginContext,
}

impl DefaultBrowseDataLauncher {
    pub fn new(base_ctx: PluginContext) -> Self {
        Self { base_ctx }
    }

    fn scoped_plugin_ctx(&self, launch_ctx: &BrowseDataLaunchContext) -> PluginContext {
        let mut ctx = self.base_ctx.clone();
        ctx.conversation_id = launch_ctx.session_id.as_str().to_string();
        ctx.session_id = launch_ctx.session_id.clone();
        ctx.run_id = launch_ctx.parent_run_id.clone();
        ctx.agent_id = launch_ctx.parent_agent_id.clone();
        ctx.read_file_state = self
            .base_ctx
            .read_file_state
            .as_ref()
            .map(|cache| cache.clone_for_child());
        ctx
    }
}

#[async_trait]
impl BrowseDataLauncher for DefaultBrowseDataLauncher {
    async fn launch(
        &self,
        request: BrowseDataLaunchRequest,
        context: BrowseDataLaunchContext,
    ) -> Result<String> {
        let scoped_ctx = self.scoped_plugin_ctx(&context);
        launch_browse_data_with_plugin_ctx(
            &scoped_ctx,
            request,
            Some(context.cancellation),
            self.base_ctx.run_id.is_some(),
        )
        .await
    }
}

async fn launch_browse_data_with_plugin_ctx(
    ctx: &PluginContext,
    request: BrowseDataLaunchRequest,
    cancel_token: Option<CancellationToken>,
    sub_agent_background: bool,
) -> Result<String> {
    let BrowseDataLaunchRequest { task, url } = request;
    let url = url.as_deref();

    let gateway = ctx
        .gateway
        .as_ref()
        .ok_or_else(|| anyhow!("LLM gateway not available for sub-agent"))?;
    let tool_registry = ctx
        .tool_registry
        .as_ref()
        .ok_or_else(|| anyhow!("Tool registry not available for sub-agent"))?;
    let app_settings = ctx
        .app_settings
        .as_ref()
        .ok_or_else(|| anyhow!("App settings not available for sub-agent"))?;

    info!("[CONNECTOR] browse_data: task='{}', url={:?}", task, url);

    // Load browser_agent prompt
    let loaded_prompt = crate::llm::prompts::get_browser_agent_prompt();
    info!(
        "[CONNECTOR] browser_agent prompt: {} chars, starts_with='{}'",
        loaded_prompt.len(),
        loaded_prompt.chars().take(60).collect::<String>()
    );
    let system_prompt = if loaded_prompt.len() > 50 {
        loaded_prompt
    } else {
        // Fallback: inline prompt (in case file loading fails)
        warn!(
            "[CONNECTOR] browser_agent prompt NOT loaded (got {} chars), using inline fallback",
            loaded_prompt.len()
        );
        r#"你是数据提取专家。从内部业务系统中提取用户需要的数据。

## 严格规则（必须遵守）

1. **提取表格数据必须用 `extract_table_data()`** — 禁止用 page_execute_js 写 JS 自行提取表格
2. **翻页必须用 `page_execute_js` 点击翻页按钮** — 不要用 URL 翻页
3. **每翻一页后必须再调 `extract_table_data()`** — 数据会自动追加到同一文件
4. 一次只提取一个数据表
5. ACCESS DENIED → 立即停止并报告

## 固定流程

步骤 1: browse_and_extract(url) — 打开目标页面
步骤 2: extract_table_data() — 提取当前页表格数据（返回分页信息）
步骤 3: 如果有下一页：page_execute_js("点击下一页") → extract_table_data() → 循环
步骤 4: 没有下一页时停止，报告文件路径、总行数、列名

## 禁止事项
- 禁止用 page_execute_js 提取表格数据
- 禁止用 page_execute_js 遍历 DOM 获取行数据
- 禁止用 browse_navigate 翻页"#
            .to_string()
    };

    // Build dynamic context: site map from connector engine
    let mut dynamic_context = String::new();
    let mut has_known_apis = false;
    let mut has_known_tables = false;
    let mut target_page_hint = String::new();

    if let Some(ref engine) = ctx.connector_engine {
        // Try Playwright first, fall back to CDP for site map context
        let pw = engine.playwright_browser_ref().await;
        if let Some(pw) = pw.as_ref() {
            if let Some(ctx_str) = pw.get_site_map_context(None).await {
                dynamic_context = ctx_str;
            }
            if let Some(target_url) = url {
                let url_path = url::Url::parse(target_url)
                    .ok()
                    .map(|u| u.path().to_string())
                    .unwrap_or_default();
                let origin = url::Url::parse(target_url)
                    .ok()
                    .map(|u| format!("{}://{}", u.scheme(), u.host_str().unwrap_or("")))
                    .unwrap_or_default();
                if let Some(profile) = pw.get_cached_page_profile(&origin, &url_path).await {
                    has_known_apis = !profile.api_endpoints.is_empty();
                    has_known_tables = !profile.table_schemas.is_empty();
                    if has_known_tables {
                        let table_info: Vec<String> = profile
                            .table_schemas
                            .iter()
                            .map(|t| {
                                format!(
                                    "{} ({} rows, cols: {})",
                                    if t.name.is_empty() { "table" } else { &t.name },
                                    t.row_count,
                                    t.headers.join(", ")
                                )
                            })
                            .collect();
                        target_page_hint = format!(
                            "\n\n[已知页面信息: {}]\n表格: {}\nAPI端点: {}\n表单: {}",
                            url_path,
                            table_info.join("; "),
                            if has_known_apis {
                                "有"
                            } else {
                                "无（该系统可能是传统SSR架构，数据直接嵌在HTML中）"
                            },
                            if profile.forms.is_empty() {
                                "无"
                            } else {
                                "有"
                            },
                        );
                    }
                }
            }
        }
    }

    // Check if site map has multiple pages with tables — ask user to choose
    if let Some(ref engine) = ctx.connector_engine {
        if let Some(target_url) = url {
            let origin = url::Url::parse(target_url)
                .ok()
                .map(|u| format!("{}://{}", u.scheme(), u.host_str().unwrap_or("")))
                .unwrap_or_default();
            if !origin.is_empty() {
                let pw = engine.playwright_browser_ref().await;
                if let Some(pw) = pw.as_ref() {
                    let pages_with_tables = pw.get_pages_with_tables(&origin).await;
                    if pages_with_tables.len() > 1 {
                        // Multiple data pages found — return list for user to choose
                        let mut output = format!(
                            "Found {} pages with data tables on {}:\n\n",
                            pages_with_tables.len(),
                            origin
                        );
                        for (i, p) in pages_with_tables.iter().enumerate() {
                            let tables_desc: Vec<String> = p
                                .table_schemas
                                .iter()
                                .map(|t| {
                                    format!(
                                        "{} ({} rows)",
                                        if t.name.is_empty() { "table" } else { &t.name },
                                        t.row_count
                                    )
                                })
                                .collect();
                            output.push_str(&format!(
                                "{}. **{}** — {}{}\n   Tables: {}\n\n",
                                i + 1,
                                p.title,
                                origin,
                                p.url_path,
                                tables_desc.join(", ")
                            ));
                        }
                        output.push_str("Please ask the user which page to extract data from, then call browse_data again with the specific URL.");
                        return Ok(output);
                    }
                }
            }
        }
    }

    // Build task message with strategy hints based on cached site map
    let mut task_msg = if let Some(url) = url {
        format!("{}\n\nTarget URL: {}", task, url)
    } else {
        task.to_string()
    };

    // If site map has a cached page with tables, give SubAgent a shortcut to skip exploration
    if let Some(ref engine) = ctx.connector_engine {
        if let Some(target_url) = url {
            let origin = url::Url::parse(target_url)
                .ok()
                .map(|u| format!("{}://{}", u.scheme(), u.host_str().unwrap_or("")))
                .unwrap_or_default();
            if !origin.is_empty() {
                let pw = engine.playwright_browser_ref().await;
                if let Some(pw) = pw.as_ref() {
                    let pages = pw.get_pages_with_tables(&origin).await;
                    if pages.len() == 1 {
                        let p = &pages[0];
                        let data_url = format!("{}{}", origin, p.url_path);
                        task_msg.push_str(&format!(
                            "\n\n**快速路径（跳过探索）**: 站点地图已缓存数据页面。\
                             直接执行以下 3 步：\n\
                             1. `browse_navigate(\"{}\")` — 打开数据页面\n\
                             2. `extract_table_data()` — 提取第一页数据\n\
                             3. 如果提示 MORE DATA AVAILABLE：`page_execute_js(\"...\")` 翻页 → `extract_table_data()` → 循环\n\
                             **不需要从首页开始探索菜单。**",
                            data_url,
                        ));
                    }
                }
            }
        }
    }

    task_msg.push_str(&target_page_hint);

    // Add strategy based on what we know
    if has_known_tables && !has_known_apis {
        task_msg.push_str(
            "\n\n**策略**: 该系统是传统 SSR，没有 JSON API。\
            用 `extract_table_data` 提取表格，用 `page_execute_js` 翻页。",
        );
    } else if has_known_apis {
        task_msg.push_str(
            "\n\n**策略**: 该页面有已知的 API 端点。直接用 browse_and_extract 的 API 模式调用。",
        );
    }

    let config = crate::llm::sub_agent::SubAgentConfig {
        task: task_msg,
        system_prompt,
        allowed_tools: vec![
            "browse_and_extract".to_string(),
            "browse_navigate".to_string(),
            "read_page_content".to_string(),
            "page_execute_js".to_string(),
            "extract_table_data".to_string(),
            "extract_with_pagination".to_string(),
        ],
        max_iterations: 30,
        dynamic_context,
        conversation_id: ctx.conversation_id.clone(),
        parent_run_id: ctx.run_id.clone(),
        background: sub_agent_background,
        app_handle: ctx.app_handle.clone(),
        cancel_token,
    };

    let result =
        crate::llm::sub_agent::run_sub_agent(gateway, tool_registry, ctx, config, app_settings)
            .await
            .map_err(|e| {
                warn!("[CONNECTOR] browse_data sub-agent failed: {}", e);
                anyhow!("Browser agent failed: {}", e)
            })?;

    info!(
        "[CONNECTOR] browse_data complete: iterations={}, files={}, output_len={}",
        result.iterations_used,
        result.files.len(),
        result.output.len()
    );

    // Format result for main agent
    let mut output = format!(
        "Browser agent completed in {} iterations.\n\n",
        result.iterations_used
    );

    if !result.files.is_empty() {
        output.push_str("### Extracted Data Files\n");
        for f in &result.files {
            // Try to register each file into the conversation
            let src = std::path::Path::new(f);
            if src.exists() {
                let file_name = src
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_else(|| "data.json".to_string());
                if let Ok(content) = std::fs::read(src) {
                    match ctx
                        .file_manager
                        .write_file("generated", &file_name, &content)
                    {
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
                                1,
                                true,
                                None,
                                None,
                                None,
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
            let end = result
                .output
                .char_indices()
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

/// Handle browse_navigate tool invocations (V4 — open browsing mode).
///
/// Opens any URL in the CDP browser. No app lookup, no pre-configuration needed.
pub(crate) async fn handle_browse_navigate(ctx: &PluginContext, args: &Value) -> Result<String> {
    let url = require_str(args, "url")?;

    let engine = ctx
        .connector_engine
        .as_ref()
        .ok_or_else(|| anyhow!("Internal app connector not initialized"))?;

    info!("[CONNECTOR] browse_navigate: url='{}'", url);

    let result = engine.browser_navigate(url).await.map_err(|e| {
        warn!(
            "[CONNECTOR] browse_navigate failed: url='{}', error={}",
            url, e
        );
        anyhow!(e)
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
            output.push_str(
                "\nUse read_page_content to extract data, or page_execute_js to interact.",
            );
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

    let engine = ctx
        .connector_engine
        .as_ref()
        .ok_or_else(|| anyhow!("Internal app connector not initialized"))?;

    info!("[CONNECTOR] read_page_content");

    let result = engine
        .browser_read_content(extract_script)
        .await
        .map_err(|e| {
            warn!("[CONNECTOR] read_page_content failed: error={}", e);
            anyhow!(e)
        })?;

    info!(
        "[CONNECTOR] read_page_content complete: tables={}, text_len={}",
        result.tables.len(),
        result.text.len()
    );

    // Format result for LLM
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

    Ok(output)
}

/// Handle page_execute_js tool invocations (V4 — open browsing mode).
///
/// Executes custom JavaScript on the active page (click, fill, scroll, paginate).
pub(crate) async fn handle_page_execute_js(ctx: &PluginContext, args: &Value) -> Result<String> {
    let script = require_str(args, "script")?;

    let engine = ctx
        .connector_engine
        .as_ref()
        .ok_or_else(|| anyhow!("Internal app connector not initialized"))?;

    info!("[CONNECTOR] page_execute_js: script_len={}", script.len());

    let result = engine.browser_execute_js(script).await.map_err(|e| {
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
    let request = BrowseDataLaunchRequest {
        task: require_str(args, "task")?.to_string(),
        url: optional_str(args, "url").map(str::to_string),
    };
    launch_browse_data_with_plugin_ctx(ctx, request, None, ctx.run_id.is_some()).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::tools::capability::{FileState, FileStateCache};
    use crate::storage::file_manager::FileManager;
    use crate::storage::file_store::AppStorage;
    use std::path::PathBuf;
    use std::sync::Arc;
    use tempfile::TempDir;

    #[allow(deprecated)]
    fn make_plugin_ctx(
        workspace: &std::path::Path,
        read_file_state: Option<Arc<FileStateCache>>,
    ) -> PluginContext {
        let storage = Arc::new(AppStorage::new(workspace).expect("AppStorage::new failed"));
        let file_manager = Arc::new(FileManager::new(workspace));
        let session_manager = Arc::new(crate::python::session::PythonSessionManager::new(
            workspace.to_path_buf(),
            None,
        ));

        PluginContext {
            storage,
            file_manager,
            workspace_path: workspace.to_path_buf(),
            conversation_id: "parent-conv".to_string(),
            session_id: crate::runtime::ids::SessionId::new("parent-conv"),
            run_id: Some(crate::runtime::ids::RunId::new("run-parent")),
            agent_id: Some(crate::runtime::ids::AgentId::new("agent-parent")),
            tavily_api_key: None,
            bocha_api_key: None,
            app_handle: None,
            session_manager,
            auth_manager: None,
            connector_engine: None,
            use_cloud: false,
            model: String::new(),
            gateway: None,
            tool_registry: None,
            app_settings: None,
            agent_runtime: None,
            event_bus: None,
            authorized_workspace: None,
            read_file_state,
        }
    }

    #[test]
    fn scoped_plugin_ctx_clones_read_file_state_for_child() {
        let workspace = TempDir::new().expect("TempDir::new failed");
        let target = PathBuf::from("/tmp/subagent-launcher-cache.txt");
        let parent_cache = Arc::new(FileStateCache::new());
        parent_cache.set(
            target.clone(),
            FileState {
                content: "parent".to_string(),
                mtime_secs: 1_000,
                offset: None,
                limit: None,
            },
        );

        let launcher = DefaultBrowseDataLauncher::new(make_plugin_ctx(
            workspace.path(),
            Some(parent_cache.clone()),
        ));

        let child_ctx = launcher.scoped_plugin_ctx(&BrowseDataLaunchContext {
            session_id: crate::runtime::ids::SessionId::new("child-conv"),
            parent_run_id: Some(crate::runtime::ids::RunId::new("run-child-parent")),
            parent_agent_id: Some(crate::runtime::ids::AgentId::new("agent-child-parent")),
            cancellation: crate::runtime::cancellation::CancellationToken::new(),
        });

        let child_cache = child_ctx
            .read_file_state
            .expect("scoped subagent ctx should carry read_file_state");
        assert!(
            !Arc::ptr_eq(&parent_cache, &child_cache),
            "child subagent context must receive a cloned file-state cache"
        );
        let inherited = child_cache
            .get(&target)
            .expect("child cache should inherit parent snapshot");
        assert_eq!(inherited.content, "parent");

        child_cache.set(
            target.clone(),
            FileState {
                content: "child".to_string(),
                mtime_secs: 2_000,
                offset: None,
                limit: None,
            },
        );

        let parent_state = parent_cache
            .get(&target)
            .expect("parent cache should stay intact");
        assert_eq!(parent_state.content, "parent");
        assert_eq!(parent_state.mtime_secs, 1_000);
    }
}

/// Handle extract_table_data — extract current page table data + pagination info.
/// Does NOT auto-paginate. LLM decides how to flip pages.
pub(crate) async fn handle_extract_table_data(ctx: &PluginContext, args: &Value) -> Result<String> {
    let engine = ctx
        .connector_engine
        .as_ref()
        .ok_or_else(|| anyhow!("Internal app connector not initialized"))?;

    // Build save path in conversation's generated dir (for incremental append)
    let filename = format!(
        "table_data_{}.json",
        chrono::Utc::now().format("%Y%m%d_%H%M%S")
    );
    let file_info = ctx
        .file_manager
        .write_file("generated", &filename, b"{}")
        .map_err(|e| anyhow!("Failed to create output file: {}", e))?;
    let save_path = ctx.file_manager.full_path(&file_info.stored_path);

    info!("[CONNECTOR] extract_table_data: save_path={:?}", save_path);

    let result = engine
        .browser_extract_table_data(&save_path.to_string_lossy(), None, None)
        .await
        .map_err(|e| anyhow!("extract_table_data failed: {}", e))?;

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

    // Register file in conversation
    let file_size = std::fs::metadata(&save_path)
        .map(|m| m.len() as i64)
        .unwrap_or(0);
    if file_size > 10 {
        let file_id = uuid::Uuid::new_v4().to_string();
        let _ = ctx.storage.insert_generated_file(
            &file_id,
            &ctx.conversation_id,
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

    // Pagination info for LLM to decide next steps
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
             You MUST use `page_execute_js` to click the next-page button in the iframe, \
             then call `extract_table_data` again. Repeat until all pages are extracted.\n\
             Example: page_execute_js(\"document.querySelector('.layui-laypage-next').click()\")\n",
            total, total_saved,
        ));
    } else {
        output.push_str("\nAll data has been extracted.\n");
    }

    // Sample rows
    if let Some(sample) = result.get("sampleRows").and_then(|s| s.as_array()) {
        if !sample.is_empty() {
            output.push_str("\n### Sample\n```json\n");
            let sample_str = serde_json::to_string_pretty(sample).unwrap_or_default();
            if sample_str.len() > 2000 {
                // Safe UTF-8 truncation
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

    Ok(output)
}

/// Handle extract_with_pagination.
pub(crate) async fn handle_extract_with_pagination(
    ctx: &PluginContext,
    args: &Value,
) -> Result<String> {
    let pagination_js = optional_str(args, "pagination_js").unwrap_or("");
    let max_pages = args["max_pages"].as_u64().map(|v| v as u32);
    let engine = ctx
        .connector_engine
        .as_ref()
        .ok_or_else(|| anyhow!("Internal app connector not initialized"))?;
    let filename = format!(
        "table_data_{}.json",
        chrono::Utc::now().format("%Y%m%d_%H%M%S")
    );
    let file_info = ctx
        .file_manager
        .write_file("generated", &filename, b"{}")
        .map_err(|e| anyhow!("Failed to create output file: {}", e))?;
    let save_path = ctx.file_manager.full_path(&file_info.stored_path);
    info!(
        "[CONNECTOR] extract_with_pagination: save_path={:?}",
        save_path
    );
    let result = engine
        .browser_extract_with_pagination(&save_path.to_string_lossy(), pagination_js, max_pages)
        .await
        .map_err(|e| anyhow!("extract_with_pagination failed: {}", e))?;
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
    let _ = ctx.storage.insert_generated_file(
        &file_id,
        &ctx.conversation_id,
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
        "### Data extracted successfully\n- **Total rows**: {}\n- **Total pages**: {}\n- **Columns**: {}\n- **File**: {}\n- **Size**: {:.1} KB\n\nUse `execute_python` with `pd.read_json('{}')` to load.",
        total_rows, total_pages, headers, save_path.display(), file_size as f64 / 1024.0, save_path.display(),
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

    let engine = ctx
        .connector_engine
        .as_ref()
        .ok_or_else(|| anyhow!("Internal app connector not initialized"))?;

    // Smart routing: non-GET or has body → API mode
    let is_api_mode = method != "GET" || body.is_some();

    if is_api_mode {
        // ── API Mode ──
        info!(
            "[CONNECTOR] browse_and_extract API mode: {} '{}'",
            method, url
        );

        let result = engine
            .browser_api_fetch(url, &method, body, headers)
            .await
            .map_err(|e| {
                warn!("[CONNECTOR] browse_and_extract API failed: {}", e);
                anyhow!(e)
            })?;

        let mut output = format!(
            "API Response: {} {}\nStatus: {}, Content-Type: {}\n\n",
            method, url, result.status, result.content_type
        );

        if let Some(ref path) = result.saved_file_path {
            // Copy to workspace and register in conversation file_index
            let file_name = path
                .file_name()
                .map(|f| f.to_string_lossy().to_string())
                .unwrap_or_else(|| "api_data.json".to_string());

            let registered_path = if let Ok(content) = std::fs::read(path) {
                // Write to conversation's generated dir via file_manager
                match ctx
                    .file_manager
                    .write_file("generated", &file_name, &content)
                {
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
                            1,
                            true,
                            None,
                            None,
                            None,
                        );
                        info!(
                            "[CONNECTOR] Registered API data file: {} ({})",
                            file_info.stored_path, file_info.file_size
                        );
                        // Use the workspace path for LLM
                        ctx.file_manager
                            .full_path(&file_info.stored_path)
                            .display()
                            .to_string()
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

        // Format JSON data compactly (safe UTF-8 truncation)
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

        Ok(output)
    } else {
        // ── Page Mode ──
        info!("[CONNECTOR] browse_and_extract page mode: '{}'", url);

        let result = engine
            .browser_navigate_and_extract(url, extract_script)
            .await
            .map_err(|e| {
                warn!("[CONNECTOR] browse_and_extract page failed: {}", e);
                anyhow!(e)
            })?;

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

        // Navigation & Actions (links)
        if !result.content.links.is_empty() {
            output.push_str("\n### Navigation & Actions\n");
            for link in &result.content.links {
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

        // Discovered API endpoints
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
                    "- {} {} → {} ({} {})\n",
                    api.method, api.url, api.status, size, ct_short
                ));
            }
            output.push_str(
                "Tip: Use browse_and_extract with these API URLs to fetch data directly.\n",
            );
        }

        // Forms
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

        // Page text (only if no tables)
        if !result.content.text.is_empty() && result.content.tables.is_empty() {
            output.push_str("\n### Page Text\n");
            output.push_str(&result.content.text);
            output.push('\n');
        }

        Ok(output)
    }
}
