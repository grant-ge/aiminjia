//! Tavily API client for AI-optimized web search.
//!
//! Provides high-quality search results suitable for LLM consumption.
//! API docs: https://docs.tavily.com/docs/rest-api/api-reference
#![allow(dead_code)]

use anyhow::{anyhow, Result};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::json;

const TAVILY_API_URL: &str = "https://api.tavily.com/search";

/// A single search result from Tavily.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResult {
    pub title: String,
    pub url: String,
    pub content: String,
    #[serde(default)]
    pub score: f64,
}

/// Full search response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResponse {
    /// AI-generated answer summarizing the results (if requested)
    pub answer: Option<String>,
    /// Individual search results
    pub results: Vec<SearchResult>,
    /// Query used for the search
    pub query: String,
}

/// Tavily search client.
pub struct TavilyClient {
    client: Client,
    api_key: String,
}

impl TavilyClient {
    pub fn new(api_key: String) -> Self {
        Self {
            client: Client::new(),
            api_key,
        }
    }

    /// Search the web with the given query.
    ///
    /// - `include_answer`: If true, Tavily returns an AI-generated answer summary.
    /// - `max_results`: Maximum number of results (1-10, default 5).
    pub async fn search(
        &self,
        query: &str,
        include_answer: bool,
        max_results: u32,
    ) -> Result<SearchResponse> {
        let body = json!({
            "query": query,
            "include_answer": include_answer,
            "max_results": max_results.min(10).max(1),
            "search_depth": "advanced",
        });

        let resp = self.client
            .post(TAVILY_API_URL)
            .header("Content-Type", "application/json")
            .bearer_auth(&self.api_key)
            .json(&body)
            .send()
            .await?;

        let status = resp.status();
        if !status.is_success() {
            let error_text = resp.text().await.unwrap_or_default();
            return Err(anyhow!("Tavily API error ({}): {}", status.as_u16(), error_text));
        }

        let data: serde_json::Value = resp.json().await?;

        let answer = data.get("answer").and_then(|v| v.as_str()).map(|s| s.to_string());

        let results = data.get("results")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter().filter_map(|item| {
                    Some(SearchResult {
                        title: item.get("title")?.as_str()?.to_string(),
                        url: item.get("url")?.as_str()?.to_string(),
                        content: item.get("content")?.as_str()?.to_string(),
                        score: item.get("score").and_then(|v| v.as_f64()).unwrap_or(0.0),
                    })
                }).collect()
            })
            .unwrap_or_default();

        Ok(SearchResponse {
            answer,
            results,
            query: query.to_string(),
        })
    }

    /// Validate the API key by sending a minimal search.
    pub async fn validate_key(&self) -> Result<bool> {
        let body = json!({
            "query": "test",
            "max_results": 1,
        });

        let resp = self.client
            .post(TAVILY_API_URL)
            .bearer_auth(&self.api_key)
            .json(&body)
            .send()
            .await?;

        Ok(resp.status().is_success())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_search_result_deserialization() {
        let json = r#"{
            "title": "Test",
            "url": "https://example.com",
            "content": "Test content",
            "score": 0.95
        }"#;
        let result: SearchResult = serde_json::from_str(json).unwrap();
        assert_eq!(result.title, "Test");
        assert!((result.score - 0.95).abs() < f64::EPSILON);
    }

    #[test]
    fn test_search_response_deserialization() {
        let json = r#"{
            "answer": "Test answer",
            "results": [],
            "query": "test query"
        }"#;
        let resp: SearchResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.answer, Some("Test answer".to_string()));
        assert!(resp.results.is_empty());
    }
}
