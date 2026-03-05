//! Bing Web Search client — free, no API key required.
//!
//! Scrapes Bing search result pages to extract titles, URLs, and snippets.
//! Used as the primary free search engine (no API key required).

use anyhow::{anyhow, Result};
use log::info;
use reqwest::Client;
use serde::{Deserialize, Serialize};

/// A single search result from Bing.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BingResult {
    pub title: String,
    pub url: String,
    pub content: String,
}

/// Bing search client.
pub struct BingClient {
    client: Client,
}

impl BingClient {
    pub fn new() -> Self {
        Self {
            client: Client::builder()
                .timeout(std::time::Duration::from_secs(15))
                .build()
                .unwrap_or_else(|_| Client::new()),
        }
    }

    /// Search Bing and parse results from HTML.
    pub async fn search(&self, query: &str, max_results: u32) -> Result<Vec<BingResult>> {
        let url = "https://www.bing.com/search";

        let resp = self
            .client
            .get(url)
            .query(&[
                ("q", query),
                ("count", &max_results.to_string()),
                ("setlang", "zh-Hans"),
            ])
            .header(
                "User-Agent",
                "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36",
            )
            .header("Accept", "text/html,application/xhtml+xml")
            .header("Accept-Language", "zh-CN,zh;q=0.9,en;q=0.8")
            .send()
            .await?;

        let status = resp.status();
        if !status.is_success() {
            return Err(anyhow!("Bing search error: HTTP {}", status.as_u16()));
        }

        let html = resp.text().await?;
        let results = parse_bing_results(&html, max_results as usize);

        info!(
            "Bing search for '{}': {} results extracted",
            query,
            results.len()
        );

        Ok(results)
    }
}

/// Parse Bing search result HTML to extract structured results.
///
/// Bing result items are in `<li class="b_algo">` blocks:
/// - Title + URL in `<h2><a href="...">title</a></h2>`
/// - Snippet in `<p>` or `<div class="b_caption"><p>`
fn parse_bing_results(html: &str, max_results: usize) -> Vec<BingResult> {
    let mut results = Vec::new();

    // Split by b_algo result blocks
    let blocks: Vec<&str> = html.split("class=\"b_algo\"").collect();

    // Skip first element (before first result)
    for block in blocks.iter().skip(1).take(max_results) {
        let title = extract_between(block, "<h2", "</h2>")
            .and_then(|h2| extract_tag_text(&h2));

        let url = extract_between(block, "<h2", "</h2>")
            .and_then(|h2| extract_href(&h2));

        let snippet = extract_snippet(block);

        if let (Some(title), Some(url)) = (title, url) {
            if !title.is_empty() && !url.is_empty() && url.starts_with("http") {
                results.push(BingResult {
                    title: decode_html_entities(&title),
                    url,
                    content: decode_html_entities(&snippet.unwrap_or_default()),
                });
            }
        }
    }

    results
}

/// Extract content between a start tag pattern and an end tag.
fn extract_between<'a>(html: &'a str, start_pattern: &str, end_tag: &str) -> Option<String> {
    let start_idx = html.find(start_pattern)?;
    let after_start = &html[start_idx..];
    let end_idx = after_start.find(end_tag)?;
    Some(after_start[..end_idx + end_tag.len()].to_string())
}

/// Extract href attribute value from an <a> tag.
fn extract_href(html: &str) -> Option<String> {
    let href_start = html.find("href=\"")?;
    let after_href = &html[href_start + 6..];
    let end = after_href.find('"')?;
    let url = &after_href[..end];
    // Skip Bing redirect URLs, extract actual URL
    if url.contains("bing.com") && !url.starts_with("https://www.bing.com") {
        None
    } else {
        Some(url.to_string())
    }
}

/// Extract visible text from an HTML fragment (strip tags).
fn extract_tag_text(html: &str) -> Option<String> {
    let mut text = String::new();
    let mut in_tag = false;
    for ch in html.chars() {
        match ch {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => text.push(ch),
            _ => {}
        }
    }
    let trimmed = text.trim().to_string();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed)
    }
}

/// Extract the snippet/description from a Bing result block.
fn extract_snippet(block: &str) -> Option<String> {
    // Try <p class="b_lineclamp..."> first (common pattern)
    if let Some(p_content) = extract_between(block, "<p", "</p>") {
        let text = extract_tag_text(&p_content)?;
        if text.len() > 20 {
            return Some(text);
        }
    }
    // Fallback: look in b_caption div
    if let Some(caption) = extract_between(block, "b_caption", "</div>") {
        if let Some(p_content) = extract_between(&caption, "<p", "</p>") {
            return extract_tag_text(&p_content);
        }
    }
    None
}

/// Decode common HTML entities.
fn decode_html_entities(s: &str) -> String {
    s.replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&nbsp;", " ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_tag_text() {
        assert_eq!(
            extract_tag_text("<a href=\"x\">Hello <b>World</b></a>"),
            Some("Hello World".to_string())
        );
    }

    #[test]
    fn test_extract_href() {
        assert_eq!(
            extract_href("<a href=\"https://example.com/page\">text</a>"),
            Some("https://example.com/page".to_string())
        );
    }

    #[test]
    fn test_decode_html_entities() {
        assert_eq!(
            decode_html_entities("Tom &amp; Jerry &lt;3&gt;"),
            "Tom & Jerry <3>"
        );
    }

    #[test]
    fn test_parse_empty_html() {
        let results = parse_bing_results("<html><body>no results</body></html>", 5);
        assert!(results.is_empty());
    }
}
