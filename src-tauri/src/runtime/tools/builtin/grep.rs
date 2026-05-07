//! GrepContentTool — search workspace file contents with regex.

use anyhow::Result;
use async_trait::async_trait;
use regex::Regex;
use serde_json::{json, Value};
use std::path::Path;

use crate::runtime::path_auth::PathOp;
use crate::runtime::tools::catalog::TOOL_CATALOG;
use crate::runtime::tools::context::ToolExecutionContext;
use crate::runtime::tools::definition::ToolDefinition;
use crate::runtime::tools::executor::{ToolError, ToolResult};
use crate::runtime::tools::permission::PermissionDecision;
use crate::runtime::tools::RuntimeTool;

use super::workspace::{check_path_permission, matches_glob, require_workspace_root};

const MAX_RESULTS: usize = 1000;
const MAX_FILE_SIZE_BYTES: u64 = 2 * 1024 * 1024;

#[derive(Clone, Copy)]
enum OutputMode {
    Content,
    FilesWithMatches,
    Count,
}

struct GrepResults {
    filenames: Vec<String>,
    content_lines: Vec<String>,
    count_lines: Vec<String>,
    num_matches: usize,
    files_searched: usize,
    truncated: bool,
}

pub struct GrepContentTool;

fn tool_result_grep(content: String, value: Value) -> ToolResult {
    ToolResult {
        tool_name: "grep_content".to_string(),
        content,
        data: Some(value),
        file_meta: None,
        is_degraded: false,
        degradation_notice: None,
    }
}

fn parse_output_mode(input: &Value) -> Result<OutputMode, ToolError> {
    match input
        .get("output_mode")
        .and_then(Value::as_str)
        .unwrap_or("files_with_matches")
    {
        "content" => Ok(OutputMode::Content),
        "files_with_matches" => Ok(OutputMode::FilesWithMatches),
        "count" => Ok(OutputMode::Count),
        other => Err(ToolError::ExecutionFailed(format!(
            "Unsupported output_mode: {other}"
        ))),
    }
}

fn build_display_path(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .map(|rel| rel.to_string_lossy().to_string())
        .unwrap_or_else(|_| path.to_string_lossy().to_string())
}

fn search_file(path: &Path, root: &Path, regex: &Regex, results: &mut GrepResults) {
    if results.num_matches >= MAX_RESULTS {
        results.truncated = true;
        return;
    }

    let metadata = match path.metadata() {
        Ok(metadata) => metadata,
        Err(_) => return,
    };
    if metadata.len() > MAX_FILE_SIZE_BYTES {
        return;
    }

    results.files_searched += 1;

    let bytes = match std::fs::read(path) {
        Ok(bytes) => bytes,
        Err(_) => return,
    };
    let content = String::from_utf8_lossy(&bytes);
    let rel = build_display_path(root, path);

    let mut file_match_count = 0usize;
    let mut file_lines = Vec::new();
    for (line_index, line) in content.lines().enumerate() {
        if regex.is_match(line) {
            file_match_count += 1;
            results.num_matches += 1;
            file_lines.push(format!("{rel}:{}:{line}", line_index + 1));
            if results.num_matches >= MAX_RESULTS {
                results.truncated = true;
                break;
            }
        }
    }

    if file_match_count > 0 {
        results.filenames.push(rel.clone());
        results
            .count_lines
            .push(format!("{rel}:{file_match_count}"));
        results.content_lines.extend(file_lines);
    }
}

fn walk_path(path: &Path, root: &Path, regex: &Regex, glob: &str, results: &mut GrepResults) {
    if results.num_matches >= MAX_RESULTS {
        results.truncated = true;
        return;
    }

    if path.is_file() {
        if let Some(name) = path.file_name().and_then(|value| value.to_str()) {
            if glob.is_empty() || matches_glob(name, glob) {
                search_file(path, root, regex, results);
            }
        }
        return;
    }

    let Ok(entries) = std::fs::read_dir(path) else {
        return;
    };
    let mut entries: Vec<_> = entries.flatten().collect();
    entries.sort_by_key(|entry| entry.file_name());

    for entry in entries {
        if results.num_matches >= MAX_RESULTS {
            results.truncated = true;
            return;
        }

        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if file_type.is_symlink() {
            continue;
        }

        let entry_path = entry.path();
        if file_type.is_dir() {
            let name = entry.file_name().to_string_lossy().to_string();
            if name.starts_with('.') {
                continue;
            }
            walk_path(&entry_path, root, regex, glob, results);
            continue;
        }

        if file_type.is_file() {
            let name = entry.file_name().to_string_lossy().to_string();
            if glob.is_empty() || matches_glob(&name, glob) {
                search_file(&entry_path, root, regex, results);
            }
        }
    }
}

#[async_trait]
impl RuntimeTool for GrepContentTool {
    fn definition(&self) -> ToolDefinition {
        TOOL_CATALOG
            .get("grep_content")
            .unwrap_or_else(|| ToolDefinition::new("grep_content", "Search file content"))
    }

    fn is_concurrency_safe(&self, _input: &Value) -> bool {
        true
    }

    fn is_read_only(&self, _input: &Value) -> bool {
        true
    }

    async fn check_permissions(
        &self,
        input: &Value,
        ctx: &ToolExecutionContext,
    ) -> Option<PermissionDecision> {
        check_path_permission(input, ctx, PathOp::Read, "grep_content")
    }

    fn validate_input(&self, input: &Value) -> Option<ToolError> {
        match input.get("pattern") {
            None => Some(ToolError::InputValidationError {
                tool_name: "grep_content".to_string(),
                message: "Missing required field: pattern (string regex)".to_string(),
            }),
            Some(value) if !value.is_string() => Some(ToolError::InputValidationError {
                tool_name: "grep_content".to_string(),
                message: "Field 'pattern' must be a string".to_string(),
            }),
            _ => None,
        }
    }

    async fn execute(
        &self,
        input: Value,
        ctx: ToolExecutionContext,
    ) -> Result<ToolResult, ToolError> {
        let display_root = require_workspace_root(&ctx)
            .map(|r| r.canonicalize().unwrap_or(r))
            .unwrap_or_default();
        let pattern = input
            .get("pattern")
            .and_then(Value::as_str)
            .ok_or_else(|| ToolError::ExecutionFailed("Missing required: pattern".into()))?;
        let path = input.get("path").and_then(Value::as_str).unwrap_or(".");
        let glob = input.get("glob").and_then(Value::as_str).unwrap_or("");
        let output_mode = parse_output_mode(&input)?;

        let base = super::workspace::resolve_and_authorize_path(&ctx, path, PathOp::Read).await?;
        if !base.exists() {
            return Err(ToolError::ExecutionFailed(format!(
                "Path does not exist: {path}"
            )));
        }

        let regex = Regex::new(pattern)
            .map_err(|e| ToolError::ExecutionFailed(format!("Invalid regex: {e}")))?;

        let mut results = GrepResults {
            filenames: Vec::new(),
            content_lines: Vec::new(),
            count_lines: Vec::new(),
            num_matches: 0,
            files_searched: 0,
            truncated: false,
        };
        walk_path(&base, &display_root, &regex, glob, &mut results);

        let response = match output_mode {
            OutputMode::FilesWithMatches => json!({
                "mode": "files_with_matches",
                "num_files": results.filenames.len(),
                "filenames": results.filenames,
                "files_searched": results.files_searched,
                "truncated": results.truncated,
            }),
            OutputMode::Content => json!({
                "mode": "content",
                "num_files": results.filenames.len(),
                "filenames": [],
                "content": results.content_lines.join("\n"),
                "num_lines": results.content_lines.len(),
                "files_searched": results.files_searched,
                "truncated": results.truncated,
            }),
            OutputMode::Count => json!({
                "mode": "count",
                "num_files": results.filenames.len(),
                "filenames": [],
                "content": results.count_lines.join("\n"),
                "num_matches": results.num_matches,
                "files_searched": results.files_searched,
                "truncated": results.truncated,
            }),
        };

        let content = match output_mode {
            OutputMode::FilesWithMatches => {
                if results.filenames.is_empty() {
                    "No files found".to_string()
                } else {
                    results.filenames.join("\n")
                }
            }
            OutputMode::Content => {
                if results.content_lines.is_empty() {
                    "No matches found".to_string()
                } else {
                    results.content_lines.join("\n")
                }
            }
            OutputMode::Count => {
                if results.count_lines.is_empty() {
                    "No matches found".to_string()
                } else {
                    results.count_lines.join("\n")
                }
            }
        };

        Ok(tool_result_grep(content, response))
    }
}
