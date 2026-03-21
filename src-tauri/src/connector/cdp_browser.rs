//! CDP-based browser automation via chromiumoxide.
//!
//! V4: Open browsing mode — no app_id, no cookie injection, no pre-configuration.
//! User navigates to any URL → Chrome launches lazily → user logs in directly
//! in Chrome → AI reads data. Pages are indexed by origin for tab reuse.
//!
//! Architecture: single `Mutex<BrowserState>` holds all mutable state. Lock is
//! acquired briefly to get a `Page` clone, then released before any async I/O.
//! All recovery logic is centralized in `ensure_ready()`.

use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Duration;

use chromiumoxide::browser::{Browser, BrowserConfig};
use chromiumoxide::Page;
use futures::StreamExt;
use log::{info, warn};
use tauri::{AppHandle, Emitter, Manager};
use tokio::sync::Mutex;

use super::webview_auth::{BrowseNavigateResult, BrowseResult, ExecuteJsResult, TableData};

/// Default JS extraction function for read_content().
pub(crate) const DEFAULT_EXTRACT_SCRIPT: &str = r#"
function __aijia_extract() {
    var tables = [];
    var tableEls = document.querySelectorAll('table');
    for (var i = 0; i < tableEls.length && i < 10; i++) {
        var t = tableEls[i];
        var headers = [];
        var thEls = t.querySelectorAll('thead th, thead td, tr:first-child th');
        if (thEls.length === 0) {
            var firstRow = t.querySelector('tr');
            if (firstRow) thEls = firstRow.querySelectorAll('td, th');
        }
        for (var h = 0; h < thEls.length; h++) {
            headers.push(thEls[h].innerText.trim());
        }
        var rows = [];
        var trEls = t.querySelectorAll('tbody tr, tr');
        var startIdx = (thEls.length > 0 && !t.querySelector('thead')) ? 1 : 0;
        for (var r = startIdx; r < trEls.length && rows.length < 100; r++) {
            var cells = trEls[r].querySelectorAll('td');
            if (cells.length === 0) continue;
            var row = {};
            for (var c = 0; c < cells.length; c++) {
                var key = (c < headers.length) ? headers[c] : ('col_' + c);
                row[key] = cells[c].innerText.trim();
            }
            rows.push(row);
        }
        if (headers.length > 0 || rows.length > 0) {
            tables.push({headers: headers, rows: rows});
        }
    }

    var textEl = document.querySelector('main') || document.querySelector('[role="main"]')
        || document.querySelector('.main-content') || document.querySelector('#app') || document.body;
    var text = (textEl ? textEl.innerText : '').substring(0, 4000);

    return {
        url: window.location.href,
        title: document.title,
        tables: tables,
        text: text
    };
}
"#;

// ── Internal types ──────────────────────────────────────────────

struct BrowserHandle {
    browser: Browser,
    handler_task: tokio::task::JoinHandle<()>,
}

/// All mutable state in one struct, protected by a single Mutex.
/// Invariant: if `browser` is None, `pages` is empty and `active_origin` is None.
struct BrowserState {
    browser: Option<BrowserHandle>,
    pages: HashMap<String, Page>,     // origin → Page
    active_origin: Option<String>,    // last navigated origin
    request_counter: u32,             // per-turn rate limit
}

impl BrowserState {
    fn new() -> Self {
        Self {
            browser: None,
            pages: HashMap::new(),
            active_origin: None,
            request_counter: 0,
        }
    }

    /// Tear down everything. Returns the old handle for async cleanup.
    fn reset(&mut self) -> Option<BrowserHandle> {
        self.pages.clear();
        self.active_origin = None;
        self.browser.take()
    }
}

// ── Public API ──────────────────────────────────────────────────

/// CDP-based browser for open browsing mode.
///
/// Single Chromium process (launched lazily), one tab per origin.
/// User logs in directly in Chrome — no cookie injection needed.
pub struct CdpBrowser {
    app_handle: AppHandle,
    state: Mutex<BrowserState>,
    chrome_path: Option<PathBuf>,
}

impl CdpBrowser {
    pub fn new(app_handle: AppHandle, chrome_path: Option<PathBuf>) -> Self {
        Self {
            app_handle,
            state: Mutex::new(BrowserState::new()),
            chrome_path,
        }
    }

    /// Navigate to a URL. Chrome launches lazily. Returns page info.
    ///
    /// Pages are reused per origin. Login redirects are detected.
    pub async fn navigate(&self, url: &str) -> Result<BrowseNavigateResult, String> {
        let target_origin = Self::extract_origin(url)?;

        // Get a working page for this origin (handles launch + recovery)
        let page = self.ensure_page(&target_origin).await?;

        info!("[CDP] navigate: url={}", url);
        let _ = self.app_handle.emit("browser:navigating", serde_json::json!({ "url": url }));

        // Navigate — if it fails, try full recovery once
        if let Err(e) = page.goto(url).await {
            let err_str = format!("{}", e);
            if Self::is_connection_error(&err_str) {
                warn!("[CDP] goto failed ({}), recovering...", err_str);
                self.force_reset().await;
                let page = self.ensure_page(&target_origin).await?;
                page.goto(url)
                    .await
                    .map_err(|e| format!("Navigate failed after recovery: {}", e))?;
            } else {
                return Err(format!("Failed to navigate to '{}': {}", url, err_str));
            }
        }

        // Wait for content to stabilize, then read title/URL
        // (page is a clone, no lock held during these awaits)
        self.wait_for_content_stable(&page, 10_000).await;

        let title = Self::eval_string(&page, "document.title").await;
        let final_url = Self::eval_string(&page, "window.location.href").await;
        let final_url = if final_url.is_empty() { url.to_string() } else { final_url };

        // Update active origin
        self.state.lock().await.active_origin = Some(target_origin.clone());

        // Detect login redirect
        let redirected_to_login = Self::detect_login_redirect(url, &final_url, &target_origin);

        let result = BrowseNavigateResult {
            url: final_url,
            title,
            redirected_to_login,
        };

        let _ = self.app_handle.emit("browser:page-ready", serde_json::json!({
            "url": &result.url, "title": &result.title,
        }));

        info!("[CDP] navigate complete: url={}, title={}, redirected={}",
            result.url, result.title, result.redirected_to_login);
        Ok(result)
    }

    /// Read structured data from the active page.
    pub async fn read_content(&self, extract_script: Option<&str>) -> Result<BrowseResult, String> {
        let page = self.get_active_page().await?;

        let extract_js = extract_script.unwrap_or(DEFAULT_EXTRACT_SCRIPT);
        let eval_js = format!(
            r#"(() => {{
    {extract_script}
    var data = __aijia_extract();
    var MAX_LEN = 60000;
    var result = JSON.stringify(data);
    if (result.length > MAX_LEN) {{
        data.text = (data.text || '').substring(0, 2000);
        if (data.tables) {{
            for (var i = 0; i < data.tables.length; i++) {{
                data.tables[i].rows = data.tables[i].rows.slice(0, 50);
            }}
        }}
        data.truncated = true;
    }}
    return data;
}})()"#,
            extract_script = extract_js,
        );

        let eval_result = page.evaluate(eval_js.as_str())
            .await
            .map_err(|e| self.handle_page_error(e, "extraction script"))?;

        let result: serde_json::Value = eval_result.into_value()
            .map_err(|e| format!("Failed to parse extraction result: {}", e))?;

        if let Some(err) = result.get("error").and_then(|e| e.as_str()) {
            return Err(format!("Extraction script error: {}", err));
        }

        let page_title = result["title"].as_str().unwrap_or("").to_string();
        let page_url = result["url"].as_str().unwrap_or("").to_string();
        let text = result["text"].as_str().unwrap_or("").to_string();
        let tables = Self::parse_tables(&result);

        info!("[CDP] read_content: url={}, tables={}, text_len={}",
            page_url, tables.len(), text.len());

        Ok(BrowseResult { url: page_url, title: page_title, tables, text })
    }

    /// Execute JavaScript on the active page.
    pub async fn execute_js(&self, script: &str) -> Result<ExecuteJsResult, String> {
        let page = self.get_active_page().await?;

        info!("[CDP] execute_js: script_len={}", script.len());

        let url_before = Self::eval_string(&page, "window.location.href").await;

        let eval_js = format!(
            r#"(async () => {{
    try {{
        var result = await (async function() {{ {script} }})();
        return {{
            type: 'result',
            value: (result === undefined ? null : result),
            url: window.location.href,
            title: document.title
        }};
    }} catch(e) {{
        return {{
            type: 'error',
            error: e.message,
            url: window.location.href,
            title: document.title
        }};
    }}
}})()"#,
            script = script,
        );

        let result: serde_json::Value = page.evaluate(eval_js.as_str())
            .await
            .map_err(|e| self.handle_page_error(e, "script"))?
            .into_value()
            .map_err(|e| format!("Failed to parse script result: {}", e))?;

        let result_type = result["type"].as_str().unwrap_or("");
        let new_url = result["url"].as_str().map(String::from);
        let new_title = result["title"].as_str().map(String::from);

        if result_type == "error" {
            let error_msg = result["error"].as_str().unwrap_or("Unknown error").to_string();
            info!("[CDP] execute_js error: {}", error_msg);
            return Ok(ExecuteJsResult { value: serde_json::Value::Null, error: Some(error_msg), new_url, new_title });
        }

        // Check if JS triggered navigation
        tokio::time::sleep(Duration::from_millis(300)).await;
        let url_after = Self::eval_string(&page, "window.location.href").await;

        if url_after != url_before && !url_before.is_empty() {
            self.wait_for_content_stable(&page, 5_000).await;
            let new_title_after = Self::eval_string(&page, "document.title").await;
            let _ = self.app_handle.emit("browser:page-ready", serde_json::json!({
                "url": &url_after, "title": &new_title_after,
            }));
            info!("[CDP] execute_js triggered navigation: {} -> {}", url_before, url_after);
            return Ok(ExecuteJsResult {
                value: result["value"].clone(), error: None,
                new_url: Some(url_after), new_title: Some(new_title_after),
            });
        }

        info!("[CDP] execute_js complete");
        Ok(ExecuteJsResult { value: result["value"].clone(), error: None, new_url, new_title })
    }

    /// Bring the active tab to front.
    pub async fn show_active_page(&self) -> Result<(), String> {
        let page = self.get_active_page().await?;
        page.bring_to_front().await.map_err(|e| format!("Failed to bring page to front: {}", e))?;
        Ok(())
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

    /// Shutdown Chrome. Call on app exit.
    pub async fn shutdown(&self) {
        let old_handle = self.state.lock().await.reset();
        if let Some(handle) = old_handle {
            Self::shutdown_browser(handle).await;
        }
    }

    // ── Core internals ──────────────────────────────────────────

    /// Get a working Page for the given origin. Launches Chrome if needed,
    /// creates tab if needed, recovers if crashed. Returns a cloned Page
    /// handle — caller does NOT hold any lock.
    async fn ensure_page(&self, origin: &str) -> Result<Page, String> {
        // Fast path: page exists and is alive
        {
            let state = self.state.lock().await;
            if let Some(page) = state.pages.get(origin) {
                if page.evaluate("1").await.is_ok() {
                    return Ok(page.clone());
                }
                info!("[CDP] Page for '{}' is stale", origin);
            }
        }

        // Slow path: need to create page (possibly launching Chrome first)
        self.create_page_with_recovery(origin).await
    }

    /// Launch Chrome if needed, create a page for the origin.
    /// On failure, does a full reset and retries once.
    async fn create_page_with_recovery(&self, origin: &str) -> Result<Page, String> {
        // First attempt
        match self.try_create_page(origin).await {
            Ok(page) => return Ok(page),
            Err(e) => {
                warn!("[CDP] First attempt failed: {}, doing full recovery", e);
            }
        }

        // Full reset: kill everything, clean profile, retry
        self.force_reset().await;

        // Second attempt
        self.try_create_page(origin).await
            .map_err(|e| format!("Browser failed after recovery: {}", e))
    }

    /// Try to ensure Chrome is running and create a page. May fail.
    async fn try_create_page(&self, origin: &str) -> Result<Page, String> {
        self.ensure_browser_running().await?;

        let mut state = self.state.lock().await;

        // Remove stale entry if any
        state.pages.remove(origin);

        let browser = state.browser.as_ref().ok_or("Browser not initialized")?;
        let page = browser.browser
            .new_page("about:blank")
            .await
            .map_err(|e| format!("Failed to create tab: {}", e))?;

        state.pages.insert(origin.to_string(), page.clone());
        info!("[CDP] Created page for origin='{}'", origin);
        Ok(page)
    }

    /// Ensure Chrome process is running. Idempotent.
    async fn ensure_browser_running(&self) -> Result<(), String> {
        {
            let state = self.state.lock().await;
            if state.browser.is_some() {
                return Ok(());
            }
        }
        // Lock is released here — launch is slow, don't hold it.
        // Re-check after launch to handle concurrent callers.
        self.launch_browser().await
    }

    /// Launch a fresh Chrome process.
    async fn launch_browser(&self) -> Result<(), String> {
        let mut state = self.state.lock().await;

        // Double-check after acquiring lock (another task may have launched)
        if state.browser.is_some() {
            return Ok(());
        }

        let chrome_exe = self.find_chrome()?;
        let user_data_dir = self.get_profile_dir()?;

        // Clean up orphaned processes from previous crashes
        self.cleanup_profile(&user_data_dir).await;

        info!("[CDP] Launching Chrome: {:?} (profile: {:?})", chrome_exe, user_data_dir);

        let config = BrowserConfig::builder()
            .chrome_executable(chrome_exe)
            .user_data_dir(&user_data_dir)
            .with_head()
            .viewport(None)
            .arg("--disable-blink-features=AutomationControlled")
            .arg("--disable-infobars")
            .arg("--no-first-run")
            .arg("--no-default-browser-check")
            .arg("--disable-search-engine-choice-screen")
            .arg("--disable-session-crashed-bubble")
            .arg("--hide-crash-restore-bubble")
            .arg("--disable-features=ProfilePicker,ChromeWhatsNewUI,TranslateUI")
            .arg("--disable-extensions")
            .arg("--noerrdialogs")
            .build()
            .map_err(|e| format!("BrowserConfig error: {}", e))?;

        let (browser, mut handler) = Browser::launch(config)
            .await
            .map_err(|e| format!("Failed to launch Chrome: {}", e))?;

        let handler_task = tokio::spawn(async move {
            while let Some(event) = handler.next().await {
                if event.is_err() { break; }
            }
        });

        // Verify Chrome is actually responsive by creating and closing a test page
        let test_page = browser.new_page("about:blank")
            .await
            .map_err(|e| format!("Chrome launched but is not responsive: {}", e))?;
        let _ = test_page.close().await;

        state.browser = Some(BrowserHandle { browser, handler_task });
        info!("[CDP] Chrome launched and verified");
        Ok(())
    }

    /// Tear down everything: kill Chrome, clear state, clean profile.
    async fn force_reset(&self) {
        let old_handle = self.state.lock().await.reset();

        // Shut down the old browser gracefully (or forcefully)
        if let Some(handle) = old_handle {
            Self::shutdown_browser(handle).await;
        }

        // Kill any orphaned processes and clean up profile lock
        if let Ok(dir) = self.get_profile_dir() {
            self.cleanup_profile(&dir).await;
        }

        info!("[CDP] Full reset completed");
    }

    /// Get a clone of the active page, or error if none.
    async fn get_active_page(&self) -> Result<Page, String> {
        let state = self.state.lock().await;
        let origin = state.active_origin.as_ref()
            .ok_or("No active page. Use browse_navigate first.")?;
        let page = state.pages.get(origin)
            .ok_or("No active page. Use browse_navigate first.")?;

        // Liveness check before returning
        if page.evaluate("1").await.is_err() {
            return Err("The browser tab was closed. Use browse_navigate to reopen.".to_string());
        }

        Ok(page.clone())
    }

    // ── Utilities ───────────────────────────────────────────────

    fn get_profile_dir(&self) -> Result<PathBuf, String> {
        let dir = self.app_handle.path().app_data_dir()
            .unwrap_or_else(|_| std::env::temp_dir())
            .join("cdp-browser-profile");
        std::fs::create_dir_all(&dir).ok();
        Ok(dir)
    }

    /// Clean up stale locks and orphaned Chrome processes for a profile dir.
    async fn cleanup_profile(&self, user_data_dir: &PathBuf) {
        // Kill processes using this profile
        let dir_str = user_data_dir.to_string_lossy().to_string();
        let _ = std::process::Command::new("pkill")
            .args(["-f", &format!("--user-data-dir={}", dir_str)])
            .output();

        // Also kill legacy V3 chromiumoxide-runner processes
        let legacy_dir = std::env::temp_dir().join("chromiumoxide-runner");
        if legacy_dir.exists() {
            let legacy_str = legacy_dir.to_string_lossy().to_string();
            let _ = std::process::Command::new("pkill")
                .args(["-f", &format!("--user-data-dir={}", legacy_str)])
                .output();
        }

        // Wait for processes to die, then remove lock file
        tokio::time::sleep(Duration::from_millis(500)).await;
        let _ = std::fs::remove_file(user_data_dir.join("SingletonLock"));
    }

    async fn shutdown_browser(mut handle: BrowserHandle) {
        info!("[CDP] Shutting down Chrome...");
        let _ = handle.browser.close().await;
        let _ = tokio::time::timeout(Duration::from_secs(5), handle.browser.wait()).await;
        handle.handler_task.abort();
        info!("[CDP] Chrome shut down");
    }

    fn find_chrome(&self) -> Result<PathBuf, String> {
        if let Some(ref p) = self.chrome_path {
            if p.exists() { return Ok(p.clone()); }
            warn!("[CDP] Configured chrome_path {:?} does not exist", p);
        }

        if let Ok(resource_dir) = self.app_handle.path().resource_dir() {
            let bundled = resource_dir.join("chromium-runtime");
            let mac_exe = bundled.join("Chromium.app/Contents/MacOS/Chromium");
            if mac_exe.exists() { return Ok(mac_exe); }
            for name in ["chromium", "chrome", "chrome.exe", "chromium.exe"] {
                let p = bundled.join(name);
                if p.exists() { return Ok(p); }
            }
        }

        let mac = PathBuf::from("/Applications/Google Chrome.app/Contents/MacOS/Google Chrome");
        if mac.exists() { return Ok(mac); }

        for p in ["/usr/bin/google-chrome", "/usr/bin/google-chrome-stable", "/usr/bin/chromium", "/usr/bin/chromium-browser"] {
            let path = PathBuf::from(p);
            if path.exists() { return Ok(path); }
        }

        Err("Chrome/Chromium not found. Please install Google Chrome.".to_string())
    }

    fn extract_origin(url: &str) -> Result<String, String> {
        let parsed = url::Url::parse(url).map_err(|e| format!("Invalid URL '{}': {}", url, e))?;
        let host = parsed.host_str().unwrap_or("");
        match parsed.port() {
            Some(port) => Ok(format!("{}://{}:{}", parsed.scheme(), host, port)),
            None => Ok(format!("{}://{}", parsed.scheme(), host)),
        }
    }

    fn is_connection_error(err: &str) -> bool {
        err.contains("canceled") || err.contains("receiver is gone") || err.contains("timed out")
    }

    fn detect_login_redirect(original_url: &str, final_url: &str, target_origin: &str) -> bool {
        let final_origin = Self::extract_origin(final_url).unwrap_or_default();
        if final_origin != *target_origin {
            return true;
        }
        // Same-origin: check if path changed to a login-like page
        let target_path = url::Url::parse(original_url).ok().map(|u| u.path().to_string()).unwrap_or_default();
        let final_path = url::Url::parse(final_url).ok().map(|u| u.path().to_string()).unwrap_or_default();
        if final_path != target_path {
            let fp = final_path.to_lowercase();
            return fp.contains("login") || fp.contains("signin") || fp.contains("/sso")
                || fp.contains("/auth") || fp.contains("/cas/");
        }
        false
    }

    fn handle_page_error(&self, e: impl std::fmt::Display, context: &str) -> String {
        let msg = format!("{}", e);
        if Self::is_connection_error(&msg) {
            "The browser tab was closed or crashed. Use browse_navigate to reopen.".to_string()
        } else {
            format!("Failed to evaluate {}: {}", context, msg)
        }
    }

    async fn eval_string(page: &Page, js: &str) -> String {
        page.evaluate(js).await.ok()
            .and_then(|v| v.into_value::<String>().ok())
            .unwrap_or_default()
    }

    fn parse_tables(result: &serde_json::Value) -> Vec<TableData> {
        result["tables"].as_array().map(|arr| {
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

    async fn wait_for_content_stable(&self, page: &Page, timeout_ms: u64) {
        let js = r#"(() => {
            const el = document.querySelector('main, [role="main"], .main-content, #app') || document.body;
            return el ? el.innerHTML.length : 0;
        })()"#;

        let mut last_len: usize = 0;
        let mut stable_count = 0u32;
        let interval = 300u64;
        let max_iters = (timeout_ms / interval) as usize;

        for _ in 0..max_iters {
            tokio::time::sleep(Duration::from_millis(interval)).await;
            let len: usize = page.evaluate(js).await.ok()
                .and_then(|v| v.into_value::<usize>().ok())
                .unwrap_or(0);

            if len == last_len && len > 0 {
                stable_count += 1;
                if stable_count >= 3 { return; }
            } else {
                stable_count = 0;
            }
            last_len = len;
        }
    }
}
