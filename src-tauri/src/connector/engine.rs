//! ConnectorEngine — Playwright browser automation runtime.
//!
//! Manages PlaywrightBrowser for open browsing mode: navigate any URL,
//! extract data, execute JS, auto-paginate. No pre-configuration needed.

use std::sync::Arc;
use log::info;
use serde_json::Value;
use tokio::sync::RwLock;

use super::playwright_browser::PlaywrightBrowser;
use super::types::{ApiFetchResult, BrowseNavigateResult, BrowseResult, ExecuteJsResult, FullPageResult};

/// Max browser requests per LLM turn.
const MAX_BROWSER_REQUESTS_PER_TURN: u32 = 50;

pub struct ConnectorEngine {
    playwright_browser: RwLock<Option<Arc<PlaywrightBrowser>>>,
}

impl ConnectorEngine {
    pub fn new() -> Self {
        Self {
            playwright_browser: RwLock::new(None),
        }
    }

    /// Inject the PlaywrightBrowser.
    pub async fn set_playwright_browser(&self, pw: Arc<PlaywrightBrowser>) {
        *self.playwright_browser.write().await = Some(pw);
    }

    /// Get a read reference to the Playwright browser (for sub-agent context building).
    pub async fn playwright_browser_ref(&self) -> tokio::sync::RwLockReadGuard<'_, Option<Arc<PlaywrightBrowser>>> {
        self.playwright_browser.read().await
    }

    /// Reset browser request counter (call at start of each LLM turn).
    pub async fn reset_request_counters(&self) {
        let pw = self.playwright_browser.read().await;
        if let Some(ref pw) = *pw {
            pw.reset_counter().await;
        }
    }

    // ── Browser Methods ─────────────────────────────────────────

    pub async fn browser_navigate(&self, url: &str) -> Result<BrowseNavigateResult, String> {
        let pw = self.playwright_browser.read().await;
        let pw = pw.as_ref().ok_or("Playwright browser not initialized")?;
        pw.check_rate_limit(MAX_BROWSER_REQUESTS_PER_TURN).await?;
        info!("[CONNECTOR] browser_navigate: url='{}'", url);
        pw.navigate(url).await
    }

    pub async fn browser_read_content(&self, extract_script: Option<&str>) -> Result<BrowseResult, String> {
        let pw = self.playwright_browser.read().await;
        let pw = pw.as_ref().ok_or("Playwright browser not initialized")?;
        pw.check_rate_limit(MAX_BROWSER_REQUESTS_PER_TURN).await?;
        info!("[CONNECTOR] browser_read_content");
        pw.read_content(extract_script).await
    }

    pub async fn browser_execute_js(&self, script: &str) -> Result<ExecuteJsResult, String> {
        let pw = self.playwright_browser.read().await;
        let pw = pw.as_ref().ok_or("Playwright browser not initialized")?;
        pw.check_rate_limit(MAX_BROWSER_REQUESTS_PER_TURN).await?;
        info!("[CONNECTOR] browser_execute_js");
        pw.execute_js(script).await
    }

    pub async fn browser_show(&self) -> Result<(), String> {
        let pw = self.playwright_browser.read().await;
        let pw = pw.as_ref().ok_or("Playwright browser not initialized")?;
        pw.show_active_page().await
    }

    pub async fn browser_navigate_and_extract(&self, url: &str, extract_script: Option<&str>) -> Result<FullPageResult, String> {
        let pw = self.playwright_browser.read().await;
        let pw = pw.as_ref().ok_or("Playwright browser not initialized")?;
        pw.check_rate_limit(MAX_BROWSER_REQUESTS_PER_TURN).await?;
        info!("[CONNECTOR] browser_navigate_and_extract: url='{}'", url);
        pw.navigate_and_extract(url, extract_script).await
    }

    pub async fn browser_api_fetch(&self, url: &str, method: &str, body: Option<&str>, headers: Option<&str>) -> Result<ApiFetchResult, String> {
        let pw = self.playwright_browser.read().await;
        let pw = pw.as_ref().ok_or("Playwright browser not initialized")?;
        pw.check_rate_limit(MAX_BROWSER_REQUESTS_PER_TURN).await?;
        info!("[CONNECTOR] browser_api_fetch: {} '{}'", method, url);
        pw.api_fetch(url, method, body, headers).await
    }

    pub async fn browser_extract_table_data(&self, save_path: &str, max_pages: Option<u32>, page_size: Option<u32>) -> Result<Value, String> {
        let pw = self.playwright_browser.read().await;
        let pw = pw.as_ref().ok_or("Playwright browser not initialized")?;
        info!("[CONNECTOR] browser_extract_table_data");
        pw.extract_table_data(save_path, max_pages, page_size).await
    }

    pub async fn browser_extract_with_pagination(&self, save_path: &str, pagination_js: &str, max_pages: Option<u32>) -> Result<Value, String> {
        let pw = self.playwright_browser.read().await;
        let pw = pw.as_ref().ok_or("Playwright browser not initialized")?;
        info!("[CONNECTOR] browser_extract_with_pagination");
        pw.extract_with_pagination(save_path, pagination_js, max_pages).await
    }

    pub async fn browser_screenshot(&self) -> Option<std::path::PathBuf> {
        let pw = self.playwright_browser.read().await;
        let pw = pw.as_ref()?;
        pw.capture_screenshot().await
    }

    /// Shutdown browser (call on app exit).
    pub async fn shutdown_cdp(&self) {
        let pw = self.playwright_browser.read().await;
        if let Some(ref pw) = *pw {
            pw.shutdown().await;
        }
    }

    /// Build context string injected into LLM dynamic context.
    pub async fn build_context(&self) -> String {
        "如需从内部业务系统（ERP/OA/CRM 等）提取数据，使用 `browse_data(task, url?)` 工具描述需求，浏览器助手会自动完成。\n".to_string()
    }
}
