//! Playwright-based browser automation via Node.js sidecar process.
//!
//! Replaces chromiumoxide with Playwright for reliable iframe handling.
//! Communicates with a long-running Node.js process (browser.js) via
//! stdin/stdout JSON line protocol.

use std::collections::HashMap;
use std::path::PathBuf;
use std::process::Stdio;

use log::{info, warn};
use serde_json::Value;
use tauri::{AppHandle, Emitter, Manager};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, BufWriter};
use tokio::process::{Child, ChildStdin, ChildStdout};
use tokio::sync::Mutex;

use super::site_map::{PageProfile, SiteMap, TableSchema};
use super::webview_auth::{
    ApiFetchResult, BrowseNavigateResult, BrowseResult, DiscoveredApi, ExecuteJsResult,
    FormData, FormField, FullPageResult, LinkData, TableData,
};

// ── Internal types ──────────────────────────────────────────────

struct PlaywrightProcess {
    child: Child,
    next_id: u64,
}

/// I/O handles for the sidecar process, protected by a separate mutex
/// to avoid holding the state lock during async I/O.
struct PlaywrightIO {
    stdin: BufWriter<ChildStdin>,
    stdout: BufReader<ChildStdout>,
}

/// Timeout for a single Playwright command (covers navigate + extract).
const PLAYWRIGHT_CMD_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(120);

struct BrowserState {
    process: Option<PlaywrightProcess>,
    active_origin: Option<String>,
    active_url: Option<String>,
    request_counter: u32,
}

impl BrowserState {
    fn new() -> Self {
        Self { process: None, active_origin: None, active_url: None, request_counter: 0 }
    }
}

// ── Public API ──────────────────────────────────────────────────

pub struct PlaywrightBrowser {
    app_handle: AppHandle,
    state: Mutex<BrowserState>,
    /// Separate mutex for stdin/stdout I/O — prevents deadlock with state lock.
    io: Mutex<Option<PlaywrightIO>>,
    site_maps: Mutex<HashMap<String, SiteMap>>,
}

impl PlaywrightBrowser {
    pub fn new(app_handle: AppHandle) -> Self {
        Self {
            app_handle,
            state: Mutex::new(BrowserState::new()),
            io: Mutex::new(None),
            site_maps: Mutex::new(HashMap::new()),
        }
    }

    /// Navigate to a URL. Launches browser lazily.
    pub async fn navigate(&self, url: &str) -> Result<BrowseNavigateResult, String> {
        self.ensure_running().await?;

        let target_origin = Self::extract_origin(url)?;
        info!("[Playwright] navigate: url={}", url);
        let _ = self.app_handle.emit("browser:navigating", serde_json::json!({ "url": url }));

        // Navigate
        let nav_result = self.send_command("navigate", serde_json::json!({ "url": url })).await?;
        let title = nav_result["title"].as_str().unwrap_or("").to_string();
        let final_url = nav_result["url"].as_str().unwrap_or(url).to_string();

        // Update state
        {
            let mut state = self.state.lock().await;
            state.active_origin = Some(target_origin.clone());
            state.active_url = Some(final_url.clone());
        }

        // Detect redirect
        let redirected_to_login = Self::detect_redirect(url, &final_url, &target_origin);

        // Auto-explore (extract page profile)
        let page_profile = if !redirected_to_login {
            let url_path = Self::extract_path(&final_url);
            let cached = {
                let maps = self.site_maps.lock().await;
                maps.get(&target_origin).and_then(|m| m.get_page(&url_path)).cloned()
            };
            if let Some(profile) = cached {
                info!("[Playwright] Using cached profile for {}", url_path);
                Some(profile)
            } else {
                let profile = self.auto_explore(&title, &target_origin, &url_path).await;
                let app_data_dir = self.get_app_data_dir();
                {
                    let mut maps = self.site_maps.lock().await;
                    let site_map = maps.entry(target_origin.clone())
                        .or_insert_with(|| SiteMap::load(&app_data_dir, &target_origin)
                            .unwrap_or_else(|| SiteMap::new(&target_origin)));
                    site_map.set_page(profile.clone());
                    let _ = site_map.save(&app_data_dir);
                }
                Some(profile)
            }
        } else {
            None
        };

        // Screenshot
        let screenshot_path = self.capture_screenshot().await;

        let result = BrowseNavigateResult {
            url: final_url.clone(),
            title: title.clone(),
            redirected_to_login,
            page_profile,
            screenshot_path,
        };

        let _ = self.app_handle.emit("browser:page-ready", serde_json::json!({
            "url": &result.url, "title": &result.title,
        }));

        info!("[Playwright] navigate complete: url={}, title={}, redirected={}",
            result.url, result.title, result.redirected_to_login);
        Ok(result)
    }

    /// Read structured data from the active page.
    pub async fn read_content(&self, _extract_script: Option<&str>) -> Result<BrowseResult, String> {
        self.ensure_running().await?;

        info!("[Playwright] read_content (extract from all frames)");
        let data = self.send_command("extract", serde_json::json!({})).await?;

        let url = data["url"].as_str().unwrap_or("").to_string();
        let title = data["title"].as_str().unwrap_or("").to_string();
        let text = data["text"].as_str().unwrap_or("").to_string();
        let tables = Self::parse_tables(&data["tables"]);
        let links = Self::parse_links(&data["links"]);

        info!("[Playwright] read_content: url={}, tables={}, links={}, text_len={}",
            url, tables.len(), links.len(), text.len());

        Ok(BrowseResult { url, title, tables, text, links })
    }

    /// Execute JavaScript on the active page.
    pub async fn execute_js(&self, script: &str) -> Result<ExecuteJsResult, String> {
        self.ensure_running().await?;

        info!("[Playwright] execute_js: script_len={}", script.len());

        let data = self.send_command("execute_js", serde_json::json!({ "script": script })).await?;

        let result_type = data["type"].as_str().unwrap_or("result");
        let new_url = data["url"].as_str().map(String::from);
        let new_title = data["title"].as_str().map(String::from);

        if result_type == "error" {
            let error_msg = data["error"].as_str().unwrap_or("Unknown error").to_string();
            info!("[Playwright] execute_js error: {}", error_msg);
            return Ok(ExecuteJsResult {
                value: serde_json::Value::Null, error: Some(error_msg), new_url, new_title,
            });
        }

        info!("[Playwright] execute_js complete");
        Ok(ExecuteJsResult {
            value: data["value"].clone(), error: None, new_url, new_title,
        })
    }

    /// Navigate + extract everything in one shot.
    pub async fn navigate_and_extract(
        &self, url: &str, extract_script: Option<&str>,
    ) -> Result<FullPageResult, String> {
        // Navigate
        let nav = self.navigate(url).await?;

        let target_origin = Self::extract_origin(url)?;
        let redirected = nav.redirected_to_login;

        // Extract content
        let content = if !redirected {
            self.read_content(extract_script).await.unwrap_or_else(|e| {
                warn!("[Playwright] navigate_and_extract: read_content failed: {}", e);
                BrowseResult {
                    url: nav.url.clone(), title: nav.title.clone(),
                    tables: vec![], text: String::new(), links: vec![],
                }
            })
        } else {
            BrowseResult {
                url: nav.url.clone(), title: nav.title.clone(),
                tables: vec![], text: String::new(), links: vec![],
            }
        };

        // Discover forms
        let forms = if !redirected {
            self.extract_forms().await
        } else {
            vec![]
        };

        let navigate_result = BrowseNavigateResult {
            url: nav.url, title: nav.title, redirected_to_login: redirected,
            page_profile: None, screenshot_path: None,
        };

        // Save page profile
        if !redirected {
            let url_path = Self::extract_path(&navigate_result.url);
            let profile = PageProfile {
                url_path: url_path.clone(),
                title: navigate_result.title.clone(),
                nav_links: content.links.clone(),
                table_schemas: content.tables.iter().map(|t| TableSchema {
                    name: String::new(),
                    headers: t.headers.clone(),
                    row_count: t.rows.len(),
                }).collect(),
                forms: forms.clone(),
                api_endpoints: vec![],
                explored_at: chrono::Utc::now(),
                access_denied: false,
            };
            let app_data_dir = self.get_app_data_dir();
            let mut maps = self.site_maps.lock().await;
            let site_map = maps.entry(target_origin.clone())
                .or_insert_with(|| SiteMap::load(&app_data_dir, &target_origin)
                    .unwrap_or_else(|| SiteMap::new(&target_origin)));
            site_map.set_page(profile);
            let _ = site_map.save(&app_data_dir);
        }

        info!("[Playwright] navigate_and_extract complete: url={}, tables={}, links={}, forms={}",
            content.url, content.tables.len(), content.links.len(), forms.len());

        Ok(FullPageResult {
            navigate: navigate_result,
            content,
            api_calls: vec![],
            forms,
        })
    }

    /// Execute fetch() in page context for REST API calls.
    pub async fn api_fetch(
        &self, url: &str, method: &str, body: Option<&str>, headers: Option<&str>,
    ) -> Result<ApiFetchResult, String> {
        self.ensure_running().await?;

        info!("[Playwright] api_fetch: {} {}", method, url);

        let params = serde_json::json!({
            "url": url,
            "method": method,
            "body": body.and_then(|b| serde_json::from_str::<Value>(b).ok()),
            "headers": headers.and_then(|h| serde_json::from_str::<Value>(h).ok()),
        });

        let data = self.send_command("fetch", params).await?;

        let status = data["status"].as_u64().unwrap_or(0) as u16;
        let content_type = data["contentType"].as_str().unwrap_or("").to_string();
        let total_rows = data["totalRows"].as_u64();
        let resp_data = data["data"].clone();

        // Save large data to file
        let data_json = serde_json::to_string(&resp_data).unwrap_or_default();
        let (final_data, truncated, saved_file_path) = if data_json.len() > 50_000 {
            let dir = self.get_app_data_dir().join("api-data");
            std::fs::create_dir_all(&dir).ok();
            let filename = format!("api_{}_{}.json",
                chrono::Utc::now().format("%Y%m%d_%H%M%S"),
                url.split('/').last().unwrap_or("data").split('?').next().unwrap_or("data"));
            let path = dir.join(&filename);
            let _ = std::fs::write(&path, &data_json);
            info!("[Playwright] api_fetch: large response ({} bytes) saved to {:?}", data_json.len(), path);
            let sample = Self::build_data_sample(&resp_data, 5);
            (sample, true, Some(path))
        } else {
            (resp_data, false, None)
        };

        info!("[Playwright] api_fetch complete: {} {} → status={}, rows={:?}",
            method, url, status, total_rows);

        Ok(ApiFetchResult { status, content_type, data: final_data, total_rows, truncated, saved_file_path })
    }

    /// Bring browser to front.
    pub async fn show_active_page(&self) -> Result<(), String> {
        self.send_command("show_page", serde_json::json!({})).await?;
        Ok(())
    }

    /// Capture screenshot.
    pub async fn capture_screenshot(&self) -> Option<PathBuf> {
        let dir = self.get_app_data_dir().join("screenshots");
        std::fs::create_dir_all(&dir).ok()?;
        let data = self.send_command("screenshot", serde_json::json!({ "dir": dir.to_string_lossy() })).await.ok()?;
        let path_str = data["path"].as_str()?;
        let path = PathBuf::from(path_str);
        let size = data["size"].as_u64().unwrap_or(0);
        info!("[Playwright] Screenshot saved: {:?} ({} bytes)", path, size);
        Some(path)
    }

    /// Auto-paginate and extract ALL table data. Saves to JSON file.
    /// This is a single command that handles pagination automatically —
    /// no LLM involvement needed for iterating pages.
    pub async fn extract_all_pages(
        &self,
        save_path: &str,
        max_pages: Option<u32>,
        page_size: Option<u32>,
    ) -> Result<Value, String> {
        self.ensure_running().await?;

        info!("[Playwright] extract_all_pages: save_path={}, max_pages={:?}, page_size={:?}",
            save_path, max_pages, page_size);

        let params = serde_json::json!({
            "savePath": save_path,
            "maxPages": max_pages.unwrap_or(50),
            "pageSize": page_size,
        });

        let result = self.send_command("extract_all_pages", params).await?;

        let total_rows = result["totalRows"].as_u64().unwrap_or(0);
        let total_pages = result["totalPages"].as_u64().unwrap_or(0);
        let saved_to = result["savedTo"].as_str().unwrap_or("");

        info!("[Playwright] extract_all_pages complete: {} rows, {} pages, saved to {}",
            total_rows, total_pages, saved_to);

        Ok(result)
    }

    /// Check and increment per-turn rate limit.
    pub async fn check_rate_limit(&self, max_per_turn: u32) -> Result<(), String> {
        let mut state = self.state.lock().await;
        state.request_counter += 1;
        if state.request_counter > max_per_turn {
            return Err(format!("Browser request limit exceeded: {} (max {})", state.request_counter, max_per_turn));
        }
        Ok(())
    }

    /// Reset per-turn counter.
    pub async fn reset_counter(&self) {
        self.state.lock().await.request_counter = 0;
    }

    /// Shutdown browser and Node.js process.
    pub async fn shutdown(&self) {
        let _ = self.send_command("shutdown", serde_json::json!({})).await;
        // Clean up IO
        *self.io.lock().await = None;
        // Clean up process
        let mut state = self.state.lock().await;
        if let Some(mut proc) = state.process.take() {
            let _ = proc.child.kill().await;
        }
        state.active_origin = None;
        state.active_url = None;
        // Clean up profile lock
        let lock_path = self.get_app_data_dir().join("playwright-profile/SingletonLock");
        let _ = std::fs::remove_file(&lock_path);
        info!("[Playwright] Shutdown complete");
    }

    /// Get site map context for LLM injection.
    pub async fn get_site_map_context(&self, active_path: Option<&str>) -> Option<String> {
        let state = self.state.lock().await;
        let origin = state.active_origin.as_ref()?;
        let maps = self.site_maps.lock().await;
        let site_map = maps.get(origin)?;
        Some(site_map.format_for_context(active_path))
    }

    /// Get active origin.
    pub async fn get_active_origin(&self) -> Option<String> {
        self.state.lock().await.active_origin.clone()
    }

    /// Get a cached PageProfile.
    pub async fn get_cached_page_profile(&self, origin: &str, url_path: &str) -> Option<PageProfile> {
        let maps = self.site_maps.lock().await;
        maps.get(origin)?.get_page(url_path).cloned()
    }

    /// Get all cached pages with tables for a given origin.
    pub async fn get_pages_with_tables(&self, origin: &str) -> Vec<PageProfile> {
        let maps = self.site_maps.lock().await;
        match maps.get(origin) {
            Some(site_map) => site_map.pages.values()
                .filter(|p| !p.table_schemas.is_empty() && !p.access_denied)
                .cloned()
                .collect(),
            None => vec![],
        }
    }

    // ── Internals ───────────────────────────────────────────────

    /// Ensure Node.js sidecar is running, launch if needed.
    async fn ensure_running(&self) -> Result<(), String> {
        let mut state = self.state.lock().await;
        if state.process.is_some() {
            return Ok(());
        }
        drop(state); // Release lock during launch

        let node_path = self.find_node()?;
        let script_path = self.find_browser_js()?;
        let browsers_path = self.find_browsers_dir()?;
        let user_data_dir = self.get_app_data_dir().join("playwright-profile");
        std::fs::create_dir_all(&user_data_dir).ok();

        // Clean up stale profile locks from previous crashes
        let singleton_lock = user_data_dir.join("SingletonLock");
        if singleton_lock.exists() {
            warn!("[Playwright] Removing stale SingletonLock, killing orphaned Chromium");
            // Kill all Chromium processes spawned by Playwright (our browsers dir)
            let browsers_dir_str = browsers_path.to_string_lossy().to_string();
            let _ = std::process::Command::new("pkill")
                .args(["-f", &browsers_dir_str])
                .output();
            // Also try killing by profile dir
            let dir_str = user_data_dir.to_string_lossy().to_string();
            let _ = std::process::Command::new("pkill")
                .args(["-f", &dir_str])
                .output();
            // Also kill by "Google Chrome for Testing" (Playwright's Chromium name)
            let _ = std::process::Command::new("pkill")
                .args(["-f", "Google Chrome for Testing"])
                .output();
            tokio::time::sleep(std::time::Duration::from_millis(1000)).await;
            // Force remove lock file
            let _ = std::fs::remove_file(&singleton_lock);
            // Also remove SingletonSocket and SingletonCookie
            let _ = std::fs::remove_file(user_data_dir.join("SingletonSocket"));
            let _ = std::fs::remove_file(user_data_dir.join("SingletonCookie"));
        }

        info!("[Playwright] Launching sidecar: node={:?}, script={:?}", node_path, script_path);

        let mut child = tokio::process::Command::new(&node_path)
            .arg(&script_path)
            .env("PLAYWRIGHT_BROWSERS_PATH", &browsers_path)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit()) // stderr goes to app log
            .kill_on_drop(true)
            .spawn()
            .map_err(|e| format!("Failed to launch Playwright sidecar: {}", e))?;

        let stdin = child.stdin.take().ok_or("Failed to get stdin")?;
        let stdout = child.stdout.take().ok_or("Failed to get stdout")?;

        let mut io = PlaywrightIO {
            stdin: BufWriter::new(stdin),
            stdout: BufReader::new(stdout),
        };
        let mut proc = PlaywrightProcess {
            child,
            next_id: 1,
        };

        // Send launch command (before storing in state, so failure kills process via drop)
        let launch_params = serde_json::json!({
            "userDataDir": user_data_dir.to_string_lossy(),
        });
        let id = proc.next_id;
        proc.next_id += 1;
        let cmd = serde_json::json!({ "id": id, "method": "launch", "params": launch_params });
        let line = serde_json::to_string(&cmd).unwrap() + "\n";
        io.stdin.write_all(line.as_bytes()).await
            .map_err(|e| format!("Failed to send launch: {}", e))?;
        io.stdin.flush().await.map_err(|e| format!("stdin flush: {}", e))?;

        // Read launch response with timeout
        let mut response_line = String::new();
        let read_result = tokio::time::timeout(
            std::time::Duration::from_secs(30),
            io.stdout.read_line(&mut response_line)
        ).await
            .map_err(|_| "Playwright launch timeout (30s)".to_string())?
            .map_err(|e| format!("Failed to read launch response: {}", e))?;

        if read_result == 0 {
            return Err("Playwright sidecar process terminated during launch".to_string());
        }

        let response: Value = serde_json::from_str(response_line.trim())
            .map_err(|e| format!("Invalid launch response: {} (raw: {})", e, response_line.trim()))?;

        if let Some(err) = response.get("error").and_then(|e| e.as_str()) {
            return Err(format!("Playwright launch failed: {}", err));
        }

        info!("[Playwright] Sidecar launched and browser ready");

        // Store process and IO separately
        let mut state = self.state.lock().await;
        state.process = Some(proc);
        drop(state);
        *self.io.lock().await = Some(io);
        Ok(())
    }

    /// Send a JSON-RPC command to the sidecar and return the result.
    /// Uses separate IO mutex to avoid deadlock with state lock.
    async fn send_command(&self, method: &str, params: Value) -> Result<Value, String> {
        // Get next ID from state (brief lock)
        let id = {
            let mut state = self.state.lock().await;
            let proc = state.process.as_mut()
                .ok_or("Playwright sidecar not running")?;
            let id = proc.next_id;
            proc.next_id += 1;
            id
        }; // State lock released

        // IO operations use separate lock (no deadlock risk)
        let mut io_guard = self.io.lock().await;
        let io = io_guard.as_mut()
            .ok_or("Playwright IO not initialized")?;

        let cmd = serde_json::json!({ "id": id, "method": method, "params": params });
        let line = serde_json::to_string(&cmd).unwrap() + "\n";

        io.stdin.write_all(line.as_bytes()).await
            .map_err(|e| format!("stdin write: {}", e))?;
        io.stdin.flush().await
            .map_err(|e| format!("stdin flush: {}", e))?;

        // Read response with timeout
        let read_future = async {
            loop {
                let mut response_line = String::new();
                let bytes_read = io.stdout.read_line(&mut response_line).await
                    .map_err(|e| format!("stdout read: {}", e))?;

                if bytes_read == 0 {
                    return Err("Playwright sidecar process terminated".to_string());
                }

                let trimmed = response_line.trim();
                if trimmed.is_empty() { continue; }

                let response: Value = serde_json::from_str(trimmed)
                    .map_err(|e| format!("Invalid response JSON: {} (raw: {})", e, trimmed))?;

                if response["id"].as_u64() == Some(id) {
                    if let Some(err) = response.get("error").and_then(|e| e.as_str()) {
                        return Err(err.to_string());
                    }
                    return Ok(response["result"].clone());
                }
            }
        };

        tokio::time::timeout(PLAYWRIGHT_CMD_TIMEOUT, read_future)
            .await
            .map_err(|_| format!("Playwright command '{}' timed out after {:?}", method, PLAYWRIGHT_CMD_TIMEOUT))?
    }

    async fn auto_explore(&self, title: &str, origin: &str, url_path: &str) -> PageProfile {
        info!("[Playwright] auto_explore: {}", url_path);

        let data = match self.send_command("extract", serde_json::json!({})).await {
            Ok(d) => d,
            Err(e) => {
                warn!("[Playwright] auto_explore failed: {}", e);
                return PageProfile {
                    url_path: url_path.to_string(), title: title.to_string(),
                    nav_links: vec![], table_schemas: vec![], forms: vec![],
                    api_endpoints: vec![], explored_at: chrono::Utc::now(), access_denied: false,
                };
            }
        };

        let nav_links = Self::parse_links(&data["links"]);
        let tables_raw = Self::parse_tables(&data["tables"]);
        let table_schemas: Vec<TableSchema> = tables_raw.iter().map(|t| TableSchema {
            name: String::new(),
            headers: t.headers.clone(),
            row_count: t.rows.len(),
        }).collect();

        let forms = self.extract_forms().await;

        info!("[Playwright] auto_explore complete: path={}, links={}, tables={}, forms={}",
            url_path, nav_links.len(), table_schemas.len(), forms.len());

        PageProfile {
            url_path: url_path.to_string(),
            title: title.to_string(),
            nav_links,
            table_schemas,
            forms,
            api_endpoints: vec![],
            explored_at: chrono::Utc::now(),
            access_denied: false,
        }
    }

    async fn extract_forms(&self) -> Vec<FormData> {
        // Forms are extracted as part of the extract command in browser.js
        let data = match self.send_command("extract", serde_json::json!({})).await {
            Ok(d) => d,
            Err(_) => return vec![],
        };
        Self::parse_forms(&data["forms"])
    }

    // ── Path resolution ─────────────────────────────────────────

    fn get_app_data_dir(&self) -> PathBuf {
        self.app_handle.path().app_data_dir()
            .unwrap_or_else(|_| std::env::temp_dir())
    }

    fn find_node(&self) -> Result<PathBuf, String> {
        // Check bundled runtime first (production)
        if let Ok(resource_dir) = self.app_handle.path().resource_dir() {
            let bundled = resource_dir.join("playwright-runtime/node/bin/node");
            if bundled.exists() { return Ok(bundled); }
        }
        // Dev mode: relative to src-tauri, canonicalize to absolute
        let dev = PathBuf::from("playwright-runtime/node/bin/node");
        if dev.exists() { return Ok(std::fs::canonicalize(&dev).unwrap_or(dev)); }
        // System node
        let system = PathBuf::from("/usr/local/bin/node");
        if system.exists() { return Ok(system); }
        Err("Node.js not found. Run scripts/setup-playwright.sh".to_string())
    }

    fn find_browser_js(&self) -> Result<PathBuf, String> {
        if let Ok(resource_dir) = self.app_handle.path().resource_dir() {
            let bundled = resource_dir.join("playwright-runtime/browser.js");
            if bundled.exists() { return Ok(bundled); }
        }
        let dev = PathBuf::from("playwright-runtime/browser.js");
        if dev.exists() { return Ok(std::fs::canonicalize(&dev).unwrap_or(dev)); }
        Err("browser.js not found".to_string())
    }

    fn find_browsers_dir(&self) -> Result<PathBuf, String> {
        if let Ok(resource_dir) = self.app_handle.path().resource_dir() {
            let bundled = resource_dir.join("playwright-runtime/browsers");
            if bundled.exists() { return Ok(bundled); }
        }
        let dev = PathBuf::from("playwright-runtime/browsers");
        if dev.exists() { return Ok(std::fs::canonicalize(&dev).unwrap_or(dev)); }
        Err("Playwright browsers not found. Run scripts/setup-playwright.sh".to_string())
    }

    // ── Parsing helpers ─────────────────────────────────────────

    fn extract_origin(url: &str) -> Result<String, String> {
        let parsed = url::Url::parse(url).map_err(|e| format!("Invalid URL '{}': {}", url, e))?;
        let host = parsed.host_str().unwrap_or("");
        match parsed.port() {
            Some(port) => Ok(format!("{}://{}:{}", parsed.scheme(), host, port)),
            None => Ok(format!("{}://{}", parsed.scheme(), host)),
        }
    }

    fn extract_path(url: &str) -> String {
        url::Url::parse(url).ok().map(|u| u.path().to_string()).unwrap_or_default()
    }

    fn detect_redirect(original_url: &str, final_url: &str, target_origin: &str) -> bool {
        let final_origin = Self::extract_origin(final_url).unwrap_or_default();
        if final_origin != *target_origin { return true; }
        let target_path = url::Url::parse(original_url).ok().map(|u| u.path().to_string()).unwrap_or_default();
        let final_path = url::Url::parse(final_url).ok().map(|u| u.path().to_string()).unwrap_or_default();
        if final_path != target_path {
            let fp = final_path.to_lowercase();
            return fp.contains("login") || fp.contains("signin") || fp.contains("/sso")
                || fp.contains("/auth") || fp.contains("/cas/")
                || fp.contains("error") || fp.contains("forbidden") || fp.contains("no_resource")
                || fp.contains("no_permission") || fp.contains("/403") || fp.contains("/404");
        }
        false
    }

    fn parse_tables(val: &Value) -> Vec<TableData> {
        val.as_array().map(|arr| {
            arr.iter().filter_map(|t| {
                let headers: Vec<String> = t["headers"].as_array()?
                    .iter().filter_map(|h| h.as_str().map(String::from)).collect();
                let rows: Vec<HashMap<String, String>> = t["rows"].as_array()?
                    .iter().filter_map(|r| {
                        r.as_object().map(|obj| obj.iter()
                            .map(|(k, v)| (k.clone(), v.as_str().unwrap_or("").to_string()))
                            .collect())
                    }).collect();
                Some(TableData { headers, rows })
            }).collect()
        }).unwrap_or_default()
    }

    fn parse_links(val: &Value) -> Vec<LinkData> {
        val.as_array().map(|arr| {
            arr.iter().filter_map(|l| {
                let label = l["label"].as_str()?.to_string();
                if label.is_empty() { return None; }
                Some(LinkData {
                    label,
                    href: l["href"].as_str().unwrap_or("").to_string(),
                    link_type: l["type"].as_str().unwrap_or("link").to_string(),
                    selector: l["selector"].as_str().unwrap_or("").to_string(),
                })
            }).collect()
        }).unwrap_or_default()
    }

    fn parse_forms(val: &Value) -> Vec<FormData> {
        val.as_array().map(|arr| {
            arr.iter().filter_map(|f| {
                Some(FormData {
                    id: f["id"].as_str().unwrap_or("").to_string(),
                    action: f["action"].as_str().unwrap_or("").to_string(),
                    method: f["method"].as_str().unwrap_or("GET").to_string(),
                    fields: f["fields"].as_array().map(|farr| {
                        farr.iter().filter_map(|fl| {
                            Some(FormField {
                                name: fl["name"].as_str()?.to_string(),
                                field_type: fl["fieldType"].as_str().unwrap_or("text").to_string(),
                                value: fl["value"].as_str().unwrap_or("").to_string(),
                            })
                        }).collect()
                    }).unwrap_or_default(),
                })
            }).collect()
        }).unwrap_or_default()
    }

    fn build_data_sample(data: &Value, sample_rows: usize) -> Value {
        let (arr, wrapper_keys) = if let Some(arr) = data.as_array() {
            (arr.clone(), vec![])
        } else if let Some(obj) = data.as_object() {
            let data_keys = ["list", "rows", "data", "items", "records", "content"];
            let mut found = None;
            let mut wrapper: Vec<String> = vec![];
            for key in &data_keys {
                if let Some(arr) = obj.get(*key).and_then(|v| v.as_array()) {
                    found = Some(arr.clone());
                    for (k, v) in obj {
                        if k != *key && !v.is_array() {
                            wrapper.push(format!("{}: {}", k, v));
                        }
                    }
                    break;
                }
            }
            match found {
                Some(arr) => (arr, wrapper),
                None => return data.clone(),
            }
        } else {
            return data.clone();
        };

        let total = arr.len();
        let sample: Vec<Value> = arr.into_iter().take(sample_rows).collect();
        let columns: Vec<String> = sample.first()
            .and_then(|r| r.as_object())
            .map(|obj| obj.keys().cloned().collect())
            .unwrap_or_default();

        let mut result = serde_json::json!({
            "_summary": format!("{} total rows, showing first {}", total, sample.len()),
            "_columns": columns,
            "_sample": sample,
        });
        if !wrapper_keys.is_empty() {
            result["_metadata"] = Value::String(wrapper_keys.join(", "));
        }
        result
    }
}
