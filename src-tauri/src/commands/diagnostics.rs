//! Diagnostic log upload — splits client logs into bounded chunks and uploads
//! them to the gateway's `/v1/diagnostics` endpoint so they land in SLS for
//! support investigation.
//!
//! The chunking helpers here are pure functions covered by unit tests; the
//! `upload_diagnostic_logs` Tauri command wires them up to filesystem reads
//! + reqwest + AuthManager.

use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tauri::State;

use crate::auth::AuthManager;
use crate::storage::{AiJiaHome, file_manager::FileManager};

/// Server-side per-request limits (mirrors `code/api-gateway/internal/handler/diagnostics.go`).
const MAX_APP_LOG_BYTES_PER_CHUNK: usize = 256 * 1024;
const MAX_EVENTS_PER_CHUNK: usize = 500;

/// Default upload destination. Same gateway used for chat/search etc.
const DIAGNOSTICS_URL: &str = "https://ai-tenant.renlijia.com/v1/diagnostics";

/// Split a raw app-log string into UTF-8-safe, line-aligned chunks no larger
/// than `max_bytes` each. A single logical line is never split across chunks
/// even if it exceeds `max_bytes` (oversize lines stay in their own chunk).
pub fn chunk_app_log(raw: &str, max_bytes: usize) -> Vec<String> {
    if raw.is_empty() {
        return Vec::new();
    }

    let mut chunks: Vec<String> = Vec::new();
    let mut current = String::new();

    for line in raw.split_inclusive('\n') {
        // Oversize line: flush whatever we have, then push the line on its own.
        if line.len() > max_bytes {
            if !current.is_empty() {
                chunks.push(std::mem::take(&mut current));
            }
            chunks.push(line.to_string());
            continue;
        }
        if current.len() + line.len() > max_bytes {
            chunks.push(std::mem::take(&mut current));
        }
        current.push_str(line);
    }

    if !current.is_empty() {
        chunks.push(current);
    }

    chunks
}

/// Split a slice of JSONL event lines into chunks of at most
/// `max_per_chunk` events. Empty input yields an empty Vec.
pub fn chunk_events(events: &[String], max_per_chunk: usize) -> Vec<Vec<String>> {
    if events.is_empty() || max_per_chunk == 0 {
        return Vec::new();
    }
    events
        .chunks(max_per_chunk)
        .map(|slice| slice.to_vec())
        .collect()
}

/// Parse the raw `metrics.jsonl` text. Returns `(valid_events, bad_lines)`:
/// - `valid_events` are successfully parsed JSON values, sent via the `events` field
/// - `bad_lines` are unparseable lines tagged with a `[BAD_METRICS_LINE]` prefix,
///   sent via `app_log` so support can still see the original text in SLS
///   (typical cause: the file was being written when the app crashed and the
///   trailing line is truncated mid-JSON).
///
/// Blank lines are silently dropped from both buckets.
pub fn parse_metrics_lines(raw: &str) -> (Vec<serde_json::Value>, Vec<String>) {
    let mut valid: Vec<serde_json::Value> = Vec::new();
    let mut bad: Vec<String> = Vec::new();
    for line in raw.lines() {
        if line.trim().is_empty() {
            continue;
        }
        match serde_json::from_str::<serde_json::Value>(line) {
            Ok(value) => valid.push(value),
            Err(_) => bad.push(format!("[BAD_METRICS_LINE] {line}")),
        }
    }
    (valid, bad)
}

#[derive(Debug, Serialize)]
struct DiagnosticsChunkPayload<'a> {
    client_version: &'a str,
    os: &'a str,
    report_reason: &'a str,
    upload_session_id: &'a str,
    chunk_index: usize,
    chunk_total: usize,
    #[serde(skip_serializing_if = "str::is_empty")]
    app_log: &'a str,
    #[serde(skip_serializing_if = "<[serde_json::Value]>::is_empty")]
    events: &'a [serde_json::Value],
}

#[derive(Debug, Serialize, Deserialize)]
pub struct UploadDiagnosticsResult {
    pub session_id: String,
    pub chunks_uploaded: usize,
    pub chunks_total: usize,
    pub events_uploaded: usize,
    pub app_log_lines_uploaded: usize,
    /// Number of metrics.jsonl lines that failed to parse and were sent as
    /// `[BAD_METRICS_LINE]` entries via app_log instead of being dropped.
    pub bad_metrics_lines: usize,
}

/// Tauri command — read local diagnostic logs, split into chunks, and upload
/// them to the gateway. Returns a small summary so the UI can show "uploaded
/// 12/12 chunks" toast.
#[tauri::command]
pub async fn upload_diagnostic_logs(
    auth: State<'_, Arc<AuthManager>>,
    aijia_home: State<'_, Arc<AiJiaHome>>,
    file_mgr: State<'_, Arc<FileManager>>,
) -> Result<UploadDiagnosticsResult, String> {
    let session_key = auth
        .get_session_key()
        .await
        .map_err(|e| format!("无法获取登录凭证: {e}"))?;

    // Read the active tauri-plugin-log file (KeepOne rotation, single file).
    let app_log_path = aijia_home.root().join("logs").join("renlijia.log");
    let app_log_raw = std::fs::read_to_string(&app_log_path).unwrap_or_default();

    // Read metrics.jsonl (active shard only — rotated `metrics.{N}.jsonl`
    // files are intentionally skipped; the client team treats them as
    // archived and out-of-scope per product decision).
    let metrics_path = file_mgr.workspace_path().join("logs").join("metrics.jsonl");
    let metrics_raw = std::fs::read_to_string(&metrics_path).unwrap_or_default();

    // Parse metrics lines up-front so we can also recover unparseable rows
    // (truncated / corrupt JSONL) as plain text for support — silently
    // dropping them was hiding real diagnostic data.
    let (parsed_events, bad_metrics_lines) = parse_metrics_lines(&metrics_raw);
    let event_strings: Vec<serde_json::Value> = parsed_events;

    // Combine: app_log_raw (renlijia.log) + bad metrics lines, joined with
    // newlines so the existing chunking helper handles size limits.
    let combined_app_log = if bad_metrics_lines.is_empty() {
        app_log_raw.clone()
    } else {
        let mut combined = app_log_raw.clone();
        if !combined.is_empty() && !combined.ends_with('\n') {
            combined.push('\n');
        }
        for line in &bad_metrics_lines {
            combined.push_str(line);
            combined.push('\n');
        }
        combined
    };

    let app_chunks = chunk_app_log(&combined_app_log, MAX_APP_LOG_BYTES_PER_CHUNK);
    // chunk_events still operates on a flat list; reuse the existing helper
    // by handing it a Vec of pre-serialized strings, then re-deserialize per
    // chunk below. Simpler than introducing a parallel helper for Values.
    let event_lines_for_chunking: Vec<String> = event_strings
        .iter()
        .map(|v| v.to_string())
        .collect();
    let event_chunks = chunk_events(&event_lines_for_chunking, MAX_EVENTS_PER_CHUNK);
    let chunks_total = app_chunks.len() + event_chunks.len();

    if chunks_total == 0 {
        return Err("没有可上传的日志（renlijia.log 与 metrics.jsonl 均为空）".to_string());
    }

    let session_id = uuid::Uuid::new_v4().to_string();
    let client_version = env!("CARGO_PKG_VERSION");
    let os = std::env::consts::OS;
    let report_reason = "user_upload";

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|e| format!("初始化 HTTP 客户端失败: {e}"))?;

    let mut chunks_uploaded = 0usize;
    let mut chunk_index = 0usize;

    // 1) app_log chunks first.
    for chunk in &app_chunks {
        let payload = DiagnosticsChunkPayload {
            client_version,
            os,
            report_reason,
            upload_session_id: &session_id,
            chunk_index,
            chunk_total: chunks_total,
            app_log: chunk,
            events: &[],
        };
        post_chunk(&client, &session_key, &payload).await?;
        chunks_uploaded += 1;
        chunk_index += 1;
    }

    // 2) events chunks. Lines were already parsed up front and re-serialized
    // for chunking; re-parse here to send real JSON values.
    for events_block in &event_chunks {
        let parsed: Vec<serde_json::Value> = events_block
            .iter()
            .filter_map(|line| serde_json::from_str(line).ok())
            .collect();

        let payload = DiagnosticsChunkPayload {
            client_version,
            os,
            report_reason,
            upload_session_id: &session_id,
            chunk_index,
            chunk_total: chunks_total,
            app_log: "",
            events: &parsed,
        };
        post_chunk(&client, &session_key, &payload).await?;
        chunks_uploaded += 1;
        chunk_index += 1;
    }

    let app_log_lines_uploaded = app_log_raw
        .lines()
        .filter(|l| !l.trim().is_empty())
        .count();

    Ok(UploadDiagnosticsResult {
        session_id,
        chunks_uploaded,
        chunks_total,
        events_uploaded: event_strings.len(),
        app_log_lines_uploaded,
        bad_metrics_lines: bad_metrics_lines.len(),
    })
}

async fn post_chunk(
    client: &reqwest::Client,
    session_key: &str,
    payload: &DiagnosticsChunkPayload<'_>,
) -> Result<(), String> {
    let resp = client
        .post(DIAGNOSTICS_URL)
        .bearer_auth(session_key)
        .json(payload)
        .send()
        .await
        .map_err(|e| format!("上传失败: {e}"))?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        return Err(format!(
            "上传失败 chunk={} 状态={status}: {body}",
            payload.chunk_index
        ));
    }
    Ok(())
}

// ─── Background incremental upload ────────────────────────────────────────
//
// The interactive `upload_diagnostic_logs` Tauri command above always uploads
// the *entire* current log set (renlijia.log + active metrics shard). For
// startup / periodic / error-driven auto-upload we instead only ship bytes
// the server has not seen, tracked via `~/.renlijia/.diag-watermark.json`.
// Existing tests on `chunk_app_log` / `chunk_events` / `parse_metrics_lines`
// continue to apply because incremental uploads reuse those helpers.

/// Persistent cursor for the auto-upload pipeline. Written atomically after a
/// successful upload so partial failures (e.g. mid-batch network drop) leave
/// the watermark untouched and the next attempt re-sends the same window.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DiagWatermark {
    /// Byte offset into the active `renlijia.log` (the tauri-plugin-log file
    /// uses KeepOne rotation, so a single file is the only source).
    #[serde(default)]
    pub renlijia_log_offset: u64,
    /// Byte offset into the active `metrics.jsonl` shard. Rotated shards
    /// (`metrics.{N}.jsonl`) are intentionally excluded — see the comment on
    /// `upload_diagnostic_logs`.
    #[serde(default)]
    pub metrics_jsonl_offset: u64,
    /// Last time an upload attempt completed (success or no-op).
    #[serde(default)]
    pub last_upload_at: String,
}

const WATERMARK_FILENAME: &str = ".diag-watermark.json";
/// Cap the chunks shipped per auto-upload tick so a backlog (e.g. first run
/// after a long-lived install) doesn't slam the gateway. Surplus stays in
/// the file; the next tick picks up where this one left off.
const MAX_CHUNKS_PER_AUTO_UPLOAD: usize = 50;

fn watermark_path(aijia_home: &AiJiaHome) -> std::path::PathBuf {
    aijia_home.root().join(WATERMARK_FILENAME)
}

fn load_watermark(aijia_home: &AiJiaHome) -> DiagWatermark {
    let path = watermark_path(aijia_home);
    match std::fs::read_to_string(&path) {
        Ok(raw) => serde_json::from_str(&raw).unwrap_or_default(),
        Err(_) => DiagWatermark::default(),
    }
}

fn save_watermark(aijia_home: &AiJiaHome, wm: &DiagWatermark) -> Result<(), String> {
    let path = watermark_path(aijia_home);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("watermark mkdir failed: {e}"))?;
    }
    let tmp = path.with_extension("json.tmp");
    let raw = serde_json::to_string_pretty(wm).map_err(|e| format!("watermark serialize: {e}"))?;
    std::fs::write(&tmp, raw).map_err(|e| format!("watermark write tmp: {e}"))?;
    std::fs::rename(&tmp, &path).map_err(|e| format!("watermark rename: {e}"))?;
    Ok(())
}

/// Read a UTF-8 file slice starting at `start_byte`. If the file shrank
/// (rotation, manual delete) we reset the cursor to 0 and return the whole
/// thing. Returns `(content, new_offset)`.
fn read_from_offset(path: &std::path::Path, start_byte: u64) -> (String, u64) {
    let metadata = match std::fs::metadata(path) {
        Ok(md) => md,
        Err(_) => return (String::new(), 0),
    };
    let file_len = metadata.len();
    if file_len == 0 {
        return (String::new(), 0);
    }
    // File got smaller — assume rotated; restart from 0.
    let effective_start = if start_byte > file_len { 0 } else { start_byte };
    let raw = match std::fs::read(path) {
        Ok(bytes) => bytes,
        Err(_) => return (String::new(), 0),
    };
    let actual_len = raw.len() as u64;
    let slice = &raw[effective_start as usize..];
    // Walk back from the slice end to a UTF-8 char boundary, but only if we
    // are not at the absolute file end (we want to keep mid-line content if
    // the writer is mid-flush; partial last line gets re-sent next tick).
    let safe_end = utf8_safe_truncate(slice);
    let new_offset = effective_start + safe_end as u64;
    let content = String::from_utf8_lossy(&slice[..safe_end]).into_owned();
    let _ = actual_len; // suppress unused if the file shrank further mid-read
    (content, new_offset)
}

fn utf8_safe_truncate(bytes: &[u8]) -> usize {
    if bytes.is_empty() {
        return 0;
    }
    // Drop everything after the last newline so we never split a log line.
    if let Some(last_nl) = bytes.iter().rposition(|b| *b == b'\n') {
        return last_nl + 1;
    }
    // No newline yet — file is mid-write; defer the whole slice to next tick.
    0
}

/// Run one incremental upload pass. Reads only bytes after the persisted
/// watermark, splits + uploads them, and advances the watermark on success.
/// Designed to be called from a background tokio task (startup + periodic
/// timer + error-driven nudge). Returns the number of chunks shipped (0 if
/// nothing new to send).
pub async fn upload_incremental(
    auth: &Arc<AuthManager>,
    aijia_home: &Arc<AiJiaHome>,
    file_mgr: &Arc<FileManager>,
    report_reason: &str,
) -> Result<usize, String> {
    let session_key = match auth.get_session_key().await {
        Ok(k) => k,
        Err(_) => return Ok(0), // not logged in yet — skip silently
    };

    let wm_before = load_watermark(aijia_home);

    let app_log_path = aijia_home.root().join("logs").join("renlijia.log");
    let (app_log_slice, new_app_offset) =
        read_from_offset(&app_log_path, wm_before.renlijia_log_offset);

    let metrics_path = file_mgr.workspace_path().join("logs").join("metrics.jsonl");
    let (metrics_slice, new_metrics_offset) =
        read_from_offset(&metrics_path, wm_before.metrics_jsonl_offset);

    if app_log_slice.is_empty() && metrics_slice.is_empty() {
        return Ok(0);
    }

    let (parsed_events, bad_metrics_lines) = parse_metrics_lines(&metrics_slice);
    let combined_app_log = if bad_metrics_lines.is_empty() {
        app_log_slice
    } else {
        let mut combined = app_log_slice;
        if !combined.is_empty() && !combined.ends_with('\n') {
            combined.push('\n');
        }
        for line in &bad_metrics_lines {
            combined.push_str(line);
            combined.push('\n');
        }
        combined
    };

    let mut app_chunks = chunk_app_log(&combined_app_log, MAX_APP_LOG_BYTES_PER_CHUNK);
    let event_lines_for_chunking: Vec<String> =
        parsed_events.iter().map(|v| v.to_string()).collect();
    let mut event_chunks = chunk_events(&event_lines_for_chunking, MAX_EVENTS_PER_CHUNK);

    // Cap per-tick traffic. If we truncate, we DON'T advance the watermark
    // past the kept slice — the next tick will re-pick the leftovers.
    let total_planned = app_chunks.len() + event_chunks.len();
    let truncated = total_planned > MAX_CHUNKS_PER_AUTO_UPLOAD;
    if truncated {
        let keep_app = app_chunks.len().min(MAX_CHUNKS_PER_AUTO_UPLOAD);
        app_chunks.truncate(keep_app);
        let remaining_budget = MAX_CHUNKS_PER_AUTO_UPLOAD - keep_app;
        event_chunks.truncate(remaining_budget);
    }

    let chunks_total = app_chunks.len() + event_chunks.len();
    if chunks_total == 0 {
        return Ok(0);
    }

    let session_id = uuid::Uuid::new_v4().to_string();
    let client_version = env!("CARGO_PKG_VERSION");
    let os = std::env::consts::OS;

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|e| format!("初始化 HTTP 客户端失败: {e}"))?;

    let mut chunk_index = 0usize;
    for chunk in &app_chunks {
        let payload = DiagnosticsChunkPayload {
            client_version,
            os,
            report_reason,
            upload_session_id: &session_id,
            chunk_index,
            chunk_total: chunks_total,
            app_log: chunk,
            events: &[],
        };
        post_chunk(&client, &session_key, &payload).await?;
        chunk_index += 1;
    }
    for events_block in &event_chunks {
        let parsed: Vec<serde_json::Value> = events_block
            .iter()
            .filter_map(|line| serde_json::from_str(line).ok())
            .collect();
        let payload = DiagnosticsChunkPayload {
            client_version,
            os,
            report_reason,
            upload_session_id: &session_id,
            chunk_index,
            chunk_total: chunks_total,
            app_log: "",
            events: &parsed,
        };
        post_chunk(&client, &session_key, &payload).await?;
        chunk_index += 1;
    }

    // Only advance the watermark if we shipped the full slice. When
    // truncated, the surplus tail stays for the next tick.
    let next_wm = if truncated {
        wm_before
    } else {
        DiagWatermark {
            renlijia_log_offset: new_app_offset,
            metrics_jsonl_offset: new_metrics_offset,
            last_upload_at: chrono::Utc::now().to_rfc3339(),
        }
    };
    save_watermark(aijia_home, &next_wm)?;

    Ok(chunks_total)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn utf8_safe_truncate_keeps_last_complete_line() {
        // Bytes have two complete lines and a partial third — truncate keeps the two.
        let bytes = b"line1\nline2\npartial";
        assert_eq!(utf8_safe_truncate(bytes), 12); // "line1\nline2\n"
    }

    #[test]
    fn utf8_safe_truncate_returns_zero_when_no_newline() {
        assert_eq!(utf8_safe_truncate(b"partial-only"), 0);
        assert_eq!(utf8_safe_truncate(b""), 0);
    }

    #[test]
    fn read_from_offset_resets_on_file_shrink() {
        // Watermark says we read past byte 100, but the actual file is 10 bytes
        // (rotated). Behaviour: reset to 0 and return the whole file.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("shard.log");
        std::fs::write(&path, "fresh\n").unwrap();
        let (content, new_offset) = read_from_offset(&path, 100);
        assert_eq!(content, "fresh\n");
        assert_eq!(new_offset, 6);
    }

    #[test]
    fn read_from_offset_only_returns_complete_lines() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("shard.log");
        std::fs::write(&path, "line1\nline2\nin-progress-no-newline").unwrap();
        let (content, new_offset) = read_from_offset(&path, 0);
        assert_eq!(content, "line1\nline2\n");
        assert_eq!(new_offset, 12);
    }

    #[test]
    fn watermark_round_trips() {
        let dir = tempfile::tempdir().unwrap();
        let home = AiJiaHome::from_path(dir.path().to_path_buf());
        let wm = DiagWatermark {
            renlijia_log_offset: 4242,
            metrics_jsonl_offset: 99,
            last_upload_at: "2026-05-11T01:00:00Z".to_string(),
        };
        save_watermark(&home, &wm).expect("save ok");
        let loaded = load_watermark(&home);
        assert_eq!(loaded.renlijia_log_offset, 4242);
        assert_eq!(loaded.metrics_jsonl_offset, 99);
        assert_eq!(loaded.last_upload_at, "2026-05-11T01:00:00Z");
    }

    #[test]
    fn watermark_missing_file_defaults_to_zero() {
        let dir = tempfile::tempdir().unwrap();
        let home = AiJiaHome::from_path(dir.path().to_path_buf());
        let loaded = load_watermark(&home);
        assert_eq!(loaded.renlijia_log_offset, 0);
        assert_eq!(loaded.metrics_jsonl_offset, 0);
    }

    #[test]
    fn chunk_app_log_empty_returns_empty() {
        assert!(chunk_app_log("", 1024).is_empty());
    }

    #[test]
    fn chunk_app_log_single_chunk_when_under_limit() {
        let chunks = chunk_app_log("line1\nline2\n", 1024);
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0], "line1\nline2\n");
    }

    #[test]
    fn chunk_app_log_splits_on_line_boundary_when_over_limit() {
        // Each line is 6 bytes ("line1\n"). With a 7-byte cap each chunk
        // can hold exactly one line.
        let raw = "line1\nline2\nline3\n";
        let chunks = chunk_app_log(raw, 7);
        assert_eq!(chunks.len(), 3);
        assert_eq!(chunks[0], "line1\n");
        assert_eq!(chunks[1], "line2\n");
        assert_eq!(chunks[2], "line3\n");
    }

    #[test]
    fn chunk_app_log_oversize_line_kept_in_own_chunk() {
        let big = "x".repeat(100);
        let raw = format!("short\n{big}\nshort2\n");
        let chunks = chunk_app_log(&raw, 10);
        // Expected layout: ["short\n"], [big without \n + "\n"], ["short2\n"]
        assert!(chunks.len() >= 3, "got {} chunks: {:?}", chunks.len(), chunks);
        assert!(chunks.iter().any(|c| c.contains(&big)));
    }

    #[test]
    fn chunk_app_log_no_trailing_newline() {
        let chunks = chunk_app_log("only_line", 1024);
        assert_eq!(chunks, vec!["only_line".to_string()]);
    }

    #[test]
    fn chunk_app_log_packs_multiple_short_lines_into_one_chunk() {
        // 3 lines of 6 bytes = 18 bytes total, cap 20 -> all fit in one chunk.
        let raw = "line1\nline2\nline3\n";
        let chunks = chunk_app_log(raw, 20);
        assert_eq!(chunks.len(), 1);
    }

    #[test]
    fn chunk_events_empty_returns_empty() {
        assert!(chunk_events(&[], 500).is_empty());
    }

    #[test]
    fn chunk_events_single_chunk_when_under_limit() {
        let events: Vec<String> = (0..10).map(|i| format!("e{i}")).collect();
        let chunks = chunk_events(&events, 500);
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].len(), 10);
    }

    #[test]
    fn chunk_events_splits_at_max_per_chunk() {
        let events: Vec<String> = (0..1200).map(|i| format!("e{i}")).collect();
        let chunks = chunk_events(&events, 500);
        assert_eq!(chunks.len(), 3);
        assert_eq!(chunks[0].len(), 500);
        assert_eq!(chunks[1].len(), 500);
        assert_eq!(chunks[2].len(), 200);
    }

    #[test]
    fn chunk_events_zero_max_returns_empty() {
        let events: Vec<String> = vec!["a".into()];
        assert!(chunk_events(&events, 0).is_empty());
    }

    #[test]
    fn parse_metrics_lines_returns_parsed_and_drops_blank() {
        let raw = "{\"a\":1}\n\n{\"b\":2}\n   \n";
        let (parsed, bad) = parse_metrics_lines(raw);
        assert_eq!(parsed.len(), 2);
        assert!(bad.is_empty());
    }

    #[test]
    fn parse_metrics_lines_collects_bad_lines_with_marker() {
        let raw = "{\"good\":1}\nthis is not json\n{\"good\":2}\n}}}\n";
        let (parsed, bad) = parse_metrics_lines(raw);
        assert_eq!(parsed.len(), 2, "should keep 2 valid JSON lines");
        assert_eq!(bad.len(), 2, "should collect 2 bad lines");
        assert!(bad[0].starts_with("[BAD_METRICS_LINE]"));
        assert!(bad[0].contains("this is not json"));
        assert!(bad[1].contains("}}}"));
    }

    #[test]
    fn parse_metrics_lines_empty_input_returns_empty_pair() {
        let (parsed, bad) = parse_metrics_lines("");
        assert!(parsed.is_empty());
        assert!(bad.is_empty());
    }
}
