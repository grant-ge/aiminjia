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

use super::site_map::{PageProfile, SiteMap, TableSchema};
use super::webview_auth::{
    ApiFetchResult, BrowseNavigateResult, BrowseResult, DiscoveredApi, ExecuteJsResult,
    FormData, FormField, FullPageResult, LinkData, TableData,
};

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

    // Extract navigation links, menu items, and clickable elements
    var links = [];
    var seen = {};

    // 1. <a> tags with href
    var anchors = document.querySelectorAll('a[href]');
    for (var i = 0; i < anchors.length && links.length < 50; i++) {
        var a = anchors[i];
        var href = a.href || '';
        var label = (a.innerText || a.title || a.getAttribute('aria-label') || '').trim();
        if (!label || !href || href === '#' || href.startsWith('javascript:')) continue;
        label = label.substring(0, 80).replace(/\n/g, ' ');
        var key = label + '|' + href;
        if (seen[key]) continue;
        seen[key] = true;
        links.push({label: label, href: href, type: 'link'});
    }

    // 2. Menu items in nav, sidebar, [role="menu"], [role="navigation"]
    var menuSels = 'nav a, nav [role="menuitem"], [role="navigation"] a, [role="menu"] a, ' +
        '.sidebar a, .side-menu a, .ant-menu a, .el-menu a, .nav-menu a, ' +
        '.ant-menu-item, .el-menu-item, .el-sub-menu__title';
    var menuEls = document.querySelectorAll(menuSels);
    for (var i = 0; i < menuEls.length && links.length < 80; i++) {
        var el = menuEls[i];
        var label = (el.innerText || el.title || el.getAttribute('aria-label') || '').trim();
        if (!label) continue;
        label = label.substring(0, 80).replace(/\n/g, ' ');
        var href = el.href || el.getAttribute('data-href') || '';
        var key = 'menu|' + label;
        if (seen[key]) continue;
        seen[key] = true;
        // Build a CSS selector for AI to click
        var selector = '';
        if (el.id) selector = '#' + el.id;
        else if (el.className && typeof el.className === 'string') {
            var cls = el.className.trim().split(/\s+/).slice(0, 3).join('.');
            if (cls) selector = el.tagName.toLowerCase() + '.' + cls;
        }
        links.push({label: label, href: href, type: 'menu', selector: selector});
    }

    // 3. Buttons (submit, action buttons)
    var buttons = document.querySelectorAll('button, [role="button"], input[type="submit"]');
    for (var i = 0; i < buttons.length && links.length < 100; i++) {
        var btn = buttons[i];
        var label = (btn.innerText || btn.value || btn.title || btn.getAttribute('aria-label') || '').trim();
        if (!label || label.length > 80) continue;
        label = label.replace(/\n/g, ' ');
        var key = 'btn|' + label;
        if (seen[key]) continue;
        seen[key] = true;
        links.push({label: label, href: '', type: 'button'});
    }

    return {
        url: window.location.href,
        title: document.title,
        tables: tables,
        text: text,
        links: links
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
    site_maps: Mutex<HashMap<String, SiteMap>>,
}

impl CdpBrowser {
    pub fn new(app_handle: AppHandle, chrome_path: Option<PathBuf>) -> Self {
        Self {
            app_handle,
            state: Mutex::new(BrowserState::new()),
            chrome_path,
            site_maps: Mutex::new(HashMap::new()),
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

        // Navigate — handle redirect-induced cancellations gracefully.
        // chromiumoxide's goto() waits for `loadEventFired`, which can fail on
        // sites with iframes or complex redirects (e.g. SSO → login page).
        // Strategy: if goto() fails with "oneshot canceled", fall back to
        // JS-based navigation which doesn't wait for the load event.
        if let Err(e) = page.goto(url).await {
            let err_str = format!("{}", e);
            if Self::is_redirect_cancel(&err_str) {
                // Fallback: use JS navigation (doesn't wait for load event)
                info!("[CDP] goto got redirect cancel, falling back to JS navigation");
                let js_nav = format!("window.location.href = '{}'", url.replace('\'', "\\'"));
                let _ = page.evaluate(js_nav.as_str()).await;
                // Give the browser a moment to start the navigation
                tokio::time::sleep(Duration::from_millis(500)).await;
            } else if Self::is_connection_error(&err_str) {
                warn!("[CDP] goto failed ({}), recovering...", err_str);
                self.force_reset().await;
                let new_page = self.ensure_page(&target_origin).await?;
                // Use JS navigation for the retry too, since goto may fail again
                let js_nav = format!("window.location.href = '{}'", url.replace('\'', "\\'"));
                let _ = new_page.evaluate(js_nav.as_str()).await;
                tokio::time::sleep(Duration::from_millis(500)).await;
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

        // Auto-explore: build PageProfile for this page (or use cache)
        let page_profile = if !redirected_to_login {
            let url_path = Self::extract_path(&final_url);
            let cached = {
                let maps = self.site_maps.lock().await;
                maps.get(&target_origin).and_then(|m| m.get_page(&url_path)).cloned()
            };
            if let Some(profile) = cached {
                info!("[CDP] Using cached profile for {}", url_path);
                Some(profile)
            } else {
                let profile = self.auto_explore(&page, &title, &target_origin, &url_path).await;
                let app_data_dir = self.get_app_data_dir();
                {
                    let mut maps = self.site_maps.lock().await;
                    let site_map = maps.entry(target_origin.clone())
                        .or_insert_with(|| {
                            SiteMap::load(&app_data_dir, &target_origin)
                                .unwrap_or_else(|| SiteMap::new(&target_origin))
                        });
                    site_map.set_page(profile.clone());
                    let _ = site_map.save(&app_data_dir);
                }
                Some(profile)
            }
        } else {
            // Mark as access denied in site map
            let url_path = Self::extract_path(&final_url);
            let profile = PageProfile {
                url_path: url_path.clone(),
                title: title.clone(),
                nav_links: vec![],
                table_schemas: vec![],
                forms: vec![],
                api_endpoints: vec![],
                explored_at: chrono::Utc::now(),
                access_denied: true,
            };
            let app_data_dir = self.get_app_data_dir();
            {
                let mut maps = self.site_maps.lock().await;
                let site_map = maps.entry(target_origin.clone())
                    .or_insert_with(|| {
                        SiteMap::load(&app_data_dir, &target_origin)
                            .unwrap_or_else(|| SiteMap::new(&target_origin))
                    });
                site_map.set_page(profile.clone());
                let _ = site_map.save(&app_data_dir);
            }
            Some(profile)
        };

        // Capture screenshot
        let screenshot_path = self.capture_screenshot().await;

        let result = BrowseNavigateResult {
            url: final_url,
            title,
            redirected_to_login,
            page_profile,
            screenshot_path,
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
        let links = Self::parse_links(&result);

        info!("[CDP] read_content: url={}, tables={}, links={}, text_len={}",
            page_url, tables.len(), links.len(), text.len());

        Ok(BrowseResult { url: page_url, title: page_title, tables, text, links })
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

        // Check if JS triggered navigation by comparing URL returned from JS wrapper.
        // The wrapper captures URL at return time, which catches synchronous navigations.
        // For async navigations (click → navigate, form.submit), we do a brief re-check.
        let url_after_sync = result["url"].as_str().unwrap_or("");
        let navigated = if url_after_sync != url_before && !url_before.is_empty() && !url_after_sync.is_empty() {
            true
        } else {
            // Brief pause to catch async navigations (click, form.submit, location.href=...)
            tokio::time::sleep(Duration::from_millis(50)).await;
            let url_after_async = Self::eval_string(&page, "window.location.href").await;
            url_after_async != url_before && !url_before.is_empty()
        };

        if navigated {
            self.wait_for_content_stable(&page, 5_000).await;
            let final_url = Self::eval_string(&page, "window.location.href").await;
            let new_title_after = Self::eval_string(&page, "document.title").await;
            let _ = self.app_handle.emit("browser:page-ready", serde_json::json!({
                "url": &final_url, "title": &new_title_after,
            }));
            info!("[CDP] execute_js triggered navigation: {} -> {}", url_before, final_url);
            return Ok(ExecuteJsResult {
                value: result["value"].clone(), error: None,
                new_url: Some(final_url), new_title: Some(new_title_after),
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

    /// Navigate + inject XHR interceptor + extract everything in one shot.
    /// Combines navigate + read_content + API discovery + form discovery.
    pub async fn navigate_and_extract(
        &self,
        url: &str,
        extract_script: Option<&str>,
    ) -> Result<FullPageResult, String> {
        let target_origin = Self::extract_origin(url)?;
        let page = self.ensure_page(&target_origin).await?;

        info!("[CDP] navigate_and_extract: url={}", url);
        let _ = self.app_handle.emit("browser:navigating", serde_json::json!({ "url": url }));

        // 1. Inject XHR/fetch interceptor BEFORE navigation
        let inject_js = r#"
window.__aijia_api_calls = [];
(function() {
    // Intercept fetch
    var origFetch = window.fetch;
    window.fetch = function(input, init) {
        var url = (typeof input === 'string') ? input : (input.url || '');
        var method = (init && init.method) ? init.method.toUpperCase() : 'GET';
        var entry = {method: method, url: url, status: 0, contentType: '', sizeBytes: 0, ts: Date.now()};
        var idx = window.__aijia_api_calls.length;
        window.__aijia_api_calls.push(entry);
        return origFetch.apply(this, arguments).then(function(resp) {
            entry.status = resp.status;
            entry.contentType = resp.headers.get('content-type') || '';
            var cl = resp.headers.get('content-length');
            if (cl) entry.sizeBytes = parseInt(cl, 10);
            return resp;
        });
    };
    // Intercept XMLHttpRequest
    var origOpen = XMLHttpRequest.prototype.open;
    var origSend = XMLHttpRequest.prototype.send;
    XMLHttpRequest.prototype.open = function(method, url) {
        this.__aijia = {method: (method||'GET').toUpperCase(), url: url||'', status: 0, contentType: '', sizeBytes: 0, ts: Date.now()};
        return origOpen.apply(this, arguments);
    };
    XMLHttpRequest.prototype.send = function() {
        var self = this;
        var entry = self.__aijia;
        if (entry) {
            var idx = window.__aijia_api_calls.length;
            window.__aijia_api_calls.push(entry);
            self.addEventListener('load', function() {
                entry.status = self.status;
                entry.contentType = self.getResponseHeader('content-type') || '';
                entry.sizeBytes = (self.responseText || '').length;
            });
        }
        return origSend.apply(this, arguments);
    };
})();
"#;
        let _ = page.evaluate(inject_js).await;

        // 2. Navigate (same logic as navigate())
        if let Err(e) = page.goto(url).await {
            let err_str = format!("{}", e);
            if Self::is_redirect_cancel(&err_str) {
                info!("[CDP] navigate_and_extract: goto redirect cancel, JS fallback");
                let js_nav = format!("window.location.href = '{}'", url.replace('\'', "\\'"));
                let _ = page.evaluate(js_nav.as_str()).await;
                tokio::time::sleep(Duration::from_millis(500)).await;
            } else if Self::is_connection_error(&err_str) {
                warn!("[CDP] navigate_and_extract: goto failed ({}), recovering", err_str);
                self.force_reset().await;
                let new_page = self.ensure_page(&target_origin).await?;
                // Re-inject interceptor on new page
                let _ = new_page.evaluate(inject_js).await;
                let js_nav = format!("window.location.href = '{}'", url.replace('\'', "\\'"));
                let _ = new_page.evaluate(js_nav.as_str()).await;
                tokio::time::sleep(Duration::from_millis(500)).await;
            } else {
                return Err(format!("Failed to navigate to '{}': {}", url, err_str));
            }
        }

        // 3. Wait for content to stabilize
        self.wait_for_content_stable(&page, 10_000).await;

        let title = Self::eval_string(&page, "document.title").await;
        let final_url = Self::eval_string(&page, "window.location.href").await;
        let final_url = if final_url.is_empty() { url.to_string() } else { final_url };

        self.state.lock().await.active_origin = Some(target_origin.clone());

        let redirected_to_login = Self::detect_login_redirect(url, &final_url, &target_origin);

        let navigate_result = BrowseNavigateResult {
            url: final_url.clone(),
            title: title.clone(),
            redirected_to_login,
            page_profile: None, // navigate_and_extract returns full content separately
            screenshot_path: None, // screenshot handled below
        };

        let _ = self.app_handle.emit("browser:page-ready", serde_json::json!({
            "url": &navigate_result.url, "title": &navigate_result.title,
        }));

        // 4. Extract content (tables + text + links)
        let content = self.read_content(extract_script).await.unwrap_or_else(|e| {
            warn!("[CDP] navigate_and_extract: read_content failed: {}", e);
            BrowseResult { url: final_url.clone(), title: title.clone(), tables: vec![], text: String::new(), links: vec![] }
        });

        // 5. Read intercepted API calls
        let api_calls = Self::read_intercepted_apis(&page).await;

        // 6. Discover forms
        let forms = Self::discover_forms(&page).await;

        // 7. Auto-explore and cache PageProfile
        let url_path = Self::extract_path(&final_url);
        if !redirected_to_login {
            let profile = self.auto_explore(&page, &title, &target_origin, &url_path).await;
            let app_data_dir = self.get_app_data_dir();
            let mut maps = self.site_maps.lock().await;
            let site_map = maps.entry(target_origin.clone())
                .or_insert_with(|| {
                    SiteMap::load(&app_data_dir, &target_origin)
                        .unwrap_or_else(|| SiteMap::new(&target_origin))
                });
            site_map.set_page(profile);
            let _ = site_map.save(&app_data_dir);
        }

        info!("[CDP] navigate_and_extract complete: url={}, tables={}, links={}, apis={}, forms={}",
            content.url, content.tables.len(), content.links.len(), api_calls.len(), forms.len());

        Ok(FullPageResult { navigate: navigate_result, content, api_calls, forms })
    }

    /// Execute fetch() in the active page context for REST API calls.
    /// Automatically includes cookies and session headers.
    pub async fn api_fetch(
        &self,
        url: &str,
        method: &str,
        body: Option<&str>,
        headers: Option<&str>,
    ) -> Result<ApiFetchResult, String> {
        let page = self.get_active_page().await?;

        info!("[CDP] api_fetch: {} {}", method, url);

        let headers_obj = headers.unwrap_or("{}");
        let body_str = match body {
            Some(b) => format!("JSON.stringify({})", b),
            None => "undefined".to_string(),
        };

        let fetch_js = format!(
            r#"(async () => {{
    try {{
        var opts = {{
            method: '{method}',
            headers: Object.assign({{'Accept': 'application/json', 'Content-Type': 'application/json'}}, {headers}),
        }};
        var body = {body};
        if (body !== undefined) opts.body = body;

        var resp = await fetch('{url}', opts);
        var ct = resp.headers.get('content-type') || '';
        var status = resp.status;
        var text = await resp.text();
        var data = null;
        var totalRows = null;

        if (ct.includes('json')) {{
            try {{
                data = JSON.parse(text);
                if (Array.isArray(data)) {{
                    totalRows = data.length;
                }} else if (data && typeof data === 'object') {{
                    var arr = data.list || data.rows || data.data || data.items || data.records || data.content;
                    if (Array.isArray(arr)) {{
                        totalRows = data.total || data.totalCount || data.count || arr.length;
                    }}
                }}
            }} catch(e) {{ data = text; }}
        }} else if (ct.includes('html')) {{
            var parser = new DOMParser();
            var doc = parser.parseFromString(text, 'text/html');
            var tables = [];
            doc.querySelectorAll('table').forEach(function(t, i) {{
                if (i >= 5) return;
                var headers = Array.from(t.querySelectorAll('thead th')).map(function(h) {{ return h.textContent.trim(); }});
                var rows = [];
                t.querySelectorAll('tbody tr').forEach(function(tr) {{
                    var cells = Array.from(tr.querySelectorAll('td')).map(function(td) {{ return td.textContent.trim(); }});
                    if (cells.length > 0) rows.push(cells);
                }});
                if (headers.length > 0 || rows.length > 0) tables.push({{headers: headers, rows: rows}});
                totalRows = (totalRows || 0) + rows.length;
            }});
            data = tables.length > 0 ? tables : text.substring(0, 10000);
        }} else {{
            data = text.substring(0, 10000);
        }}

        return {{status: status, contentType: ct, data: data, totalRows: totalRows}};
    }} catch(e) {{
        return {{status: 0, contentType: '', data: e.message, totalRows: null}};
    }}
}})()"#,
            method = method.to_uppercase(),
            url = url.replace('\'', "\\'"),
            headers = headers_obj,
            body = body_str,
        );

        let result: serde_json::Value = page.evaluate(fetch_js.as_str())
            .await
            .map_err(|e| self.handle_page_error(e, "api_fetch"))?
            .into_value()
            .map_err(|e| format!("Failed to parse api_fetch result: {}", e))?;

        let status = result["status"].as_u64().unwrap_or(0) as u16;
        let content_type = result["contentType"].as_str().unwrap_or("").to_string();
        let total_rows = result["totalRows"].as_u64();
        let data = result["data"].clone();

        // If data is large (>50KB JSON), save to file and return path + sample
        let data_json = serde_json::to_string(&data).unwrap_or_default();
        let (final_data, truncated, saved_file_path) = if data_json.len() > 50_000 {
            // Save full data to file
            let dir = self.get_app_data_dir().join("api-data");
            std::fs::create_dir_all(&dir).ok();
            let filename = format!("api_{}_{}.json",
                chrono::Utc::now().format("%Y%m%d_%H%M%S"),
                url.split('/').last().unwrap_or("data").split('?').next().unwrap_or("data")
            );
            let path = dir.join(&filename);
            let _ = std::fs::write(&path, &data_json);
            info!("[CDP] api_fetch: large response ({} bytes) saved to {:?}", data_json.len(), path);

            // Build sample: first 5 rows + schema info
            let sample = Self::build_data_sample(&data, 5);
            (sample, true, Some(path))
        } else {
            (data, false, None)
        };

        info!("[CDP] api_fetch complete: {} {} → status={}, rows={:?}, truncated={}, saved={:?}",
            method, url, status, total_rows, truncated, saved_file_path);

        Ok(ApiFetchResult { status, content_type, data: final_data, total_rows, truncated, saved_file_path })
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
        // Kill any orphaned Chrome processes using our profile dir
        if let Ok(dir) = self.get_profile_dir() {
            self.cleanup_profile(&dir).await;
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
                // Log errors but keep processing — transient errors (e.g. from
                // redirects or closed iframes) should not kill the handler.
                if let Err(e) = event {
                    log::warn!("[CDP] Handler event error (non-fatal): {}", e);
                }
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
    /// Liveness is not checked here — if the page is dead, the subsequent
    /// evaluate/goto call will fail and the caller handles recovery.
    async fn get_active_page(&self) -> Result<Page, String> {
        let state = self.state.lock().await;
        let origin = state.active_origin.as_ref()
            .ok_or("No active page. Use browse_navigate first.")?;
        let page = state.pages.get(origin)
            .ok_or("No active page. Use browse_navigate first.")?;

        Ok(page.clone())
    }

    // ── Utilities ───────────────────────────────────────────────

    fn get_app_data_dir(&self) -> PathBuf {
        self.app_handle.path().app_data_dir()
            .unwrap_or_else(|_| std::env::temp_dir())
    }

    fn get_profile_dir(&self) -> Result<PathBuf, String> {
        let dir = self.get_app_data_dir().join("cdp-browser-profile");
        std::fs::create_dir_all(&dir).ok();
        Ok(dir)
    }

    fn extract_path(url: &str) -> String {
        url::Url::parse(url).ok()
            .map(|u| u.path().to_string())
            .unwrap_or_else(|| url.to_string())
    }

    /// Auto-explore a page: collect nav links, table schemas, forms, intercepted APIs.
    /// Pure JS execution, no LLM involved. Typically < 500ms.
    async fn auto_explore(
        &self,
        page: &Page,
        title: &str,
        _origin: &str,
        url_path: &str,
    ) -> PageProfile {
        info!("[CDP] auto_explore: {}", url_path);

        // Collect all info in one JS call for speed — scans main doc + all same-origin iframes
        let js = r#"(() => {
    var result = {nav_links: [], table_schemas: [], forms: [], api_endpoints: []};
    var seen = {};

    // Collect all documents: main + same-origin iframes (recursive)
    function collectDocs(doc, depth) {
        if (!doc || depth > 3) return;
        var docs = [doc];
        try {
            var frames = doc.querySelectorAll('iframe, frame');
            for (var i = 0; i < frames.length; i++) {
                try {
                    var fd = frames[i].contentDocument || (frames[i].contentWindow && frames[i].contentWindow.document);
                    if (fd) { docs.push(fd); collectDocs(fd, depth + 1); }
                } catch(e) {} // cross-origin, skip
            }
        } catch(e) {}
        return docs;
    }
    var allDocs = [];
    collectDocs(document, 0).forEach(function(d) { if (d) allDocs.push(d); });

    allDocs.forEach(function(doc) {
        // 1. Navigation links — menu items
        var menuSels = 'nav a, nav [role="menuitem"], [role="navigation"] a, [role="menu"] a, ' +
            '.sidebar a, .side-menu a, .ant-menu a, .el-menu a, .nav-menu a, ' +
            '.ant-menu-item, .el-menu-item, .el-sub-menu__title, ' +
            '.layui-nav a, .layui-side a, .left-nav a, #menu a, .menu a';
        try {
        doc.querySelectorAll(menuSels).forEach(function(el) {
            if (result.nav_links.length >= 80) return;
            var label = (el.innerText || el.title || el.getAttribute('aria-label') || '').trim().substring(0, 80).replace(/\n/g, ' ');
            if (!label) return;
            var href = el.href || el.getAttribute('data-href') || el.getAttribute('data-url') || '';
            var key = 'menu|' + label;
            if (seen[key]) return;
            seen[key] = true;
            var selector = '';
            if (el.id) selector = '#' + el.id;
            else if (el.className && typeof el.className === 'string') {
                var cls = el.className.trim().split(/\s+/).slice(0, 3).join('.');
                if (cls) selector = el.tagName.toLowerCase() + '.' + cls;
            }
            result.nav_links.push({label: label, href: href, type: 'menu', selector: selector});
        });
        } catch(e) {}

        // Regular links
        try {
        doc.querySelectorAll('a[href]').forEach(function(a) {
            if (result.nav_links.length >= 120) return;
            var label = (a.innerText || a.title || '').trim().substring(0, 80).replace(/\n/g, ' ');
            var href = a.href || '';
            if (!label || !href || href === '#' || href.startsWith('javascript:')) return;
            var key = label + '|' + href;
            if (seen[key]) return;
            seen[key] = true;
            result.nav_links.push({label: label, href: href, type: 'link', selector: ''});
        });
        } catch(e) {}

        // Buttons
        try {
        doc.querySelectorAll('button, [role="button"], input[type="submit"]').forEach(function(btn) {
            if (result.nav_links.length >= 150) return;
            var label = (btn.innerText || btn.value || btn.title || '').trim().substring(0, 80).replace(/\n/g, ' ');
            if (!label) return;
            var key = 'btn|' + label;
            if (seen[key]) return;
            seen[key] = true;
            result.nav_links.push({label: label, href: '', type: 'button', selector: ''});
        });
        } catch(e) {}

        // 2. Table schemas
        try {
        doc.querySelectorAll('table').forEach(function(t, i) {
            if (result.table_schemas.length >= 10) return;
            var headers = [];
            t.querySelectorAll('thead th, thead td, tr:first-child th').forEach(function(h) {
                var text = h.innerText.trim();
                if (text) headers.push(text);
            });
            if (headers.length === 0) {
                var firstRow = t.querySelector('tr');
                if (firstRow) firstRow.querySelectorAll('td, th').forEach(function(h) {
                    var text = h.innerText.trim();
                    if (text) headers.push(text);
                });
            }
            var rowCount = t.querySelectorAll('tbody tr').length || Math.max(0, t.querySelectorAll('tr').length - 1);
            var name = '';
            var caption = t.querySelector('caption');
            if (caption) name = caption.innerText.trim();
            if (!name) {
                var prev = t.previousElementSibling;
                if (prev && /^H[1-6]$/.test(prev.tagName)) name = prev.innerText.trim();
            }
            if (headers.length > 0 || rowCount > 0) {
                result.table_schemas.push({name: name, headers: headers, rowCount: rowCount});
            }
        });
        } catch(e) {}

        // 3. Forms
        try {
        doc.querySelectorAll('form').forEach(function(f, i) {
            if (result.forms.length >= 10) return;
            var fields = [];
            f.querySelectorAll('input, select, textarea').forEach(function(el) {
                var name = el.name || el.id || '';
                if (!name) return;
                var ftype = el.type || el.tagName.toLowerCase();
                var value = el.value || '';
                if (el.tagName === 'SELECT') {
                    var opts = Array.from(el.options).map(function(o) { return o.value; });
                    value = opts.join(',');
                    ftype = 'select(' + opts.length + ')';
                }
                fields.push({name: name, fieldType: ftype, value: value.substring(0, 100)});
            });
            result.forms.push({
                id: f.id || ('form_' + i),
                action: f.action || '',
                method: (f.method || 'GET').toUpperCase(),
                fields: fields
            });
        });
        } catch(e) {}
    }); // end allDocs.forEach

    // 4. Intercepted API calls (main window only)
    var calls = window.__aijia_api_calls || [];
    var apiSeen = {};
    calls.forEach(function(c) {
        var key = c.method + ' ' + c.url;
        if (!apiSeen[key] || c.status > 0) apiSeen[key] = c;
    });
    Object.values(apiSeen).forEach(function(c) {
        var u = c.url.toLowerCase();
        if (u.match(/\.(js|css|png|jpg|gif|svg|woff|ttf|ico)(\?|$)/)) return;
        if (c.url.length >= 500) return;
        result.api_endpoints.push({
            method: c.method, url: c.url, status: c.status || 0,
            contentType: c.contentType || '', sizeBytes: c.sizeBytes || 0
        });
    });

    return result;
})()"#;

        let data: serde_json::Value = match page.evaluate(js).await {
            Ok(v) => v.into_value().unwrap_or(serde_json::Value::Null),
            Err(e) => {
                warn!("[CDP] auto_explore JS failed: {}", e);
                return PageProfile {
                    url_path: url_path.to_string(),
                    title: title.to_string(),
                    nav_links: vec![], table_schemas: vec![], forms: vec![],
                    api_endpoints: vec![], explored_at: chrono::Utc::now(), access_denied: false,
                };
            }
        };

        // Parse results
        let nav_links = Self::parse_links_from_value(&data["nav_links"]);
        let table_schemas = Self::parse_table_schemas(&data["table_schemas"]);
        let forms = Self::parse_forms_from_value(&data["forms"]);
        let api_endpoints = Self::parse_apis_from_value(&data["api_endpoints"]);

        info!("[CDP] auto_explore complete: path={}, links={}, tables={}, forms={}, apis={}",
            url_path, nav_links.len(), table_schemas.len(), forms.len(), api_endpoints.len());

        PageProfile {
            url_path: url_path.to_string(),
            title: title.to_string(),
            nav_links,
            table_schemas,
            forms,
            api_endpoints,
            explored_at: chrono::Utc::now(),
            access_denied: false,
        }
    }

    /// Capture a screenshot of the current page, save to temp file, return path.
    pub async fn capture_screenshot(&self) -> Option<PathBuf> {
        let page = self.get_active_page().await.ok()?;

        let params = chromiumoxide::page::ScreenshotParams::builder()
            .format(chromiumoxide::cdp::browser_protocol::page::CaptureScreenshotFormat::Png)
            .build();

        let bytes = page.screenshot(params).await.ok()?;

        if bytes.is_empty() {
            warn!("[CDP] Screenshot returned empty data");
            return None;
        }

        // Save to temp file
        let dir = self.get_app_data_dir().join("screenshots");
        std::fs::create_dir_all(&dir).ok()?;
        let filename = format!("page_{}.png", chrono::Utc::now().format("%Y%m%d_%H%M%S"));
        let path = dir.join(&filename);
        std::fs::write(&path, &bytes).ok()?;

        info!("[CDP] Screenshot saved: {:?} ({} bytes)", path, bytes.len());
        Some(path)
    }

    /// Get the site map for a given origin (for context injection).
    pub async fn get_site_map_context(&self, active_path: Option<&str>) -> Option<String> {
        let state = self.state.lock().await;
        let origin = state.active_origin.as_ref()?;
        let maps = self.site_maps.lock().await;
        let site_map = maps.get(origin)?;
        Some(site_map.format_for_context(active_path))
    }

    /// Get the active origin.
    pub async fn get_active_origin(&self) -> Option<String> {
        self.state.lock().await.active_origin.clone()
    }

    /// Get a cached PageProfile for a given origin + path.
    pub async fn get_cached_page_profile(&self, origin: &str, url_path: &str) -> Option<PageProfile> {
        let maps = self.site_maps.lock().await;
        maps.get(origin)?.get_page(url_path).cloned()
    }

    fn parse_links_from_value(val: &serde_json::Value) -> Vec<LinkData> {
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

    fn parse_table_schemas(val: &serde_json::Value) -> Vec<TableSchema> {
        val.as_array().map(|arr| {
            arr.iter().filter_map(|t| {
                let headers: Vec<String> = t["headers"].as_array()?
                    .iter().filter_map(|h| h.as_str().map(String::from)).collect();
                Some(TableSchema {
                    name: t["name"].as_str().unwrap_or("").to_string(),
                    headers,
                    row_count: t["rowCount"].as_u64().unwrap_or(0) as usize,
                })
            }).collect()
        }).unwrap_or_default()
    }

    fn parse_forms_from_value(val: &serde_json::Value) -> Vec<FormData> {
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

    fn parse_apis_from_value(val: &serde_json::Value) -> Vec<DiscoveredApi> {
        val.as_array().map(|arr| {
            arr.iter().filter_map(|a| {
                Some(DiscoveredApi {
                    method: a["method"].as_str().unwrap_or("GET").to_string(),
                    url: a["url"].as_str()?.to_string(),
                    status: a["status"].as_u64().unwrap_or(0) as u16,
                    content_type: a["contentType"].as_str().unwrap_or("").to_string(),
                    size_bytes: a["sizeBytes"].as_u64().unwrap_or(0),
                })
            }).collect()
        }).unwrap_or_default()
    }

    /// Build a small sample from a large data set for LLM preview.
    /// Returns JSON with schema info + first N rows.
    fn build_data_sample(data: &serde_json::Value, sample_rows: usize) -> serde_json::Value {
        // Find the data array (top-level or nested)
        let (arr, wrapper_keys) = if let Some(arr) = data.as_array() {
            (arr.clone(), vec![])
        } else if let Some(obj) = data.as_object() {
            let data_keys = ["list", "rows", "data", "items", "records", "content"];
            let mut found = None;
            let mut wrapper: Vec<String> = vec![];
            for key in &data_keys {
                if let Some(arr) = obj.get(*key).and_then(|v| v.as_array()) {
                    found = Some(arr.clone());
                    // Collect non-array fields as metadata
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
                None => return data.clone(), // Not a recognizable data structure
            }
        } else {
            return data.clone();
        };

        // Build sample
        let total = arr.len();
        let sample: Vec<serde_json::Value> = arr.into_iter().take(sample_rows).collect();

        // Extract column names from first row
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
            result["_metadata"] = serde_json::Value::String(wrapper_keys.join(", "));
        }

        result
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

    /// "oneshot canceled" typically means a redirect happened during navigation.
    /// The page may still be alive — don't treat this as a fatal connection error.
    fn is_redirect_cancel(err: &str) -> bool {
        err.contains("oneshot canceled")
    }

    fn is_connection_error(err: &str) -> bool {
        // Exclude "oneshot canceled" — that's a redirect, not a dead connection
        if Self::is_redirect_cancel(err) {
            return false;
        }
        err.contains("canceled") || err.contains("receiver is gone") || err.contains("timed out")
    }

    fn detect_login_redirect(original_url: &str, final_url: &str, target_origin: &str) -> bool {
        let final_origin = Self::extract_origin(final_url).unwrap_or_default();
        if final_origin != *target_origin {
            return true;
        }
        // Same-origin: check if path changed to a login/error page
        let target_path = url::Url::parse(original_url).ok().map(|u| u.path().to_string()).unwrap_or_default();
        let final_path = url::Url::parse(final_url).ok().map(|u| u.path().to_string()).unwrap_or_default();
        if final_path != target_path {
            let fp = final_path.to_lowercase();
            return fp.contains("login") || fp.contains("signin") || fp.contains("/sso")
                || fp.contains("/auth") || fp.contains("/cas/")
                || fp.contains("error") || fp.contains("forbidden") || fp.contains("/403")
                || fp.contains("/404") || fp.contains("no_resource") || fp.contains("no_permission");
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

    fn parse_links(result: &serde_json::Value) -> Vec<LinkData> {
        result["links"].as_array().map(|arr| {
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

    /// Read intercepted XHR/fetch calls from window.__aijia_api_calls.
    async fn read_intercepted_apis(page: &Page) -> Vec<DiscoveredApi> {
        let js = r#"(() => {
            var calls = window.__aijia_api_calls || [];
            // Deduplicate by method+url, keep latest status
            var seen = {};
            calls.forEach(function(c) {
                var key = c.method + ' ' + c.url;
                if (!seen[key] || c.status > 0) seen[key] = c;
            });
            return Object.values(seen).filter(function(c) {
                // Skip static assets, only keep API-like calls
                var u = c.url.toLowerCase();
                return !u.match(/\.(js|css|png|jpg|gif|svg|woff|ttf|ico)(\?|$)/) && c.url.length < 500;
            }).slice(0, 30);
        })()"#;

        let result = match page.evaluate(js).await {
            Ok(v) => v.into_value::<serde_json::Value>().unwrap_or(serde_json::Value::Null),
            Err(_) => return vec![],
        };

        result.as_array().map(|arr| {
            arr.iter().filter_map(|a| {
                Some(DiscoveredApi {
                    method: a["method"].as_str().unwrap_or("GET").to_string(),
                    url: a["url"].as_str()?.to_string(),
                    status: a["status"].as_u64().unwrap_or(0) as u16,
                    content_type: a["contentType"].as_str().unwrap_or("").to_string(),
                    size_bytes: a["sizeBytes"].as_u64().unwrap_or(0),
                })
            }).collect()
        }).unwrap_or_default()
    }

    /// Discover all <form> elements on the page.
    async fn discover_forms(page: &Page) -> Vec<FormData> {
        let js = r#"(() => {
            var forms = [];
            document.querySelectorAll('form').forEach(function(f, i) {
                if (i >= 10) return;
                var fields = [];
                f.querySelectorAll('input, select, textarea').forEach(function(el) {
                    var name = el.name || el.id || '';
                    if (!name) return;
                    var ftype = el.type || el.tagName.toLowerCase();
                    var value = el.value || '';
                    if (el.tagName === 'SELECT') {
                        var opts = Array.from(el.options).map(function(o) { return o.value; });
                        value = opts.join(',');
                        ftype = 'select(' + opts.length + ')';
                    }
                    fields.push({name: name, fieldType: ftype, value: value.substring(0, 100)});
                });
                forms.push({
                    id: f.id || ('form_' + i),
                    action: f.action || '',
                    method: (f.method || 'GET').toUpperCase(),
                    fields: fields
                });
            });
            return forms;
        })()"#;

        let result = match page.evaluate(js).await {
            Ok(v) => v.into_value::<serde_json::Value>().unwrap_or(serde_json::Value::Null),
            Err(_) => return vec![],
        };

        result.as_array().map(|arr| {
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

    async fn wait_for_content_stable(&self, page: &Page, timeout_ms: u64) {
        let js = r#"(() => {
            const el = document.querySelector('main, [role="main"], .main-content, #app') || document.body;
            return el ? el.innerHTML.length : 0;
        })()"#;

        let mut last_len: usize = 0;
        let mut stable_count = 0u32;
        let interval = 150u64;
        let max_iters = (timeout_ms / interval) as usize;

        for _ in 0..max_iters {
            tokio::time::sleep(Duration::from_millis(interval)).await;
            let len: usize = page.evaluate(js).await.ok()
                .and_then(|v| v.into_value::<usize>().ok())
                .unwrap_or(0);

            if len == last_len && len > 0 {
                stable_count += 1;
                if stable_count >= 2 { return; }
            } else {
                stable_count = 0;
            }
            last_len = len;
        }
    }
}
