//! Bocha Web Search API client.
//!
//! Bocha is a Chinese search engine optimized for AI applications.
//! Provides structured search results with AI-generated summaries.
//! API docs: https://open.bochaai.com
//!
//! Free tier: 1000 calls. Paid: 3.6 RMB / 1000 calls.

use anyhow::{anyhow, Result};
use log::info;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::json;

const BOCHA_API_URL: &str = "https://api.bochaai.com/v1/web-search";

/// A single search result from Bocha.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BochaResult {
    pub title: String,
    pub url: String,
    pub summary: String,
    #[serde(default)]
    pub site_name: String,
}

/// Bocha search client.
pub struct BochaClient {
    client: Client,
    api_key: String,
}

impl BochaClient {
    pub fn new(api_key: String) -> Self {
        Self {
            client: Client::builder()
                .timeout(std::time::Duration::from_secs(15))
                .build()
                .unwrap_or_else(|_| Client::new()),
            api_key,
        }
    }

    /// Search the web with the given query.
    ///
    /// - `max_results`: Maximum number of results (1-50, default 8).
    pub async fn search(&self, query: &str, max_results: u32) -> Result<Vec<BochaResult>> {
        let body = json!({
            "query": query,
            "summary": true,
            "freshness": "noLimit",
            "count": max_results.min(50).max(1),
        });

        let resp = self
            .client
            .post(BOCHA_API_URL)
            .header("Content-Type", "application/json")
            .bearer_auth(&self.api_key)
            .json(&body)
            .send()
            .await?;

        let status = resp.status();
        if !status.is_success() {
            let error_text = resp.text().await.unwrap_or_default();
            return Err(anyhow!(
                "Bocha API error ({}): {}",
                status.as_u16(),
                error_text
            ));
        }

        let data: serde_json::Value = resp.json().await?;

        // Response structure: { "data": { "webPages": { "value": [...] } } }
        let results: Vec<BochaResult> = data
            .get("data")
            .and_then(|d| d.get("webPages"))
            .and_then(|wp| wp.get("value"))
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|item| {
                        let title = item.get("name")?.as_str()?.to_string();
                        let url = item.get("url")?.as_str()?.to_string();
                        if title.is_empty() || url.is_empty() {
                            return None;
                        }
                        // Prefer AI summary, fall back to snippet
                        let summary = item
                            .get("summary")
                            .and_then(|v| v.as_str())
                            .filter(|s| !s.is_empty())
                            .or_else(|| item.get("snippet").and_then(|v| v.as_str()))
                            .unwrap_or("")
                            .to_string();
                        let site_name = item
                            .get("siteName")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string();
                        Some(BochaResult {
                            title,
                            url,
                            summary,
                            site_name,
                        })
                    })
                    .collect()
            })
            .unwrap_or_default();

        info!(
            "Bocha search for '{}': {} results",
            query,
            results.len()
        );

        Ok(results)
    }
}

#[cfg(test)]
mod tests {

    #[test]
    fn test_bocha_result_parse() {
        let json_str = r#"{
            "data": {
                "webPages": {
                    "value": [
                        {
                            "name": "Test Title",
                            "url": "https://example.com",
                            "summary": "AI summary",
                            "snippet": "Short snippet",
                            "siteName": "Example",
                            "datePublished": "2025-01-01"
                        }
                    ]
                }
            }
        }"#;
        let data: serde_json::Value = serde_json::from_str(json_str).unwrap();
        let results = data
            .get("data")
            .and_then(|d| d.get("webPages"))
            .and_then(|wp| wp.get("value"))
            .and_then(|v| v.as_array())
            .unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0]["name"], "Test Title");
        assert_eq!(results[0]["summary"], "AI summary");
    }

    #[test]
    fn test_empty_response() {
        let json_str = r#"{"data": {}}"#;
        let data: serde_json::Value = serde_json::from_str(json_str).unwrap();
        let results = data
            .get("data")
            .and_then(|d| d.get("webPages"))
            .and_then(|wp| wp.get("value"))
            .and_then(|v| v.as_array());
        assert!(results.is_none());
    }
}
