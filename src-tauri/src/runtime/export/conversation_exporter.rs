use std::fs::{self, File};
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
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
            .get_conversation(&request.conversation_id)
            .with_context(|| format!("load conversation {}", request.conversation_id))?;
        let messages = storage
            .get_messages_v2(&request.conversation_id)
            .with_context(|| format!("load messages {}", request.conversation_id))?;

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
        write_string(&temp_dir.join("README.txt"), &render_readme(conversation))?;
        write_string(
            &temp_dir.join("conversation.html"),
            &render_conversation_html(conversation, messages, request),
        )?;

        let metrics = read_metric_records(&self.paths.app_home.join("logs"))?;
        let current_diag =
            filter_current_conversation_diagnostics(&metrics, &request.conversation_id);
        let recent_warn_error = filter_recent_warn_error(&metrics);

        write_string(
            &temp_dir.join("raw/current-conversation-diagnostics.jsonl"),
            &join_jsonl(&current_diag),
        )?;
        write_string(
            &temp_dir.join("raw/recent-warn-error.jsonl"),
            &join_jsonl(&recent_warn_error),
        )?;
        write_string(
            &temp_dir.join("raw/messages.jsonl"),
            &join_jsonl(
                &messages
                    .iter()
                    .map(serde_json::to_string)
                    .collect::<std::result::Result<Vec<_>, _>>()?,
            ),
        )?;

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
        )?;

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
        )?;

        Ok(())
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct Manifest {
    #[serde(rename = "schemaVersion")]
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

#[derive(Debug)]
struct MetricRecord {
    raw_line: String,
    value: serde_json::Value,
    timestamp: Option<DateTime<Utc>>,
}

fn write_string(path: &Path, content: &str) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, content)?;
    Ok(())
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
    let workspace = conversation
        .authorized_workspace
        .as_ref()
        .map(|workspace| workspace.display_name.as_str())
        .unwrap_or("未绑定工作区");
    let rows = messages
        .iter()
        .map(|msg| {
            let error_class = if msg.error.is_some() { " error" } else { "" };
            format!(
                "<article class=\"msg{}\"><div class=\"meta\">{} · {} · {}</div><pre>{}</pre></article>",
                error_class,
                escape_html(&msg.role),
                escape_html(&msg.created_at),
                escape_html(msg.run_id.as_deref().unwrap_or("")),
                escape_html(msg.text())
            )
        })
        .collect::<Vec<_>>()
        .join("\n");

    format!(
        "<!doctype html><html><head><meta charset=\"utf-8\"><title>{}</title><style>body{{font-family:-apple-system,BlinkMacSystemFont,'Segoe UI',sans-serif;margin:32px;color:#172033}}.msg{{border-bottom:1px solid #d8dee9;padding:14px 0}}.msg.error{{background:#fff5f5}}.meta{{color:#687386;font-size:12px;margin-bottom:8px}}pre{{white-space:pre-wrap;font:inherit}}</style></head><body><h1>{}</h1><p>conversationId: {} · workspace: {} · app: {} · exportedAt: {}</p>{}</body></html>",
        escape_html(&conversation.title),
        escape_html(&conversation.title),
        escape_html(&request.conversation_id),
        escape_html(workspace),
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
        "<!doctype html><html><head><meta charset=\"utf-8\"><title>Diagnostics Summary</title></head><body><h1>{}</h1><p>当前会话 diagnostics: {}</p><p>最近 warn/error/ok=false: {}</p><p>详见 raw/current-conversation-diagnostics.jsonl 与 raw/recent-warn-error.jsonl。</p></body></html>",
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
    let trimmed = out.trim_matches('-').chars().take(48).collect::<String>();
    if trimmed.is_empty() {
        "conversation".to_string()
    } else {
        trimmed
    }
}

fn join_jsonl(lines: &[String]) -> String {
    if lines.is_empty() {
        String::new()
    } else {
        format!("{}\n", lines.join("\n"))
    }
}

fn read_metric_records(logs_dir: &Path) -> Result<Vec<MetricRecord>> {
    let mut paths = list_metric_paths(logs_dir)?;
    paths.sort_by(|a, b| metric_sort_key(a).cmp(&metric_sort_key(b)));
    let mut records = Vec::new();
    for path in paths {
        let file = File::open(&path).with_context(|| format!("open {}", path.display()))?;
        for line in BufReader::new(file).lines() {
            let raw_line = line?;
            let json = raw_line
                .split_once('\t')
                .map_or(raw_line.as_str(), |(json, _)| json);
            let Ok(value) = serde_json::from_str::<serde_json::Value>(json) else {
                continue;
            };
            let timestamp = value
                .get("ts")
                .and_then(|v| v.as_str())
                .and_then(|ts| DateTime::parse_from_rfc3339(ts).ok())
                .map(|ts| ts.with_timezone(&Utc));
            records.push(MetricRecord {
                raw_line,
                value,
                timestamp,
            });
        }
    }
    Ok(records)
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
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("");
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

fn filter_current_conversation_diagnostics(
    records: &[MetricRecord],
    conversation_id: &str,
) -> Vec<String> {
    records
        .iter()
        .filter(|record| {
            record.value.get("category").and_then(|v| v.as_str()) == Some("diagnostics")
                && record.value.get("conversationId").and_then(|v| v.as_str())
                    == Some(conversation_id)
        })
        .map(|record| record.raw_line.clone())
        .collect()
}

fn filter_recent_warn_error(records: &[MetricRecord]) -> Vec<String> {
    let now = Utc::now();
    let since = now - Duration::hours(24);

    records
        .iter()
        .filter(|record| {
            record.value.get("category").and_then(|v| v.as_str()) == Some("diagnostics")
        })
        .filter(|record| {
            record
                .timestamp
                .map(|ts| ts >= since && ts <= now)
                .unwrap_or(false)
        })
        .filter(|record| {
            matches!(
                record.value.get("level").and_then(|v| v.as_str()),
                Some("warn" | "error")
            ) || record.value.get("ok").and_then(|v| v.as_bool()) == Some(false)
        })
        .map(|record| record.raw_line.clone())
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

fn collect_manifest_files_rec(
    root: &Path,
    dir: &Path,
    files: &mut Vec<ManifestFile>,
) -> Result<()> {
    for entry in fs::read_dir(dir)? {
        let path = entry?.path();
        if path.is_dir() {
            collect_manifest_files_rec(root, &path, files)?;
        } else {
            files.push(ManifestFile {
                name: path
                    .strip_prefix(root)?
                    .to_string_lossy()
                    .replace('\\', "/"),
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
    let mut entries = fs::read_dir(dir)?.collect::<std::result::Result<Vec<_>, _>>()?;
    entries.sort_by_key(|entry| entry.path());
    for entry in entries {
        let path = entry.path();
        if path.is_dir() {
            add_dir_to_zip(root, &path, zip, options)?;
        } else {
            let name = path
                .strip_prefix(root)?
                .to_string_lossy()
                .replace('\\', "/");
            zip.start_file(name, options)?;
            let mut input = File::open(&path)?;
            std::io::copy(&mut input, zip)?;
        }
    }
    Ok(())
}
