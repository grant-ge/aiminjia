use std::fs::{self, File};
use std::io::{BufRead, BufReader, BufWriter, Write};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use chrono::{DateTime, Duration, Local, LocalResult, NaiveDateTime, TimeZone, Utc};
use serde::{Deserialize, Serialize};
use zip::write::SimpleFileOptions;

use crate::storage::file_store::types::{ConversationMeta, StoredMessage};
use crate::storage::file_store::AppStorage;

const EXPORT_LOG_WINDOW_HOURS: i64 = 24;

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
                let short_export_id = export_id.chars().take(8).collect::<String>();
                let file_name = format!(
                    "aijia-export-{}-{}.zip",
                    Local::now().format("%Y%m%d-%H%M%S"),
                    short_export_id
                );
                let zip_path = self.paths.export_root.join(file_name);
                let temp_zip_path = self
                    .paths
                    .export_root
                    .join("tmp")
                    .join(format!("{export_id}.zip"));
                let zip_result = zip_directory(&temp_dir, &temp_zip_path).and_then(|()| {
                    fs::rename(&temp_zip_path, &zip_path).with_context(|| {
                        format!(
                            "rename temp zip {} to {}",
                            temp_zip_path.display(),
                            zip_path.display()
                        )
                    })
                });
                if let Err(error) = zip_result {
                    fs::remove_file(&temp_zip_path).ok();
                    Err(error)
                } else {
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

        let logs_dir = self.paths.app_home.join("logs");
        let metrics = read_metric_records(&logs_dir)?;
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

        let log_window = Duration::hours(EXPORT_LOG_WINDOW_HOURS);
        let app_log_paths = list_app_log_paths(&logs_dir);
        let gate_log_paths = vec![logs_dir.join("gate.log")];
        let log_entries = vec![
            copy_recent_optional_logs(
                &app_log_paths,
                &temp_dir.join("raw/renlijia.log"),
                "raw/renlijia.log",
                log_window,
            ),
            copy_recent_optional_logs(
                &gate_log_paths,
                &temp_dir.join("raw/gate.log"),
                "raw/gate.log",
                log_window,
            ),
        ];

        write_string(
            &temp_dir.join("diagnostics-summary.html"),
            &render_diagnostics_summary_html(conversation, &current_diag, &recent_warn_error),
        )?;

        let mut manifest = Manifest {
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
            files: Vec::new(),
        };
        write_manifest_with_file_index(temp_dir, &mut manifest)?;

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
        "AI小家对话导出\n\n会话：{}\n\n打开 conversation.html 可以查看对话过程。\nraw/ 目录包含排查问题所需的原始材料，其中运行日志仅保留最近 24 小时。此文件不会自动上传，只有你主动发送后他人才可看到。\n",
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
            let text = msg.text();
            format!(
                "<article class=\"msg{}\"><div class=\"meta\">{} · {} · {}</div><div class=\"rendered\" data-view-panel=\"markdown\">{}</div><pre class=\"raw\" data-view-panel=\"raw\">{}</pre></article>",
                error_class,
                escape_html(&msg.role),
                escape_html(&msg.created_at),
                escape_html(msg.run_id.as_deref().unwrap_or("")),
                render_markdown(&text),
                escape_html(&text)
            )
        })
        .collect::<Vec<_>>()
        .join("\n");

    format!(
        "<!doctype html><html><head><meta charset=\"utf-8\"><title>{}</title><style>body{{font-family:-apple-system,BlinkMacSystemFont,'Segoe UI',sans-serif;margin:32px;color:#172033;line-height:1.58}}.top{{display:flex;align-items:center;justify-content:space-between;gap:16px;margin-bottom:12px}}.meta-line{{color:#687386}}button{{border:1px solid #b8c2d4;background:#fff;border-radius:6px;padding:7px 12px;color:#172033;cursor:pointer}}button:hover{{background:#f5f7fb}}.msg{{border-bottom:1px solid #d8dee9;padding:14px 0}}.msg.error{{background:#fff5f5}}.meta{{color:#687386;font-size:12px;margin-bottom:8px}}.rendered>*:first-child{{margin-top:0}}.rendered>*:last-child{{margin-bottom:0}}.rendered pre,.raw{{white-space:pre-wrap;background:#f6f8fa;border:1px solid #d8dee9;border-radius:6px;padding:12px;overflow:auto}}.rendered code{{background:#eef2f7;border-radius:4px;padding:1px 4px}}.rendered pre code{{background:transparent;padding:0}}.raw{{display:none;font:inherit}}body.raw-mode .rendered{{display:none}}body.raw-mode .raw{{display:block}}</style></head><body><div class=\"top\"><h1>{}</h1><button type=\"button\" data-view-toggle aria-pressed=\"false\">查看原始文本</button></div><p class=\"meta-line\">conversationId: {} · workspace: {} · app: {} · exportedAt: {}</p>{}<script>(()=>{{const button=document.querySelector('[data-view-toggle]');if(!button)return;button.addEventListener('click',()=>{{const raw=document.body.classList.toggle('raw-mode');button.textContent=raw?'查看 Markdown 渲染':'查看原始文本';button.setAttribute('aria-pressed',raw?'true':'false');}});}})();</script></body></html>",
        escape_html(&conversation.title),
        escape_html(&conversation.title),
        escape_html(&request.conversation_id),
        escape_html(workspace),
        escape_html(&request.app_version),
        escape_html(&Local::now().to_rfc3339()),
        rows
    )
}

fn render_markdown(input: &str) -> String {
    let mut out = String::new();
    let mut in_list = false;
    let mut in_code = false;
    let mut code = String::new();

    for line in input.lines() {
        if line.trim_start().starts_with("```") {
            if in_code {
                out.push_str("<pre><code>");
                out.push_str(&escape_html(code.trim_end_matches('\n')));
                out.push_str("</code></pre>");
                code.clear();
                in_code = false;
            } else {
                close_list(&mut out, &mut in_list);
                in_code = true;
            }
            continue;
        }

        if in_code {
            code.push_str(line);
            code.push('\n');
            continue;
        }

        let trimmed = line.trim();
        if trimmed.is_empty() {
            close_list(&mut out, &mut in_list);
            continue;
        }

        if let Some((level, title)) = markdown_heading(trimmed) {
            close_list(&mut out, &mut in_list);
            out.push_str(&format!(
                "<h{level}>{}</h{level}>",
                render_inline_markdown(title)
            ));
            continue;
        }

        if let Some(item) = trimmed
            .strip_prefix("- ")
            .or_else(|| trimmed.strip_prefix("* "))
        {
            if !in_list {
                out.push_str("<ul>");
                in_list = true;
            }
            out.push_str("<li>");
            out.push_str(&render_inline_markdown(item));
            out.push_str("</li>");
            continue;
        }

        if let Some(quote) = trimmed.strip_prefix("> ") {
            close_list(&mut out, &mut in_list);
            out.push_str("<blockquote><p>");
            out.push_str(&render_inline_markdown(quote));
            out.push_str("</p></blockquote>");
            continue;
        }

        close_list(&mut out, &mut in_list);
        out.push_str("<p>");
        out.push_str(&render_inline_markdown(trimmed));
        out.push_str("</p>");
    }

    if in_code {
        out.push_str("<pre><code>");
        out.push_str(&escape_html(code.trim_end_matches('\n')));
        out.push_str("</code></pre>");
    }
    close_list(&mut out, &mut in_list);

    out
}

fn close_list(out: &mut String, in_list: &mut bool) {
    if *in_list {
        out.push_str("</ul>");
        *in_list = false;
    }
}

fn markdown_heading(line: &str) -> Option<(usize, &str)> {
    let level = line.chars().take_while(|ch| *ch == '#').count();
    if (1..=6).contains(&level) && line.chars().nth(level) == Some(' ') {
        Some((level, &line[level + 1..]))
    } else {
        None
    }
}

fn render_inline_markdown(input: &str) -> String {
    let mut out = String::new();
    let mut index = 0;

    while index < input.len() {
        let rest = &input[index..];

        if let Some(after_tick) = rest.strip_prefix('`') {
            if let Some(end) = after_tick.find('`') {
                out.push_str("<code>");
                out.push_str(&escape_html(&after_tick[..end]));
                out.push_str("</code>");
                index += end + 2;
                continue;
            }
        }

        if let Some(after_marker) = rest.strip_prefix("**") {
            if let Some(end) = after_marker.find("**") {
                out.push_str("<strong>");
                out.push_str(&render_inline_markdown(&after_marker[..end]));
                out.push_str("</strong>");
                index += end + 4;
                continue;
            }
        }

        if let Some((html, consumed)) = render_link(rest) {
            out.push_str(&html);
            index += consumed;
            continue;
        }

        let ch = rest.chars().next().expect("index is inside string");
        out.push_str(&escape_html(&ch.to_string()));
        index += ch.len_utf8();
    }

    out
}

fn render_link(input: &str) -> Option<(String, usize)> {
    let after_open = input.strip_prefix('[')?;
    let label_end = after_open.find("](")?;
    let url_start = label_end + 2;
    let url_end = after_open[url_start..].find(')')? + url_start;
    let label = &after_open[..label_end];
    let url = &after_open[url_start..url_end];
    let consumed = url_end + 2;

    if !is_safe_link_url(url) {
        return Some((
            format!("{} ({})", render_inline_markdown(label), escape_html(url)),
            consumed,
        ));
    }

    Some((
        format!(
            "<a href=\"{}\" target=\"_blank\" rel=\"noreferrer noopener\">{}</a>",
            escape_html(url),
            render_inline_markdown(label)
        ),
        consumed,
    ))
}

fn is_safe_link_url(url: &str) -> bool {
    url.starts_with("https://") || url.starts_with("http://") || url.starts_with("mailto:")
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
        if !path.is_file() {
            continue;
        }
        let Ok(file) = File::open(&path) else {
            continue;
        };
        for line in BufReader::new(file).lines() {
            let Ok(raw_line) = line else {
                continue;
            };
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
    let Ok(entries) = fs::read_dir(logs_dir) else {
        return Ok(paths);
    };
    for entry in entries {
        let Ok(entry) = entry else {
            continue;
        };
        let path = entry.path();
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

fn list_app_log_paths(logs_dir: &Path) -> Vec<PathBuf> {
    let mut paths = Vec::new();
    if let Ok(entries) = fs::read_dir(logs_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
                continue;
            };
            if name == "renlijia.log" || name.starts_with("renlijia.") {
                paths.push(path);
            }
        }
    }

    if paths.is_empty() {
        paths.push(logs_dir.join("renlijia.log"));
    }
    paths.sort_by_key(|path| path.file_name().map(|name| name.to_os_string()));
    paths
}

fn copy_recent_optional_logs(
    sources: &[PathBuf],
    dest: &Path,
    name: &str,
    window: Duration,
) -> ManifestLog {
    let source_path = sources
        .iter()
        .map(|path| path.to_string_lossy().to_string())
        .collect::<Vec<_>>()
        .join(", ");
    let mut readable_sources = Vec::new();
    let mut source_size_bytes = 0_u64;
    let mut latest_modified = None;

    for source in sources {
        let Ok(meta) = fs::metadata(source) else {
            continue;
        };
        if !meta.is_file() {
            continue;
        }
        source_size_bytes = source_size_bytes.saturating_add(meta.len());
        if let Ok(modified) = meta.modified() {
            latest_modified = Some(match latest_modified {
                Some(current) if current >= modified => current,
                _ => modified,
            });
        }
        readable_sources.push((source.clone(), meta));
    }

    if readable_sources.is_empty() {
        return ManifestLog {
            name: name.to_string(),
            source_path,
            size_bytes: None,
            modified_at: None,
            included: false,
            error: Some("日志文件不存在。".to_string()),
        };
    }

    if let Some(parent) = dest.parent() {
        if let Err(error) = fs::create_dir_all(parent) {
            return ManifestLog {
                name: name.to_string(),
                source_path,
                size_bytes: Some(source_size_bytes),
                modified_at: modified_time_to_rfc3339(latest_modified),
                included: false,
                error: Some(error.to_string()),
            };
        }
    }

    let result = File::create(dest)
        .with_context(|| format!("create recent log export {}", dest.display()))
        .and_then(|file| {
            let now = Utc::now();
            let since = now - window;
            let mut writer = BufWriter::new(file);
            for (source, meta) in &readable_sources {
                append_recent_log_lines(source, meta, &mut writer, since, now)?;
            }
            writer.flush()?;
            Ok(())
        });

    match result {
        Ok(()) => ManifestLog {
            name: name.to_string(),
            source_path,
            size_bytes: fs::metadata(dest).ok().map(|meta| meta.len()),
            modified_at: modified_time_to_rfc3339(latest_modified),
            included: true,
            error: None,
        },
        Err(error) => {
            fs::remove_file(dest).ok();
            ManifestLog {
                name: name.to_string(),
                source_path,
                size_bytes: Some(source_size_bytes),
                modified_at: modified_time_to_rfc3339(latest_modified),
                included: false,
                error: Some(error.to_string()),
            }
        }
    }
}

fn append_recent_log_lines(
    source: &Path,
    meta: &fs::Metadata,
    writer: &mut BufWriter<File>,
    since: DateTime<Utc>,
    now: DateTime<Utc>,
) -> Result<()> {
    let file = File::open(source).with_context(|| format!("open log {}", source.display()))?;
    let modified_recent = meta
        .modified()
        .ok()
        .map(DateTime::<Utc>::from)
        .map(|modified| modified >= since && modified <= now)
        .unwrap_or(false);
    let mut saw_timestamp = false;
    let mut include_continuation = false;

    for line in BufReader::new(file).lines() {
        let line = line?;
        let include = if let Some(timestamp) = extract_log_timestamp(&line) {
            saw_timestamp = true;
            include_continuation = timestamp >= since && timestamp <= now;
            include_continuation
        } else if saw_timestamp {
            include_continuation
        } else {
            modified_recent
        };

        if include {
            writeln!(writer, "{line}")?;
        }
    }

    Ok(())
}

fn extract_log_timestamp(line: &str) -> Option<DateTime<Utc>> {
    extract_json_log_timestamp(line).or_else(|| extract_bracketed_log_timestamp(line))
}

fn extract_json_log_timestamp(line: &str) -> Option<DateTime<Utc>> {
    let trimmed = line.trim_start();
    if !trimmed.starts_with('{') {
        return None;
    }
    let key_start = trimmed.find("\"ts\"")?;
    let after_key = trimmed.get(key_start + 4..)?.trim_start();
    let after_colon = after_key.strip_prefix(':')?.trim_start();
    let after_quote = after_colon.strip_prefix('"')?;
    let ts = after_quote.split('"').next()?;
    DateTime::parse_from_rfc3339(ts)
        .ok()
        .map(|dt| dt.with_timezone(&Utc))
}

fn extract_bracketed_log_timestamp(line: &str) -> Option<DateTime<Utc>> {
    let rest = line.strip_prefix('[')?;
    let (date, rest) = rest.split_once("][")?;
    let (time, _) = rest.split_once(']')?;
    let naive =
        NaiveDateTime::parse_from_str(&format!("{date} {time}"), "%Y-%m-%d %H:%M:%S").ok()?;

    match Local.from_local_datetime(&naive) {
        LocalResult::Single(dt) => Some(dt.with_timezone(&Utc)),
        LocalResult::Ambiguous(first, _) => Some(first.with_timezone(&Utc)),
        LocalResult::None => None,
    }
}

fn modified_time_to_rfc3339(modified: Option<std::time::SystemTime>) -> Option<String> {
    modified
        .map(DateTime::<Utc>::from)
        .map(|dt| dt.to_rfc3339())
}

fn collect_manifest_files(root: &Path) -> Result<Vec<ManifestFile>> {
    let mut files = Vec::new();
    collect_manifest_files_rec(root, root, &mut files)?;
    files.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(files)
}

fn write_manifest_with_file_index(root: &Path, manifest: &mut Manifest) -> Result<()> {
    let manifest_path = root.join("manifest.json");
    let mut previous = String::new();
    for _ in 0..8 {
        manifest.files = collect_manifest_files(root)?;
        let content = serde_json::to_string_pretty(manifest)?;
        if content == previous {
            return Ok(());
        }
        write_string(&manifest_path, &content)?;
        previous = content;
    }
    Ok(())
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
    let file = File::options()
        .write(true)
        .create_new(true)
        .open(zip_path)
        .with_context(|| format!("create temp zip {}", zip_path.display()))?;
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
