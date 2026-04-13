//! Web search as RuntimeTool.
//!
//! `WebSearchRuntimeTool` does NOT accept a `PluginContext`.  All search
//! dependencies are injected at construction time via `SearchDeps`.

use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;
use chrono::Datelike;
use log::info;
use once_cell::sync::Lazy;
use reqwest::Client;
use serde_json::Value;

use crate::auth::AuthManager;
use crate::runtime::tools::catalog::TOOL_CATALOG;
use crate::runtime::tools::context::ToolExecutionContext;
use crate::runtime::tools::definition::ToolDefinition;
use crate::runtime::tools::executor::{ToolError, ToolResult};
use crate::runtime::tools::RuntimeTool;
use crate::search::bing::BingClient;
use crate::search::bocha::BochaClient;
use crate::search::tavily::TavilyClient;

// ── SearchDeps ───────────────────────────────────────────────────────────────

/// Narrow search dependencies — injected at construction, not from CapabilityContext.
///
/// Only the fields that `web_search` actually needs are present here.
/// `CapabilityContext` is intentionally NOT extended with these fields.
pub struct SearchDeps {
    pub tavily_api_key: Option<String>,
    pub bocha_api_key: Option<String>,
    pub use_cloud: bool,
    pub auth_manager: Option<Arc<AuthManager>>,
}

// ── WebSearchRuntimeTool ─────────────────────────────────────────────────────

pub struct WebSearchRuntimeTool {
    deps: Arc<SearchDeps>,
}

impl WebSearchRuntimeTool {
    pub fn new(deps: SearchDeps) -> Self {
        Self {
            deps: Arc::new(deps),
        }
    }
}

#[async_trait]
impl RuntimeTool for WebSearchRuntimeTool {
    fn definition(&self) -> ToolDefinition {
        TOOL_CATALOG
            .get("web_search")
            .cloned()
            .unwrap_or_else(|| ToolDefinition::new("web_search", "搜索互联网获取最新信息"))
    }

    async fn execute(&self, input: Value, _ctx: ToolExecutionContext) -> Result<ToolResult, ToolError> {
        let result = execute_web_search(&self.deps, &input)
            .await
            .map_err(ToolError::Other)?;

        Ok(ToolResult {
            tool_name: "web_search".to_string(),
            content: result.clone(),
            data: Some(Value::String(result)),
        })
    }
}

// ── Search logic — no PluginContext dependency ───────────────────────────────

/// Shared HTTP client for cloud search (connection pool reuse).
static CLOUD_SEARCH_CLIENT: Lazy<Client> = Lazy::new(|| {
    Client::builder()
        .connect_timeout(std::time::Duration::from_secs(15))
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .unwrap_or_else(|_| Client::new())
});

async fn execute_web_search(deps: &SearchDeps, args: &Value) -> Result<String> {
    let raw_query = args
        .get("query")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow::anyhow!("Missing required argument: query"))?;
    let max_results = args
        .get("max_results")
        .and_then(Value::as_i64)
        .unwrap_or(5) as u32;

    // Auto-append recent year range if the query doesn't already mention any year.
    let has_year = raw_query
        .chars()
        .collect::<Vec<_>>()
        .windows(4)
        .any(|w| {
            if let Ok(n) = w.iter().collect::<String>().parse::<u32>() {
                (2020..=2030).contains(&n)
            } else {
                false
            }
        });
    let now = chrono::Local::now();
    let this_year = now.format("%Y");
    let last_year = now.year() - 1;
    let query = if has_year {
        raw_query.to_string()
    } else {
        format!("{} {}-{}", raw_query, last_year, this_year)
    };

    // 0. Cloud search (if use_cloud enabled and logged in).
    if deps.use_cloud {
        if let Some(ref auth_mgr) = deps.auth_manager {
            if auth_mgr.is_logged_in().await {
                match cloud_search(auth_mgr, &query, max_results).await {
                    Ok(output) if !output.is_empty() => return Ok(output),
                    Ok(_) => info!("[web_search] Cloud search returned empty, trying local fallback"),
                    Err(e) => info!("[web_search] Cloud search failed: {}, trying local fallback", e),
                }
            }
        }
    }

    // 1. Try Bocha first (if API key is configured).
    if let Some(api_key) = deps.bocha_api_key.as_deref() {
        let bocha = BochaClient::new(api_key.to_string());
        match bocha.search(&query, max_results).await {
            Ok(results) if !results.is_empty() => {
                let mut output = String::new();
                for (i, result) in results.iter().enumerate() {
                    output.push_str(&format!(
                        "{}. **{}**\n   URL: {}\n   {}\n\n",
                        i + 1,
                        result.title,
                        result.url,
                        result.summary
                    ));
                }
                return Ok(output);
            }
            Ok(_) => info!("[web_search] Bocha returned empty, trying Bing fallback"),
            Err(e) => info!("[web_search] Bocha failed: {}, trying Bing fallback", e),
        }
    }

    // 2. Try Bing (free, no API key needed).
    let bing = BingClient::new();
    match bing.search(&query, max_results).await {
        Ok(results) if !results.is_empty() => {
            let mut output = String::new();
            for (i, result) in results.iter().enumerate() {
                output.push_str(&format!(
                    "{}. **{}**\n   URL: {}\n   {}\n\n",
                    i + 1,
                    result.title,
                    result.url,
                    result.content
                ));
            }
            return Ok(output);
        }
        Ok(_) => info!("[web_search] Bing returned empty, trying Tavily fallback"),
        Err(e) => info!("[web_search] Bing failed: {}, trying Tavily fallback", e),
    }

    // 3. Fallback: use Tavily if an API key is available.
    if let Some(api_key) = deps.tavily_api_key.as_deref() {
        let tavily = TavilyClient::new(api_key.to_string());
        match tavily.search(&query, true, max_results).await {
            Ok(response) => {
                let mut output = String::new();
                if let Some(answer) = &response.answer {
                    output.push_str(&format!("**Summary:** {}\n\n", answer));
                }
                for (i, result) in response.results.iter().enumerate() {
                    output.push_str(&format!(
                        "{}. **{}**\n   URL: {}\n   {}\n\n",
                        i + 1,
                        result.title,
                        result.url,
                        result.content
                    ));
                }
                if output.is_empty() {
                    output = "No search results found.".to_string();
                }
                return Ok(output);
            }
            Err(e) => info!("[web_search] Tavily also failed: {}", e),
        }
    }

    Err(anyhow::anyhow!(
        "[搜索不可用] 搜索引擎暂时无法访问。请基于已有知识回答，不要编造搜索结果。"
    ))
}

/// Cloud search via Lotus /v1/search endpoint.
async fn cloud_search(
    auth_mgr: &Arc<AuthManager>,
    query: &str,
    max_results: u32,
) -> Result<String> {
    let session_key = auth_mgr.get_session_key().await?;
    let url = "https://ai-tenant.renlijia.com/v1/search";
    let resp = CLOUD_SEARCH_CLIENT
        .post(url)
        .header("Authorization", format!("Bearer {}", session_key))
        .json(&serde_json::json!({
            "query": query,
            "max_results": max_results,
        }))
        .send()
        .await?;

    let status = resp.status();
    if !status.is_success() {
        let body = resp.text().await.unwrap_or_default();
        if let Ok(json) = serde_json::from_str::<Value>(&body) {
            if let Some(msg) = json["error"]["message"].as_str() {
                return Err(anyhow::anyhow!("云端搜索失败: {}", msg));
            }
        }
        return Err(anyhow::anyhow!("云端搜索失败 ({})", status.as_u16()));
    }

    let body: Value = resp.json().await?;
    let mut output = String::new();
    if let Some(results) = body["results"].as_array() {
        for (i, result) in results.iter().enumerate() {
            let title = result["title"].as_str().unwrap_or("");
            let url = result["url"].as_str().unwrap_or("");
            let content = result["content"].as_str().unwrap_or("");
            output.push_str(&format!(
                "{}. **{}**\n   URL: {}\n   {}\n\n",
                i + 1,
                title,
                url,
                content
            ));
        }
    }
    Ok(output)
}
