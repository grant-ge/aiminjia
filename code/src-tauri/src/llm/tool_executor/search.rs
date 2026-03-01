//! web_search handler.

use anyhow::{anyhow, Result};
use chrono::Datelike;
use log::info;
use serde_json::Value;

use crate::plugin::context::PluginContext;
use crate::search::bing::BingClient;
use crate::search::bocha::BochaClient;
use crate::search::tavily::TavilyClient;

use super::require_str;

/// 1. web_search — search the web via Bocha (primary, if key configured), Bing (free fallback), or Tavily (paid fallback).
pub(crate) async fn handle_web_search(ctx: &PluginContext, args: &Value) -> Result<String> {
    let raw_query = require_str(args, "query")?;
    let max_results = super::optional_i64(args, "max_results", 5) as u32;

    // Auto-append recent year range if the query doesn't already mention any year
    let has_year = raw_query.chars().collect::<Vec<_>>()
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

    // 1. Try Bocha first (if API key is configured)
    if let Some(api_key) = ctx.bocha_api_key.as_deref() {
        let bocha = BochaClient::new(api_key.to_string());
        match bocha.search(&query, max_results).await {
            Ok(results) if !results.is_empty() => {
                let mut output = String::new();
                for (i, result) in results.iter().enumerate() {
                    output.push_str(&format!(
                        "{}. **{}**\n   URL: {}\n   {}\n\n",
                        i + 1, result.title, result.url, result.summary
                    ));
                }
                return Ok(output);
            }
            Ok(_) => {
                info!("Bocha returned empty results, trying Bing fallback");
            }
            Err(e) => {
                info!("Bocha search failed, trying Bing fallback: {}", e);
            }
        }
    }

    // 2. Try Bing (free, no API key needed)
    let bing = BingClient::new();
    match bing.search(&query, max_results).await {
        Ok(results) if !results.is_empty() => {
            let mut output = String::new();
            for (i, result) in results.iter().enumerate() {
                output.push_str(&format!(
                    "{}. **{}**\n   URL: {}\n   {}\n\n",
                    i + 1, result.title, result.url, result.content
                ));
            }
            return Ok(output);
        }
        Ok(_) => {
            info!("Bing returned empty results, trying Tavily fallback");
        }
        Err(e) => {
            info!("Bing search failed, trying Tavily fallback: {}", e);
        }
    }

    // 3. Fallback: use Tavily if an API key is available
    if let Some(api_key) = ctx.tavily_api_key.as_deref() {
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
                        i + 1, result.title, result.url, result.content
                    ));
                }
                if output.is_empty() {
                    output = "No search results found.".to_string();
                }
                return Ok(output);
            }
            Err(e) => {
                info!("Tavily search also failed: {}", e);
            }
        }
    }

    // All engines failed (or none configured)
    Err(anyhow!("[搜索不可用] 搜索引擎暂时无法访问。请基于已有知识回答，不要编造搜索结果。"))
}
