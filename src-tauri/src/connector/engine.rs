//! ConnectorEngine — internal system app connector runtime.
//!
//! Talks to Lotus API for config sync, connect/disconnect, and knowledge CRUD.
//! Delegates HTTP proxy requests to WebViewAuthManager (cookie-based auth).
//! V4: Also manages CdpBrowser for open browsing mode (no pre-configuration needed).

use std::collections::HashMap;
use std::sync::Arc;
use log::{info, warn};
use once_cell::sync::Lazy;
use serde_json::Value;
use tokio::sync::RwLock;

use crate::storage::file_store::AppStorage;
use crate::auth::AuthManager;
use super::cdp_browser::CdpBrowser;
use super::credential_store;
use super::types::*;
use super::webview_auth::{BrowseNavigateResult, BrowseResult, ExecuteJsResult, ProxyResponse, WebLoginConfig, WebViewAuthManager};

/// The Lotus API base URL (same as auth client).
const API_BASE_URL: &str = "https://ai-tenant.renlijia.com";

/// Max browser requests per LLM turn (open browsing mode).
/// 50 allows navigate + read_content + execute_js across ~15 pages per turn.
const MAX_BROWSER_REQUESTS_PER_TURN: u32 = 50;

/// Wrapper for Lotus API responses: `{"data": ...}`.
#[derive(Debug, serde::Deserialize)]
struct LotusResponse<T> {
    data: T,
}

static HTTP_CLIENT: Lazy<reqwest::Client> = Lazy::new(|| {
    reqwest::Client::builder()
        .connect_timeout(std::time::Duration::from_secs(15))
        .timeout(std::time::Duration::from_secs(60))
        .build()
        .unwrap_or_else(|_| reqwest::Client::new())
});

pub struct ConnectorEngine {
    storage: Arc<AppStorage>,
    config: RwLock<Option<InternalAppConfig>>,
    auth_manager: RwLock<Option<Arc<AuthManager>>>,
    webview_auth: RwLock<Option<Arc<WebViewAuthManager>>>,
    request_counters: RwLock<HashMap<u64, u32>>,
    /// CDP browser for open browsing mode (V4).
    cdp_browser: RwLock<Option<Arc<CdpBrowser>>>,
}

impl ConnectorEngine {
    pub fn new(storage: Arc<AppStorage>) -> Self {
        Self {
            storage,
            config: RwLock::new(None),
            auth_manager: RwLock::new(None),
            webview_auth: RwLock::new(None),
            request_counters: RwLock::new(HashMap::new()),
            cdp_browser: RwLock::new(None),
        }
    }

    /// Inject the AuthManager after both are created in lib.rs setup.
    pub async fn set_auth_manager(&self, am: Arc<AuthManager>) {
        *self.auth_manager.write().await = Some(am);
    }

    /// Inject the WebViewAuthManager after both are created in lib.rs setup.
    pub async fn set_webview_auth(&self, wam: Arc<WebViewAuthManager>) {
        *self.webview_auth.write().await = Some(wam);
    }

    /// Inject the CdpBrowser after both are created in lib.rs setup.
    pub async fn set_cdp_browser(&self, cdp: Arc<CdpBrowser>) {
        *self.cdp_browser.write().await = Some(cdp);
    }

    /// Get a valid session key from AuthManager, or error if not logged in.
    async fn get_session_key(&self) -> Result<String, String> {
        let am = self.auth_manager.read().await;
        let am = am.as_ref().ok_or("Not logged in — please log in first")?;
        am.get_session_key().await.map_err(|e| format!("Auth error: {}", e))
    }

    /// Load cached config from local storage on startup.
    pub async fn init(&self) {
        if let Ok(Some(config)) = credential_store::load_internal_config(&self.storage) {
            info!("[CONNECTOR] Loaded cached internal-app config: {} apps", config.apps.len());
            *self.config.write().await = Some(config);
        }
    }

    // ── Config sync ─────────────────────────────────────────────────

    /// Fetch internal apps list from Lotus API and persist locally.
    pub async fn sync_config(&self) -> Result<(), String> {
        let session_key = self.get_session_key().await?;

        let url = format!("{}/v1/employee/internal-apps", API_BASE_URL);
        let resp = HTTP_CLIENT
            .get(&url)
            .header("Authorization", format!("Bearer {}", session_key))
            .send()
            .await
            .map_err(|e| format!("Failed to fetch internal apps: {}", e))?;

        if !resp.status().is_success() {
            let status = resp.status().as_u16();
            let body = resp.text().await.unwrap_or_default();
            return Err(format!("Lotus API error ({}): {}", status, body));
        }

        let wrapper: LotusResponse<Vec<InternalAppInfo>> = resp
            .json()
            .await
            .map_err(|e| format!("Failed to parse internal apps response: {}", e))?;

        let config = InternalAppConfig {
            apps: wrapper.data,
            last_synced: Some(chrono::Local::now().format("%Y-%m-%dT%H:%M:%S").to_string()),
        };

        credential_store::save_internal_config(&self.storage, &config)
            .map_err(|e| format!("Failed to save config: {}", e))?;

        info!("[CONNECTOR] Synced internal-app config: {} apps", config.apps.len());
        *self.config.write().await = Some(config);
        Ok(())
    }

    /// Return cached list of apps.
    pub async fn get_apps(&self) -> Vec<InternalAppInfo> {
        let config = self.config.read().await;
        match config.as_ref() {
            Some(c) => c.apps.clone(),
            None => Vec::new(),
        }
    }

    // ── Connect / disconnect ────────────────────────────────────────

    /// Notify Lotus that this employee connected to an app, then open WebView login.
    pub async fn connect(&self, app_id: u64) -> Result<(), String> {
        let app = self.find_app(app_id).await?;

        // Notify Lotus (best-effort, don't block on failure)
        if let Ok(sk) = self.get_session_key().await {
            let url = format!("{}/v1/employee/internal-apps/{}/connect", API_BASE_URL, app_id);
            let _ = HTTP_CLIENT
                .post(&url)
                .header("Authorization", format!("Bearer {}", sk))
                .send()
                .await;
        }

        // Open WebView login
        let wam = self.webview_auth.read().await;
        let wam = wam.as_ref().ok_or("WebViewAuthManager not initialized")?;

        let login_config = WebLoginConfig {
            app_id,
            app_name: app.name.clone(),
            login_url: app.login_url.clone(),
            base_url: app.base_url.clone(),
            success_url_prefix: app.success_url_prefix.clone(),
        };

        wam.open_login(login_config).await?;

        // Update local connected status
        self.set_connected(app_id, true).await;

        info!("[CONNECTOR] Connected to app '{}' (id={})", app.name, app_id);
        Ok(())
    }

    /// Disconnect from an app — notify Lotus and close WebView session.
    pub async fn disconnect(&self, app_id: u64) -> Result<(), String> {
        // Notify Lotus (best-effort)
        if let Ok(sk) = self.get_session_key().await {
            let url = format!("{}/v1/employee/internal-apps/{}/disconnect", API_BASE_URL, app_id);
            let _ = HTTP_CLIENT
                .post(&url)
                .header("Authorization", format!("Bearer {}", sk))
                .send()
                .await;
        }

        // Close WebView session
        let wam = self.webview_auth.read().await;
        if let Some(ref wam) = *wam {
            wam.close_session(app_id).await;
        }

        // Update local connected status
        self.set_connected(app_id, false).await;

        info!("[CONNECTOR] Disconnected app {}", app_id);
        Ok(())
    }

    // ── Proxy request (via WebViewAuthManager) ──────────────────────

    /// Execute an HTTP request through the connected internal system.
    ///
    /// Security enforced:
    /// 1. URL must start with app.base_url (whitelist)
    /// 2. HTTP method must be in app.allowed_methods
    /// 3. URL must not match any app.blocked_paths
    /// 4. Per-turn request counter must not exceed app.max_requests_per_turn
    pub async fn request(
        &self,
        app_id: u64,
        method: &str,
        url: &str,
        headers: Option<&Value>,
        body: Option<&Value>,
    ) -> Result<ProxyResponse, String> {
        let app = self.find_app(app_id).await?;

        // Security: URL whitelist
        if !url.starts_with(&app.base_url) {
            return Err(format!(
                "URL security check failed: '{}' does not start with '{}'",
                url, app.base_url
            ));
        }

        // Security: method whitelist
        let method_upper = method.to_uppercase();
        if !app.allowed_methods.iter().any(|m| m.to_uppercase() == method_upper) {
            return Err(format!(
                "Method '{}' not allowed for app '{}'. Allowed: {:?}",
                method, app.name, app.allowed_methods
            ));
        }

        // Security: blocked paths
        let url_path = url.strip_prefix(&app.base_url).unwrap_or(url);
        for blocked in &app.blocked_paths {
            if url_path.starts_with(blocked) {
                return Err(format!(
                    "Path '{}' is blocked for app '{}'",
                    url_path, app.name
                ));
            }
        }

        // Security: request counter per turn
        {
            let mut counters = self.request_counters.write().await;
            let count = counters.entry(app_id).or_insert(0);
            *count += 1;
            if *count > app.max_requests_per_turn {
                return Err(format!(
                    "Request limit exceeded for app '{}': {} requests per turn (max {})",
                    app.name, *count, app.max_requests_per_turn
                ));
            }
        }

        // Delegate to WebViewAuthManager
        let wam = self.webview_auth.read().await;
        let wam = wam.as_ref().ok_or("WebViewAuthManager not initialized")?;

        wam.proxy_request(app_id, method, url, headers, body, None).await
    }

    /// Reset all per-app request counters and CDP browser counter (call at start of each LLM turn).
    pub async fn reset_request_counters(&self) {
        self.request_counters.write().await.clear();
        // Also reset CDP browser counter
        let cdp = self.cdp_browser.read().await;
        if let Some(ref cdp) = *cdp {
            cdp.reset_counter().await;
        }
    }

    // ── Open Browser Session (V4) ──────────────────────────────────

    /// Navigate the CDP browser to any URL (open browsing mode).
    ///
    /// No URL whitelist, no pre-configuration. Rate limit only.
    pub async fn browser_navigate(&self, url: &str) -> Result<BrowseNavigateResult, String> {
        let cdp = self.cdp_browser.read().await;
        let cdp = cdp.as_ref().ok_or("CDP browser not initialized")?;

        cdp.check_rate_limit(MAX_BROWSER_REQUESTS_PER_TURN).await?;

        info!("[CONNECTOR] browser_navigate: url='{}'", url);
        cdp.navigate(url).await
    }

    /// Read content from the active page in the CDP browser.
    pub async fn browser_read_content(
        &self,
        extract_script: Option<&str>,
    ) -> Result<BrowseResult, String> {
        let cdp = self.cdp_browser.read().await;
        let cdp = cdp.as_ref().ok_or("CDP browser not initialized")?;

        cdp.check_rate_limit(MAX_BROWSER_REQUESTS_PER_TURN).await?;

        info!("[CONNECTOR] browser_read_content");
        cdp.read_content(extract_script).await
    }

    /// Execute JavaScript on the active page in the CDP browser.
    pub async fn browser_execute_js(&self, script: &str) -> Result<ExecuteJsResult, String> {
        let cdp = self.cdp_browser.read().await;
        let cdp = cdp.as_ref().ok_or("CDP browser not initialized")?;

        cdp.check_rate_limit(MAX_BROWSER_REQUESTS_PER_TURN).await?;

        info!("[CONNECTOR] browser_execute_js");
        cdp.execute_js(script).await
    }

    /// Show the active CDP browser tab (bring to front).
    pub async fn browser_show(&self) -> Result<(), String> {
        let cdp = self.cdp_browser.read().await;
        let cdp = cdp.as_ref().ok_or("CDP browser not initialized")?;
        cdp.show_active_page().await
    }

    /// Shutdown the CDP browser (call on app exit).
    pub async fn shutdown_cdp(&self) {
        let cdp = self.cdp_browser.read().await;
        if let Some(ref cdp) = *cdp {
            cdp.shutdown().await;
        }
    }

    // ── Knowledge CRUD ──────────────────────────────────────────────

    /// Save API knowledge discovered by the LLM to Lotus.
    pub async fn save_knowledge(
        &self,
        app_id: u64,
        knowledge: &Value,
    ) -> Result<(), String> {
        let session_key = self.get_session_key().await?;
        let url = format!(
            "{}/v1/employee/internal-apps/{}/knowledge",
            API_BASE_URL, app_id
        );
        let resp = HTTP_CLIENT
            .post(&url)
            .header("Authorization", format!("Bearer {}", session_key))
            .json(knowledge)
            .send()
            .await
            .map_err(|e| format!("Failed to save knowledge: {}", e))?;

        if !resp.status().is_success() {
            let status = resp.status().as_u16();
            let body = resp.text().await.unwrap_or_default();
            return Err(format!("Lotus API error ({}): {}", status, body));
        }

        info!("[CONNECTOR] Saved knowledge for app {}", app_id);
        Ok(())
    }

    /// Get known API knowledge for an app from Lotus.
    pub async fn get_knowledge(&self, app_id: u64) -> Result<Vec<ApiKnowledgeItem>, String> {
        let session_key = self.get_session_key().await?;
        let url = format!(
            "{}/v1/employee/internal-apps/{}/knowledge",
            API_BASE_URL, app_id
        );
        let resp = HTTP_CLIENT
            .get(&url)
            .header("Authorization", format!("Bearer {}", session_key))
            .send()
            .await
            .map_err(|e| format!("Failed to fetch knowledge: {}", e))?;

        if !resp.status().is_success() {
            let status = resp.status().as_u16();
            let body = resp.text().await.unwrap_or_default();
            return Err(format!("Lotus API error ({}): {}", status, body));
        }

        let wrapper: LotusResponse<Vec<ApiKnowledgeItem>> = resp
            .json()
            .await
            .map_err(|e| format!("Failed to parse knowledge response: {}", e))?;

        Ok(wrapper.data)
    }

    // ── LLM context builder ─────────────────────────────────────────

    /// Build context string injected into the LLM system prompt.
    ///
    /// Lists connected apps with their hints and known API knowledge.
    /// Also includes open browsing mode instructions.
    pub async fn build_context(&self) -> String {
        let mut ctx = String::new();

        // Open browsing mode instructions (always available)
        ctx.push_str("## 开放浏览模式\n\n");
        ctx.push_str("你可以使用浏览器直接访问任何内部系统 URL，无需预配置：\n");
        ctx.push_str("1. `browse_navigate(url)` — 打开 Chrome 浏览器导航到指定 URL\n");
        ctx.push_str("2. 如果被重定向到登录页，提示用户在 Chrome 中登录，然后重新导航\n");
        ctx.push_str("3. `read_page_content()` — 提取当前页面的表格和文本数据\n");
        ctx.push_str("4. `page_execute_js(script)` — 在页面上执行 JS（点击、翻页、筛选等）\n");
        ctx.push_str("5. 用户也可以在 Chrome 窗口中手动操作，之后再用 `read_page_content` 读取\n\n");
        ctx.push_str("**大数据量策略（超过 1 页的数据）：**\n");
        ctx.push_str("- 不要逐页翻页读取，效率太低\n");
        ctx.push_str("- 优先用 `page_execute_js` 在页面上执行 fetch() 调用底层 API 批量获取 JSON 数据\n");
        ctx.push_str("- 方法：在 Network 面板观察页面加载数据时调用的 API URL，然后用 fetch() 重放\n");
        ctx.push_str("- 示例：`return await fetch('/api/orders?page=1&size=1000').then(r => r.json())`\n");
        ctx.push_str("- 如果 API 未知，先用 `page_execute_js` 执行 `performance.getEntriesByType('resource').filter(e=>e.initiatorType==='fetch'||e.initiatorType==='xmlhttprequest').map(e=>e.name)` 发现 API 端点\n");
        ctx.push_str("- fetch() 在页面上下文执行，自动携带 cookie，无需额外认证\n\n");

        // Legacy connector apps section
        let config = self.config.read().await;
        let config = match config.as_ref() {
            Some(c) => c,
            None => return ctx,
        };

        let active_apps: Vec<&InternalAppInfo> = config.apps.iter()
            .filter(|a| a.status == "active")
            .collect();

        if active_apps.is_empty() {
            return ctx;
        }

        ctx.push_str("## 已配置的内部系统\n\n");

        for app in &active_apps {
            ctx.push_str(&format!("### {}  {}\n\n", app.name,
                if app.connected { "✅ 已连接" } else { "❌ 未连接" }));

            if !app.description.is_empty() {
                ctx.push_str(&format!("> {}\n\n", app.description));
            }

            if !app.connected {
                ctx.push_str("用户需要先连接此系统才能使用 http_request/save_api_knowledge。\n\n");
                continue;
            }

            // Hints
            if !app.hints.is_empty() {
                ctx.push_str("**功能:**\n");
                for hint in &app.hints {
                    ctx.push_str(&format!("- {}: {}", hint.name, hint.description));
                    if let Some(ref clues) = hint.clues {
                        ctx.push_str(&format!(" [线索: {}]", clues));
                    }
                    ctx.push('\n');
                }
                ctx.push('\n');
            }

            // Fetch knowledge (best-effort, don't fail the whole context)
            {
                match self.get_knowledge(app.id).await {
                    Ok(items) if !items.is_empty() => {
                        ctx.push_str("**已知 API (历史验证过的):**\n");
                        for k in &items {
                            ctx.push_str(&format!("- {} {} — {}", k.method, k.path, k.name));
                            if let Some(ref params) = k.params_doc {
                                ctx.push_str(&format!(" (参数: {})", params));
                            }
                            ctx.push('\n');
                        }
                        ctx.push('\n');
                    }
                    Ok(_) => {} // no knowledge yet
                    Err(e) => {
                        warn!("[CONNECTOR] Failed to fetch knowledge for app {}: {}", app.id, e);
                    }
                }
            }

            ctx.push('\n');
        }

        // Add strategy instructions for connected apps
        let has_connected = active_apps.iter().any(|a| a.connected);
        if has_connected {
            ctx.push_str("**取数策略:**\n");
            ctx.push_str("1. 优先使用\"已知 API\"，直接调用 http_request\n");
            ctx.push_str("2. 如果没有匹配的已知 API，根据功能线索推断新路径\n");
            ctx.push_str("3. 尝试 http_request 获取 JSON 数据\n");
            ctx.push_str("4. 如果返回 HTML，改用 browse_navigate 打开页面，然后 read_page_content 提取数据\n");
            ctx.push_str("5. 最多尝试 5 次，未果则请用户提供 API URL\n");
            ctx.push_str("6. **重要：每次成功获取到数据后，必须立刻调用 save_api_knowledge 保存该 API 的调用方法**\n");
        }

        ctx
    }

    // ── Internals ───────────────────────────────────────────────────

    async fn find_app(&self, app_id: u64) -> Result<InternalAppInfo, String> {
        let config = self.config.read().await;
        let config = config.as_ref().ok_or("Internal app config not loaded")?;
        config
            .apps
            .iter()
            .find(|a| a.id == app_id)
            .cloned()
            .ok_or_else(|| format!("App with id {} not found", app_id))
    }

    async fn set_connected(&self, app_id: u64, connected: bool) {
        let mut config = self.config.write().await;
        if let Some(ref mut c) = *config {
            if let Some(app) = c.apps.iter_mut().find(|a| a.id == app_id) {
                app.connected = connected;
            }
            let _ = credential_store::save_internal_config(&self.storage, c);
        }
    }
}
