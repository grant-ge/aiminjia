//! WebView-based authentication for enterprise internal systems.
//!
//! Opens the real login page in a Tauri WebView. User authenticates by any
//! method (password, QR code, SSO). After login:
//!   - Cookies (including HttpOnly) are read via Tauri's native `cookies_for_url()`
//!   - JWT tokens in localStorage are captured via `initialization_script()`
//!   - Login success is detected via `on_navigation()` URL matching
//!
//! The WebView is kept alive (hidden) for proxying subsequent API requests,
//! where `fetch()` in the WebView context automatically attaches all cookies.
//!
//! ## V4 — Browser browsing moved to CdpBrowser
//!
//! Browsing is now handled directly by CdpBrowser (open mode, no cookie injection).
//! This module only handles WebView login + HTTP proxy for the legacy connector flow.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use log::info;
use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager, WebviewUrl, WebviewWindowBuilder};
use tokio::sync::{Mutex, watch};

/// Credential captured from a WebView login session.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapturedCredential {
    /// All cookies for the domain (including HttpOnly), read via Tauri native API.
    pub cookies: Vec<CookieEntry>,
    /// JWT / access token found in localStorage/sessionStorage.
    pub token: Option<String>,
    /// Key name where the token was found.
    pub token_key: Option<String>,
    /// Timestamp when credential was captured.
    pub captured_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CookieEntry {
    pub name: String,
    pub value: String,
    pub domain: Option<String>,
    pub path: Option<String>,
    pub http_only: bool,
    pub secure: bool,
}

/// Configuration for a WebView login flow.
#[derive(Debug, Clone)]
pub struct WebLoginConfig {
    pub app_id: u64,
    pub app_name: String,
    pub login_url: String,
    pub base_url: String,
    pub success_url_prefix: String,
}

/// Manages WebView-based auth sessions for legacy connector flow.
pub struct WebViewAuthManager {
    app_handle: AppHandle,
    active_views: Mutex<std::collections::HashMap<u64, ActiveWebView>>,
    login_signals: Mutex<std::collections::HashMap<u64, Arc<watch::Sender<Option<String>>>>>,
    /// Cached credentials keyed by app_id, used for reqwest proxy.
    cached_creds: Mutex<std::collections::HashMap<u64, CapturedCredential>>,
}

struct ActiveWebView {
    label: String,
    #[allow(dead_code)]
    base_url: String,
    #[allow(dead_code)]
    connected_at: i64,
}

const TITLE_BRIDGE_PREFIX: &str = "___AIJIA_BRIDGE___";

static HTTP_CLIENT: Lazy<reqwest::Client> = Lazy::new(|| {
    reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(15))
        .timeout(Duration::from_secs(60))
        .redirect(reqwest::redirect::Policy::limited(10))
        .build()
        .unwrap_or_else(|_| reqwest::Client::new())
});

impl WebViewAuthManager {
    pub fn new(app_handle: AppHandle) -> Self {
        Self {
            app_handle,
            active_views: Mutex::new(std::collections::HashMap::new()),
            login_signals: Mutex::new(std::collections::HashMap::new()),
            cached_creds: Mutex::new(std::collections::HashMap::new()),
        }
    }

    /// Open a WebView window for the user to log in.
    ///
    /// Strategy:
    /// - `on_navigation()` detects URL change to success_url_prefix
    /// - `cookies_for_url()` reads all cookies (including HttpOnly) from Rust
    /// - `initialization_script()` scans localStorage for JWT tokens on page load
    pub async fn open_login(
        &self,
        config: WebLoginConfig,
    ) -> Result<CapturedCredential, String> {
        let label = format!("internal-app-{}", config.app_id);
        self.close_webview(&label).await;

        info!(
            "[WEBVIEW_AUTH] Opening login for '{}': {}",
            config.app_name, config.login_url
        );

        let (tx, mut rx) = watch::channel::<Option<String>>(None);
        let tx = Arc::new(tx);
        {
            let mut signals = self.login_signals.lock().await;
            signals.insert(config.app_id, tx.clone());
        }

        // JS init script: scans localStorage/sessionStorage for JWT tokens
        // and writes result to document.title. Runs on every page load.
        let init_js = build_token_scan_script(&config.success_url_prefix);

        let success_prefix = config.success_url_prefix.clone();
        let tx_nav = tx.clone();

        let _webview = WebviewWindowBuilder::new(
            &self.app_handle,
            &label,
            WebviewUrl::External(
                config.login_url.parse().map_err(|e| format!("Invalid URL: {}", e))?,
            ),
        )
        .title(&format!("登录 — {}", config.app_name))
        .inner_size(1000.0, 700.0)
        .center()
        .focused(true)
        .visible(true)
        .initialization_script(&init_js)
        .on_navigation(move |url| {
            let url_str = url.to_string();
            info!("[WEBVIEW_AUTH] Navigation: {}", url_str);
            if url_str.starts_with(&success_prefix) {
                info!("[WEBVIEW_AUTH] Success URL detected!");
                let _ = tx_nav.send(Some(url_str));
            }
            true
        })
        .build()
        .map_err(|e| format!("Failed to create WebView: {}", e))?;

        // Wait for login success
        let timeout = Duration::from_secs(300);
        let poll_interval = Duration::from_millis(500);
        let start = std::time::Instant::now();

        loop {
            tokio::time::sleep(poll_interval).await;

            let win = self.app_handle.get_webview_window(&label);
            if win.is_none() {
                self.cleanup_signal(config.app_id).await;
                return Err("Login window was closed".to_string());
            }
            let win = win.unwrap();

            // Check if on_navigation detected success
            if rx.has_changed().unwrap_or(false) {
                let val = rx.borrow_and_update().clone();
                if val.is_some() {
                    info!("[WEBVIEW_AUTH] Login success detected, waiting for page to settle...");
                    // Wait for the page to fully load and SPA to store tokens
                    tokio::time::sleep(Duration::from_millis(3000)).await;

                    // === Read cookies via Tauri native API (includes HttpOnly!) ===
                    let cookie_url: tauri::Url = config.base_url.parse().unwrap_or_else(|_| {
                        config.login_url.parse().unwrap()
                    });
                    let cookies = match win.cookies_for_url(cookie_url) {
                        Ok(cookie_list) => {
                            info!("[WEBVIEW_AUTH] Read {} cookies via native API", cookie_list.len());
                            let entries: Vec<CookieEntry> = cookie_list
                                .iter()
                                .map(|c| CookieEntry {
                                    name: c.name().to_string(),
                                    value: c.value().to_string(),
                                    domain: c.domain().map(|d| d.to_string()),
                                    path: c.path().map(|p| p.to_string()),
                                    http_only: c.http_only().unwrap_or(false),
                                    secure: c.secure().unwrap_or(false),
                                })
                                .collect();
                            for e in &entries {
                                info!("[WEBVIEW_AUTH]   cookie: {}={} (domain={:?}, path={:?}, httpOnly={}, secure={})",
                                    e.name, &e.value[..e.value.len().min(30)],
                                    e.domain, e.path, e.http_only, e.secure);
                            }
                            entries
                        }
                        Err(e) => {
                            info!("[WEBVIEW_AUTH] cookies_for_url failed: {}, continuing without", e);
                            vec![]
                        }
                    };

                    // === Read JWT token from title bridge (set by initialization_script) ===
                    let title = win.title().unwrap_or_default();
                    let (token, token_key) = if title.starts_with(TITLE_BRIDGE_PREFIX) {
                        let json_str = &title[TITLE_BRIDGE_PREFIX.len()..];
                        if let Ok(v) = serde_json::from_str::<serde_json::Value>(json_str) {
                            let t = v["token"].as_str().map(|s| s.to_string());
                            let k = v["token_key"].as_str().map(|s| s.to_string());
                            info!("[WEBVIEW_AUTH] Token from init_script: key={:?}", k);
                            (t, k)
                        } else {
                            (None, None)
                        }
                    } else {
                        info!("[WEBVIEW_AUTH] No token in title (init_script may not have run)");
                        (None, None)
                    };

                    let cred = CapturedCredential {
                        cookies,
                        token,
                        token_key,
                        captured_at: chrono::Utc::now().timestamp(),
                    };

                    info!(
                        "[WEBVIEW_AUTH] Credentials captured: cookies={}, token={}, token_key={:?}",
                        cred.cookies.len(),
                        cred.token.as_ref().map(|t| format!("{}...({} chars)", &t[..t.len().min(20)], t.len())).unwrap_or("None".into()),
                        cred.token_key
                    );

                    // Hide WebView (keep alive for proxy requests)
                    let _ = win.hide();

                    {
                        let mut views = self.active_views.lock().await;
                        views.insert(config.app_id, ActiveWebView {
                            label: label.clone(),
                            base_url: config.base_url.clone(),
                            connected_at: chrono::Utc::now().timestamp(),
                        });
                    }

                    // Cache credential for reqwest proxy
                    {
                        let mut creds = self.cached_creds.lock().await;
                        creds.insert(config.app_id, cred.clone());
                    }

                    self.cleanup_signal(config.app_id).await;
                    let _ = self.app_handle.emit("connector:login-success", &config.app_name);

                    info!(
                        "[WEBVIEW_AUTH] Login complete for '{}': {} cookies, token={}",
                        config.app_name, cred.cookies.len(), cred.token.is_some()
                    );

                    return Ok(cred);
                }
            }

            if start.elapsed() > timeout {
                let _ = win.close();
                self.cleanup_signal(config.app_id).await;
                return Err("Login timed out (5 minutes)".to_string());
            }
        }
    }

    /// Execute an HTTP request using reqwest with captured cookies.
    ///
    /// Cookies (including HttpOnly) are injected via Cookie header.
    /// JWT token is injected via Authorization header if available.
    pub async fn proxy_request(
        &self,
        app_id: u64,
        method: &str,
        url: &str,
        headers: Option<&serde_json::Value>,
        body: Option<&serde_json::Value>,
        cached_token: Option<&str>,
    ) -> Result<ProxyResponse, String> {
        let creds = self.cached_creds.lock().await;
        let cred = creds
            .get(&app_id)
            .ok_or("No credential for this app. Please login first.")?;

        // Build Cookie header from captured cookies
        let cookie_str: String = cred.cookies.iter()
            .map(|c| format!("{}={}", c.name, c.value))
            .collect::<Vec<_>>()
            .join("; ");

        // Build request
        let mut req = match method.to_uppercase().as_str() {
            "GET" => HTTP_CLIENT.get(url),
            "POST" => HTTP_CLIENT.post(url),
            "PUT" => HTTP_CLIENT.put(url),
            "DELETE" => HTTP_CLIENT.delete(url),
            "PATCH" => HTTP_CLIENT.patch(url),
            _ => return Err(format!("Unsupported method: {}", method)),
        };

        // Inject cookies
        if !cookie_str.is_empty() {
            req = req.header("Cookie", &cookie_str);
        }

        // Add standard browser headers (many SSR apps check these for CSRF/validity)
        req = req
            .header("User-Agent", "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/131.0.0.0 Safari/537.36")
            .header("Accept", "application/json, text/plain, */*")
            .header("Accept-Language", "zh-CN,zh;q=0.9,en;q=0.8")
            .header("Referer", url)
            .header("X-Requested-With", "XMLHttpRequest");

        // Inject JWT token if available
        let token = cached_token
            .map(|t| t.to_string())
            .or_else(|| cred.token.clone());
        if let Some(ref t) = token {
            let auth_val = if t.starts_with("Bearer ") { t.clone() } else { format!("Bearer {}", t) };
            req = req.header("Authorization", auth_val);
        }

        // Add custom headers
        if let Some(h) = headers {
            if let Some(obj) = h.as_object() {
                for (k, v) in obj {
                    if let Some(val) = v.as_str() {
                        req = req.header(k.as_str(), val);
                    }
                }
            }
        }

        // Add body for POST/PUT/PATCH
        if matches!(method.to_uppercase().as_str(), "POST" | "PUT" | "PATCH") {
            if let Some(b) = body {
                req = req.json(b);
            }
        }

        info!("[WEBVIEW_AUTH] Proxy {} {} (cookies: {} bytes, has_token: {})",
            method, url, cookie_str.len(), token.is_some());

        let resp = req.send().await
            .map_err(|e| format!("Request failed: {}", e))?;

        let status = resp.status().as_u16();
        let body_text = resp.text().await.unwrap_or_default();
        let truncated = body_text.len() > 8000;
        let body_out = if truncated { body_text[..8000].to_string() } else { body_text };

        Ok(ProxyResponse {
            status,
            body: body_out,
            truncated,
            error: None,
        })
    }

    // ── Session lifecycle ───────────────────────────────────────

    pub async fn has_active_session(&self, app_id: u64) -> bool {
        let views = self.active_views.lock().await;
        if let Some(view) = views.get(&app_id) {
            self.app_handle.get_webview_window(&view.label).is_some()
        } else {
            false
        }
    }

    pub async fn close_session(&self, app_id: u64) {
        let mut views = self.active_views.lock().await;
        if let Some(view) = views.remove(&app_id) {
            if let Some(win) = self.app_handle.get_webview_window(&view.label) {
                let _ = win.close();
            }
        }
        // Emit closed event for frontend
        let _ = self.app_handle.emit("browser:closed", serde_json::json!({
            "appId": app_id,
        }));
        self.cleanup_signal(app_id).await;
    }

    // ── Private helpers ─────────────────────────────────────────

    async fn close_webview(&self, label: &str) {
        if let Some(win) = self.app_handle.get_webview_window(label) {
            let _ = win.close();
            tokio::time::sleep(Duration::from_millis(200)).await;
        }
    }

    async fn cleanup_signal(&self, app_id: u64) {
        let mut signals = self.login_signals.lock().await;
        signals.remove(&app_id);
    }
}

/// Response from a proxied HTTP request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProxyResponse {
    pub status: u16,
    pub body: String,
    #[serde(default)]
    pub truncated: bool,
    pub error: Option<String>,
}

/// Result from navigate() — URL, title, and redirect detection.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BrowseNavigateResult {
    pub url: String,
    pub title: String,
    /// True if the final URL's origin differs from the target (e.g. redirected to login page).
    #[serde(default)]
    pub redirected_to_login: bool,
}

/// Result from browsing a page and extracting DOM data.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BrowseResult {
    pub url: String,
    pub title: String,
    pub tables: Vec<TableData>,
    pub text: String,
}

/// Extracted table data from a page.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TableData {
    pub headers: Vec<String>,
    pub rows: Vec<HashMap<String, String>>,
}

/// Result from execute_js().
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecuteJsResult {
    pub value: serde_json::Value,
    pub error: Option<String>,
    pub new_url: Option<String>,
    pub new_title: Option<String>,
}

/// JS that runs on every page load via initialization_script.
/// On success pages: scans localStorage/sessionStorage for tokens, writes to title.
fn build_token_scan_script(success_url_prefix: &str) -> String {
    format!(
        r#"(function() {{
    var PREFIX = '{prefix}';
    var BRIDGE = '{bridge}';

    if (window.location.href.indexOf(PREFIX) !== 0) return;

    console.log('[AIJIA] Success page detected, scanning for tokens...');

    // Delay to let SPA finish storing tokens
    setTimeout(function() {{
        var KEYS = [
            'token','access_token','accessToken','auth_token','authToken',
            'jwt','Authorization','x-token','X-Token','admin-token'
        ];
        var OBJ_KEYS = ['user','userInfo','auth','pinia','vuex','persist','loginInfo'];

        var foundToken = null;
        var foundKey = null;

        function scan(store, name) {{
            if (!store || foundToken) return;
            for (var i = 0; i < KEYS.length; i++) {{
                try {{
                    var v = store.getItem(KEYS[i]);
                    if (v && v.length > 20 && !v.startsWith('{{{{')) {{
                        foundToken = v; foundKey = name + ':' + KEYS[i]; return;
                    }}
                }} catch(e) {{}}
            }}
            for (var j = 0; j < OBJ_KEYS.length; j++) {{
                try {{
                    var raw = store.getItem(OBJ_KEYS[j]);
                    if (!raw) continue;
                    var t = dig(JSON.parse(raw), 0);
                    if (t) {{ foundToken = t; foundKey = name + ':' + OBJ_KEYS[j]; return; }}
                }} catch(e) {{}}
            }}
            // Full scan: look for JWT-shaped values
            try {{
                for (var k = 0; k < store.length; k++) {{
                    var key = store.key(k);
                    var val = store.getItem(key);
                    if (val && /^[A-Za-z0-9\-_]+\.[A-Za-z0-9\-_]+\.[A-Za-z0-9\-_]+$/.test(val)) {{
                        foundToken = val; foundKey = name + ':' + key; return;
                    }}
                    if (val && val.length > 20 && val.length < 4096) {{
                        try {{
                            var t2 = dig(JSON.parse(val), 0);
                            if (t2) {{ foundToken = t2; foundKey = name + ':' + key; return; }}
                        }} catch(e2) {{}}
                    }}
                }}
            }} catch(e) {{}}
        }}

        function dig(obj, d) {{
            if (d > 4 || !obj || typeof obj !== 'object') return null;
            var keys = Object.keys(obj);
            for (var i = 0; i < keys.length; i++) {{
                var k = keys[i], v = obj[k];
                if (/token|jwt|auth/i.test(k) && typeof v === 'string' && v.length > 20) return v;
                var f = dig(v, d + 1);
                if (f) return f;
            }}
            return null;
        }}

        scan(localStorage, 'localStorage');
        if (!foundToken) scan(sessionStorage, 'sessionStorage');

        console.log('[AIJIA] Token scan result:', foundKey || 'none');
        document.title = BRIDGE + JSON.stringify({{
            token: foundToken,
            token_key: foundKey
        }});
    }}, 1500);
}})();"#,
        prefix = success_url_prefix,
        bridge = TITLE_BRIDGE_PREFIX,
    )
}
