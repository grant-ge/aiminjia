//! Telemetry — structured metrics collection to JSONL.
//!
//! Records metrics entries (tool, python, step, memory, checkpoint, tokens)
//! as JSONL lines in `{workspace}/logs/metrics.jsonl` with 2MB auto-split.
//! All errors are silently caught — telemetry must never break business logic.

use std::collections::HashMap;
use std::fs::{self, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::OnceLock;

use serde::{Deserialize, Serialize};
use tauri::Emitter;

use crate::storage::file_store::io;

/// A single metrics entry persisted to JSONL.
#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MetricsEntry {
    pub timestamp: String,
    pub category: String,
    pub fields: HashMap<String, String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DiagnosticLevel {
    Debug,
    Info,
    Warn,
    Error,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DiagnosticSource {
    Frontend,
    Backend,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiagnosticEvent {
    pub ts: String,
    pub seq: u64,
    pub category: String,
    pub level: DiagnosticLevel,
    pub source: DiagnosticSource,
    pub event: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ok: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub conversation_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub run_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_message_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent_id: Option<String>,
    /// PR8 (per-team disk layout §6.4): team this event belongs to.
    /// Optional because not all events are team-scoped (e.g. global tool
    /// invocations, persona dispatch).  Set via `.team_name(...)` builder.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub team_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub interaction_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub task_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub elapsed_ms: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub payload: Option<serde_json::Value>,
}

impl DiagnosticEvent {
    pub fn new(event: impl Into<String>, source: DiagnosticSource) -> Self {
        Self {
            ts: chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
            seq: DIAGNOSTIC_SEQ.fetch_add(1, Ordering::Relaxed),
            category: "diagnostics".to_string(),
            level: DiagnosticLevel::Info,
            source,
            event: event.into(),
            ok: None,
            conversation_id: None,
            run_id: None,
            message_id: None,
            client_message_id: None,
            tool_call_id: None,
            agent_id: None,
            team_name: None,
            interaction_id: None,
            task_id: None,
            command: None,
            duration_ms: None,
            elapsed_ms: None,
            error: None,
            payload: None,
        }
    }

    pub fn level(mut self, level: DiagnosticLevel) -> Self {
        self.level = level;
        self
    }

    pub fn ok(mut self, ok: bool) -> Self {
        self.ok = Some(ok);
        self
    }

    pub fn conversation_id(mut self, value: impl Into<String>) -> Self {
        self.conversation_id = Some(value.into());
        self
    }

    pub fn run_id(mut self, value: impl Into<String>) -> Self {
        self.run_id = Some(value.into());
        self
    }

    pub fn message_id(mut self, value: impl Into<String>) -> Self {
        self.message_id = Some(value.into());
        self
    }

    pub fn client_message_id(mut self, value: impl Into<String>) -> Self {
        self.client_message_id = Some(value.into());
        self
    }

    pub fn tool_call_id(mut self, value: impl Into<String>) -> Self {
        self.tool_call_id = Some(value.into());
        self
    }

    pub fn agent_id(mut self, value: impl Into<String>) -> Self {
        self.agent_id = Some(value.into());
        self
    }

    /// PR8 (per-team disk layout §6.4): tag this event with the team it
    /// belongs to.  Empty strings are dropped to keep the field meaningful
    /// (Lead-only / pre-team events leave it `None`).
    pub fn team_name(mut self, value: impl Into<String>) -> Self {
        let v = value.into();
        if !v.is_empty() {
            self.team_name = Some(v);
        }
        self
    }

    pub fn interaction_id(mut self, value: impl Into<String>) -> Self {
        self.interaction_id = Some(value.into());
        self
    }

    pub fn task_id(mut self, value: impl Into<String>) -> Self {
        self.task_id = Some(value.into());
        self
    }

    pub fn command(mut self, value: impl Into<String>) -> Self {
        self.command = Some(redact_sensitive_text(&value.into()));
        self
    }

    pub fn duration_ms(mut self, value: u64) -> Self {
        self.duration_ms = Some(value);
        self
    }

    pub fn elapsed_ms(mut self, value: u64) -> Self {
        self.elapsed_ms = Some(value);
        self
    }

    pub fn error(mut self, value: impl Into<String>) -> Self {
        self.error = Some(redact_sensitive_text(&value.into()));
        self.level = DiagnosticLevel::Error;
        self.ok = Some(false);
        self
    }

    pub fn payload(mut self, value: serde_json::Value) -> Self {
        self.payload = Some(redact_value(value));
        self
    }
}

/// JSONL file name under `{workspace}/logs/`.
const METRICS_FILE: &str = "metrics.jsonl";

/// Auto-split threshold (2 MB, consistent with audit logs).
const SPLIT_THRESHOLD: u64 = 2 * 1024 * 1024;

static DIAGNOSTIC_SEQ: AtomicU64 = AtomicU64::new(1);

/// Process-wide fallback workspace for telemetry writes.
///
/// Set once at startup (`lib.rs`) to `AiJiaHome::root()`. Code paths that
/// don't have a `workspace_path` in hand (global tool dispatcher,
/// sub-agent worker, interaction control plane) call
/// [`diagnostics_workspace`] instead of `std::env::current_dir()`—on
/// macOS the cwd is `/` when launched from Finder, which is read-only
/// and caused "Failed to write diagnostic entry: Read-only file system"
/// spam (~1300/day).
static DIAGNOSTICS_WORKSPACE: OnceLock<PathBuf> = OnceLock::new();

/// Initialize the process-wide telemetry workspace. Safe to call multiple
/// times; the first value wins. Call from `lib.rs` after `AiJiaHome` is
/// resolved.
pub fn set_diagnostics_workspace(path: PathBuf) {
    let _ = DIAGNOSTICS_WORKSPACE.set(path);
}

/// Returns the workspace path callers without explicit context should use
/// for `record_diagnostic`. Falls back to `./` only if initialization was
/// somehow skipped (tests, early-boot paths).
pub fn diagnostics_workspace() -> PathBuf {
    DIAGNOSTICS_WORKSPACE
        .get()
        .cloned()
        .unwrap_or_else(|| PathBuf::from("."))
}

/// Metrics base path: `{workspace}/logs/metrics.jsonl`.
fn metrics_path(workspace: &Path) -> std::path::PathBuf {
    workspace.join("logs").join(METRICS_FILE)
}

fn is_secret_key(key: &str) -> bool {
    let lower = key.to_ascii_lowercase();
    lower.contains("token")
        || lower.contains("apikey")
        || lower.contains("api_key")
        || lower.contains("authorization")
        || lower.contains("cookie")
        || lower.contains("password")
        || lower.contains("secret")
}

fn redact_value(value: serde_json::Value) -> serde_json::Value {
    match value {
        serde_json::Value::Object(map) => serde_json::Value::Object(
            map.into_iter()
                .map(|(key, value)| {
                    if is_secret_key(&key) {
                        (key, serde_json::Value::String("[REDACTED]".to_string()))
                    } else {
                        (key, redact_value(value))
                    }
                })
                .collect(),
        ),
        serde_json::Value::Array(values) => {
            serde_json::Value::Array(values.into_iter().map(redact_value).collect())
        }
        serde_json::Value::String(value) => {
            serde_json::Value::String(redact_sensitive_text(&value))
        }
        other => other,
    }
}

fn redact_sensitive_text(value: &str) -> String {
    let mut redacted = redact_after_case_insensitive(value, "bearer ");
    for marker in [
        "access_token=",
        "token=",
        "api_key=",
        "apikey=",
        "password=",
        "secret=",
    ] {
        redacted = redact_after_case_insensitive(&redacted, marker);
    }
    redact_prefixed_secret(&redacted, "sk-")
}

fn redact_after_case_insensitive(value: &str, marker: &str) -> String {
    let mut output = String::with_capacity(value.len());
    let lower = value.to_ascii_lowercase();
    let marker_lower = marker.to_ascii_lowercase();
    let mut cursor = 0;

    while let Some(relative_start) = lower[cursor..].find(&marker_lower) {
        let marker_start = cursor + relative_start;
        let value_start = marker_start + marker.len();
        let value_end = find_secret_end(value, value_start);
        output.push_str(&value[cursor..value_start]);
        output.push_str("[REDACTED]");
        cursor = value_end;
    }

    output.push_str(&value[cursor..]);
    output
}

fn redact_prefixed_secret(value: &str, prefix: &str) -> String {
    let mut output = String::with_capacity(value.len());
    let lower = value.to_ascii_lowercase();
    let prefix_lower = prefix.to_ascii_lowercase();
    let mut cursor = 0;

    while let Some(relative_start) = lower[cursor..].find(&prefix_lower) {
        let secret_start = cursor + relative_start;
        let secret_end = find_secret_end(value, secret_start);
        output.push_str(&value[cursor..secret_start]);
        output.push_str("[REDACTED]");
        cursor = secret_end;
    }

    output.push_str(&value[cursor..]);
    output
}

fn find_secret_end(value: &str, start: usize) -> usize {
    value[start..]
        .char_indices()
        .find_map(|(offset, ch)| {
            if ch.is_whitespace() || matches!(ch, '"' | '\'' | '`' | ',' | ';' | '&' | ')' | '}') {
                Some(start + offset)
            } else {
                None
            }
        })
        .unwrap_or(value.len())
}

fn append_plain_jsonl_with_split<T: Serialize>(
    path: &Path,
    record: &T,
    max_bytes: u64,
) -> std::io::Result<()> {
    if path.exists() && io::file_size_bytes(path) >= max_bytes {
        rotate_metrics_shard(path)?;
    }

    let json = serde_json::to_string(record)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut file = OpenOptions::new().create(true).append(true).open(path)?;
    file.write_all(json.as_bytes())?;
    file.write_all(b"\n")?;
    file.flush()?;
    Ok(())
}

fn rotate_metrics_shard(path: &Path) -> std::io::Result<()> {
    let next_num = list_all_shard_files(path)
        .iter()
        .filter_map(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .and_then(parse_metrics_shard_number)
        })
        .max()
        .unwrap_or(0)
        + 1;
    let parent = path.parent().unwrap_or(Path::new("."));
    fs::rename(path, parent.join(format!("metrics.{}.jsonl", next_num)))
}

fn parse_metrics_shard_number(name: &str) -> Option<u32> {
    name.strip_prefix("metrics.")?
        .strip_suffix(".jsonl")?
        .parse::<u32>()
        .ok()
}

fn parse_metrics_jsonl_line(line: &str) -> Option<serde_json::Value> {
    let json = line.split_once('\t').map_or(line, |(json, _)| json);
    serde_json::from_str(json).ok()
}

fn read_all_metric_values(base_path: &Path) -> std::io::Result<Vec<serde_json::Value>> {
    let mut values = Vec::new();

    for path in list_metric_paths_in_order(base_path) {
        if !path.exists() {
            continue;
        }
        let file = fs::File::open(&path)?;
        let reader = BufReader::new(file);
        for line in reader.lines() {
            let line = line?;
            if let Some(value) = parse_metrics_jsonl_line(&line) {
                values.push(value);
            }
        }
    }

    Ok(values)
}

fn list_metric_paths_in_order(base_path: &Path) -> Vec<std::path::PathBuf> {
    let mut shards: Vec<(u32, std::path::PathBuf)> = list_all_shard_files(base_path)
        .into_iter()
        .filter_map(|path| {
            let number = path
                .file_name()
                .and_then(|name| name.to_str())
                .and_then(parse_metrics_shard_number)?;
            Some((number, path))
        })
        .collect();
    shards.sort_by_key(|(number, _)| *number);

    let mut paths: Vec<std::path::PathBuf> = shards.into_iter().map(|(_, path)| path).collect();
    if base_path.exists() {
        paths.push(base_path.to_path_buf());
    }
    paths
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
    if let Err(e) = append_plain_jsonl_with_split(&path, &entry, SPLIT_THRESHOLD) {
        log::warn!(
            "[telemetry] Failed to write metrics entry to {}: {}",
            path.display(),
            e
        );
    }
}

pub fn record_diagnostic(workspace: &Path, event: DiagnosticEvent) {
    let path = metrics_path(workspace);
    if let Err(e) = append_plain_jsonl_with_split(&path, &event, SPLIT_THRESHOLD) {
        log::warn!(
            "[telemetry] Failed to write diagnostic entry to {}: {}",
            path.display(),
            e
        );
    }
}

fn record_diagnostic_and_emit_with<F>(workspace: &Path, event: DiagnosticEvent, emit: F)
where
    F: FnOnce(&DiagnosticEvent) -> Result<(), String>,
{
    record_diagnostic(workspace, event.clone());
    if let Err(err) = emit(&event) {
        log::warn!("[telemetry] Failed to emit diagnostic event: {}", err);
    }
}

pub fn record_diagnostic_and_emit(
    app: &tauri::AppHandle,
    workspace: &Path,
    event: DiagnosticEvent,
) {
    record_diagnostic_and_emit_with(workspace, event, |event| {
        app.emit("diagnostics:event", event)
            .map_err(|err: tauri::Error| err.to_string())
    });
}

/// Export all metrics entries as a JSON array string.
///
/// Returns `(json_content, entry_count)`.
pub fn export_all(workspace: &Path) -> Result<(String, usize), String> {
    let path = metrics_path(workspace);
    let entries = read_all_metric_values(&path).map_err(|e| e.to_string())?;
    let count = entries.len();
    let json = serde_json::to_string_pretty(&entries).map_err(|e| e.to_string())?;
    Ok((json, count))
}

/// Get metrics file info: `(entry_count, total_bytes)`.
pub fn get_info(workspace: &Path) -> Result<(usize, u64), String> {
    let path = metrics_path(workspace);

    // Count both legacy marker-suffixed metrics and plain diagnostics records.
    let count = read_all_metric_values(&path)
        .map_err(|e| e.to_string())?
        .len();

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
    let stem = base_path.file_stem().unwrap_or_default().to_string_lossy();

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

    fn parse_jsonl_line(line: &str) -> serde_json::Value {
        let json = line.split_once('\t').map_or(line, |(json, _)| json);
        serde_json::from_str(json).unwrap()
    }

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

        record(
            "step",
            workspace,
            &[("conv", "c1"), ("status", "completed")],
        );

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

    #[test]
    fn test_record_diagnostic_writes_flat_queryable_jsonl() {
        let dir = TempDir::new().unwrap();
        let workspace = dir.path();

        let event = DiagnosticEvent::new("turn.started", DiagnosticSource::Backend)
            .level(DiagnosticLevel::Info)
            .conversation_id("conv_1")
            .run_id("run_1")
            .message_id("msg_1")
            .duration_ms(12)
            .ok(true)
            .payload(serde_json::json!({"phase":"start"}));

        record_diagnostic(workspace, event);

        let path = metrics_path(workspace);
        let raw = std::fs::read_to_string(path).unwrap();
        let line = raw.lines().next().unwrap();
        let value: serde_json::Value = serde_json::from_str(line).unwrap();

        assert_eq!(value["category"], "diagnostics");
        assert_eq!(value["source"], "backend");
        assert_eq!(value["event"], "turn.started");
        assert_eq!(value["conversationId"], "conv_1");
        assert_eq!(value["runId"], "run_1");
        assert_eq!(value["messageId"], "msg_1");
        assert_eq!(value["durationMs"], 12);
        assert_eq!(value["ok"], true);
        assert_eq!(value["payload"]["phase"], "start");
        assert!(value.get("fields").is_none());
        assert!(value["ts"].as_str().unwrap().ends_with('Z'));
        assert!(value["seq"].as_u64().unwrap() >= 1);
    }

    #[test]
    fn test_record_diagnostic_and_emit_writes_first_then_emits_same_event() {
        let dir = TempDir::new().unwrap();
        let workspace = dir.path();

        let event = DiagnosticEvent::new("diagnostics.event.received", DiagnosticSource::Backend)
            .conversation_id("conv_1")
            .run_id("run_1")
            .payload(serde_json::json!({"source":"backend"}));

        let mut emitted: Option<DiagnosticEvent> = None;
        record_diagnostic_and_emit_with(workspace, event.clone(), |payload| {
            emitted = Some(payload.clone());
            Ok(())
        });

        let raw = std::fs::read_to_string(metrics_path(workspace)).unwrap();
        let value: serde_json::Value = serde_json::from_str(raw.lines().next().unwrap()).unwrap();

        assert_eq!(value["event"], "diagnostics.event.received");
        assert_eq!(value["conversationId"], "conv_1");
        assert_eq!(value["runId"], "run_1");
        assert_eq!(value["payload"]["source"], "backend");
        assert_eq!(
            emitted.as_ref().map(|event| event.event.as_str()),
            Some("diagnostics.event.received")
        );
        assert_eq!(
            emitted
                .as_ref()
                .and_then(|event| event.conversation_id.as_deref()),
            Some("conv_1")
        );
    }

    #[test]
    fn test_record_diagnostic_writes_plain_jq_parseable_jsonl() {
        let dir = TempDir::new().unwrap();
        let workspace = dir.path();

        record_diagnostic(
            workspace,
            DiagnosticEvent::new("tool.execute.failed", DiagnosticSource::Backend)
                .conversation_id("conv_1")
                .run_id("run_1"),
        );

        let raw = std::fs::read_to_string(metrics_path(workspace)).unwrap();
        assert!(!raw.contains('\t'));
        for line in raw.lines() {
            serde_json::from_str::<serde_json::Value>(line).unwrap();
        }
    }

    #[test]
    fn test_record_diagnostic_redacts_secret_values() {
        let dir = TempDir::new().unwrap();
        let workspace = dir.path();

        let event = DiagnosticEvent::new("ipc.invoke.started", DiagnosticSource::Frontend).payload(
            serde_json::json!({
                "authorization": "Bearer secret-token",
                "apiKey": "sk-secret",
                "nested": {"password": "123456", "safe": "visible"}
            }),
        );

        record_diagnostic(workspace, event);

        let raw = std::fs::read_to_string(metrics_path(workspace)).unwrap();
        assert!(!raw.contains("secret-token"));
        assert!(!raw.contains("sk-secret"));
        assert!(!raw.contains("123456"));
        assert!(raw.contains("[REDACTED]"));
        assert!(raw.contains("visible"));
    }

    #[test]
    fn test_record_diagnostic_redacts_secret_strings_and_error() {
        let dir = TempDir::new().unwrap();
        let workspace = dir.path();

        let event = DiagnosticEvent::new("ipc.invoke.failed", DiagnosticSource::Frontend)
            .command("curl -H 'Authorization: Bearer command-secret' https://example.test")
            .error("Authorization: Bearer string-secret")
            .payload(serde_json::json!({
                "header": "Authorization: Bearer nested-secret",
                "url": "https://example.test?access_token=query-secret&ok=1",
                "key": "sk-live-secret"
            }));

        record_diagnostic(workspace, event);

        let raw = std::fs::read_to_string(metrics_path(workspace)).unwrap();
        assert!(!raw.contains("string-secret"));
        assert!(!raw.contains("command-secret"));
        assert!(!raw.contains("nested-secret"));
        assert!(!raw.contains("query-secret"));
        assert!(!raw.contains("sk-live-secret"));
        assert!(raw.contains("[REDACTED]"));
    }

    #[test]
    fn test_existing_metrics_record_still_uses_fields_shape() {
        let dir = TempDir::new().unwrap();
        let workspace = dir.path();

        record("tool", workspace, &[("conv", "c1")]);

        let raw = std::fs::read_to_string(metrics_path(workspace)).unwrap();
        let value = parse_jsonl_line(raw.lines().next().unwrap());
        assert_eq!(value["category"], "tool");
        assert_eq!(value["fields"]["conv"], "c1");
        assert!(value.get("event").is_none());
    }

    #[test]
    fn test_export_and_info_include_mixed_metrics_and_diagnostics() {
        let dir = TempDir::new().unwrap();
        let workspace = dir.path();

        record("tool", workspace, &[("conv", "c1")]);
        record_diagnostic(
            workspace,
            DiagnosticEvent::new("turn.completed", DiagnosticSource::Backend)
                .conversation_id("conv_1"),
        );

        let (json, count) = export_all(workspace).unwrap();
        assert_eq!(count, 2);
        assert!(json.contains("tool"));
        assert!(json.contains("turn.completed"));
        assert!(json.contains("conversationId"));

        let (info_count, bytes) = get_info(workspace).unwrap();
        assert_eq!(info_count, 2);
        assert!(bytes > 0);
    }
}
