//! Browser automation result types.
//!
//! Shared between PlaywrightBrowser, SiteMap, and tool handlers.

use std::collections::HashMap;
use serde::{Deserialize, Serialize};

/// Result from navigate().
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BrowseNavigateResult {
    pub url: String,
    pub title: String,
    #[serde(default)]
    pub redirected_to_login: bool,
    #[serde(skip)]
    pub page_profile: Option<super::site_map::PageProfile>,
    #[serde(skip)]
    pub screenshot_path: Option<std::path::PathBuf>,
}

/// Result from read_content() — tables + text + links.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BrowseResult {
    pub url: String,
    pub title: String,
    pub tables: Vec<TableData>,
    pub text: String,
    #[serde(default)]
    pub links: Vec<LinkData>,
}

/// Extracted table data.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TableData {
    pub headers: Vec<String>,
    pub rows: Vec<HashMap<String, String>>,
}

/// Extracted link/menu/button.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LinkData {
    pub label: String,
    #[serde(default)]
    pub href: String,
    #[serde(default, rename = "type")]
    pub link_type: String,
    #[serde(default)]
    pub selector: String,
}

/// Result from execute_js().
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecuteJsResult {
    pub value: serde_json::Value,
    pub error: Option<String>,
    pub new_url: Option<String>,
    pub new_title: Option<String>,
}

/// Full page result from navigate_and_extract().
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FullPageResult {
    pub navigate: BrowseNavigateResult,
    pub content: BrowseResult,
    #[serde(default)]
    pub api_calls: Vec<DiscoveredApi>,
    #[serde(default)]
    pub forms: Vec<FormData>,
}

/// An XHR/fetch request intercepted during page load.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscoveredApi {
    pub method: String,
    pub url: String,
    #[serde(default)]
    pub status: u16,
    #[serde(default)]
    pub content_type: String,
    #[serde(default)]
    pub size_bytes: u64,
}

/// A <form> element discovered on the page.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FormData {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub action: String,
    #[serde(default)]
    pub method: String,
    #[serde(default)]
    pub fields: Vec<FormField>,
}

/// A field within a form.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FormField {
    pub name: String,
    #[serde(default)]
    pub field_type: String,
    #[serde(default)]
    pub value: String,
}

/// Result from api_fetch().
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiFetchResult {
    pub status: u16,
    #[serde(default)]
    pub content_type: String,
    pub data: serde_json::Value,
    pub total_rows: Option<u64>,
    #[serde(default)]
    pub truncated: bool,
    #[serde(skip)]
    pub saved_file_path: Option<std::path::PathBuf>,
}
