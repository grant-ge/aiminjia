//! SearXNG client — free, open-source meta-search engine.
//!
//! Uses public SearXNG instances for web search without requiring an API key.
//! Falls back through multiple instances if one is unavailable.
//! API docs: https://docs.searxng.org/dev/search_api.html
#![allow(dead_code)]

use anyhow::{anyhow, Result};
use log::{info, warn};
use reqwest::Client;
use serde::{Deserialize, Serialize};

/// Well-known public SearXNG instances (JSON API enabled).
/// Ordered by reliability. The client tries each in order until one succeeds.
const PUBLIC_INSTANCES: &[&str] = &[
    "https://search.bus-hit.me",
    "https://search.sapti.me",
    "https://searx.tiekoetter.com",
    "https://search.ononoki.org",
    "https://searx.be",
];

/// A single search result from SearXNG.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearxngResult {
    pub title: String,
    pub url: String,
    pub content: String,
}

/// SearXNG search client.
pub struct SearxngClient {
    client: Client,
    /// Custom instance URL. If `None`, cycles through `PUBLIC_INSTANCES`.
    instance_url: Option<String>,
}

impl SearxngClient {
    /// Create a client that uses public instances (no API key needed).
    pub fn new() -> Self {
        Self {
            client: Client::builder()
                .timeout(std::time::Duration::from_secs(10))
                .build()
                .unwrap_or_else(|_| Client::new()),
            instance_url: None,
        }
    }

    /// Create a client targeting a specific self-hosted instance.
    pub fn with_instance(instance_url: String) -> Self {
        Self {
            client: Client::builder()
                .timeout(std::time::Duration::from_secs(10))
                .build()
                .unwrap_or_else(|_| Client::new()),
            instance_url: Some(instance_url),
        }
    }

    /// Search the web. Tries multiple instances if using public endpoints.
    pub async fn search(
        &self,
        query: &str,
        max_results: u32,
    ) -> Result<Vec<SearxngResult>> {
        if let Some(ref url) = self.instance_url {
            return self.search_instance(url, query, max_results).await;
        }

        // Try public instances in order
        let mut last_error = anyhow!("No SearXNG instances available");
        for instance in PUBLIC_INSTANCES {
            match self.search_instance(instance, query, max_results).await {
                Ok(results) => {
                    info!("SearXNG search succeeded via {}", instance);
                    return Ok(results);
                }
                Err(e) => {
                    warn!("SearXNG instance {} failed: {}", instance, e);
                    last_error = e;
                }
            }
        }

        Err(last_error)
    }

    /// Search a single SearXNG instance.
    async fn search_instance(
        &self,
        base_url: &str,
        query: &str,
        max_results: u32,
    ) -> Result<Vec<SearxngResult>> {
        let url = format!("{}/search", base_url.trim_end_matches('/'));

        let resp = self
            .client
            .get(&url)
            .query(&[
                ("q", query),
                ("format", "json"),
                ("categories", "general"),
                ("language", "auto"),
                ("safesearch", "1"),
            ])
            .header("Accept", "application/json")
            .send()
            .await?;

        let status = resp.status();
        if !status.is_success() {
            let error_text = resp.text().await.unwrap_or_default();
            return Err(anyhow!(
                "SearXNG error ({}) from {}: {}",
                status.as_u16(),
                base_url,
                error_text
            ));
        }

        let data: serde_json::Value = resp.json().await?;

        let results = data
            .get("results")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .take(max_results as usize)
                    .filter_map(|item| {
                        Some(SearxngResult {
                            title: item.get("title")?.as_str()?.to_string(),
                            url: item.get("url")?.as_str()?.to_string(),
                            content: item
                                .get("content")
                                .and_then(|v| v.as_str())
                                .unwrap_or("")
                                .to_string(),
                        })
                    })
                    .collect()
            })
            .unwrap_or_default();

        Ok(results)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_searxng_result_deserialization() {
        let json = r#"{"title": "Test", "url": "https://example.com", "content": "Content"}"#;
        let result: SearxngResult = serde_json::from_str(json).unwrap();
        assert_eq!(result.title, "Test");
        assert_eq!(result.url, "https://example.com");
    }

    #[test]
    fn test_new_client() {
        let client = SearxngClient::new();
        assert!(client.instance_url.is_none());
    }

    #[test]
    fn test_with_instance() {
        let client = SearxngClient::with_instance("https://my.searxng.local".to_string());
        assert_eq!(
            client.instance_url.as_deref(),
            Some("https://my.searxng.local")
        );
    }
}
