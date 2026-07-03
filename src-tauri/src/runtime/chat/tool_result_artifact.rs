use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::fs::{self, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};

use crate::runtime::chat::tool_round_driver::ToolRoundResult;

pub const TOOL_RESULTS_DIR_NAME: &str = "tool-results";
pub const TOOL_RESULTS_MANIFEST_FILE: &str = "manifest.jsonl";
pub const DEFAULT_PREVIEW_CHARS: usize = 2_000;

#[derive(Debug, Clone)]
pub struct CompactionEvidenceConfig {
    pub max_chars_per_artifact: usize,
    pub aggregate_char_budget: usize,
}

impl Default for CompactionEvidenceConfig {
    fn default() -> Self {
        Self {
            max_chars_per_artifact: 80_000,
            aggregate_char_budget: 240_000,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ToolResultArtifactRef {
    pub schema_version: u32,
    pub tool_call_id: String,
    pub tool_name: String,
    pub path: String,
    pub content_type: String,
    pub original_chars: usize,
    pub preview_chars: usize,
    pub preview: String,
    pub sha256: String,
    pub created_at_ms: i64,
    pub digest: Option<String>,
    pub legacy_state: Option<String>,
}

impl ToolResultArtifactRef {
    pub fn path_buf(&self) -> PathBuf {
        PathBuf::from(&self.path)
    }
}

pub fn tool_results_dir(conv_dir: &Path) -> PathBuf {
    conv_dir.join(TOOL_RESULTS_DIR_NAME)
}

pub fn tool_results_manifest_path(conv_dir: &Path) -> PathBuf {
    tool_results_dir(conv_dir).join(TOOL_RESULTS_MANIFEST_FILE)
}

pub fn build_persisted_tool_result_message(record: &ToolResultArtifactRef) -> String {
    format!(
        concat!(
            "<persisted-tool-result tool_call_id=\"{}\" tool_name=\"{}\">\n",
            "Full output saved to: {}\n",
            "Original chars: {}\n",
            "Sha256: {}\n",
            "Note: Preview is incomplete. If omitted output matters, inspect or search the saved file. If the user requested a named deliverable, prefer using this evidence to update that deliverable instead of continuing broad exploratory reads.\n",
            "Preview:\n",
            "{}\n",
            "</persisted-tool-result>"
        ),
        xml_attr_escape(&record.tool_call_id),
        xml_attr_escape(&record.tool_name),
        record.path,
        record.original_chars,
        record.sha256,
        record.preview
    )
}

pub fn is_persisted_tool_result_message(content: &str) -> bool {
    content.trim_start().starts_with("<persisted-tool-result ")
}

pub fn build_tool_result_artifact_replacements_from_round_results(
    conv_dir: &Path,
    round_results: &[ToolRoundResult],
) -> HashMap<String, String> {
    let mut replacements = HashMap::new();
    for round_result in round_results {
        let ToolRoundResult::Ok(outcome) = round_result else {
            continue;
        };
        let max_chars = outcome.max_result_size_chars();
        let content = outcome.content();
        if max_chars == 0 || content.len() <= max_chars {
            continue;
        }

        match persist_tool_result_artifact(
            conv_dir,
            outcome.tool_call_id(),
            outcome.tool_name(),
            content,
            "text/plain",
        ) {
            Ok(record) => {
                replacements.insert(
                    outcome.tool_call_id().to_string(),
                    build_persisted_tool_result_message(&record),
                );
            }
            Err(err) => {
                log::warn!(
                    "[tool_result_artifact] failed to persist tool_call_id={} tool={}: {}",
                    outcome.tool_call_id(),
                    outcome.tool_name(),
                    err
                );
            }
        }
    }
    replacements
}

pub fn apply_tool_result_artifact_replacements(
    tool_result_messages: &mut [serde_json::Value],
    replacements: &HashMap<String, String>,
) {
    if replacements.is_empty() {
        return;
    }

    for message in tool_result_messages {
        let Some(tool_call_id) = message.get("toolCallId").and_then(|value| value.as_str()) else {
            continue;
        };
        let Some(replacement) = replacements.get(tool_call_id) else {
            continue;
        };
        if let Some(object) = message.as_object_mut() {
            object.insert(
                "content".to_string(),
                serde_json::Value::String(replacement.clone()),
            );
        }
    }
}

pub fn build_compaction_evidence_messages(
    messages: &[serde_json::Value],
    config: &CompactionEvidenceConfig,
) -> Vec<serde_json::Value> {
    if config.max_chars_per_artifact == 0 || config.aggregate_char_budget == 0 {
        return messages.to_vec();
    }

    let mut remaining_chars = config.aggregate_char_budget;
    messages
        .iter()
        .map(|message| {
            if remaining_chars == 0 {
                return message.clone();
            }
            if message.get("role").and_then(serde_json::Value::as_str) != Some("tool") {
                return message.clone();
            }
            let Some(content) = message.get("content").and_then(serde_json::Value::as_str) else {
                return message.clone();
            };
            if !is_persisted_tool_result_message(content) {
                return message.clone();
            }
            let Some(path) = persisted_tool_result_path_from_message(content) else {
                return message.clone();
            };
            let Ok(artifact_content) = fs::read_to_string(&path) else {
                log::warn!(
                    "[tool_result_artifact] failed to read compaction evidence artifact {}",
                    path.display()
                );
                return message.clone();
            };

            let allowed_chars = config.max_chars_per_artifact.min(remaining_chars);
            let evidence_content =
                build_compaction_evidence_content(content, &artifact_content, allowed_chars);
            remaining_chars =
                remaining_chars.saturating_sub(artifact_content.chars().count().min(allowed_chars));

            let mut expanded = message.clone();
            if let Some(object) = expanded.as_object_mut() {
                object.insert(
                    "content".to_string(),
                    serde_json::Value::String(evidence_content),
                );
            }
            expanded
        })
        .collect()
}

pub fn persist_tool_result_artifact(
    conv_dir: &Path,
    tool_call_id: &str,
    tool_name: &str,
    content: &str,
    content_type: &str,
) -> Result<ToolResultArtifactRef> {
    if let Some(existing) = find_manifest_record(conv_dir, tool_call_id)? {
        return Ok(existing);
    }

    let dir = tool_results_dir(conv_dir);
    fs::create_dir_all(&dir).with_context(|| {
        format!(
            "failed to create tool result artifact directory {}",
            dir.display()
        )
    })?;

    let sha256 = sha256_hex(content.as_bytes());
    let extension = extension_for_content_type(content_type);
    let filename = artifact_filename(tool_call_id, extension);
    let path = dir.join(filename);

    match OpenOptions::new().write(true).create_new(true).open(&path) {
        Ok(mut file) => {
            file.write_all(content.as_bytes()).with_context(|| {
                format!("failed to write tool result artifact {}", path.display())
            })?;
            file.flush().with_context(|| {
                format!("failed to flush tool result artifact {}", path.display())
            })?;
        }
        Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => {
            // The tool_use_id is stable per invocation; reuse the existing file.
        }
        Err(err) => {
            return Err(err).with_context(|| {
                format!("failed to create tool result artifact {}", path.display())
            });
        }
    }

    let record = ToolResultArtifactRef {
        schema_version: 1,
        tool_call_id: tool_call_id.to_string(),
        tool_name: tool_name.to_string(),
        path: path.to_string_lossy().to_string(),
        content_type: content_type.to_string(),
        original_chars: content.chars().count(),
        preview_chars: DEFAULT_PREVIEW_CHARS,
        preview: preview_at_char_boundary(content, DEFAULT_PREVIEW_CHARS).to_string(),
        sha256,
        created_at_ms: chrono::Utc::now().timestamp_millis(),
        digest: None,
        legacy_state: None,
    };

    append_manifest_record(conv_dir, &record)?;
    Ok(record)
}

pub fn find_manifest_record(
    conv_dir: &Path,
    tool_call_id: &str,
) -> Result<Option<ToolResultArtifactRef>> {
    let path = tool_results_manifest_path(conv_dir);
    if !path.exists() {
        return Ok(None);
    }

    let file = fs::File::open(&path)
        .with_context(|| format!("failed to open tool result manifest {}", path.display()))?;
    let reader = BufReader::new(file);
    for line in reader.lines() {
        let line = line.with_context(|| {
            format!(
                "failed to read tool result manifest line {}",
                path.display()
            )
        })?;
        if line.trim().is_empty() {
            continue;
        }
        let record: ToolResultArtifactRef = serde_json::from_str(&line).with_context(|| {
            format!(
                "failed to parse tool result manifest line {}",
                path.display()
            )
        })?;
        if record.tool_call_id == tool_call_id {
            return Ok(Some(record));
        }
    }
    Ok(None)
}

fn append_manifest_record(conv_dir: &Path, record: &ToolResultArtifactRef) -> Result<()> {
    let path = tool_results_manifest_path(conv_dir);
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .with_context(|| format!("failed to open tool result manifest {}", path.display()))?;
    let line = serde_json::to_string(record)?;
    writeln!(file, "{}", line)
        .with_context(|| format!("failed to append tool result manifest {}", path.display()))?;
    Ok(())
}

fn artifact_filename(tool_call_id: &str, extension: &str) -> String {
    let safe_id = sanitize_tool_call_id(tool_call_id);
    let id_hash = sha256_hex(tool_call_id.as_bytes());
    let suffix = &id_hash[..12];
    format!("{}-{}.{}", safe_id, suffix, extension)
}

fn sanitize_tool_call_id(tool_call_id: &str) -> String {
    let mut safe = String::with_capacity(tool_call_id.len().min(80));
    for ch in tool_call_id.chars() {
        if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' {
            safe.push(ch);
        } else {
            safe.push('_');
        }
        if safe.len() >= 80 {
            break;
        }
    }
    if safe.trim_matches('_').is_empty() {
        "tool-result".to_string()
    } else {
        safe
    }
}

fn extension_for_content_type(content_type: &str) -> &'static str {
    if content_type.eq_ignore_ascii_case("application/json") {
        "json"
    } else {
        "txt"
    }
}

fn preview_at_char_boundary(content: &str, max_chars: usize) -> &str {
    if content.chars().count() <= max_chars {
        return content;
    }
    let mut end = 0usize;
    for (count, (idx, ch)) in content.char_indices().enumerate() {
        if count >= max_chars {
            break;
        }
        end = idx + ch.len_utf8();
    }
    &content[..end]
}

fn compact_artifact_content_for_evidence(content: &str, max_chars: usize) -> String {
    let char_count = content.chars().count();
    if char_count <= max_chars {
        return content.to_string();
    }
    if max_chars <= 512 {
        return preview_at_char_boundary(content, max_chars).to_string();
    }

    let head_chars = max_chars / 2;
    let tail_chars = max_chars - head_chars;
    let head = preview_at_char_boundary(content, head_chars);
    let tail_start = content
        .char_indices()
        .rev()
        .nth(tail_chars.saturating_sub(1))
        .map(|(idx, _)| idx)
        .unwrap_or(0);
    let tail = &content[tail_start..];
    format!(
        "{}\n[artifact evidence truncated: original chars={}, omitted middle]\n{}",
        head, char_count, tail
    )
}

fn build_compaction_evidence_content(
    persisted_ref: &str,
    artifact_content: &str,
    max_chars: usize,
) -> String {
    let evidence = compact_artifact_content_for_evidence(artifact_content, max_chars);
    format!(
        concat!(
            "<persisted-tool-result-evidence>\n",
            "Original reference:\n",
            "{}\n",
            "Recovered artifact content for compaction:\n",
            "{}\n",
            "</persisted-tool-result-evidence>"
        ),
        persisted_ref, evidence
    )
}

fn persisted_tool_result_path_from_message(content: &str) -> Option<PathBuf> {
    content.lines().find_map(|line| {
        line.strip_prefix("Full output saved to: ")
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
    })
}

fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut out = String::with_capacity(digest.len() * 2);
    for byte in digest {
        out.push_str(&format!("{:02x}", byte));
    }
    out
}

fn xml_attr_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('"', "&quot;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn persists_tool_result_and_manifest_record() {
        let tmp = tempfile::tempdir().unwrap();
        let record = persist_tool_result_artifact(
            tmp.path(),
            "call_1",
            "Bash",
            "important fact: BUILD_ID=abc123",
            "text/plain",
        )
        .unwrap();

        assert_eq!(record.schema_version, 1);
        assert_eq!(record.tool_call_id, "call_1");
        assert!(record.path_buf().exists());
        assert_eq!(
            std::fs::read_to_string(record.path_buf()).unwrap(),
            "important fact: BUILD_ID=abc123"
        );

        let manifest = std::fs::read_to_string(tool_results_manifest_path(tmp.path())).unwrap();
        assert!(manifest.contains("\"toolCallId\":\"call_1\""));
        assert!(manifest.contains("\"toolName\":\"Bash\""));
    }

    #[test]
    fn persisted_message_contains_recovery_fields() {
        let tmp = tempfile::tempdir().unwrap();
        let record = persist_tool_result_artifact(
            tmp.path(),
            "tc_<danger>",
            "tool \"quoted\"",
            &"x".repeat(DEFAULT_PREVIEW_CHARS + 20),
            "text/plain",
        )
        .unwrap();

        let message = build_persisted_tool_result_message(&record);
        assert!(message.starts_with("<persisted-tool-result "));
        assert!(message.contains("tool_call_id=\"tc_&lt;danger&gt;\""));
        assert!(message.contains("tool_name=\"tool &quot;quoted&quot;\""));
        assert!(message.contains("Full output saved to:"));
        assert!(message.contains("Sha256:"));
        assert!(message.contains("</persisted-tool-result>"));
        assert_eq!(record.preview.chars().count(), DEFAULT_PREVIEW_CHARS);
    }

    #[test]
    fn duplicate_persist_reuses_manifest_record() {
        let tmp = tempfile::tempdir().unwrap();
        let first = persist_tool_result_artifact(
            tmp.path(),
            "call_same",
            "Bash",
            "first content",
            "text/plain",
        )
        .unwrap();
        let second = persist_tool_result_artifact(
            tmp.path(),
            "call_same",
            "Bash",
            "second content",
            "text/plain",
        )
        .unwrap();

        assert_eq!(first, second);
        assert_eq!(
            std::fs::read_to_string(first.path_buf()).unwrap(),
            "first content"
        );

        let manifest = std::fs::read_to_string(tool_results_manifest_path(tmp.path())).unwrap();
        assert_eq!(manifest.lines().count(), 1);
    }

    #[test]
    fn unsafe_tool_call_id_cannot_escape_artifact_directory() {
        let tmp = tempfile::tempdir().unwrap();
        let record = persist_tool_result_artifact(
            tmp.path(),
            "..\\..\\secret/file",
            "Bash",
            "safe",
            "text/plain",
        )
        .unwrap();

        let artifact_dir = tool_results_dir(tmp.path()).canonicalize().unwrap();
        let artifact_path = record.path_buf().canonicalize().unwrap();
        assert!(artifact_path.starts_with(artifact_dir));
        assert!(!artifact_path
            .file_name()
            .unwrap()
            .to_string_lossy()
            .contains(".."));
    }
}
