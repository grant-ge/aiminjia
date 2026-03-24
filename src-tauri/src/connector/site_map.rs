//! Site map cache — per-origin PageProfile persistence.
//!
//! Every navigate() auto-explores the page (menus, tables, forms, API endpoints)
//! and caches the result as a PageProfile. Profiles are persisted per-origin and
//! reused across conversations with a configurable TTL.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};
use log::info;
use serde::{Deserialize, Serialize};

use super::types::{DiscoveredApi, FormData, LinkData};

/// TTL for a page profile before re-exploration (seconds).
const PAGE_PROFILE_TTL_SECS: i64 = 24 * 3600; // 24 hours

// ── Types ───────────────────────────────────────────────────────

/// Schema of a table on a page (headers + row count, no data).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TableSchema {
    #[serde(default)]
    pub name: String,
    pub headers: Vec<String>,
    #[serde(default)]
    pub row_count: usize,
}

/// Profile of a single page — everything the LLM needs to know.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PageProfile {
    pub url_path: String,
    pub title: String,
    #[serde(default)]
    pub nav_links: Vec<LinkData>,
    #[serde(default)]
    pub table_schemas: Vec<TableSchema>,
    #[serde(default)]
    pub forms: Vec<FormData>,
    #[serde(default)]
    pub api_endpoints: Vec<DiscoveredApi>,
    pub explored_at: DateTime<Utc>,
    #[serde(default)]
    pub access_denied: bool,
}

impl PageProfile {
    pub fn is_expired(&self) -> bool {
        let age = Utc::now().signed_duration_since(self.explored_at);
        age.num_seconds() > PAGE_PROFILE_TTL_SECS
    }

    /// One-line summary for the site map context.
    pub fn summary_line(&self) -> String {
        let mut parts = vec![];

        if !self.table_schemas.is_empty() {
            let schemas: Vec<String> = self.table_schemas.iter().map(|t| {
                let name = if t.name.is_empty() { "table".to_string() } else { t.name.clone() };
                let cols = if t.headers.len() > 4 {
                    format!("{}...+{}", t.headers[..4].join("|"), t.headers.len() - 4)
                } else {
                    t.headers.join("|")
                };
                format!("{}({} rows: {})", name, t.row_count, cols)
            }).collect();
            parts.push(format!("tables: {}", schemas.join(", ")));
        }

        if !self.forms.is_empty() {
            let form_info: Vec<String> = self.forms.iter().map(|f| {
                let fields: Vec<&str> = f.fields.iter().map(|fl| fl.name.as_str()).collect();
                format!("{} {} [{}]", f.method, f.action, fields.join(","))
            }).collect();
            parts.push(format!("forms: {}", form_info.join("; ")));
        }

        if !self.api_endpoints.is_empty() {
            let apis: Vec<String> = self.api_endpoints.iter().take(3).map(|a| {
                format!("{} {}", a.method, a.url)
            }).collect();
            let suffix = if self.api_endpoints.len() > 3 {
                format!(" +{} more", self.api_endpoints.len() - 3)
            } else { String::new() };
            parts.push(format!("apis: {}{}", apis.join(", "), suffix));
        }

        if self.access_denied {
            parts.push("❌ ACCESS DENIED".to_string());
        }

        if parts.is_empty() {
            parts.push(format!("{} links", self.nav_links.len()));
        }

        format!("{} ({}) — {}", self.url_path, self.title, parts.join(" | "))
    }

    /// Detailed block for the current active page.
    pub fn format_detail(&self) -> String {
        let mut out = String::new();

        if !self.table_schemas.is_empty() {
            out.push_str("Tables:\n");
            for t in &self.table_schemas {
                let name = if t.name.is_empty() { "table" } else { &t.name };
                out.push_str(&format!("  - {} ({} rows): {}\n", name, t.row_count, t.headers.join(" | ")));
            }
        }

        if !self.forms.is_empty() {
            out.push_str("Forms:\n");
            for f in &self.forms {
                out.push_str(&format!("  - {}#{}: {} {}\n", "Form", f.id, f.method, f.action));
                for field in &f.fields {
                    let val = if field.value.is_empty() { String::new() }
                        else { format!("={}", field.value) };
                    out.push_str(&format!("    {} ({}{})\n", field.name, field.field_type, val));
                }
            }
        }

        if !self.api_endpoints.is_empty() {
            out.push_str("Discovered APIs:\n");
            for a in &self.api_endpoints {
                let size = if a.size_bytes > 1024 {
                    format!("{:.1}KB", a.size_bytes as f64 / 1024.0)
                } else {
                    format!("{}B", a.size_bytes)
                };
                let ct = if a.content_type.contains("json") { "JSON" }
                    else if a.content_type.contains("html") { "HTML" }
                    else { &a.content_type };
                out.push_str(&format!("  - {} {} → {} ({} {})\n", a.method, a.url, a.status, size, ct));
            }
        }

        if !self.nav_links.is_empty() {
            out.push_str("Navigation:\n");
            for link in &self.nav_links {
                match link.link_type.as_str() {
                    "menu" => {
                        if !link.href.is_empty() {
                            out.push_str(&format!("  - [menu] {} → {}\n", link.label, link.href));
                        } else {
                            out.push_str(&format!("  - [menu] {}\n", link.label));
                        }
                    }
                    "button" => out.push_str(&format!("  - [button] {}\n", link.label)),
                    _ => {
                        if !link.href.is_empty() {
                            out.push_str(&format!("  - {} → {}\n", link.label, link.href));
                        }
                    }
                }
            }
        }

        out
    }
}

// ── SiteMap ─────────────────────────────────────────────────────

/// Per-origin collection of page profiles with file persistence.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SiteMap {
    pub origin: String,
    pub pages: HashMap<String, PageProfile>,
    pub updated_at: DateTime<Utc>,
}

impl SiteMap {
    pub fn new(origin: &str) -> Self {
        Self {
            origin: origin.to_string(),
            pages: HashMap::new(),
            updated_at: Utc::now(),
        }
    }

    /// Get a non-expired profile for the given path.
    pub fn get_page(&self, url_path: &str) -> Option<&PageProfile> {
        self.pages.get(url_path).filter(|p| !p.is_expired())
    }

    /// Insert or update a page profile.
    pub fn set_page(&mut self, profile: PageProfile) {
        self.pages.insert(profile.url_path.clone(), profile);
        self.updated_at = Utc::now();
    }

    /// Format for LLM dynamic context injection.
    pub fn format_for_context(&self, active_path: Option<&str>) -> String {
        if self.pages.is_empty() {
            return String::new();
        }

        let mut out = format!("\n[Site Map: {}]\n", self.origin);
        out.push_str("Explored pages:\n");

        for (_, profile) in &self.pages {
            out.push_str(&format!("- {}\n", profile.summary_line()));
        }

        // Show detail for the active page
        if let Some(path) = active_path {
            if let Some(profile) = self.pages.get(path) {
                out.push_str(&format!("\nCurrent page: {} ({})\n", profile.url_path, profile.title));
                out.push_str(&profile.format_detail());
            }
        }

        out
    }

    // ── Persistence ─────────────────────────────────────────────

    fn cache_dir(app_data_dir: &Path) -> PathBuf {
        app_data_dir.join("site-profiles")
    }

    fn file_path(app_data_dir: &Path, origin: &str) -> PathBuf {
        // Sanitize origin for filename: https://foo.com → foo.com.json
        let name = origin
            .replace("https://", "")
            .replace("http://", "")
            .replace(':', "_")
            .replace('/', "_");
        Self::cache_dir(app_data_dir).join(format!("{}.json", name))
    }

    pub fn load(app_data_dir: &Path, origin: &str) -> Option<Self> {
        let path = Self::file_path(app_data_dir, origin);
        let content = std::fs::read_to_string(&path).ok()?;
        let map: Self = serde_json::from_str(&content).ok()?;
        info!("[SiteMap] Loaded {} pages for {}", map.pages.len(), origin);
        Some(map)
    }

    pub fn save(&self, app_data_dir: &Path) -> Result<(), String> {
        let dir = Self::cache_dir(app_data_dir);
        std::fs::create_dir_all(&dir).map_err(|e| format!("mkdir: {}", e))?;
        let path = Self::file_path(app_data_dir, &self.origin);
        let json = serde_json::to_string_pretty(self)
            .map_err(|e| format!("serialize: {}", e))?;
        std::fs::write(&path, json).map_err(|e| format!("write: {}", e))?;
        info!("[SiteMap] Saved {} pages for {} → {:?}", self.pages.len(), self.origin, path);
        Ok(())
    }
}
