//! Workspace primitive tools as RuntimeTool.
//!
//! These tools require workspace capability via `ctx.capability`.
//! They NEVER accept a PluginContext — permissions come from CapabilityContext.

use anyhow::Result;
use async_trait::async_trait;
use serde_json::{json, Value};
use std::path::Path;
use std::time::UNIX_EPOCH;

use crate::runtime::tools::catalog::TOOL_CATALOG;
use crate::runtime::tools::capability::FileState;
use crate::runtime::tools::context::ToolExecutionContext;
use crate::runtime::tools::definition::ToolDefinition;
use crate::runtime::tools::executor::{ToolError, ToolResult};
use crate::runtime::tools::RuntimeTool;
use crate::storage::file_manager;

/// Returns the root path for workspace file operations.
///
/// Priority: `authorized_workspace.root_path` > `workspace_path` (Lotus internal workspace).
///
/// Fallback to `workspace_path` is intentional: these tools operate on the Lotus
/// internal workspace by default and switch to the user-authorized external directory
/// when one is set for the session. This is distinct from requiring explicit authorization —
/// these tools are always available, but scope to the authorized directory when present.
///
/// Returns `PermissionDenied` only when no capability context is present at all
/// (i.e., the tool was invoked outside a proper session context).
fn require_workspace_root(ctx: &ToolExecutionContext) -> Result<std::path::PathBuf, ToolError> {
    ctx.capability
        .as_ref()
        .and_then(|c| c.storage.as_ref())
        .map(|s| {
            s.authorized_workspace
                .as_ref()
                .map(|aw| aw.root_path.clone())
                .unwrap_or_else(|| s.workspace_path.clone())
        })
        .ok_or_else(|| {
            ToolError::PermissionDenied(
                "No capability context. Authorized workspace required for file tools.".into(),
            )
        })
}

fn resolve_path(root: &Path, rel: &str) -> Result<std::path::PathBuf, ToolError> {
    file_manager::resolve_local_reference(root, rel)
        .map_err(|e| ToolError::PermissionDenied(e.to_string()))
}

fn tool_result(tool_name: &str, value: Value) -> ToolResult {
    ToolResult {
        tool_name: tool_name.to_string(),
        content: serde_json::to_string_pretty(&value).unwrap_or_default(),
        data: Some(value),
        file_meta: None,
        is_degraded: false,
        degradation_notice: None,
    }
}

fn truncate_text_to_max_bytes(content: &str, max_bytes: usize) -> String {
    if content.len() <= max_bytes {
        return content.to_string();
    }

    let mut end = max_bytes;
    while end > 0 && !content.is_char_boundary(end) {
        end -= 1;
    }
    content[..end].to_string()
}

fn limit_text_content(content: &str, max_bytes: usize) -> (String, bool) {
    let limited = truncate_text_to_max_bytes(content, max_bytes);
    let truncated = limited.len() < content.len();
    (limited, truncated)
}

// ── ListDirectoryRuntimeTool ──────────────────────────────────────────────

pub struct ListDirectoryRuntimeTool;

#[async_trait]
impl RuntimeTool for ListDirectoryRuntimeTool {
    fn definition(&self) -> ToolDefinition {
        TOOL_CATALOG
            .get("list_directory")
            .cloned()
            .unwrap_or_else(|| ToolDefinition::new("list_directory", "List authorized directory"))
    }

    fn is_concurrency_safe(&self, _input: &Value) -> bool {
        true
    }

    async fn execute(
        &self,
        input: Value,
        ctx: ToolExecutionContext,
    ) -> Result<ToolResult, ToolError> {
        let root = require_workspace_root(&ctx)?;
        let rel = input.get("path").and_then(Value::as_str).unwrap_or(".");
        let resolved = resolve_path(&root, rel)?;
        if !resolved.is_dir() {
            return Err(ToolError::ExecutionFailed(format!("Not a directory: {rel}")));
        }
        let mut files = Vec::new();
        for entry in std::fs::read_dir(&resolved)
            .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?
            .flatten()
        {
            let meta = entry.metadata().ok();
            let is_dir = meta.as_ref().map(|m| m.is_dir()).unwrap_or(false);
            files.push(json!({
                "name": entry.file_name().to_string_lossy(),
                "type": if is_dir { "directory" } else { "file" },
                "size": meta.as_ref().map(|m| m.len()).unwrap_or(0),
            }));
        }
        files.sort_by(|a, b| {
            b["type"]
                .as_str()
                .unwrap_or("")
                .cmp(a["type"].as_str().unwrap_or(""))
                .then(
                    a["name"]
                        .as_str()
                        .unwrap_or("")
                        .cmp(b["name"].as_str().unwrap_or("")),
                )
        });
        let count = files.len();
        Ok(tool_result(
            "list_directory",
            json!({ "path": rel, "files": files, "count": count }),
        ))
    }
}

// ── ReadWorkspaceFileRuntimeTool ──────���───────────────────────────────────

pub struct ReadWorkspaceFileRuntimeTool;

#[async_trait]
impl RuntimeTool for ReadWorkspaceFileRuntimeTool {
    fn definition(&self) -> ToolDefinition {
        TOOL_CATALOG
            .get("read_workspace_file")
            .cloned()
            .unwrap_or_else(|| ToolDefinition::new("read_workspace_file", "Read workspace file"))
    }

    fn is_concurrency_safe(&self, _input: &Value) -> bool {
        true
    }

    async fn execute(
        &self,
        input: Value,
        ctx: ToolExecutionContext,
    ) -> Result<ToolResult, ToolError> {
        let capability = ctx.capability.as_ref();
        let root = require_workspace_root(&ctx)?;
        let rel = input
            .get("path")
            .and_then(Value::as_str)
            .ok_or_else(|| ToolError::ExecutionFailed("Missing required: path".into()))?;
        let max_bytes = capability
            .and_then(|cap| cap.file_reading_limits.as_ref())
            .map(|limits| limits.max_size_bytes)
            .or_else(|| input.get("max_bytes").and_then(Value::as_u64).map(|v| v as usize))
            .unwrap_or(1_048_576);
        let offset = input.get("offset").and_then(Value::as_u64).map(|v| v as usize);
        let limit = input.get("limit").and_then(Value::as_u64).map(|v| v as usize);
        let resolved = resolve_path(&root, rel)?;
        if !resolved.is_file() {
            return Err(ToolError::ExecutionFailed(format!("Not a file: {rel}")));
        }
        let metadata = std::fs::metadata(&resolved)
            .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;
        let mtime_secs = metadata
            .modified()
            .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?
            .duration_since(UNIX_EPOCH)
            .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?
            .as_secs();
        let cache = capability.and_then(|cap| cap.read_file_state.as_ref());
        if offset.is_none() && limit.is_none() {
            if let Some(state) = cache.and_then(|cache| cache.get(&resolved)) {
                if state.mtime_secs == mtime_secs && state.offset.is_none() && state.limit.is_none()
                {
                    let cache_is_too_short = metadata.len() as usize > state.content.len()
                        && max_bytes > state.content.len();
                    if !cache_is_too_short {
                        let (content, limit_truncated) =
                            limit_text_content(&state.content, max_bytes);
                        let truncated =
                            limit_truncated || metadata.len() as usize > content.len();
                        let mut result = json!({
                            "path": rel,
                            "content": content,
                            "size": metadata.len(),
                            "cached": true,
                        });
                        if truncated {
                            result["truncated"] = json!(true);
                        }
                        return Ok(tool_result("read_workspace_file", result));
                    }
                }
            }
        }
        let bytes = std::fs::read(&resolved)
            .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;
        let full_content = String::from_utf8_lossy(&bytes).to_string();
        let (content, limit_truncated) = limit_text_content(&full_content, max_bytes);
        let truncated = limit_truncated || bytes.len() > content.len();
        if offset.is_none() && limit.is_none() {
            if let Some(cache) = cache {
                cache.set(
                    resolved.clone(),
                    FileState {
                        content: content.clone(),
                        mtime_secs,
                        offset: None,
                        limit: None,
                    },
                );
            }
        }
        let mut result = json!({ "path": rel, "content": content, "size": bytes.len() });
        if truncated {
            result["truncated"] = json!(true);
        }
        Ok(tool_result("read_workspace_file", result))
    }
}

// ── SearchFilesRuntimeTool ────────────────────────────────────────────────

pub struct SearchFilesRuntimeTool;

fn matches_glob(name: &str, pattern: &str) -> bool {
    let parts: Vec<&str> = pattern.split('*').collect();
    if parts.len() == 1 {
        return name == pattern;
    }
    let mut remaining = name;
    for (i, part) in parts.iter().enumerate() {
        if part.is_empty() {
            continue;
        }
        if i == 0 {
            if !remaining.starts_with(part) {
                return false;
            }
            remaining = &remaining[part.len()..];
        } else if i == parts.len() - 1 {
            if !remaining.ends_with(part) {
                return false;
            }
        } else if let Some(pos) = remaining.find(part) {
            remaining = &remaining[pos + part.len()..];
        } else {
            return false;
        }
    }
    true
}

fn walk_dir_collect(
    dir: &Path,
    file_pattern: &str,
    root: &Path,
    results: &mut Vec<Value>,
    max: usize,
) {
    if results.len() >= max {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        if results.len() >= max {
            break;
        }
        let Ok(ft) = entry.file_type() else {
            continue;
        };
        if ft.is_symlink() {
            continue;
        }
        let path = entry.path();
        if ft.is_dir() {
            walk_dir_collect(&path, file_pattern, root, results, max);
        } else if ft.is_file() {
            let name = entry.file_name().to_string_lossy().to_string();
            if matches_glob(&name, file_pattern) {
                let rel = path
                    .strip_prefix(root)
                    .map(|p| p.to_string_lossy().to_string())
                    .unwrap_or_else(|_| path.to_string_lossy().to_string());
                results.push(json!({
                    "name": name,
                    "path": rel,
                    "size": entry.metadata().map(|m| m.len()).unwrap_or(0),
                }));
            }
        }
    }
}

#[async_trait]
impl RuntimeTool for SearchFilesRuntimeTool {
    fn definition(&self) -> ToolDefinition {
        TOOL_CATALOG
            .get("search_files")
            .cloned()
            .unwrap_or_else(|| ToolDefinition::new("search_files", "Search files"))
    }

    fn is_concurrency_safe(&self, _input: &Value) -> bool {
        true
    }

    async fn execute(
        &self,
        input: Value,
        ctx: ToolExecutionContext,
    ) -> Result<ToolResult, ToolError> {
        let root = require_workspace_root(&ctx)?;
        let pattern = input
            .get("pattern")
            .and_then(Value::as_str)
            .ok_or_else(|| ToolError::ExecutionFailed("Missing required: pattern".into()))?;
        let sub = input.get("path").and_then(Value::as_str).unwrap_or(".");
        let max = input
            .get("max_results")
            .and_then(Value::as_u64)
            .unwrap_or(100) as usize;
        let base = resolve_path(&root, sub)?;
        let file_pattern = Path::new(pattern)
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| pattern.to_string());
        let mut matches = Vec::new();
        walk_dir_collect(&base, &file_pattern, &root, &mut matches, max);
        let count = matches.len();
        Ok(tool_result(
            "search_files",
            json!({ "pattern": pattern, "path": sub, "matches": matches, "count": count }),
        ))
    }
}

// ── GetFileInfoRuntimeTool ────────────────────────────────────────────────

pub struct GetFileInfoRuntimeTool;

#[async_trait]
impl RuntimeTool for GetFileInfoRuntimeTool {
    fn definition(&self) -> ToolDefinition {
        TOOL_CATALOG
            .get("get_file_info")
            .cloned()
            .unwrap_or_else(|| ToolDefinition::new("get_file_info", "Get file info"))
    }

    fn is_concurrency_safe(&self, _input: &Value) -> bool {
        true
    }

    async fn execute(
        &self,
        input: Value,
        ctx: ToolExecutionContext,
    ) -> Result<ToolResult, ToolError> {
        let root = require_workspace_root(&ctx)?;
        let rel = input
            .get("path")
            .and_then(Value::as_str)
            .ok_or_else(|| ToolError::ExecutionFailed("Missing required: path".into()))?;
        let resolved = resolve_path(&root, rel)?;
        if !resolved.exists() {
            return Err(ToolError::ExecutionFailed(format!(
                "Path does not exist: {rel}"
            )));
        }
        let meta = std::fs::metadata(&resolved)
            .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;
        let is_dir = meta.is_dir();
        let modified = meta
            .modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_secs());
        let mut info = json!({
            "path": rel,
            "name": resolved.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default(),
            "type": if is_dir { "directory" } else { "file" },
            "size": meta.len(),
        });
        if let Some(ts) = modified {
            info["modified_unix"] = json!(ts);
        }
        if !is_dir {
            let ext = resolved
                .extension()
                .map(|e| e.to_string_lossy().to_string())
                .unwrap_or_default();
            info["extension"] = json!(ext);
        }
        Ok(tool_result("get_file_info", info))
    }
}
