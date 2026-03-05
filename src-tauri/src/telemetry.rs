//! Telemetry — structured metrics collection to JSONL.
//!
//! Records metrics entries (tool, python, step, memory, checkpoint, tokens)
//! as JSONL lines in `{workspace}/logs/metrics.jsonl` with 2MB auto-split.
//! All errors are silently caught — telemetry must never break business logic.

use std::collections::HashMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::storage::file_store::io;

/// A single metrics entry persisted to JSONL.
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MetricsEntry {
    pub timestamp: String,
    pub category: String,
    pub fields: HashMap<String, String>,
}

/// JSONL file name under `{workspace}/logs/`.
const METRICS_FILE: &str = "metrics.jsonl";

/// Auto-split threshold (2 MB, consistent with audit logs).
const SPLIT_THRESHOLD: u64 = 2 * 1024 * 1024;

/// Metrics base path: `{workspace}/logs/metrics.jsonl`.
fn metrics_path(workspace: &Path) -> std::path::PathBuf {
    workspace.join("logs").join(METRICS_FILE)
}

/// Record a single metrics entry to JSONL.
///
/// Errors are silently caught — telemetry must never interrupt the agent loop.
pub fn record(category: &str, workspace: &Path, fields: &[(&str, &str)]) {
    let entry = MetricsEntry {
        timestamp: chrono::Utc::now().to_rfc3339(),
        category: category.to_string(),
        fields: fields
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect(),
    };

    let path = metrics_path(workspace);
    if let Err(e) = io::append_jsonl_with_split(&path, &entry, SPLIT_THRESHOLD) {
        log::warn!("[telemetry] Failed to write metrics entry: {}", e);
    }
}

/// Export all metrics entries as a JSON array string.
///
/// Returns `(json_content, entry_count)`.
pub fn export_all(workspace: &Path) -> Result<(String, usize), String> {
    let path = metrics_path(workspace);
    let entries: Vec<MetricsEntry> =
        io::read_all_jsonl_shards(&path).map_err(|e| e.to_string())?;
    let count = entries.len();
    let json = serde_json::to_string_pretty(&entries).map_err(|e| e.to_string())?;
    Ok((json, count))
}

/// Get metrics file info: `(entry_count, total_bytes)`.
pub fn get_info(workspace: &Path) -> Result<(usize, u64), String> {
    let path = metrics_path(workspace);

    // Count entries across all shards
    let entries: Vec<MetricsEntry> =
        io::read_all_jsonl_shards(&path).map_err(|e| e.to_string())?;
    let count = entries.len();

    // Sum file sizes across all shards + base file
    let mut total_bytes: u64 = list_all_shard_files(&path)
        .iter()
        .map(|p| io::file_size_bytes(p))
        .sum();
    total_bytes += io::file_size_bytes(&path);

    Ok((count, total_bytes))
}

/// Clear all metrics JSONL files (base + shards).
///
/// Returns the number of files deleted.
pub fn clear_all(workspace: &Path) -> Result<usize, String> {
    let path = metrics_path(workspace);
    let files = list_all_shard_files(&path);

    let mut deleted = 0;
    for f in &files {
        if std::fs::remove_file(f).is_ok() {
            deleted += 1;
        }
    }

    // Also remove the base file itself
    if path.exists() {
        if std::fs::remove_file(&path).is_ok() {
            deleted += 1;
        }
    }

    Ok(deleted)
}

/// List all shard files + the base file for the metrics path.
fn list_all_shard_files(base_path: &Path) -> Vec<std::path::PathBuf> {
    let parent = base_path.parent().unwrap_or(Path::new("."));
    let stem = base_path
        .file_stem()
        .unwrap_or_default()
        .to_string_lossy();

    let mut files = Vec::new();

    if let Ok(entries) = std::fs::read_dir(parent) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().to_string();
            // Match: metrics.jsonl, metrics.1.jsonl, metrics.2.jsonl, ...
            if name == format!("{}.jsonl", stem) {
                continue; // base file handled separately in clear_all
            }
            let prefix = format!("{}.", stem);
            let suffix = ".jsonl";
            if name.starts_with(&prefix) && name.ends_with(suffix) {
                files.push(entry.path());
            }
        }
    }

    files
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_record_and_export() {
        let dir = TempDir::new().unwrap();
        let workspace = dir.path();
        std::fs::create_dir_all(workspace.join("logs")).unwrap();

        record("tool", workspace, &[("conv", "c1"), ("total", "3")]);
        record("python", workspace, &[("conv", "c1"), ("exit_code", "0")]);

        let (json, count) = export_all(workspace).unwrap();
        assert_eq!(count, 2);
        assert!(json.contains("tool"));
        assert!(json.contains("python"));
    }

    #[test]
    fn test_get_info() {
        let dir = TempDir::new().unwrap();
        let workspace = dir.path();
        std::fs::create_dir_all(workspace.join("logs")).unwrap();

        let (count, bytes) = get_info(workspace).unwrap();
        assert_eq!(count, 0);
        assert_eq!(bytes, 0);

        record("step", workspace, &[("conv", "c1"), ("status", "completed")]);

        let (count, bytes) = get_info(workspace).unwrap();
        assert_eq!(count, 1);
        assert!(bytes > 0);
    }

    #[test]
    fn test_clear_all() {
        let dir = TempDir::new().unwrap();
        let workspace = dir.path();
        std::fs::create_dir_all(workspace.join("logs")).unwrap();

        for i in 0..5 {
            record("tokens", workspace, &[("iter", &i.to_string())]);
        }

        let (count, _) = get_info(workspace).unwrap();
        assert_eq!(count, 5);

        let deleted = clear_all(workspace).unwrap();
        assert!(deleted >= 1);

        let (count, _) = get_info(workspace).unwrap();
        assert_eq!(count, 0);
    }

    #[test]
    fn test_record_creates_logs_dir() {
        let dir = TempDir::new().unwrap();
        let workspace = dir.path();
        // Don't pre-create logs dir — record() should handle it via append_jsonl_with_split

        record("memory", workspace, &[("conv", "c1")]);

        let (count, _) = get_info(workspace).unwrap();
        assert_eq!(count, 1);
    }
}
