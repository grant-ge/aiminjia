//! web_search handler.

use anyhow::{anyhow, Result};
use chrono::Datelike;
use log::info;
use once_cell::sync::Lazy;
use serde_json::Value;

use crate::plugin::context::PluginContext;
use crate::search::bing::BingClient;

use super::require_str;

/// 1. web_search — search the web via the cloud gateway (when logged in),
/// falling back to a keyless Bing scrape. Local API-key providers
/// (Bocha/Tavily) were removed from the product.
pub(crate) async fn handle_web_search(ctx: &PluginContext, args: &Value) -> Result<String> {
    let raw_query = require_str(args, "query")?;
    let max_results = super::optional_i64(args, "max_results", 5) as u32;
    execute_web_search_core(raw_query, max_results, ctx.auth_manager.as_ref()).await
}

/// Core web search implementation — does not require PluginContext.
/// Called by both the legacy handler and the new RuntimeTool path.
pub(crate) async fn execute_web_search_core(
    raw_query: &str,
    max_results: u32,
    auth_manager: Option<&std::sync::Arc<crate::auth::AuthManager>>,
) -> Result<String> {
    // Auto-append recent year range if the query doesn't already mention any year
    let has_year = raw_query.chars().collect::<Vec<_>>().windows(4).any(|w| {
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

    // 0. Cloud search — the product's primary path. Local API-key providers
    //    (Bocha/Tavily) were removed; only the keyless Bing scrape remains as a
    //    fallback when cloud search is unavailable.
    if let Some(auth_mgr) = auth_manager {
        if auth_mgr.is_logged_in().await {
            match cloud_search(auth_mgr, &query, max_results).await {
                Ok(output) if !output.is_empty() => return Ok(output),
                Ok(_) => info!("Cloud search returned empty results, trying Bing fallback"),
                Err(e) => info!("Cloud search failed, trying Bing fallback: {}", e),
            }
        }
    }

    // 1. Bing fallback (free, no API key needed).
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
        Ok(_) => {
            info!("Bing returned empty results");
        }
        Err(e) => {
            info!("Bing search failed: {}", e);
        }
    }

    // All engines failed (or none reachable)
    Err(anyhow!(
        "[搜索不可用] 搜索引擎暂时无法访问。请基于已有知识回答，不要编造搜索结果。"
    ))
}

/// Cloud search via Lotus /v1/search endpoint.
/// Shared HTTP client for cloud search (connection pool reuse).
static CLOUD_SEARCH_CLIENT: Lazy<reqwest::Client> = Lazy::new(|| {
    reqwest::Client::builder()
        .connect_timeout(std::time::Duration::from_secs(15))
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .unwrap_or_else(|_| reqwest::Client::new())
});

async fn cloud_search(
    auth_mgr: &std::sync::Arc<crate::auth::AuthManager>,
    query: &str,
    max_results: u32,
) -> Result<String> {
    let session_key = auth_mgr.get_session_key().await?;

    let url = format!("{}/v1/search", crate::environment::tenant_host());
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
        // Try to extract error message from JSON response
        if let Ok(json) = serde_json::from_str::<Value>(&body) {
            if let Some(msg) = json["error"]["message"].as_str() {
                return Err(anyhow!("云端搜索失败: {}", msg));
            }
        }
        return Err(anyhow!("云端搜索失败 ({})", status.as_u16()));
    }

    let body: Value = resp.json().await?;

    // 服务端直接转发博查（Bocha）响应：
    //   { code, msg, log_id, data: { webPages: { value: [{ name, url, summary, snippet, siteName }] } } }
    // 兼容老结构 { results: [...] }，先按博查路径解析，找不到再回退老路径。
    let mut output = String::new();
    let bocha_array = body
        .get("data")
        .and_then(|d| d.get("webPages"))
        .and_then(|wp| wp.get("value"))
        .and_then(|v| v.as_array());

    if let Some(results) = bocha_array {
        for (i, item) in results.iter().enumerate() {
            let title = item.get("name").and_then(|v| v.as_str()).unwrap_or("");
            let url = item.get("url").and_then(|v| v.as_str()).unwrap_or("");
            let content = item
                .get("summary")
                .and_then(|v| v.as_str())
                .filter(|s| !s.is_empty())
                .or_else(|| item.get("snippet").and_then(|v| v.as_str()))
                .unwrap_or("");
            if title.is_empty() && url.is_empty() {
                continue;
            }
            output.push_str(&format!(
                "{}. **{}**\n   URL: {}\n   {}\n\n",
                i + 1,
                title,
                url,
                content
            ));
        }
    } else if let Some(results) = body["results"].as_array() {
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
    } else {
        let keys: Vec<&str> = body
            .as_object()
            .map(|m| m.keys().map(|s| s.as_str()).collect())
            .unwrap_or_default();
        log::warn!(
            "[cloud_search] response missing `results` array. top-level keys={:?}, body_preview={}",
            keys,
            serde_json::to_string(&body)
                .unwrap_or_default()
                .chars()
                .take(500)
                .collect::<String>()
        );
    }

    Ok(output)
}
