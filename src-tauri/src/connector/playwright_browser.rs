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
use super::types::{
    ApiFetchResult, BrowseNavigateResult, BrowseResult, ExecuteJsResult, FormData, FormField,
    FullPageResult, LinkData, TableData,
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
        Self {
            process: None,
            active_origin: None,
            active_url: None,
            request_counter: 0,
        }
    }
}

// ── Public API ──────────────────────────────────────────────────

pub struct PlaywrightBrowser {
    app_handle: AppHandle,
    state: Mutex<BrowserState>,
    /// Separate mutex for stdin/stdout I/O — prevents deadlock with state lock.
    io: Mutex<Option<PlaywrightIO>>,
    /// Serializes concurrent launch attempts so only one browser instance is
    /// spawned even when multiple tool calls trigger `ensure_running` at once.
    launch_lock: Mutex<()>,
    site_maps: Mutex<HashMap<String, SiteMap>>,
}

impl PlaywrightBrowser {
    pub fn new(app_handle: AppHandle) -> Self {
        Self {
            app_handle,
            state: Mutex::new(BrowserState::new()),
            io: Mutex::new(None),
            launch_lock: Mutex::new(()),
            site_maps: Mutex::new(HashMap::new()),
        }
    }

    /// Navigate to a URL. Launches browser lazily. Auto-recovers if Chrome was closed.
    pub async fn navigate(&self, url: &str) -> Result<BrowseNavigateResult, String> {
        self.ensure_running().await?;

        let target_origin = Self::extract_origin(url)?;
        info!("[Playwright] navigate: url={}", url);
        let _ = self
            .app_handle
            .emit("browser:navigating", serde_json::json!({ "url": url }));

        // Check site map cache — if this origin has an iframe strategy, navigate to iframe URL directly
        let cached_iframe_src = {
            let url_path = Self::extract_path(url);
            let maps = self.site_maps.lock().await;
            maps.get(&target_origin)
                .and_then(|m| m.get_page(&url_path))
                .and_then(|p| p.iframe_src.clone())
        };

        let actual_url = if let Some(ref iframe_url) = cached_iframe_src {
            info!("[Playwright] Using cached iframe strategy: {}", iframe_url);
            iframe_url.as_str()
        } else {
            url
        };

        // Navigate with auto-recovery: if browser was closed by user, restart and retry once
        let nav_result = match self
            .send_command("navigate", serde_json::json!({ "url": actual_url }))
            .await
        {
            Ok(r) => r,
            Err(e) if e.contains("closed") || e.contains("restart") => {
                warn!("[Playwright] Browser was closed, restarting and retrying navigate");
                self.ensure_running().await?;
                self.send_command("navigate", serde_json::json!({ "url": actual_url }))
                    .await?
            }
            Err(e) => return Err(e),
        };
        let title = nav_result["title"].as_str().unwrap_or("").to_string();
        let final_url = nav_result["url"].as_str().unwrap_or(url).to_string();
        let iframe_url = nav_result["iframeUrl"].as_str().map(String::from);

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
                maps.get(&target_origin)
                    .and_then(|m| m.get_page(&url_path))
                    .cloned()
            };
            if let Some(profile) = cached {
                info!("[Playwright] Using cached profile for {}", url_path);
                Some(profile)
            } else {
                let profile = self
                    .auto_explore(&title, &target_origin, &url_path, iframe_url.clone())
                    .await;
                let aijia_home_dir = self.get_aijia_home_dir();
                {
                    let mut maps = self.site_maps.lock().await;
                    let site_map = maps.entry(target_origin.clone()).or_insert_with(|| {
                        SiteMap::load(&aijia_home_dir, &target_origin)
                            .unwrap_or_else(|| SiteMap::new(&target_origin))
                    });
                    site_map.set_page(profile.clone());
                    let _ = site_map.save(&aijia_home_dir);
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

        let _ = self.app_handle.emit(
            "browser:page-ready",
            serde_json::json!({
                "url": &result.url, "title": &result.title,
            }),
        );

        info!(
            "[Playwright] navigate complete: url={}, title={}, redirected={}",
            result.url, result.title, result.redirected_to_login
        );
        Ok(result)
    }

    /// Read structured data from the active page.
    pub async fn read_content(
        &self,
        _extract_script: Option<&str>,
    ) -> Result<BrowseResult, String> {
        self.ensure_running().await?;

        info!("[Playwright] read_content (extract from all frames)");
        let data = self.send_command("extract", serde_json::json!({})).await?;

        let url = data["url"].as_str().unwrap_or("").to_string();
        let title = data["title"].as_str().unwrap_or("").to_string();
        let text = data["text"].as_str().unwrap_or("").to_string();
        let tables = Self::parse_tables(&data["tables"]);
        let links = Self::parse_links(&data["links"]);

        info!(
            "[Playwright] read_content: url={}, tables={}, links={}, text_len={}",
            url,
            tables.len(),
            links.len(),
            text.len()
        );

        Ok(BrowseResult {
            url,
            title,
            tables,
            text,
            links,
        })
    }

    /// Execute JavaScript on the active page.
    pub async fn execute_js(&self, script: &str) -> Result<ExecuteJsResult, String> {
        self.ensure_running().await?;

        info!("[Playwright] execute_js: script_len={}", script.len());

        let data = self
            .send_command("execute_js", serde_json::json!({ "script": script }))
            .await?;

        let result_type = data["type"].as_str().unwrap_or("result");
        let new_url = data["url"].as_str().map(String::from);
        let new_title = data["title"].as_str().map(String::from);

        if result_type == "error" {
            let error_msg = data["error"]
                .as_str()
                .unwrap_or("Unknown error")
                .to_string();
            info!("[Playwright] execute_js error: {}", error_msg);
            return Ok(ExecuteJsResult {
                value: serde_json::Value::Null,
                error: Some(error_msg),
                new_url,
                new_title,
            });
        }

        info!("[Playwright] execute_js complete");
        Ok(ExecuteJsResult {
            value: data["value"].clone(),
            error: None,
            new_url,
            new_title,
        })
    }

    /// Navigate + extract everything in one shot.
    pub async fn navigate_and_extract(
        &self,
        url: &str,
        extract_script: Option<&str>,
    ) -> Result<FullPageResult, String> {
        // Navigate
        let nav = self.navigate(url).await?;

        let target_origin = Self::extract_origin(url)?;
        let redirected = nav.redirected_to_login;

        // Extract content
        let content = if !redirected {
            self.read_content(extract_script).await.unwrap_or_else(|e| {
                warn!(
                    "[Playwright] navigate_and_extract: read_content failed: {}",
                    e
                );
                BrowseResult {
                    url: nav.url.clone(),
                    title: nav.title.clone(),
                    tables: vec![],
                    text: String::new(),
                    links: vec![],
                }
            })
        } else {
            BrowseResult {
                url: nav.url.clone(),
                title: nav.title.clone(),
                tables: vec![],
                text: String::new(),
                links: vec![],
            }
        };

        // Discover forms
        let forms = if !redirected {
            self.extract_forms().await
        } else {
            vec![]
        };

        let navigate_result = BrowseNavigateResult {
            url: nav.url,
            title: nav.title,
            redirected_to_login: redirected,
            page_profile: None,
            screenshot_path: None,
        };

        // Save page profile
        if !redirected {
            let url_path = Self::extract_path(&navigate_result.url);
            let profile = PageProfile {
                url_path: url_path.clone(),
                title: navigate_result.title.clone(),
                nav_links: content.links.clone(),
                table_schemas: content
                    .tables
                    .iter()
                    .map(|t| TableSchema {
                        name: String::new(),
                        headers: t.headers.clone(),
                        row_count: t.rows.len(),
                    })
                    .collect(),
                forms: forms.clone(),
                api_endpoints: vec![],
                explored_at: chrono::Utc::now(),
                access_denied: false,
                iframe_src: None,
            };
            let aijia_home_dir = self.get_aijia_home_dir();
            let mut maps = self.site_maps.lock().await;
            let site_map = maps.entry(target_origin.clone()).or_insert_with(|| {
                SiteMap::load(&aijia_home_dir, &target_origin)
                    .unwrap_or_else(|| SiteMap::new(&target_origin))
            });
            site_map.set_page(profile);
            let _ = site_map.save(&aijia_home_dir);
        }

        info!(
            "[Playwright] navigate_and_extract complete: url={}, tables={}, links={}, forms={}",
            content.url,
            content.tables.len(),
            content.links.len(),
            forms.len()
        );

        Ok(FullPageResult {
            navigate: navigate_result,
            content,
            api_calls: vec![],
            forms,
        })
    }

    /// Execute fetch() in page context for REST API calls.
    pub async fn api_fetch(
        &self,
        url: &str,
        method: &str,
        body: Option<&str>,
        headers: Option<&str>,
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
            let dir = self.get_aijia_home_dir().join("api-data");
            std::fs::create_dir_all(&dir).ok();
            let filename = format!(
                "api_{}_{}.json",
                chrono::Utc::now().format("%Y%m%d_%H%M%S"),
                url.split('/')
                    .last()
                    .unwrap_or("data")
                    .split('?')
                    .next()
                    .unwrap_or("data")
            );
            let path = dir.join(&filename);
            let _ = std::fs::write(&path, &data_json);
            info!(
                "[Playwright] api_fetch: large response ({} bytes) saved to {:?}",
                data_json.len(),
                path
            );
            let sample = Self::build_data_sample(&resp_data, 5);
            (sample, true, Some(path))
        } else {
            (resp_data, false, None)
        };

        info!(
            "[Playwright] api_fetch complete: {} {} → status={}, rows={:?}",
            method, url, status, total_rows
        );

        Ok(ApiFetchResult {
            status,
            content_type,
            data: final_data,
            total_rows,
            truncated,
            saved_file_path,
        })
    }

    /// Bring browser to front.
    #[allow(dead_code)]
    pub async fn show_active_page(&self) -> Result<(), String> {
        self.send_command("show_page", serde_json::json!({}))
            .await?;
        Ok(())
    }

    /// Capture screenshot.
    pub async fn capture_screenshot(&self) -> Option<PathBuf> {
        let dir = self.get_aijia_home_dir().join("screenshots");
        std::fs::create_dir_all(&dir).ok()?;
        let data = self
            .send_command(
                "screenshot",
                serde_json::json!({ "dir": dir.to_string_lossy() }),
            )
            .await
            .ok()?;
        let path_str = data["path"].as_str()?;
        let path = PathBuf::from(path_str);
        let size = data["size"].as_u64().unwrap_or(0);
        info!("[Playwright] Screenshot saved: {:?} ({} bytes)", path, size);
        Some(path)
    }

    /// Inspect all frames using Playwright locator API (bypasses cross-origin).
    pub async fn frame_inspect(&self) -> Result<Value, String> {
        self.ensure_running().await?;
        info!("[Playwright] frame_inspect");
        self.send_command("frame_inspect", serde_json::json!({})).await
    }

    /// Click an element in a specific frame using Playwright locator (bypasses cross-origin).
    pub async fn frame_click(
        &self,
        frame_index: Option<u32>,
        text: Option<&str>,
        selector: Option<&str>,
        button_index: Option<u32>,
        wait_for_download: bool,
    ) -> Result<Value, String> {
        self.ensure_running().await?;
        info!("[Playwright] frame_click: frame={:?}, text={:?}, selector={:?}, buttonIndex={:?}, download={}",
            frame_index, text, selector, button_index, wait_for_download);
        let params = serde_json::json!({
            "frameIndex": frame_index,
            "text": text,
            "selector": selector,
            "buttonIndex": button_index,
            "waitForDownload": wait_for_download,
        });
        self.send_command("frame_click", params).await
    }

    /// Check and increment per-turn rate limit.
    pub async fn check_rate_limit(&self, max_per_turn: u32) -> Result<(), String> {
        let mut state = self.state.lock().await;
        state.request_counter += 1;
        if state.request_counter > max_per_turn {
            return Err(format!(
                "Browser request limit exceeded: {} (max {})",
                state.request_counter, max_per_turn
            ));
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
        let lock_path = self
            .get_aijia_home_dir()
            .join("playwright-profile/SingletonLock");
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
    #[allow(dead_code)]
    pub async fn get_active_origin(&self) -> Option<String> {
        self.state.lock().await.active_origin.clone()
    }

    /// Get a cached PageProfile.
    pub async fn get_cached_page_profile(
        &self,
        origin: &str,
        url_path: &str,
    ) -> Option<PageProfile> {
        let maps = self.site_maps.lock().await;
        maps.get(origin)?.get_page(url_path).cloned()
    }

    /// Get all cached pages with tables for a given origin.
    pub async fn get_pages_with_tables(&self, origin: &str) -> Vec<PageProfile> {
        let maps = self.site_maps.lock().await;
        match maps.get(origin) {
            Some(site_map) => site_map
                .pages
                .values()
                .filter(|p| !p.table_schemas.is_empty() && !p.access_denied)
                .cloned()
                .collect(),
            None => vec![],
        }
    }

    // ── Internals ───────────────────────────────────────────────

    /// Ensure Node.js sidecar is running, launch if needed.
    /// On first launch failure due to corrupted profile, wipes the profile and retries once.
    ///
    /// Uses a dedicated launch mutex to prevent concurrent callers from
    /// spawning multiple browser instances when the state lock is released
    /// during the (potentially slow) launch_sidecar call.
    async fn ensure_running(&self) -> Result<(), String> {
        // Fast path: already running
        {
            let state = self.state.lock().await;
            if state.process.is_some() {
                return Ok(());
            }
        }

        // Serialize concurrent launches through the launch lock.
        // Only one caller proceeds to launch; others wait and then
        // re-check state.process (which the first caller will have set).
        let _launch_guard = self.launch_lock.lock().await;

        // Re-check after acquiring launch lock — another task may have
        // completed the launch while we were waiting.
        {
            let state = self.state.lock().await;
            if state.process.is_some() {
                return Ok(());
            }
        }

        match self.launch_sidecar(false).await {
            Ok(()) => Ok(()),
            Err(e) => {
                warn!("[Playwright] First launch failed: {}. Wiping profile and retrying...", e);
                // Wipe the corrupted profile and retry
                let user_data_dir = self.get_aijia_home_dir().join("playwright-profile");
                if user_data_dir.exists() {
                    let _ = std::fs::remove_dir_all(&user_data_dir);
                    info!("[Playwright] Wiped corrupted profile at {:?}", user_data_dir);
                }
                self.launch_sidecar(true).await
            }
        }
    }

    /// Internal: launch the Node.js sidecar + Chromium browser.
    /// `is_retry` = true means we already wiped the profile on a previous failure.
    async fn launch_sidecar(&self, is_retry: bool) -> Result<(), String> {
        let node_path = self.find_node()?;
        let script_path = self.find_browser_js()?;
        let browsers_path = self.find_browsers_dir()?;
        let user_data_dir = self.get_aijia_home_dir().join("playwright-profile");
        std::fs::create_dir_all(&user_data_dir).ok();

        // Kill orphaned Chromium processes
        self.kill_orphaned_chromium(&browsers_path, &user_data_dir).await;

        // Clean up crash artifacts that can prevent Chromium from starting
        Self::clean_profile_crash_artifacts(&user_data_dir);

        if is_retry {
            info!("[Playwright] Retry launch with fresh profile");
        }

        info!(
            "[Playwright] Launching sidecar: node={:?}, script={:?}",
            node_path, script_path
        );

        let mut node_cmd = tokio::process::Command::new(&node_path);
        node_cmd
            .arg(&script_path)
            .env("PLAYWRIGHT_BROWSERS_PATH", &browsers_path)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit()) // stderr goes to app log
            .kill_on_drop(true);
        #[cfg(target_os = "windows")]
        {
            use std::os::windows::process::CommandExt;
            node_cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW
        }
        let mut child = node_cmd
            .spawn()
            .map_err(|e| format!("Failed to launch Playwright sidecar: {}", e))?;

        let stdin = child.stdin.take().ok_or("Failed to get stdin")?;
        let stdout = child.stdout.take().ok_or("Failed to get stdout")?;

        let mut io = PlaywrightIO {
            stdin: BufWriter::new(stdin),
            stdout: BufReader::new(stdout),
        };
        let mut proc = PlaywrightProcess { child, next_id: 1 };

        // Send launch command (before storing in state, so failure kills process via drop)
        let launch_params = serde_json::json!({
            "userDataDir": user_data_dir.to_string_lossy(),
        });
        let id = proc.next_id;
        proc.next_id += 1;
        let cmd = serde_json::json!({ "id": id, "method": "launch", "params": launch_params });
        let line = serde_json::to_string(&cmd).unwrap() + "\n";
        io.stdin
            .write_all(line.as_bytes())
            .await
            .map_err(|e| format!("Failed to send launch: {}", e))?;
        io.stdin
            .flush()
            .await
            .map_err(|e| format!("stdin flush: {}", e))?;

        // Read launch response with timeout
        let mut response_line = String::new();
        let read_result = tokio::time::timeout(
            std::time::Duration::from_secs(30),
            io.stdout.read_line(&mut response_line),
        )
        .await
        .map_err(|_| "Playwright launch timeout (30s)".to_string())?
        .map_err(|e| format!("Failed to read launch response: {}", e))?;

        if read_result == 0 {
            return Err("Playwright sidecar process terminated during launch".to_string());
        }

        let response: Value = serde_json::from_str(response_line.trim()).map_err(|e| {
            format!(
                "Invalid launch response: {} (raw: {})",
                e,
                response_line.trim()
            )
        })?;

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

    /// Kill any orphaned Chromium processes from previous crashes.
    async fn kill_orphaned_chromium(&self, browsers_path: &std::path::Path, user_data_dir: &std::path::Path) {
        let singleton_lock = user_data_dir.join("SingletonLock");
        if !singleton_lock.exists() {
            return;
        }
        warn!("[Playwright] Removing stale SingletonLock, killing orphaned Chromium");
        let browsers_dir_str = browsers_path.to_string_lossy().to_string();
        let _ = std::process::Command::new("pkill").args(["-f", &browsers_dir_str]).output();
        let dir_str = user_data_dir.to_string_lossy().to_string();
        let _ = std::process::Command::new("pkill").args(["-f", &dir_str]).output();
        let _ = std::process::Command::new("pkill").args(["-f", "Google Chrome for Testing"]).output();
        tokio::time::sleep(std::time::Duration::from_millis(1000)).await;
        let _ = std::fs::remove_file(&singleton_lock);
        let _ = std::fs::remove_file(user_data_dir.join("SingletonSocket"));
        let _ = std::fs::remove_file(user_data_dir.join("SingletonCookie"));
    }

    /// Remove crash artifacts from the Chromium profile that can cause startup failures.
    /// These files are expendable — Chromium regenerates them on next clean startup.
    fn clean_profile_crash_artifacts(user_data_dir: &std::path::Path) {
        // GPU shader caches — most common cause of Chromium startup crashes
        let cache_dirs = [
            "Default/GPUCache",
            "Default/DawnGraphiteCache",
            "Default/DawnWebGPUCache",
            "GraphiteDawnCache",
            "GrShaderCache",
        ];
        for dir in &cache_dirs {
            let path = user_data_dir.join(dir);
            if path.exists() {
                if let Err(e) = std::fs::remove_dir_all(&path) {
                    warn!("[Playwright] Failed to clean {}: {}", dir, e);
                } else {
                    info!("[Playwright] Cleaned crash-prone cache: {}", dir);
                }
            }
        }

        // Session/tab restore data — can cause crashes if corrupted
        let session_files = [
            "Default/Sessions",
            "Default/Session Storage",
            "Default/LOCK",
            "RunningChromeVersion",
        ];
        for f in &session_files {
            let path = user_data_dir.join(f);
            if path.is_dir() {
                let _ = std::fs::remove_dir_all(&path);
            } else if path.exists() || path.is_symlink() {
                let _ = std::fs::remove_file(&path);
            }
        }

        // Fix crash marker in Preferences
        let prefs_path = user_data_dir.join("Default/Preferences");
        if prefs_path.exists() {
            if let Ok(content) = std::fs::read_to_string(&prefs_path) {
                let fixed = content.replace(r#""exit_type":"Crashed""#, r#""exit_type":"Normal""#);
                if fixed != content {
                    let _ = std::fs::write(&prefs_path, &fixed);
                    info!("[Playwright] Fixed crash marker in Preferences");
                }
            }
        }

        // Remove macOS temp files (.com.google.chrome.for.testing.*)
        if let Ok(entries) = std::fs::read_dir(user_data_dir) {
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().to_string();
                if name.starts_with(".com.google.chrome") {
                    let _ = std::fs::remove_file(entry.path());
                }
            }
        }
    }

    /// Send a JSON-RPC command to the sidecar and return the result.
    /// Uses separate IO mutex to avoid deadlock with state lock.
    /// On fatal errors (process terminated, stdin/stdout broken), auto-cleans
    /// state so the next call to `ensure_running()` can restart the sidecar.
    async fn send_command(&self, method: &str, params: Value) -> Result<Value, String> {
        // Get next ID from state (brief lock)
        let id = {
            let mut state = self.state.lock().await;
            let proc = state
                .process
                .as_mut()
                .ok_or("Playwright sidecar not running")?;
            let id = proc.next_id;
            proc.next_id += 1;
            id
        }; // State lock released

        // IO operations use separate lock (no deadlock risk)
        let mut io_guard = self.io.lock().await;
        let io = io_guard.as_mut().ok_or("Playwright IO not initialized")?;

        let cmd = serde_json::json!({ "id": id, "method": method, "params": params });
        let line = serde_json::to_string(&cmd).unwrap() + "\n";

        // Write — if this fails, the sidecar is dead
        if let Err(e) = io.stdin.write_all(line.as_bytes()).await {
            drop(io_guard);
            self.cleanup_dead_process().await;
            return Err(format!(
                "stdin write: {} (process cleaned up, will restart on next call)",
                e
            ));
        }
        if let Err(e) = io.stdin.flush().await {
            drop(io_guard);
            self.cleanup_dead_process().await;
            return Err(format!(
                "stdin flush: {} (process cleaned up, will restart on next call)",
                e
            ));
        }

        // Read response with timeout
        let read_future = async {
            loop {
                let mut response_line = String::new();
                let bytes_read = io
                    .stdout
                    .read_line(&mut response_line)
                    .await
                    .map_err(|e| format!("stdout read: {}", e))?;

                if bytes_read == 0 {
                    return Err("__PROCESS_TERMINATED__".to_string());
                }

                let trimmed = response_line.trim();
                if trimmed.is_empty() {
                    continue;
                }

                let response: Value = serde_json::from_str(trimmed)
                    .map_err(|e| format!("Invalid response JSON: {} (raw: {})", e, trimmed))?;

                if response["id"].as_u64() == Some(id) {
                    if let Some(err) = response.get("error").and_then(|e| e.as_str()) {
                        // If Chrome was closed by user, sidecar returns "Browser not launched"
                        if err.contains("not launched") {
                            return Err("__BROWSER_CLOSED__".to_string());
                        }
                        return Err(err.to_string());
                    }
                    return Ok(response["result"].clone());
                }
            }
        };

        let result = tokio::time::timeout(PLAYWRIGHT_CMD_TIMEOUT, read_future)
            .await
            .map_err(|_| {
                format!(
                    "Playwright command '{}' timed out after {:?}",
                    method, PLAYWRIGHT_CMD_TIMEOUT
                )
            })?;

        // If process terminated or browser closed, clean up so next call restarts
        if let Err(ref e) = result {
            if e.contains("__PROCESS_TERMINATED__") || e.contains("__BROWSER_CLOSED__") {
                drop(io_guard);
                self.cleanup_dead_process().await;
                let msg = if e.contains("__BROWSER_CLOSED__") {
                    "Browser was closed (cleaned up, will restart on next call)"
                } else {
                    "Playwright process terminated (cleaned up, will restart on next call)"
                };
                return Err(msg.to_string());
            }
        }
        result
    }

    /// Clean up dead process state so `ensure_running()` will relaunch.
    /// Kills the sidecar child process AND any orphaned Chromium processes
    /// it may have spawned (which outlive the sidecar if it crashes).
    async fn cleanup_dead_process(&self) {
        warn!("[Playwright] Cleaning up dead process state");
        *self.io.lock().await = None;
        let mut state = self.state.lock().await;
        if let Some(mut proc) = state.process.take() {
            let _ = proc.child.kill().await;
        }
        state.active_origin = None;
        state.active_url = None;
        drop(state);

        // Kill orphaned Chromium processes that outlive the sidecar
        let _ = std::process::Command::new("pkill")
            .args(["-f", "Google Chrome for Testing"])
            .output();

        // Clean up profile artifacts that could prevent next launch
        let profile_dir = self.get_aijia_home_dir().join("playwright-profile");
        let _ = std::fs::remove_file(profile_dir.join("SingletonLock"));
        let _ = std::fs::remove_file(profile_dir.join("SingletonSocket"));
        let _ = std::fs::remove_file(profile_dir.join("SingletonCookie"));
        Self::clean_profile_crash_artifacts(&profile_dir);
    }

    async fn auto_explore(
        &self,
        title: &str,
        _origin: &str,
        url_path: &str,
        iframe_src: Option<String>,
    ) -> PageProfile {
        info!(
            "[Playwright] auto_explore: {} (iframe: {:?})",
            url_path, iframe_src
        );

        let data = match self.send_command("extract", serde_json::json!({})).await {
            Ok(d) => d,
            Err(e) => {
                warn!("[Playwright] auto_explore failed: {}", e);
                return PageProfile {
                    url_path: url_path.to_string(),
                    title: title.to_string(),
                    nav_links: vec![],
                    table_schemas: vec![],
                    forms: vec![],
                    api_endpoints: vec![],
                    explored_at: chrono::Utc::now(),
                    access_denied: false,
                    iframe_src,
                };
            }
        };

        let nav_links = Self::parse_links(&data["links"]);
        let tables_raw = Self::parse_tables(&data["tables"]);
        let table_schemas: Vec<TableSchema> = tables_raw
            .iter()
            .map(|t| TableSchema {
                name: String::new(),
                headers: t.headers.clone(),
                row_count: t.rows.len(),
            })
            .collect();

        let forms = self.extract_forms().await;

        info!(
            "[Playwright] auto_explore complete: path={}, links={}, tables={}, forms={}",
            url_path,
            nav_links.len(),
            table_schemas.len(),
            forms.len()
        );

        PageProfile {
            url_path: url_path.to_string(),
            title: title.to_string(),
            nav_links,
            table_schemas,
            forms,
            api_endpoints: vec![],
            explored_at: chrono::Utc::now(),
            access_denied: false,
            iframe_src,
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

    fn get_aijia_home_dir(&self) -> PathBuf {
        self.app_handle
            .try_state::<std::sync::Arc<crate::storage::AiJiaHome>>()
            .map(|home| home.root().to_path_buf())
            .unwrap_or_else(|| {
                dirs::home_dir()
                    .map(|home| home.join(".renlijia"))
                    .unwrap_or_else(std::env::temp_dir)
            })
    }

    fn find_node(&self) -> Result<PathBuf, String> {
        // Platform-specific node binary path
        let node_subpath = if cfg!(target_os = "windows") {
            "playwright-runtime/node/node.exe"
        } else {
            "playwright-runtime/node/bin/node"
        };

        // Check bundled runtime first (production)
        if let Ok(resource_dir) = self.app_handle.path().resource_dir() {
            let bundled = resource_dir.join(node_subpath);
            if bundled.exists() {
                return Ok(bundled);
            }
        }
        // Dev mode: relative to src-tauri, canonicalize to absolute
        let dev = PathBuf::from(node_subpath);
        if dev.exists() {
            return Ok(std::fs::canonicalize(&dev).unwrap_or(dev));
        }
        // System node (Unix only)
        if !cfg!(target_os = "windows") {
            let system = PathBuf::from("/usr/local/bin/node");
            if system.exists() {
                return Ok(system);
            }
        }
        Err("Node.js not found. Run scripts/setup-playwright.sh (or .ps1 on Windows)".to_string())
    }

    fn find_browser_js(&self) -> Result<PathBuf, String> {
        if let Ok(resource_dir) = self.app_handle.path().resource_dir() {
            let bundled = resource_dir.join("playwright-runtime/browser.js");
            if bundled.exists() {
                return Ok(bundled);
            }
        }
        let dev = PathBuf::from("playwright-runtime/browser.js");
        if dev.exists() {
            return Ok(std::fs::canonicalize(&dev).unwrap_or(dev));
        }
        Err("browser.js not found".to_string())
    }

    fn find_browsers_dir(&self) -> Result<PathBuf, String> {
        if let Ok(resource_dir) = self.app_handle.path().resource_dir() {
            let bundled = resource_dir.join("playwright-runtime/browsers");
            if bundled.exists() {
                return Ok(bundled);
            }
        }
        let dev = PathBuf::from("playwright-runtime/browsers");
        if dev.exists() {
            return Ok(std::fs::canonicalize(&dev).unwrap_or(dev));
        }
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
        url::Url::parse(url)
            .ok()
            .map(|u| u.path().to_string())
            .unwrap_or_default()
    }

    fn detect_redirect(original_url: &str, final_url: &str, target_origin: &str) -> bool {
        let final_origin = Self::extract_origin(final_url).unwrap_or_default();
        if final_origin != *target_origin {
            return true;
        }
        let target_path = url::Url::parse(original_url)
            .ok()
            .map(|u| u.path().to_string())
            .unwrap_or_default();
        let final_path = url::Url::parse(final_url)
            .ok()
            .map(|u| u.path().to_string())
            .unwrap_or_default();
        if final_path != target_path {
            let fp = final_path.to_lowercase();
            return fp.contains("login")
                || fp.contains("signin")
                || fp.contains("/sso")
                || fp.contains("/auth")
                || fp.contains("/cas/")
                || fp.contains("error")
                || fp.contains("forbidden")
                || fp.contains("no_resource")
                || fp.contains("no_permission")
                || fp.contains("/403")
                || fp.contains("/404");
        }
        false
    }

    fn parse_tables(val: &Value) -> Vec<TableData> {
        val.as_array()
            .map(|arr| {
                arr.iter()
                    .filter_map(|t| {
                        let headers: Vec<String> = t["headers"]
                            .as_array()?
                            .iter()
                            .filter_map(|h| h.as_str().map(String::from))
                            .collect();
                        let rows: Vec<HashMap<String, String>> = t["rows"]
                            .as_array()?
                            .iter()
                            .filter_map(|r| {
                                r.as_object().map(|obj| {
                                    obj.iter()
                                        .map(|(k, v)| {
                                            (k.clone(), v.as_str().unwrap_or("").to_string())
                                        })
                                        .collect()
                                })
                            })
                            .collect();
                        Some(TableData { headers, rows })
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    fn parse_links(val: &Value) -> Vec<LinkData> {
        val.as_array()
            .map(|arr| {
                arr.iter()
                    .filter_map(|l| {
                        let label = l["label"].as_str()?.to_string();
                        if label.is_empty() {
                            return None;
                        }
                        Some(LinkData {
                            label,
                            href: l["href"].as_str().unwrap_or("").to_string(),
                            link_type: l["type"].as_str().unwrap_or("link").to_string(),
                            selector: l["selector"].as_str().unwrap_or("").to_string(),
                        })
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    fn parse_forms(val: &Value) -> Vec<FormData> {
        val.as_array()
            .map(|arr| {
                arr.iter()
                    .filter_map(|f| {
                        Some(FormData {
                            id: f["id"].as_str().unwrap_or("").to_string(),
                            action: f["action"].as_str().unwrap_or("").to_string(),
                            method: f["method"].as_str().unwrap_or("GET").to_string(),
                            fields: f["fields"]
                                .as_array()
                                .map(|farr| {
                                    farr.iter()
                                        .filter_map(|fl| {
                                            Some(FormField {
                                                name: fl["name"].as_str()?.to_string(),
                                                field_type: fl["fieldType"]
                                                    .as_str()
                                                    .unwrap_or("text")
                                                    .to_string(),
                                                value: fl["value"]
                                                    .as_str()
                                                    .unwrap_or("")
                                                    .to_string(),
                                            })
                                        })
                                        .collect()
                                })
                                .unwrap_or_default(),
                        })
                    })
                    .collect()
            })
            .unwrap_or_default()
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
        let columns: Vec<String> = sample
            .first()
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

// ── Tests ───────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // ── extract_origin ──────────────────────────────────────────

    #[test]
    fn extract_origin_https() {
        assert_eq!(
            PlaywrightBrowser::extract_origin("https://zeus.renlijia.com/orders?page=1").unwrap(),
            "https://zeus.renlijia.com"
        );
    }

    #[test]
    fn extract_origin_with_port() {
        assert_eq!(
            PlaywrightBrowser::extract_origin("http://localhost:8080/dashboard").unwrap(),
            "http://localhost:8080"
        );
    }

    #[test]
    fn extract_origin_invalid_url() {
        assert!(PlaywrightBrowser::extract_origin("not-a-url").is_err());
    }

    // ── extract_path ────────────────────────────────────────────

    #[test]
    fn extract_path_normal() {
        assert_eq!(
            PlaywrightBrowser::extract_path("https://zeus.renlijia.com/api/orders?page=1"),
            "/api/orders"
        );
    }

    #[test]
    fn extract_path_root() {
        assert_eq!(
            PlaywrightBrowser::extract_path("https://example.com"),
            "/"
        );
    }

    #[test]
    fn extract_path_invalid() {
        assert_eq!(PlaywrightBrowser::extract_path("garbage"), "");
    }

    // ── detect_redirect ─────────────────────────────────────────

    #[test]
    fn detect_redirect_same_url_no_redirect() {
        assert!(!PlaywrightBrowser::detect_redirect(
            "https://zeus.com/orders",
            "https://zeus.com/orders",
            "https://zeus.com",
        ));
    }

    #[test]
    fn detect_redirect_cross_origin() {
        assert!(PlaywrightBrowser::detect_redirect(
            "https://zeus.com/orders",
            "https://sso.company.com/login",
            "https://zeus.com",
        ));
    }

    #[test]
    fn detect_redirect_to_login_path() {
        assert!(PlaywrightBrowser::detect_redirect(
            "https://zeus.com/orders",
            "https://zeus.com/login?redirect=/orders",
            "https://zeus.com",
        ));
    }

    #[test]
    fn detect_redirect_to_cas() {
        assert!(PlaywrightBrowser::detect_redirect(
            "https://zeus.com/orders",
            "https://zeus.com/cas/login",
            "https://zeus.com",
        ));
    }

    #[test]
    fn detect_redirect_to_forbidden() {
        assert!(PlaywrightBrowser::detect_redirect(
            "https://zeus.com/admin",
            "https://zeus.com/403",
            "https://zeus.com",
        ));
    }

    #[test]
    fn detect_redirect_normal_navigation_not_flagged() {
        assert!(!PlaywrightBrowser::detect_redirect(
            "https://zeus.com/",
            "https://zeus.com/dashboard",
            "https://zeus.com",
        ));
    }

    // ── parse_tables ────────────────────────────────────────────

    #[test]
    fn parse_tables_normal() {
        let data = json!([{
            "headers": ["Name", "Age"],
            "rows": [
                {"Name": "Alice", "Age": "30"},
                {"Name": "Bob", "Age": "25"}
            ]
        }]);
        let tables = PlaywrightBrowser::parse_tables(&data);
        assert_eq!(tables.len(), 1);
        assert_eq!(tables[0].headers, vec!["Name", "Age"]);
        assert_eq!(tables[0].rows.len(), 2);
        assert_eq!(tables[0].rows[0].get("Name").unwrap(), "Alice");
    }

    #[test]
    fn parse_tables_empty() {
        assert!(PlaywrightBrowser::parse_tables(&json!([])).is_empty());
        assert!(PlaywrightBrowser::parse_tables(&json!(null)).is_empty());
    }

    #[test]
    fn parse_tables_multiple() {
        let data = json!([
            {"headers": ["A"], "rows": [{"A": "1"}]},
            {"headers": ["B", "C"], "rows": []}
        ]);
        let tables = PlaywrightBrowser::parse_tables(&data);
        assert_eq!(tables.len(), 2);
        assert_eq!(tables[1].headers, vec!["B", "C"]);
        assert!(tables[1].rows.is_empty());
    }

    // ── parse_links ─────────────────────────────────────────────

    #[test]
    fn parse_links_normal() {
        let data = json!([
            {"label": "Orders", "href": "/orders", "type": "menu", "selector": ""},
            {"label": "Export", "href": "", "type": "button", "selector": "#btn-export"}
        ]);
        let links = PlaywrightBrowser::parse_links(&data);
        assert_eq!(links.len(), 2);
        assert_eq!(links[0].label, "Orders");
        assert_eq!(links[0].link_type, "menu");
        assert_eq!(links[1].link_type, "button");
    }

    #[test]
    fn parse_links_skips_empty_label() {
        let data = json!([
            {"label": "", "href": "/hidden", "type": "link", "selector": ""}
        ]);
        assert!(PlaywrightBrowser::parse_links(&data).is_empty());
    }

    // ── parse_forms ─────────────────────────────────────────────

    #[test]
    fn parse_forms_normal() {
        let data = json!([{
            "id": "search-form",
            "action": "/api/search",
            "method": "POST",
            "fields": [
                {"name": "keyword", "fieldType": "text", "value": ""},
                {"name": "date", "fieldType": "date", "value": "2026-01-01"}
            ]
        }]);
        let forms = PlaywrightBrowser::parse_forms(&data);
        assert_eq!(forms.len(), 1);
        assert_eq!(forms[0].id, "search-form");
        assert_eq!(forms[0].method, "POST");
        assert_eq!(forms[0].fields.len(), 2);
        assert_eq!(forms[0].fields[1].value, "2026-01-01");
    }

    #[test]
    fn parse_forms_empty() {
        assert!(PlaywrightBrowser::parse_forms(&json!([])).is_empty());
        assert!(PlaywrightBrowser::parse_forms(&json!(null)).is_empty());
    }

    // ── build_data_sample ───────────────────────────────────────

    #[test]
    fn build_data_sample_array() {
        let data = json!([
            {"id": 1, "name": "A"},
            {"id": 2, "name": "B"},
            {"id": 3, "name": "C"},
            {"id": 4, "name": "D"},
            {"id": 5, "name": "E"},
            {"id": 6, "name": "F"},
        ]);
        let sample = PlaywrightBrowser::build_data_sample(&data, 3);
        assert_eq!(sample["_summary"].as_str().unwrap(), "6 total rows, showing first 3");
        assert_eq!(sample["_sample"].as_array().unwrap().len(), 3);
        let columns = sample["_columns"].as_array().unwrap();
        assert!(columns.len() == 2); // id, name
    }

    #[test]
    fn build_data_sample_wrapped_object() {
        let data = json!({
            "total": 100,
            "pageSize": 20,
            "list": [
                {"id": 1}, {"id": 2}, {"id": 3}
            ]
        });
        let sample = PlaywrightBrowser::build_data_sample(&data, 2);
        assert_eq!(sample["_sample"].as_array().unwrap().len(), 2);
        assert!(sample["_summary"].as_str().unwrap().contains("3 total rows"));
        assert!(sample["_metadata"].as_str().unwrap().contains("total"));
    }

    #[test]
    fn build_data_sample_small_data_passthrough() {
        let data = json!({"key": "value"});
        let sample = PlaywrightBrowser::build_data_sample(&data, 5);
        assert_eq!(sample, data); // no array found, return as-is
    }

    #[test]
    fn build_data_sample_recognizes_data_keys() {
        // Test each recognized wrapper key: list, rows, data, items, records, content
        for key in &["list", "rows", "data", "items", "records", "content"] {
            let mut obj = serde_json::Map::new();
            obj.insert(key.to_string(), json!([{"x": 1}, {"x": 2}]));
            let data = Value::Object(obj);
            let sample = PlaywrightBrowser::build_data_sample(&data, 5);
            assert!(
                sample["_sample"].is_array(),
                "failed to unwrap key '{}'", key
            );
        }
    }
}
