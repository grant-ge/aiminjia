# Conversation Export Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a chat-page “导出对话” flow that creates a local zip containing user-readable HTML plus full raw diagnostics logs for engineering troubleshooting.

**Architecture:** Keep export assembly in a focused Rust runtime service, expose it through thin Tauri commands, then wire a small React dialog from the chat top bar. The exporter writes a temporary directory under `~/.renlijia/exports/conversations/tmp/{id}`, renders files, zips them, and returns the final zip path for reveal/copy actions.

**Tech Stack:** React 19, TypeScript, Vitest, Tauri 2, Rust, `zip = "2"`, JSONL file storage, `~/.renlijia/logs/metrics*.jsonl`, `renlijia.log`, `gate.log`.

---

## File Structure

- Create `src-tauri/src/runtime/export/mod.rs`
  - Owns the runtime export module namespace.
- Create `src-tauri/src/runtime/export/conversation_exporter.rs`
  - Pure Rust export service. Reads messages/logs, renders HTML/text/manifest, writes zip.
- Create `src-tauri/src/transport/tauri_commands/conversation_export.rs`
  - Thin Tauri command adapter. Calls runtime exporter and reveals a finished zip in OS file manager.
- Modify `src-tauri/src/runtime/mod.rs`
  - Expose the new `export` module.
- Modify `src-tauri/src/transport/tauri_commands/mod.rs`
  - Expose the new `conversation_export` command module.
- Modify `src-tauri/src/lib.rs`
  - Register `export_conversation` and `reveal_export_in_folder`.
- Create `src-tauri/tests/conversation_export_test.rs`
  - Integration tests for zip content, log inclusion, diagnostics filtering, and missing-log behavior.
- Modify `src/lib/tauri.ts`
  - Add typed wrappers and optional progress event typing.
- Create `src/components/chat/ConversationExportDialog.tsx`
  - Confirmation, progress, success, and failure UI.
- Create `src/components/chat/ConversationExportDialog.test.tsx`
  - UI state and callback tests.
- Modify `src/components/shell/ChatTopBar.tsx`
  - Add an export action prop rendered in the right action area.
- Modify `src/components/shell/ChatTopBar.test.tsx`
  - Verify export button rendering and callback.
- Modify `src/features/chat/ChatPage.tsx`
  - Manage dialog state and call export/reveal IPC for the active conversation.
- Modify `src/features/chat/ChatPage.test.tsx`
  - Verify ChatPage passes export action and handles export flow.

---

## Task 1: Rust Exporter Core

**Files:**
- Create: `src-tauri/src/runtime/export/mod.rs`
- Create: `src-tauri/src/runtime/export/conversation_exporter.rs`
- Modify: `src-tauri/src/runtime/mod.rs`
- Test: `src-tauri/tests/conversation_export_test.rs`

- [ ] **Step 1: Create failing exporter tests**

Add `src-tauri/tests/conversation_export_test.rs`:

```rust
use app_lib::runtime::export::conversation_exporter::{
    ConversationExportRequest, ConversationExporter, ExportPaths,
};
use app_lib::storage::file_store::types::StoredMessage;
use app_lib::storage::file_store::AppStorage;
use tempfile::TempDir;
use zip::ZipArchive;

fn write_file(path: &std::path::Path, content: &str) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(path, content).unwrap();
}

fn read_zip_entry(zip_path: &std::path::Path, name: &str) -> String {
    let file = std::fs::File::open(zip_path).unwrap();
    let mut archive = ZipArchive::new(file).unwrap();
    let mut entry = archive.by_name(name).unwrap();
    let mut out = String::new();
    std::io::Read::read_to_string(&mut entry, &mut out).unwrap();
    out
}

fn zip_names(zip_path: &std::path::Path) -> Vec<String> {
    let file = std::fs::File::open(zip_path).unwrap();
    let mut archive = ZipArchive::new(file).unwrap();
    let mut names = Vec::new();
    for index in 0..archive.len() {
        names.push(archive.by_index(index).unwrap().name().to_string());
    }
    names.sort();
    names
}

fn insert_message(storage: &AppStorage, id: &str, conversation_id: &str, role: &str, text: &str) {
    let msg = StoredMessage {
        seq: None,
        rev: None,
        id: id.to_string(),
        conversation_id: conversation_id.to_string(),
        role: role.to_string(),
        content: serde_json::json!({ "text": text }),
        created_at: "2026-06-03T00:00:00Z".to_string(),
        tool_calls: None,
        tool_call_id: None,
        name: None,
        run_id: Some("run-1".to_string()),
        schema_version: Some(2),
        sequence: None,
        error: None,
    };
    storage.insert_chat_message_record(&msg).unwrap();
}

#[test]
fn export_zip_contains_readable_html_manifest_raw_messages_and_full_logs() {
    let dir = TempDir::new().unwrap();
    let app_home = dir.path().join("home");
    let export_root = dir.path().join("exports");
    let storage = AppStorage::new(&app_home).unwrap();

    storage.create_conversation("conv-1", "Rust <debug>").unwrap();
    insert_message(&storage, "m-user", "conv-1", "user", "你好 <script>alert(1)</script>");
    insert_message(&storage, "m-assistant", "conv-1", "assistant", "已收到");

    let logs_dir = app_home.join("logs");
    write_file(
        &logs_dir.join("metrics.jsonl"),
        r#"{"category":"diagnostics","ts":"2026-06-03T00:00:00Z","source":"backend","level":"info","event":"turn.started","conversationId":"conv-1","runId":"run-1","ok":true}"#,
    );
    write_file(
        &logs_dir.join("metrics.1.jsonl"),
        r#"{"category":"diagnostics","ts":"2026-06-03T00:00:01Z","source":"backend","level":"error","event":"tool.execute.failed","conversationId":"other-conv","runId":"run-2","ok":false,"error":"boom"}"#,
    );
    write_file(&logs_dir.join("renlijia.log"), "runtime log body\n");
    write_file(&logs_dir.join("gate.log"), "gate log body\n");

    let exporter = ConversationExporter::new(ExportPaths {
        app_home: app_home.clone(),
        export_root: export_root.clone(),
    });
    let result = exporter
        .export(&storage, ConversationExportRequest {
            conversation_id: "conv-1".to_string(),
            app_version: "0.5.test".to_string(),
            platform: "test-os".to_string(),
            arch: "test-arch".to_string(),
        })
        .unwrap();

    assert!(result.zip_path.exists());
    assert_eq!(result.file_name, result.zip_path.file_name().unwrap().to_string_lossy());
    assert!(result.size_bytes > 0);

    let names = zip_names(&result.zip_path);
    assert!(names.contains(&"README.txt".to_string()));
    assert!(names.contains(&"conversation.html".to_string()));
    assert!(names.contains(&"diagnostics-summary.html".to_string()));
    assert!(names.contains(&"manifest.json".to_string()));
    assert!(names.contains(&"raw/messages.jsonl".to_string()));
    assert!(names.contains(&"raw/current-conversation-diagnostics.jsonl".to_string()));
    assert!(names.contains(&"raw/recent-warn-error.jsonl".to_string()));
    assert!(names.contains(&"raw/renlijia.log".to_string()));
    assert!(names.contains(&"raw/gate.log".to_string()));

    let html = read_zip_entry(&result.zip_path, "conversation.html");
    assert!(html.contains("Rust &lt;debug&gt;"));
    assert!(html.contains("你好 &lt;script&gt;alert(1)&lt;/script&gt;"));
    assert!(!html.contains("<script>alert(1)</script>"));

    let current_diag = read_zip_entry(&result.zip_path, "raw/current-conversation-diagnostics.jsonl");
    assert!(current_diag.contains("turn.started"));
    assert!(!current_diag.contains("other-conv"));

    let recent = read_zip_entry(&result.zip_path, "raw/recent-warn-error.jsonl");
    assert!(recent.contains("tool.execute.failed"));
    assert!(recent.contains("other-conv"));

    assert_eq!(read_zip_entry(&result.zip_path, "raw/renlijia.log"), "runtime log body\n");
    assert_eq!(read_zip_entry(&result.zip_path, "raw/gate.log"), "gate log body\n");

    let manifest: serde_json::Value =
        serde_json::from_str(&read_zip_entry(&result.zip_path, "manifest.json")).unwrap();
    assert_eq!(manifest["schemaVersion"], 1);
    assert_eq!(manifest["conversation"]["id"], "conv-1");
    assert_eq!(manifest["app"]["version"], "0.5.test");
    assert_eq!(manifest["logs"][0]["included"], true);
}

#[test]
fn export_succeeds_when_logs_are_missing_and_marks_manifest_entries() {
    let dir = TempDir::new().unwrap();
    let app_home = dir.path().join("home");
    let export_root = dir.path().join("exports");
    let storage = AppStorage::new(&app_home).unwrap();

    storage.create_conversation("conv-missing", "No logs").unwrap();
    insert_message(&storage, "m1", "conv-missing", "user", "hello");

    let exporter = ConversationExporter::new(ExportPaths {
        app_home,
        export_root,
    });
    let result = exporter
        .export(&storage, ConversationExportRequest {
            conversation_id: "conv-missing".to_string(),
            app_version: "0.5.test".to_string(),
            platform: "test-os".to_string(),
            arch: "test-arch".to_string(),
        })
        .unwrap();

    let names = zip_names(&result.zip_path);
    assert!(names.contains(&"raw/messages.jsonl".to_string()));
    assert!(!names.contains(&"raw/renlijia.log".to_string()));
    assert!(!names.contains(&"raw/gate.log".to_string()));

    let manifest: serde_json::Value =
        serde_json::from_str(&read_zip_entry(&result.zip_path, "manifest.json")).unwrap();
    let logs = manifest["logs"].as_array().unwrap();
    assert!(logs.iter().any(|entry| {
        entry["name"] == "raw/renlijia.log" && entry["included"] == false
    }));
    assert!(logs.iter().any(|entry| {
        entry["name"] == "raw/gate.log" && entry["included"] == false
    }));
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run:

```bash
cd src-tauri && cargo test --test conversation_export_test -- --nocapture
```

Expected: FAIL with unresolved module/import errors for `app_lib::runtime::export`.

- [ ] **Step 3: Add runtime module declarations**

Create `src-tauri/src/runtime/export/mod.rs`:

```rust
pub mod conversation_exporter;
```

Modify `src-tauri/src/runtime/mod.rs` by adding:

```rust
pub mod export;
```

- [ ] **Step 4: Implement exporter types and helpers**

Create `src-tauri/src/runtime/export/conversation_exporter.rs`:

```rust
use std::fs::{self, File};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};

use anyhow::{anyhow, Context, Result};
use chrono::{DateTime, Duration, Local, Utc};
use serde::{Deserialize, Serialize};
use zip::write::SimpleFileOptions;

use crate::storage::file_store::types::{ConversationMeta, StoredMessage};
use crate::storage::file_store::AppStorage;

#[derive(Debug, Clone)]
pub struct ExportPaths {
    pub app_home: PathBuf,
    pub export_root: PathBuf,
}

#[derive(Debug, Clone)]
pub struct ConversationExportRequest {
    pub conversation_id: String,
    pub app_version: String,
    pub platform: String,
    pub arch: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConversationExportResult {
    pub zip_path: PathBuf,
    pub file_name: String,
    pub size_bytes: u64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct Manifest {
    schema_version: u32,
    exported_at: String,
    app: ManifestApp,
    conversation: ManifestConversation,
    logs: Vec<ManifestLog>,
    files: Vec<ManifestFile>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ManifestApp {
    name: String,
    version: String,
    platform: String,
    arch: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ManifestConversation {
    id: String,
    title: String,
    workspace_name: Option<String>,
    workspace_path: Option<String>,
    updated_at: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ManifestLog {
    name: String,
    source_path: String,
    size_bytes: Option<u64>,
    modified_at: Option<String>,
    included: bool,
    error: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct ManifestFile {
    name: String,
    size_bytes: u64,
}

pub struct ConversationExporter {
    paths: ExportPaths,
}

impl ConversationExporter {
    pub fn new(paths: ExportPaths) -> Self {
        Self { paths }
    }

    pub fn export(
        &self,
        storage: &AppStorage,
        request: ConversationExportRequest,
    ) -> Result<ConversationExportResult> {
        let conversation = storage
            .get_conversation(&request.conversation_id)?
            .ok_or_else(|| anyhow!("conversation not found: {}", request.conversation_id))?;
        let messages = storage.get_messages_v2(&request.conversation_id)?;

        fs::create_dir_all(&self.paths.export_root)?;
        let export_id = uuid::Uuid::new_v4().to_string();
        let temp_dir = self.paths.export_root.join("tmp").join(&export_id);
        if temp_dir.exists() {
            fs::remove_dir_all(&temp_dir).ok();
        }
        fs::create_dir_all(temp_dir.join("raw"))?;

        let result = self.write_export_dir(&temp_dir, &conversation, &messages, &request);
        let result = match result {
            Ok(()) => {
                let file_name = format!(
                    "aijia-conversation-export-{}-{}.zip",
                    safe_file_stem(&conversation.title),
                    Local::now().format("%Y%m%d-%H%M%S")
                );
                let zip_path = self.paths.export_root.join(file_name);
                zip_directory(&temp_dir, &zip_path)?;
                let size_bytes = fs::metadata(&zip_path)?.len();
                let file_name = zip_path
                    .file_name()
                    .map(|name| name.to_string_lossy().to_string())
                    .unwrap_or_else(|| "conversation-export.zip".to_string());
                Ok(ConversationExportResult {
                    zip_path,
                    file_name,
                    size_bytes,
                })
            }
            Err(error) => Err(error),
        };

        fs::remove_dir_all(&temp_dir).ok();
        result
    }

    fn write_export_dir(
        &self,
        temp_dir: &Path,
        conversation: &ConversationMeta,
        messages: &[StoredMessage],
        request: &ConversationExportRequest,
    ) -> Result<()> {
        write_string(&temp_dir.join("README.txt"), &render_readme(conversation));
        write_string(
            &temp_dir.join("conversation.html"),
            &render_conversation_html(conversation, messages, request),
        );

        let metrics = read_metric_records(&self.paths.app_home.join("logs"))?;
        let current_diag = filter_current_conversation_diagnostics(&metrics, &request.conversation_id);
        let recent_warn_error = filter_recent_warn_error(&metrics, Utc::now() - Duration::hours(24));
        write_string(
            &temp_dir.join("raw/current-conversation-diagnostics.jsonl"),
            &current_diag.join("\n"),
        );
        write_string(
            &temp_dir.join("raw/recent-warn-error.jsonl"),
            &recent_warn_error.join("\n"),
        );
        write_string(
            &temp_dir.join("raw/messages.jsonl"),
            &messages
                .iter()
                .map(serde_json::to_string)
                .collect::<std::result::Result<Vec<_>, _>>()?
                .join("\n"),
        );

        let mut log_entries = Vec::new();
        for (raw_name, source_name) in [
            ("raw/renlijia.log", "renlijia.log"),
            ("raw/gate.log", "gate.log"),
        ] {
            log_entries.push(copy_optional_log(
                &self.paths.app_home.join("logs").join(source_name),
                &temp_dir.join(raw_name),
                raw_name,
            ));
        }

        write_string(
            &temp_dir.join("diagnostics-summary.html"),
            &render_diagnostics_summary_html(conversation, &current_diag, &recent_warn_error),
        );

        let manifest = Manifest {
            schema_version: 1,
            exported_at: Local::now().to_rfc3339(),
            app: ManifestApp {
                name: "AI小家".to_string(),
                version: request.app_version.clone(),
                platform: request.platform.clone(),
                arch: request.arch.clone(),
            },
            conversation: ManifestConversation {
                id: conversation.id.clone(),
                title: conversation.title.clone(),
                workspace_name: conversation
                    .authorized_workspace
                    .as_ref()
                    .map(|workspace| workspace.display_name.clone()),
                workspace_path: conversation
                    .authorized_workspace
                    .as_ref()
                    .map(|workspace| workspace.root_path.to_string_lossy().to_string()),
                updated_at: conversation.updated_at.clone(),
            },
            logs: log_entries,
            files: collect_manifest_files(temp_dir)?,
        };
        write_string(
            &temp_dir.join("manifest.json"),
            &serde_json::to_string_pretty(&manifest)?,
        );

        Ok(())
    }
}

fn write_string(path: &Path, content: &str) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("create export parent");
    }
    fs::write(path, content).expect("write export file");
}

fn render_readme(conversation: &ConversationMeta) -> String {
    format!(
        "AI小家对话导出\n\n会话：{}\n\n打开 conversation.html 可以查看对话过程。\nraw/ 目录包含排查问题所需的原始材料。此文件不会自动上传，只有你主动发送后他人才可看到。\n",
        conversation.title
    )
}

fn render_conversation_html(
    conversation: &ConversationMeta,
    messages: &[StoredMessage],
    request: &ConversationExportRequest,
) -> String {
    let rows = messages
        .iter()
        .map(|msg| {
            format!(
                "<article class=\"msg\"><div class=\"meta\">{} · {} · {}</div><pre>{}</pre></article>",
                escape_html(&msg.role),
                escape_html(&msg.created_at),
                escape_html(msg.run_id.as_deref().unwrap_or("")),
                escape_html(msg.text())
            )
        })
        .collect::<Vec<_>>()
        .join("\n");

    format!(
        "<!doctype html><html><head><meta charset=\"utf-8\"><title>{}</title><style>body{{font-family:-apple-system,BlinkMacSystemFont,'Segoe UI',sans-serif;margin:32px;color:#172033}}.msg{{border-bottom:1px solid #d8dee9;padding:14px 0}}.meta{{color:#687386;font-size:12px;margin-bottom:8px}}pre{{white-space:pre-wrap;font:inherit}}</style></head><body><h1>{}</h1><p>conversationId: {} · app: {} · exportedAt: {}</p>{}</body></html>",
        escape_html(&conversation.title),
        escape_html(&conversation.title),
        escape_html(&request.conversation_id),
        escape_html(&request.app_version),
        escape_html(&Local::now().to_rfc3339()),
        rows
    )
}

fn render_diagnostics_summary_html(
    conversation: &ConversationMeta,
    current_diag: &[String],
    recent_warn_error: &[String],
) -> String {
    format!(
        "<!doctype html><html><head><meta charset=\"utf-8\"><title>Diagnostics Summary</title></head><body><h1>{}</h1><p>当前会话 diagnostics: {}</p><p>最近 warn/error/failed: {}</p></body></html>",
        escape_html(&conversation.title),
        current_diag.len(),
        recent_warn_error.len()
    )
}

fn escape_html(input: &str) -> String {
    input
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

fn safe_file_stem(input: &str) -> String {
    let mut out = input
        .chars()
        .map(|ch| if ch.is_ascii_alphanumeric() { ch } else { '-' })
        .collect::<String>();
    while out.contains("--") {
        out = out.replace("--", "-");
    }
    out.trim_matches('-').chars().take(48).collect::<String>().if_empty("conversation")
}

trait IfEmpty {
    fn if_empty(self, fallback: &str) -> String;
}

impl IfEmpty for String {
    fn if_empty(self, fallback: &str) -> String {
        if self.is_empty() { fallback.to_string() } else { self }
    }
}

fn read_metric_records(logs_dir: &Path) -> Result<Vec<serde_json::Value>> {
    let mut paths = list_metric_paths(logs_dir)?;
    paths.sort_by(|a, b| metric_sort_key(a).cmp(&metric_sort_key(b)));
    let mut values = Vec::new();
    for path in paths {
        let file = File::open(&path).with_context(|| format!("open {}", path.display()))?;
        for line in BufReader::new(file).lines() {
            let line = line?;
            let json = line.split_once('\t').map_or(line.as_str(), |(json, _)| json);
            if let Ok(value) = serde_json::from_str::<serde_json::Value>(json) {
                values.push(value);
            }
        }
    }
    Ok(values)
}

fn list_metric_paths(logs_dir: &Path) -> Result<Vec<PathBuf>> {
    if !logs_dir.exists() {
        return Ok(Vec::new());
    }
    let mut paths = Vec::new();
    for entry in fs::read_dir(logs_dir)? {
        let path = entry?.path();
        let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        if name == "metrics.jsonl" || (name.starts_with("metrics.") && name.ends_with(".jsonl")) {
            paths.push(path);
        }
    }
    Ok(paths)
}

fn metric_sort_key(path: &Path) -> (u32, String) {
    let name = path.file_name().and_then(|name| name.to_str()).unwrap_or("");
    if name == "metrics.jsonl" {
        return (u32::MAX, name.to_string());
    }
    let number = name
        .strip_prefix("metrics.")
        .and_then(|rest| rest.strip_suffix(".jsonl"))
        .and_then(|number| number.parse::<u32>().ok())
        .unwrap_or(0);
    (number, name.to_string())
}

fn filter_current_conversation_diagnostics(values: &[serde_json::Value], conversation_id: &str) -> Vec<String> {
    values
        .iter()
        .filter(|value| {
            value.get("category").and_then(|v| v.as_str()) == Some("diagnostics")
                && value.get("conversationId").and_then(|v| v.as_str()) == Some(conversation_id)
        })
        .filter_map(|value| serde_json::to_string(value).ok())
        .collect()
}

fn filter_recent_warn_error(values: &[serde_json::Value], since: DateTime<Utc>) -> Vec<String> {
    values
        .iter()
        .filter(|value| value.get("category").and_then(|v| v.as_str()) == Some("diagnostics"))
        .filter(|value| {
            value
                .get("ts")
                .and_then(|v| v.as_str())
                .and_then(|ts| DateTime::parse_from_rfc3339(ts).ok())
                .map(|ts| ts.with_timezone(&Utc) >= since)
                .unwrap_or(true)
        })
        .filter(|value| {
            matches!(value.get("level").and_then(|v| v.as_str()), Some("warn" | "error"))
                || value.get("ok").and_then(|v| v.as_bool()) == Some(false)
        })
        .filter_map(|value| serde_json::to_string(value).ok())
        .collect()
}

fn copy_optional_log(source: &Path, dest: &Path, name: &str) -> ManifestLog {
    let source_path = source.to_string_lossy().to_string();
    match fs::metadata(source) {
        Ok(meta) => {
            if let Some(parent) = dest.parent() {
                if let Err(error) = fs::create_dir_all(parent) {
                    return ManifestLog {
                        name: name.to_string(),
                        source_path,
                        size_bytes: Some(meta.len()),
                        modified_at: modified_at(&meta),
                        included: false,
                        error: Some(error.to_string()),
                    };
                }
            }
            match fs::copy(source, dest) {
                Ok(_) => ManifestLog {
                    name: name.to_string(),
                    source_path,
                    size_bytes: Some(meta.len()),
                    modified_at: modified_at(&meta),
                    included: true,
                    error: None,
                },
                Err(error) => ManifestLog {
                    name: name.to_string(),
                    source_path,
                    size_bytes: Some(meta.len()),
                    modified_at: modified_at(&meta),
                    included: false,
                    error: Some(error.to_string()),
                },
            }
        }
        Err(error) => ManifestLog {
            name: name.to_string(),
            source_path,
            size_bytes: None,
            modified_at: None,
            included: false,
            error: Some(error.to_string()),
        },
    }
}

fn modified_at(meta: &fs::Metadata) -> Option<String> {
    meta.modified()
        .ok()
        .map(DateTime::<Utc>::from)
        .map(|dt| dt.to_rfc3339())
}

fn collect_manifest_files(root: &Path) -> Result<Vec<ManifestFile>> {
    let mut files = Vec::new();
    collect_manifest_files_rec(root, root, &mut files)?;
    files.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(files)
}

fn collect_manifest_files_rec(root: &Path, dir: &Path, files: &mut Vec<ManifestFile>) -> Result<()> {
    for entry in fs::read_dir(dir)? {
        let path = entry?.path();
        if path.is_dir() {
            collect_manifest_files_rec(root, &path, files)?;
        } else {
            files.push(ManifestFile {
                name: path.strip_prefix(root)?.to_string_lossy().replace('\\', "/"),
                size_bytes: fs::metadata(&path)?.len(),
            });
        }
    }
    Ok(())
}

fn zip_directory(source_dir: &Path, zip_path: &Path) -> Result<()> {
    if let Some(parent) = zip_path.parent() {
        fs::create_dir_all(parent)?;
    }
    let file = File::create(zip_path)?;
    let mut zip = zip::ZipWriter::new(file);
    let options = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    add_dir_to_zip(source_dir, source_dir, &mut zip, options)?;
    zip.finish()?;
    Ok(())
}

fn add_dir_to_zip(
    root: &Path,
    dir: &Path,
    zip: &mut zip::ZipWriter<File>,
    options: SimpleFileOptions,
) -> Result<()> {
    for entry in fs::read_dir(dir)? {
        let path = entry?.path();
        if path.is_dir() {
            add_dir_to_zip(root, &path, zip, options)?;
        } else {
            let name = path.strip_prefix(root)?.to_string_lossy().replace('\\', "/");
            zip.start_file(name, options)?;
            let mut input = File::open(&path)?;
            std::io::copy(&mut input, zip)?;
        }
    }
    Ok(())
}
```

- [ ] **Step 5: Run focused Rust tests**

Run:

```bash
cd src-tauri && cargo test --test conversation_export_test -- --nocapture
```

Expected: PASS for both tests.

- [ ] **Step 6: Commit exporter core**

```bash
git add src-tauri/src/runtime/mod.rs src-tauri/src/runtime/export/mod.rs src-tauri/src/runtime/export/conversation_exporter.rs src-tauri/tests/conversation_export_test.rs
git commit -m "feat: add conversation export package builder"
```

---

## Task 2: Tauri Commands and TypeScript IPC

**Files:**
- Create: `src-tauri/src/transport/tauri_commands/conversation_export.rs`
- Modify: `src-tauri/src/transport/tauri_commands/mod.rs`
- Modify: `src-tauri/src/lib.rs`
- Modify: `src/lib/tauri.ts`
- Test: `src-tauri/tests/conversation_export_test.rs`

- [ ] **Step 1: Add command shape tests**

Append to `src-tauri/tests/conversation_export_test.rs`:

```rust
#[test]
fn export_result_serializes_for_tauri_with_camel_case_path_fields() {
    let value = serde_json::to_value(
        app_lib::runtime::export::conversation_exporter::ConversationExportResult {
            zip_path: std::path::PathBuf::from("/tmp/export.zip"),
            file_name: "export.zip".to_string(),
            size_bytes: 42,
        },
    )
    .unwrap();

    assert_eq!(value["zipPath"], "/tmp/export.zip");
    assert_eq!(value["fileName"], "export.zip");
    assert_eq!(value["sizeBytes"], 42);
}
```

- [ ] **Step 2: Run test to verify serialization issue**

Run:

```bash
cd src-tauri && cargo test --test conversation_export_test export_result_serializes_for_tauri_with_camel_case_path_fields -- --nocapture
```

Expected: FAIL if `PathBuf` does not serialize as desired or field casing is not camelCase.

- [ ] **Step 3: Make export result Tauri-friendly**

Modify `src-tauri/src/runtime/export/conversation_exporter.rs` result type:

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConversationExportResult {
    pub zip_path: String,
    pub file_name: String,
    pub size_bytes: u64,
}
```

Modify the result construction in `export()`:

```rust
Ok(ConversationExportResult {
    zip_path: zip_path.to_string_lossy().to_string(),
    file_name,
    size_bytes,
})
```

Modify tests that call `result.zip_path.exists()`:

```rust
let zip_path = std::path::PathBuf::from(&result.zip_path);
assert!(zip_path.exists());
let names = zip_names(&zip_path);
```

- [ ] **Step 4: Create thin Tauri command adapter**

Create `src-tauri/src/transport/tauri_commands/conversation_export.rs`:

```rust
use std::path::Path;
use std::sync::Arc;

use tauri::{AppHandle, Manager, State};

use crate::runtime::export::conversation_exporter::{
    ConversationExportRequest, ConversationExportResult, ConversationExporter, ExportPaths,
};
use crate::storage::file_store::AppStorage;

#[tauri::command]
pub async fn export_conversation(
    app: AppHandle,
    storage: State<'_, Arc<AppStorage>>,
    conversation_id: String,
) -> Result<ConversationExportResult, String> {
    let package_info = app.package_info();
    let app_home = storage.base_dir().to_path_buf();
    let export_root = app_home.join("exports").join("conversations");
    let exporter = ConversationExporter::new(ExportPaths {
        app_home,
        export_root,
    });
    exporter
        .export(&storage, ConversationExportRequest {
            conversation_id,
            app_version: package_info.version.to_string(),
            platform: std::env::consts::OS.to_string(),
            arch: std::env::consts::ARCH.to_string(),
        })
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn reveal_export_in_folder(path: String) -> Result<(), String> {
    let full_path = Path::new(&path);
    if !full_path.exists() {
        return Err("导出文件不存在或已被移动。".to_string());
    }

    #[cfg(target_os = "macos")]
    std::process::Command::new("open")
        .arg("-R")
        .arg(full_path)
        .spawn()
        .map_err(|e| e.to_string())?;

    #[cfg(target_os = "windows")]
    std::process::Command::new("explorer")
        .arg(format!("/select,{}", full_path.display()))
        .spawn()
        .map_err(|e| e.to_string())?;

    #[cfg(target_os = "linux")]
    {
        let parent = full_path.parent().unwrap_or(full_path);
        std::process::Command::new("xdg-open")
            .arg(parent)
            .spawn()
            .map_err(|e| e.to_string())?;
    }

    Ok(())
}
```

- [ ] **Step 5: Register command module**

Modify `src-tauri/src/transport/tauri_commands/mod.rs`:

```rust
pub mod conversation_export;
```

Modify `src-tauri/src/lib.rs` inside `tauri::generate_handler![...]` near chat/file commands:

```rust
transport::tauri_commands::conversation_export::export_conversation,
transport::tauri_commands::conversation_export::reveal_export_in_folder,
```

- [ ] **Step 6: Add typed frontend IPC wrappers**

Modify `src/lib/tauri.ts` near file command wrappers:

```ts
export interface ExportConversationResult {
  zipPath: string
  fileName: string
  sizeBytes: number
}

export function exportConversation(conversationId: string): Promise<ExportConversationResult> {
  return invoke<ExportConversationResult>('export_conversation', { conversationId })
}

export function revealExportInFolder(path: string): Promise<void> {
  return invoke<void>('reveal_export_in_folder', { path })
}
```

- [ ] **Step 7: Run IPC compile checks**

Run:

```bash
pnpm exec tsc --noEmit
cd src-tauri && cargo check
```

Expected: both PASS.

- [ ] **Step 8: Commit IPC layer**

```bash
git add src-tauri/src/transport/tauri_commands/conversation_export.rs src-tauri/src/transport/tauri_commands/mod.rs src-tauri/src/lib.rs src/lib/tauri.ts src-tauri/tests/conversation_export_test.rs
git commit -m "feat: expose conversation export commands"
```

---

## Task 3: Chat Top Bar Export Entry

**Files:**
- Modify: `src/components/shell/ChatTopBar.tsx`
- Modify: `src/components/shell/ChatTopBar.test.tsx`

- [ ] **Step 1: Add failing ChatTopBar tests**

Append to `src/components/shell/ChatTopBar.test.tsx`:

```tsx
it('renders an export conversation action when provided', () => {
  render(<ChatTopBar title="调试会话" onExportConversation={() => undefined} />)

  expect(screen.getByRole('button', { name: '导出对话' })).toBeInTheDocument()
})

it('invokes export conversation when the action is clicked', () => {
  const onExportConversation = vi.fn()
  render(<ChatTopBar title="调试会话" onExportConversation={onExportConversation} />)

  fireEvent.click(screen.getByRole('button', { name: '导出对话' }))

  expect(onExportConversation).toHaveBeenCalledTimes(1)
})
```

- [ ] **Step 2: Run tests to verify failure**

Run:

```bash
pnpm exec vitest run src/components/shell/ChatTopBar.test.tsx
```

Expected: FAIL because `onExportConversation` prop does not exist and no button renders.

- [ ] **Step 3: Implement export action prop**

Modify imports in `src/components/shell/ChatTopBar.tsx`:

```tsx
import { Download, Ellipsis, Folder, GraduationCap, MessageSquare, PanelLeft, Share2 } from 'lucide-react'
```

Add to `ChatTopBarProps`:

```tsx
onExportConversation?: () => void
```

Add to destructuring:

```tsx
onExportConversation,
```

Render before `onShare` in the right action area:

```tsx
{onExportConversation ? (
  <button
    type="button"
    aria-label="导出对话"
    title="导出对话"
    onClick={onExportConversation}
    className="text-muted-foreground transition-colors hover:text-foreground"
  >
    <Download className="h-4 w-4" />
  </button>
) : null}
```

- [ ] **Step 4: Run ChatTopBar tests**

Run:

```bash
pnpm exec vitest run src/components/shell/ChatTopBar.test.tsx
```

Expected: PASS.

- [ ] **Step 5: Commit top bar entry**

```bash
git add src/components/shell/ChatTopBar.tsx src/components/shell/ChatTopBar.test.tsx
git commit -m "feat: add chat export top bar action"
```

---

## Task 4: Export Dialog UI

**Files:**
- Create: `src/components/chat/ConversationExportDialog.tsx`
- Create: `src/components/chat/ConversationExportDialog.test.tsx`

- [ ] **Step 1: Add failing dialog tests**

Create `src/components/chat/ConversationExportDialog.test.tsx`:

```tsx
import '@testing-library/jest-dom'
import { fireEvent, render, screen } from '@testing-library/react'
import { describe, expect, it, vi } from 'vitest'

import { ConversationExportDialog } from './ConversationExportDialog'

describe('ConversationExportDialog', () => {
  it('explains the export package without calling it a diagnostics package', () => {
    render(
      <ConversationExportDialog
        open
        state={{ status: 'idle' }}
        onOpenChange={() => undefined}
        onStart={() => undefined}
        onReveal={() => undefined}
        onCopyPath={() => undefined}
      />,
    )

    expect(screen.getByRole('heading', { name: '导出对话' })).toBeInTheDocument()
    expect(screen.getByText('将生成一个 zip 文件，包含当前对话和相关运行信息，便于回顾过程或排查问题。')).toBeInTheDocument()
    expect(screen.queryByText(/诊断包/)).not.toBeInTheDocument()
  })

  it('starts export from idle state', () => {
    const onStart = vi.fn()
    render(
      <ConversationExportDialog
        open
        state={{ status: 'idle' }}
        onOpenChange={() => undefined}
        onStart={onStart}
        onReveal={() => undefined}
        onCopyPath={() => undefined}
      />,
    )

    fireEvent.click(screen.getByRole('button', { name: '开始导出' }))

    expect(onStart).toHaveBeenCalledTimes(1)
  })

  it('shows progress while exporting', () => {
    render(
      <ConversationExportDialog
        open
        state={{ status: 'exporting', stage: '写入运行日志' }}
        onOpenChange={() => undefined}
        onStart={() => undefined}
        onReveal={() => undefined}
        onCopyPath={() => undefined}
      />,
    )

    expect(screen.getByText('写入运行日志')).toBeInTheDocument()
    expect(screen.getByRole('button', { name: '导出中…' })).toBeDisabled()
  })

  it('shows reveal and copy actions after success', () => {
    const onReveal = vi.fn()
    const onCopyPath = vi.fn()
    render(
      <ConversationExportDialog
        open
        state={{ status: 'success', zipPath: '/tmp/export.zip', fileName: 'export.zip', sizeBytes: 2048 }}
        onOpenChange={() => undefined}
        onStart={() => undefined}
        onReveal={onReveal}
        onCopyPath={onCopyPath}
      />,
    )

    fireEvent.click(screen.getByRole('button', { name: '打开所在文件夹' }))
    fireEvent.click(screen.getByRole('button', { name: '复制路径' }))

    expect(onReveal).toHaveBeenCalledTimes(1)
    expect(onCopyPath).toHaveBeenCalledTimes(1)
  })

  it('shows retry action after failure', () => {
    const onStart = vi.fn()
    render(
      <ConversationExportDialog
        open
        state={{ status: 'error', message: 'zip 写入失败' }}
        onOpenChange={() => undefined}
        onStart={onStart}
        onReveal={() => undefined}
        onCopyPath={() => undefined}
      />,
    )

    expect(screen.getByText('zip 写入失败')).toBeInTheDocument()
    fireEvent.click(screen.getByRole('button', { name: '重新导出' }))
    expect(onStart).toHaveBeenCalledTimes(1)
  })
})
```

- [ ] **Step 2: Run tests to verify failure**

Run:

```bash
pnpm exec vitest run src/components/chat/ConversationExportDialog.test.tsx
```

Expected: FAIL because component file does not exist.

- [ ] **Step 3: Implement dialog component**

Create `src/components/chat/ConversationExportDialog.tsx`:

```tsx
import { CheckCircle2, Copy, ExternalLink, Loader2, RotateCcw } from 'lucide-react'

import { Button } from '@/components/ui/button'
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog'

export type ConversationExportDialogState =
  | { status: 'idle' }
  | { status: 'exporting'; stage: string }
  | { status: 'success'; zipPath: string; fileName: string; sizeBytes: number }
  | { status: 'error'; message: string }

interface ConversationExportDialogProps {
  open: boolean
  state: ConversationExportDialogState
  onOpenChange: (open: boolean) => void
  onStart: () => void
  onReveal: () => void
  onCopyPath: () => void
}

function formatBytes(bytes: number) {
  if (bytes < 1024) return `${bytes} B`
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`
  return `${(bytes / 1024 / 1024).toFixed(1)} MB`
}

export function ConversationExportDialog({
  open,
  state,
  onOpenChange,
  onStart,
  onReveal,
  onCopyPath,
}: ConversationExportDialogProps) {
  const isExporting = state.status === 'exporting'

  return (
    <Dialog open={open} onOpenChange={isExporting ? undefined : onOpenChange}>
      <DialogContent className="sm:max-w-[460px]">
        <DialogHeader>
          <DialogTitle>导出对话</DialogTitle>
          <DialogDescription>
            将生成一个 zip 文件，包含当前对话和相关运行信息，便于回顾过程或排查问题。
          </DialogDescription>
        </DialogHeader>

        <div className="space-y-3 text-sm text-muted-foreground">
          {state.status === 'idle' ? (
            <div className="rounded-md border border-border bg-muted/30 p-3">
              包含：对话内容、工具记录、运行日志、应用信息。
            </div>
          ) : null}

          {state.status === 'exporting' ? (
            <div className="flex items-center gap-3 rounded-md border border-border bg-muted/30 p-3">
              <Loader2 className="h-4 w-4 animate-spin text-primary" />
              <span>{state.stage}</span>
            </div>
          ) : null}

          {state.status === 'success' ? (
            <div className="space-y-2 rounded-md border border-border bg-muted/30 p-3">
              <div className="flex items-center gap-2 text-foreground">
                <CheckCircle2 className="h-4 w-4 text-primary" />
                <span>导出完成</span>
              </div>
              <div className="break-all">{state.fileName}</div>
              <div>{formatBytes(state.sizeBytes)}</div>
            </div>
          ) : null}

          {state.status === 'error' ? (
            <div className="rounded-md border border-destructive/30 bg-destructive/10 p-3 text-destructive">
              {state.message}
            </div>
          ) : null}
        </div>

        <DialogFooter>
          {state.status === 'success' ? (
            <>
              <Button type="button" variant="secondary" onClick={onCopyPath}>
                <Copy className="h-4 w-4" />
                复制路径
              </Button>
              <Button type="button" onClick={onReveal}>
                <ExternalLink className="h-4 w-4" />
                打开所在文件夹
              </Button>
            </>
          ) : state.status === 'error' ? (
            <Button type="button" onClick={onStart}>
              <RotateCcw className="h-4 w-4" />
              重新导出
            </Button>
          ) : (
            <>
              <Button type="button" variant="secondary" onClick={() => onOpenChange(false)} disabled={isExporting}>
                取消
              </Button>
              <Button type="button" onClick={onStart} disabled={isExporting}>
                {isExporting ? '导出中…' : '开始导出'}
              </Button>
            </>
          )}
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}
```

- [ ] **Step 4: Run dialog tests**

Run:

```bash
pnpm exec vitest run src/components/chat/ConversationExportDialog.test.tsx
```

Expected: PASS.

- [ ] **Step 5: Commit dialog UI**

```bash
git add src/components/chat/ConversationExportDialog.tsx src/components/chat/ConversationExportDialog.test.tsx
git commit -m "feat: add conversation export dialog"
```

---

## Task 5: ChatPage Integration

**Files:**
- Modify: `src/features/chat/ChatPage.tsx`
- Modify: `src/features/chat/ChatPage.test.tsx`

- [ ] **Step 1: Update ChatPage mocks and add failing tests**

Modify the `ChatTopBar` mock in `src/features/chat/ChatPage.test.tsx`:

```tsx
vi.mock('@/components/shell/ChatTopBar', () => ({
  ChatTopBar: ({
    title,
    sourceLabel,
    onExportConversation,
  }: {
    title: string
    sourceLabel?: string
    onExportConversation?: () => void
  }) => (
    <header data-testid="chat-header">
      {title}
      {sourceLabel ? <span data-testid="chat-source-label">{sourceLabel}</span> : null}
      {onExportConversation ? (
        <button type="button" onClick={onExportConversation}>导出对话</button>
      ) : null}
    </header>
  ),
}))
```

Add Tauri wrapper mocks near other mocks:

```tsx
const exportConversationMock = vi.hoisted(() => vi.fn())
const revealExportInFolderMock = vi.hoisted(() => vi.fn())

vi.mock('@/lib/tauri', async () => {
  const actual = await vi.importActual<typeof import('@/lib/tauri')>('@/lib/tauri')
  return {
    ...actual,
    getConversationSource: vi.fn().mockResolvedValue({ kind: 'user' }),
    openGeneratedFile: vi.fn(),
    exportConversation: exportConversationMock,
    revealExportInFolder: revealExportInFolderMock,
  }
})
```

Add test:

```tsx
it('exports the current conversation from the top bar', async () => {
  exportConversationMock.mockResolvedValue({
    zipPath: '/tmp/aijia-conversation-export.zip',
    fileName: 'aijia-conversation-export.zip',
    sizeBytes: 4096,
  })
  useChatStore.setState({
    activeConversationId: 'conv-export',
    conversations: [{ id: 'conv-export', title: '导出测试', createdAt: '', updatedAt: '', isArchived: false }],
    messages: [],
  })

  render(<ChatPage conversationId="conv-export" />)

  fireEvent.click(screen.getByRole('button', { name: '导出对话' }))
  fireEvent.click(await screen.findByRole('button', { name: '开始导出' }))

  await waitFor(() => {
    expect(exportConversationMock).toHaveBeenCalledWith('conv-export')
  })
  expect(await screen.findByText('导出完成')).toBeInTheDocument()
  fireEvent.click(screen.getByRole('button', { name: '打开所在文件夹' }))
  expect(revealExportInFolderMock).toHaveBeenCalledWith('/tmp/aijia-conversation-export.zip')
})
```

Ensure imports include `fireEvent`:

```tsx
import { fireEvent, render, screen, waitFor } from '@testing-library/react'
```

- [ ] **Step 2: Run ChatPage test to verify failure**

Run:

```bash
pnpm exec vitest run src/features/chat/ChatPage.test.tsx
```

Expected: FAIL because ChatPage does not render the dialog or call `exportConversation`.

- [ ] **Step 3: Wire export state in ChatPage**

Modify imports in `src/features/chat/ChatPage.tsx`:

```tsx
import { ConversationExportDialog, type ConversationExportDialogState } from '@/components/chat/ConversationExportDialog'
import { exportConversation, getConversationSource, openGeneratedFile, revealExportInFolder } from '@/lib/tauri'
```

Add state near existing `useState`:

```tsx
const [exportOpen, setExportOpen] = useState(false)
const [exportState, setExportState] = useState<ConversationExportDialogState>({ status: 'idle' })
```

Add handlers:

```tsx
const handleStartExport = async () => {
  setExportState({ status: 'exporting', stage: '准备对话内容' })
  try {
    setExportState({ status: 'exporting', stage: '生成导出文件' })
    const result = await exportConversation(conversationId)
    setExportState({
      status: 'success',
      zipPath: result.zipPath,
      fileName: result.fileName,
      sizeBytes: result.sizeBytes,
    })
  } catch (err) {
    setExportState({
      status: 'error',
      message: err instanceof Error ? err.message : '导出失败，请稍后重试。',
    })
  }
}

const handleRevealExport = async () => {
  if (exportState.status !== 'success') return
  try {
    await revealExportInFolder(exportState.zipPath)
  } catch (err) {
    pushNotification({
      level: 'error',
      title: '无法打开文件夹',
      message: err instanceof Error ? err.message : '打开导出文件夹失败。',
      actions: [],
      dismissible: true,
      context: 'toast',
    })
  }
}

const handleCopyExportPath = async () => {
  if (exportState.status !== 'success') return
  await navigator.clipboard.writeText(exportState.zipPath)
  pushNotification({
    level: 'success',
    title: '已复制路径',
    message: exportState.zipPath,
    actions: [],
    dismissible: true,
    context: 'toast',
  })
}
```

Modify `ChatTopBar` usage:

```tsx
<ChatTopBar
  title={title}
  workspace={conv?.workspaceName}
  kind={conv?.kind}
  sourceLabel={sourceLabel}
  updatedAt={conv?.updatedAt}
  onExportConversation={() => {
    setExportState({ status: 'idle' })
    setExportOpen(true)
  }}
  employee={...}
/>
```

Render dialog below the main layout root inside the top-level container:

```tsx
<ConversationExportDialog
  open={exportOpen}
  state={exportState}
  onOpenChange={setExportOpen}
  onStart={() => void handleStartExport()}
  onReveal={() => void handleRevealExport()}
  onCopyPath={() => void handleCopyExportPath()}
/>
```

- [ ] **Step 4: Run ChatPage tests**

Run:

```bash
pnpm exec vitest run src/features/chat/ChatPage.test.tsx
```

Expected: PASS.

- [ ] **Step 5: Run related frontend tests**

Run:

```bash
pnpm exec vitest run src/features/chat/ChatPage.test.tsx src/components/chat/ConversationExportDialog.test.tsx src/components/shell/ChatTopBar.test.tsx
```

Expected: PASS.

- [ ] **Step 6: Commit ChatPage integration**

```bash
git add src/features/chat/ChatPage.tsx src/features/chat/ChatPage.test.tsx
git commit -m "feat: wire conversation export flow"
```

---

## Task 6: Final Verification and Polish

**Files:**
- Verify all changed files from Tasks 1-5.
- Optionally modify: `docs/superpowers/specs/2026-06-03-conversation-export-design.md` only if implementation reveals a design mismatch.

- [ ] **Step 1: Run Rust export tests**

```bash
cd src-tauri && cargo test --test conversation_export_test -- --nocapture
```

Expected: PASS.

- [ ] **Step 2: Run Rust compile check**

```bash
cd src-tauri && cargo check
```

Expected: PASS.

- [ ] **Step 3: Run frontend focused tests**

```bash
pnpm exec vitest run src/features/chat/ChatPage.test.tsx src/components/chat/ConversationExportDialog.test.tsx src/components/shell/ChatTopBar.test.tsx
```

Expected: PASS.

- [ ] **Step 4: Run frontend build**

```bash
pnpm build
```

Expected: PASS.

- [ ] **Step 5: Inspect changed files**

Run:

```bash
git diff --stat
git diff -- src-tauri/src/runtime/export/conversation_exporter.rs src/features/chat/ChatPage.tsx src/components/chat/ConversationExportDialog.tsx
```

Expected:
- No hardcoded UI colors in React components.
- No unrelated changes to `src-tauri/src/llm/prompts.rs` or `src-tauri/src/llm/providers/aijia_gateway_v2.rs`.
- Export button text remains “导出对话”, not “诊断包”.
- `renlijia.log` and `gate.log` are copied by default when present.

- [ ] **Step 6: Commit verification fixes if any**

If Step 5 required fixes:

```bash
git add <fixed-files>
git commit -m "fix: polish conversation export flow"
```

If no fixes were needed, do not create an empty commit.
