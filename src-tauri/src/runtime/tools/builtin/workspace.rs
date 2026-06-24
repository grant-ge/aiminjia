//! Workspace primitive tools as RuntimeTool.
//!
//! These tools require workspace capability via `ctx.capability`.
//! They NEVER accept a PluginContext — permissions come from CapabilityContext.

use anyhow::Result;
use async_trait::async_trait;
use serde_json::{json, Value};
use std::path::{Component, Path, PathBuf};
use std::time::UNIX_EPOCH;

use crate::runtime::path_auth::{decide, Decision, PathOp};
use crate::runtime::tools::capability::FileState;
use crate::runtime::tools::catalog::TOOL_CATALOG;
use crate::runtime::tools::context::ToolExecutionContext;
use crate::runtime::tools::definition::ToolDefinition;
use crate::runtime::tools::executor::{ToolError, ToolResult};
use crate::runtime::tools::permission::{PermissionDecision, PermissionReason};
use crate::runtime::tools::RuntimeTool;

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
pub(crate) fn require_workspace_root(
    ctx: &ToolExecutionContext,
) -> Result<std::path::PathBuf, ToolError> {
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

/// Resolve and authorize a path for a workspace tool operation.
///
/// Steps:
/// 1. Extract StorageCapability from ctx (returns PermissionDenied if missing).
/// 2. Determine primary root (authorized_workspace.root_path or workspace_path).
/// 3. Canonicalize the input path (absolute as-is, relative joined to primary root).
/// 4. Build an effective ToolPermissionContext that always has primary_root set,
///    so that paths under the workspace are Allow without requiring Ask.
/// 5. Call `path_auth::decide::is_path_allowed` with the effective ctx.
///
/// Why the effective_ctx override: `ToolPermissionContext::empty()` (used in tests
/// and new sessions before Phase 4 is fully wired) has `primary_root = None`.
/// Workspace tools should always allow access inside the primary workspace root, so
/// we implicitly set `primary_root` from `cap.workspace_path` when not already set.
pub(crate) async fn resolve_and_authorize_path(
    ctx: &ToolExecutionContext,
    input: &str,
    op: PathOp,
) -> Result<PathBuf, ToolError> {
    let cap = ctx
        .capability
        .as_ref()
        .and_then(|c| c.storage.as_ref())
        .ok_or_else(|| {
            ToolError::PermissionDenied(
                "No capability context. Workspace tools require capability.".into(),
            )
        })?;

    let perm_ctx = cap.permission_ctx.as_ref();

    // Primary root: authorized_workspace or workspace_path.
    let primary = cap
        .authorized_workspace
        .as_ref()
        .map(|aw| aw.root_path.clone())
        .unwrap_or_else(|| cap.workspace_path.clone());

    // Canonicalize the path.
    let raw = Path::new(input);
    if is_relative_traversal(raw) {
        return Err(ToolError::PermissionDenied(format!(
            "Relative path must stay inside workspace: {input}"
        )));
    }
    let canonical = if raw.is_absolute() {
        decide::canonicalize_or_ancestor(raw)
            .map_err(|e| ToolError::ExecutionFailed(format!("Failed to resolve path: {e}")))?
    } else {
        decide::canonicalize_or_ancestor(&primary.join(input))
            .map_err(|e| ToolError::ExecutionFailed(format!("Failed to resolve path: {e}")))?
    };

    // Build effective context: if primary_root is not set, inject it from the
    // workspace root so that relative-path tools continue to work without Ask.
    let effective_ctx;
    let ctx_ref = if perm_ctx.primary_root.is_none() {
        effective_ctx = crate::runtime::path_auth::ToolPermissionContext {
            primary_root: Some(primary.clone()),
            ..(*perm_ctx).clone()
        };
        &effective_ctx
    } else {
        perm_ctx
    };

    // If the dispatcher injected permission_override=Allow (i.e. user just approved),
    // skip the path-auth check entirely — the user's approval covers this path.
    if matches!(
        ctx.permission_override,
        Some(crate::runtime::tools::permission::PermissionDecision::Allow { .. })
    ) {
        return Ok(canonical);
    }

    match decide::is_path_allowed(&canonical, op, ctx_ref) {
        Decision::Allow => Ok(canonical),
        Decision::Deny(msg) => Err(ToolError::PermissionDenied(msg)),
        Decision::Ask { reason } => {
            // Ask must be surfaced via check_permissions before execute is called.
            // Reaching here means the tool was invoked without prior authorization.
            Err(ToolError::PermissionDenied(format!(
                "需要用户授权但权限请求未被处理：{reason}"
            )))
        }
    }
}

/// Shared helper for `check_permissions` across all path-model tools.
///
/// Returns:
/// - `None`  — Allow; let execute proceed.
/// - `Some(PermissionDecision::Deny)`  — blocked.
/// - `Some(PermissionDecision::Ask)`   — requires user confirmation.
///
/// Backward-compat: also consults `ctx.permission_store.get_for_path` for
/// legacy `PathGlob` deny rules that pre-Phase-4 code may have written.
/// These are not yet loaded into `permission_ctx.deny_rules` by the bridge.
pub(crate) fn check_path_permission(
    input: &Value,
    ctx: &ToolExecutionContext,
    op: PathOp,
    tool_name: &str,
) -> Option<PermissionDecision> {
    let path_str = input
        .get("file_path")
        .or_else(|| input.get("path"))
        .and_then(Value::as_str)?;

    let cap = ctx.capability.as_ref().and_then(|c| c.storage.as_ref())?;
    let perm_ctx = cap.permission_ctx.as_ref();

    let primary = cap
        .authorized_workspace
        .as_ref()
        .map(|aw| aw.root_path.clone())
        .unwrap_or_else(|| cap.workspace_path.clone());

    let raw = Path::new(path_str);
    if is_relative_traversal(raw) {
        return Some(PermissionDecision::Deny {
            message: format!("Relative path must stay inside workspace: {path_str}"),
            reason: PermissionReason::Capability,
        });
    }
    let canonical = if raw.is_absolute() {
        decide::canonicalize_or_ancestor(raw).ok()?
    } else {
        decide::canonicalize_or_ancestor(&primary.join(path_str)).ok()?
    };

    // Build effective context with primary_root set (same logic as resolve_and_authorize_path).
    let effective_ctx;
    let ctx_ref = if perm_ctx.primary_root.is_none() {
        effective_ctx = crate::runtime::path_auth::ToolPermissionContext {
            primary_root: Some(primary.clone()),
            ..(*perm_ctx).clone()
        };
        &effective_ctx
    } else {
        perm_ctx
    };

    // Backward-compat: check legacy PathGlob deny rules in PermissionStore.
    // These are the old-style rules written via `record_to(..., PathGlob(...), AlwaysDeny)`.
    // Until they are migrated into permission_ctx.deny_rules, we must still check them here.
    if let Some(store) = ctx.permission_store.as_ref() {
        use crate::runtime::store::permission_store::PolicyDecision;
        let lookup_path = canonical.to_string_lossy().to_string();
        match store.get_for_path(tool_name, &lookup_path) {
            Some(PolicyDecision::AlwaysDeny) | Some(PolicyDecision::Deny) => {
                return Some(PermissionDecision::Deny {
                    message: format!(
                        "Write to '{}' is blocked by stored PathGlob policy.",
                        path_str
                    ),
                    reason: PermissionReason::StoredPolicy,
                });
            }
            Some(PolicyDecision::AlwaysAllow) | Some(PolicyDecision::Allow) => {
                return Some(PermissionDecision::Allow {
                    updated_input: None,
                    reason: PermissionReason::StoredPolicy,
                });
            }
            None => {}
        }
    }

    // Primary path-auth decision.
    match decide::is_path_allowed(&canonical, op, ctx_ref) {
        Decision::Allow => None, // let execute proceed
        Decision::Deny(msg) => Some(PermissionDecision::Deny {
            message: msg,
            reason: PermissionReason::Capability,
        }),
        Decision::Ask { reason } => {
            // Encode step-6 vs step-4b in the path_auth_scope field for persistence routing.
            // Why §7.8 "Ask 中 path 的粒度": A 场景（step 6）的 path 应是包含目标文件的目录，
            // 而非文件本身——否则"永久允许"加的是单文件 working_dir，同目录兄弟仍被拦。
            // step-4b 写入用 working_dir 范围的 glob (canonical_dir/**)，写在 pathwrite: 后。
            let scope_path: std::path::PathBuf = if canonical.is_dir() {
                canonical.clone()
            } else {
                canonical
                    .parent()
                    .map(|p| p.to_path_buf())
                    .unwrap_or_else(|| canonical.clone())
            };
            let path_auth_scope = match op {
                PathOp::Write if is_step_4b_write(&canonical, op, ctx_ref) => {
                    format!("pathwrite:{}", scope_path.display())
                }
                PathOp::Delete => format!("pathdelete:{}", scope_path.display()),
                _ => format!("path:{}", scope_path.display()),
            };
            Some(PermissionDecision::Ask {
                message: reason,
                suggestions: vec!["仅本次允许".into(), "永久允许".into(), "拒绝".into()],
                remember_options: vec![
                    crate::runtime::tools::permission::PermissionDestination::Session,
                    crate::runtime::tools::permission::PermissionDestination::User,
                ],
                default_destination: Some(
                    crate::runtime::tools::permission::PermissionDestination::Session,
                ),
                reason: PermissionReason::Capability,
                path_auth_scope: Some(path_auth_scope),
            })
        }
    }
}

fn is_relative_traversal(path: &Path) -> bool {
    !path.is_absolute()
        && path
            .components()
            .any(|component| matches!(component, Component::ParentDir))
}

/// Returns true if this is a step-4b write Ask (inside additional_working_dirs, not primary).
fn is_step_4b_write(
    canonical: &Path,
    op: PathOp,
    ctx: &crate::runtime::path_auth::ToolPermissionContext,
) -> bool {
    if op != PathOp::Write {
        return false;
    }
    ctx.additional_working_dirs.keys().any(|dir| {
        let canonical_dir = std::fs::canonicalize(dir).unwrap_or_else(|_| dir.clone());
        canonical.starts_with(&canonical_dir)
    })
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

fn binary_media_type_from_extension(path: &Path) -> Option<&'static str> {
    let ext = path
        .extension()
        .and_then(|value| value.to_str())
        .map(|value| value.to_ascii_lowercase())?;

    match ext.as_str() {
        "png" => Some("image/png"),
        "jpg" | "jpeg" => Some("image/jpeg"),
        "gif" => Some("image/gif"),
        "webp" => Some("image/webp"),
        "bmp" => Some("image/bmp"),
        "ico" => Some("image/x-icon"),
        "tif" | "tiff" => Some("image/tiff"),
        "heic" => Some("image/heic"),
        "avif" => Some("image/avif"),
        "pdf" => Some("application/pdf"),
        "zip" => Some("application/zip"),
        "gz" | "tgz" => Some("application/gzip"),
        "bz2" => Some("application/x-bzip2"),
        "xz" => Some("application/x-xz"),
        "7z" => Some("application/x-7z-compressed"),
        "rar" => Some("application/vnd.rar"),
        "tar" => Some("application/x-tar"),
        "doc" => Some("application/msword"),
        "xls" => Some("application/vnd.ms-excel"),
        "ppt" => Some("application/vnd.ms-powerpoint"),
        "docx" => Some("application/vnd.openxmlformats-officedocument.wordprocessingml.document"),
        "xlsx" => Some("application/vnd.openxmlformats-officedocument.spreadsheetml.sheet"),
        "pptx" => Some("application/vnd.openxmlformats-officedocument.presentationml.presentation"),
        "sqlite" | "sqlite3" | "db" => Some("application/vnd.sqlite3"),
        "parquet" => Some("application/vnd.apache.parquet"),
        "pyc" => Some("application/x-python-code"),
        "exe" | "dll" | "so" | "dylib" | "bin" => Some("application/octet-stream"),
        "wasm" => Some("application/wasm"),
        "mp3" => Some("audio/mpeg"),
        "wav" => Some("audio/wav"),
        "m4a" => Some("audio/mp4"),
        "flac" => Some("audio/flac"),
        "ogg" => Some("audio/ogg"),
        "mp4" => Some("video/mp4"),
        "mov" => Some("video/quicktime"),
        "avi" => Some("video/x-msvideo"),
        "mkv" => Some("video/x-matroska"),
        "webm" => Some("video/webm"),
        _ => None,
    }
}

fn detect_binary_media_type(path: &Path, bytes: &[u8]) -> &'static str {
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        return "image/png";
    }
    if bytes.starts_with(&[0xff, 0xd8, 0xff]) {
        return "image/jpeg";
    }
    if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        return "image/gif";
    }
    if bytes.len() >= 12 && &bytes[..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        return "image/webp";
    }
    if bytes.starts_with(b"%PDF-") {
        return "application/pdf";
    }
    if bytes.starts_with(b"PK\x03\x04") || bytes.starts_with(b"PK\x05\x06") {
        return binary_media_type_from_extension(path).unwrap_or("application/zip");
    }
    if bytes.starts_with(&[0x1f, 0x8b]) {
        return "application/gzip";
    }

    binary_media_type_from_extension(path).unwrap_or("application/octet-stream")
}

fn is_known_binary_path(path: &Path) -> bool {
    binary_media_type_from_extension(path).is_some()
}

fn is_probably_binary(bytes: &[u8]) -> bool {
    if bytes.is_empty() {
        return false;
    }

    let sample_len = bytes.len().min(8192);
    let sample = &bytes[..sample_len];
    if sample.contains(&0) {
        return true;
    }

    let control_count = sample
        .iter()
        .filter(|byte| **byte < 0x20 && !matches!(**byte, b'\n' | b'\r' | b'\t' | 0x0c | 0x08))
        .count();

    control_count * 100 > sample_len * 10
}

fn binary_read_tool_result(
    rel: &str,
    size: u64,
    media_type: &'static str,
    offset: Option<usize>,
    limit: Option<usize>,
) -> ToolResult {
    let mut result = json!({
        "file_path": rel,
        "content": "",
        "size": size,
        "binary": true,
        "media_type": media_type,
        "message": "Binary or media file content was not returned as text. Use metadata, OCR, image, PDF, archive, or domain-specific parser tools instead.",
    });
    if offset.is_some() || limit.is_some() {
        result["range_ignored"] = json!(true);
    }
    tool_result("Read", result)
}

fn update_file_state_cache(ctx: &ToolExecutionContext, resolved: &Path, content: &str) {
    if let Some(cache) = ctx
        .capability
        .as_ref()
        .and_then(|cap| cap.read_file_state.as_ref())
    {
        let mtime_secs = std::fs::metadata(resolved)
            .ok()
            .and_then(|m| m.modified().ok())
            .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
            .map(|d| d.as_secs())
            .unwrap_or(0);
        cache.set(
            resolved.to_path_buf(),
            FileState {
                content: content.to_string(),
                mtime_secs,
                offset: None,
                limit: None,
            },
        );
    }
}

// ── ReadWorkspaceFileRuntimeTool ──────────────────────────────────────────

pub struct ReadWorkspaceFileRuntimeTool;

#[async_trait]
impl RuntimeTool for ReadWorkspaceFileRuntimeTool {
    fn id(&self) -> &str {
        "Read"
    }

    async fn definition(
        &self,
        _ctx: &crate::runtime::tools::ToolDescriptionContext,
    ) -> ToolDefinition {
        TOOL_CATALOG
            .get("Read")
            .unwrap_or_else(|| ToolDefinition::new("Read", "Read workspace file"))
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
        check_path_permission(input, ctx, PathOp::Read, "read_workspace_file")
    }

    async fn execute(
        &self,
        input: Value,
        ctx: ToolExecutionContext,
    ) -> Result<ToolResult, ToolError> {
        let capability = ctx.capability.as_ref();
        let rel = input
            .get("file_path")
            .and_then(Value::as_str)
            .ok_or_else(|| ToolError::ExecutionFailed("Missing required: file_path".into()))?;
        let max_bytes = capability
            .and_then(|cap| cap.file_reading_limits.as_ref())
            .map(|limits| limits.max_size_bytes)
            .or_else(|| {
                input
                    .get("max_bytes")
                    .and_then(Value::as_u64)
                    .map(|v| v as usize)
            })
            .unwrap_or(1_048_576);
        let offset = input
            .get("offset")
            .and_then(Value::as_u64)
            .map(|v| v as usize);
        let limit = input
            .get("limit")
            .and_then(Value::as_u64)
            .map(|v| v as usize);
        let resolved = resolve_and_authorize_path(&ctx, rel, PathOp::Read).await?;
        if !resolved.is_file() {
            return Err(ToolError::ExecutionFailed(format!("Not a file: {rel}")));
        }
        let metadata =
            std::fs::metadata(&resolved).map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;
        let mtime_secs = metadata
            .modified()
            .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?
            .duration_since(UNIX_EPOCH)
            .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?
            .as_secs();
        let cache = capability.and_then(|cap| cap.read_file_state.as_ref());
        let known_binary_path = is_known_binary_path(&resolved);
        if !known_binary_path && offset.is_none() && limit.is_none() {
            if let Some(state) = cache.and_then(|cache| cache.get(&resolved)) {
                if state.mtime_secs == mtime_secs && state.offset.is_none() && state.limit.is_none()
                {
                    let cache_is_too_short = metadata.len() as usize > state.content.len()
                        && max_bytes > state.content.len();
                    if !cache_is_too_short {
                        let (content, limit_truncated) =
                            limit_text_content(&state.content, max_bytes);
                        let truncated = limit_truncated || metadata.len() as usize > content.len();
                        let mut result = json!({
                            "file_path": rel,
                            "content": content,
                            "size": metadata.len(),
                            "cached": true,
                        });
                        if truncated {
                            result["truncated"] = json!(true);
                        }
                        return Ok(tool_result("Read", result));
                    }
                }
            }
        }
        let bytes =
            std::fs::read(&resolved).map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;
        if known_binary_path || is_probably_binary(&bytes) {
            return Ok(binary_read_tool_result(
                rel,
                metadata.len(),
                detect_binary_media_type(&resolved, &bytes),
                offset,
                limit,
            ));
        }
        let full_content = String::from_utf8_lossy(&bytes).to_string();
        if offset.is_some() || limit.is_some() {
            let start_line = offset.unwrap_or(1).max(1);
            let take_lines = limit.unwrap_or(2000);
            let total_lines = full_content.split_inclusive('\n').count();
            let mut sliced = String::new();
            let mut lines_returned = 0usize;
            for line in full_content
                .split_inclusive('\n')
                .skip(start_line - 1)
                .take(take_lines)
            {
                sliced.push_str(line);
                lines_returned += 1;
            }
            let (content, limit_truncated) = limit_text_content(&sliced, max_bytes);
            let mut result = json!({
                "file_path": rel,
                "content": content,
                "size": bytes.len(),
                "start_line": start_line,
                "lines_returned": lines_returned,
                "total_lines": total_lines,
            });
            if limit_truncated {
                result["truncated"] = json!(true);
            }
            return Ok(tool_result("Read", result));
        }
        let (content, limit_truncated) = limit_text_content(&full_content, max_bytes);
        let truncated = limit_truncated || bytes.len() > content.len();
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
        let mut result = json!({ "file_path": rel, "content": content, "size": bytes.len() });
        if truncated {
            result["truncated"] = json!(true);
        }
        Ok(tool_result("Read", result))
    }
}

// ── SearchFilesRuntimeTool ────────────────────────────────────────────────

pub struct SearchFilesRuntimeTool;

pub(crate) fn matches_glob(name: &str, pattern: &str) -> bool {
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
    fn id(&self) -> &str {
        "Glob"
    }

    async fn definition(
        &self,
        _ctx: &crate::runtime::tools::ToolDescriptionContext,
    ) -> ToolDefinition {
        TOOL_CATALOG
            .get("Glob")
            .unwrap_or_else(|| ToolDefinition::new("Glob", "Search files"))
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
        check_path_permission(input, ctx, PathOp::Read, "search_files")
    }

    async fn execute(
        &self,
        input: Value,
        ctx: ToolExecutionContext,
    ) -> Result<ToolResult, ToolError> {
        let pattern = input
            .get("pattern")
            .and_then(Value::as_str)
            .ok_or_else(|| ToolError::ExecutionFailed("Missing required: pattern".into()))?;
        let sub = input.get("path").and_then(Value::as_str).unwrap_or(".");
        let max = input
            .get("max_results")
            .and_then(Value::as_u64)
            .unwrap_or(100) as usize;
        let base = resolve_and_authorize_path(&ctx, sub, PathOp::Read).await?;
        // For walk, we use the authorized root as the display root.
        let root = require_workspace_root(&ctx).unwrap_or_else(|_| base.clone());
        let file_pattern = Path::new(pattern)
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| pattern.to_string());
        let mut matches = Vec::new();
        walk_dir_collect(&base, &file_pattern, &root, &mut matches, max);
        let count = matches.len();
        Ok(tool_result(
            "Glob",
            json!({ "pattern": pattern, "path": sub, "matches": matches, "count": count }),
        ))
    }
}

// ── WriteFileRuntimeTool ──────────────────────────────────────────────────

pub struct WriteFileRuntimeTool;

#[async_trait]
impl RuntimeTool for WriteFileRuntimeTool {
    fn id(&self) -> &str {
        "Write"
    }

    async fn definition(
        &self,
        _ctx: &crate::runtime::tools::ToolDescriptionContext,
    ) -> ToolDefinition {
        TOOL_CATALOG
            .get("Write")
            .unwrap_or_else(|| ToolDefinition::new("Write", "Write workspace file"))
    }

    fn is_concurrency_safe(&self, _input: &Value) -> bool {
        false
    }

    async fn check_permissions(
        &self,
        input: &Value,
        ctx: &ToolExecutionContext,
    ) -> Option<PermissionDecision> {
        check_path_permission(input, ctx, PathOp::Write, "Write")
    }

    async fn execute(
        &self,
        input: Value,
        ctx: ToolExecutionContext,
    ) -> Result<ToolResult, ToolError> {
        let rel = input
            .get("file_path")
            .and_then(Value::as_str)
            .ok_or_else(|| ToolError::ExecutionFailed("Missing required: file_path".into()))?;
        let content = input
            .get("content")
            .and_then(Value::as_str)
            .ok_or_else(|| ToolError::ExecutionFailed("Missing required: content".into()))?;
        let resolved = resolve_and_authorize_path(&ctx, rel, PathOp::Write).await?;

        if let Some(parent) = resolved.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| ToolError::ExecutionFailed(format!("Failed to create dirs: {e}")))?;
        }

        std::fs::write(&resolved, content.as_bytes())
            .map_err(|e| ToolError::ExecutionFailed(format!("Failed to write file: {e}")))?;

        update_file_state_cache(&ctx, &resolved, content);

        Ok(tool_result(
            "Write",
            json!({
                "file_path": rel,
                "size": content.len(),
                "created": true,
            }),
        ))
    }
}

// ── EditFileRuntimeTool ───────────────────────────────────────────────────

pub struct EditFileRuntimeTool;

#[async_trait]
impl RuntimeTool for EditFileRuntimeTool {
    fn id(&self) -> &str {
        "Edit"
    }

    async fn definition(
        &self,
        _ctx: &crate::runtime::tools::ToolDescriptionContext,
    ) -> ToolDefinition {
        TOOL_CATALOG
            .get("Edit")
            .unwrap_or_else(|| ToolDefinition::new("Edit", "Edit workspace file"))
    }

    fn is_concurrency_safe(&self, _input: &Value) -> bool {
        false
    }

    async fn check_permissions(
        &self,
        input: &Value,
        ctx: &ToolExecutionContext,
    ) -> Option<PermissionDecision> {
        check_path_permission(input, ctx, PathOp::Write, "Edit")
    }

    async fn execute(
        &self,
        input: Value,
        ctx: ToolExecutionContext,
    ) -> Result<ToolResult, ToolError> {
        let rel = input
            .get("file_path")
            .and_then(Value::as_str)
            .ok_or_else(|| ToolError::ExecutionFailed("Missing required: file_path".into()))?;
        let old_string = input
            .get("old_string")
            .and_then(Value::as_str)
            .ok_or_else(|| ToolError::ExecutionFailed("Missing required: old_string".into()))?;
        let new_string = input
            .get("new_string")
            .and_then(Value::as_str)
            .ok_or_else(|| ToolError::ExecutionFailed("Missing required: new_string".into()))?;
        let replace_all = input
            .get("replace_all")
            .and_then(Value::as_bool)
            .unwrap_or(false);

        // Authorize for write (the actual operation performed).
        let resolved = resolve_and_authorize_path(&ctx, rel, PathOp::Write).await?;

        let original_content = if resolved.is_file() {
            std::fs::read_to_string(&resolved)
                .map_err(|e| ToolError::ExecutionFailed(format!("Failed to read file: {e}")))?
        } else if old_string.is_empty() {
            String::new()
        } else {
            return Err(ToolError::ExecutionFailed(format!(
                "File does not exist: {rel}"
            )));
        };

        if old_string.is_empty() {
            if !original_content.trim().is_empty() {
                return Err(ToolError::ExecutionFailed(
                    "old_string is empty but file already has content. Use write_file to overwrite, or provide old_string to match existing content.".into(),
                ));
            }
            if let Some(parent) = resolved.parent() {
                std::fs::create_dir_all(parent).map_err(|e| {
                    ToolError::ExecutionFailed(format!("Failed to create dirs: {e}"))
                })?;
            }
            std::fs::write(&resolved, new_string.as_bytes())
                .map_err(|e| ToolError::ExecutionFailed(format!("Failed to write file: {e}")))?;
            update_file_state_cache(&ctx, &resolved, new_string);
            return Ok(tool_result(
                "Edit",
                json!({
                    "file_path": rel,
                    "operation": "create",
                    "bytes_written": new_string.len(),
                }),
            ));
        }

        let matches = original_content.matches(old_string).count();
        if matches == 0 {
            return Err(ToolError::ExecutionFailed(format!(
                "old_string not found in file: {rel}\nString: {old_string}"
            )));
        }
        if matches > 1 && !replace_all {
            return Err(ToolError::ExecutionFailed(format!(
                "old_string found {matches} times in file: {rel}. Provide more context to uniquely identify the target, or pass replace_all: true to replace all occurrences.\nString: {old_string}"
            )));
        }

        let updated_content = if replace_all {
            original_content.replace(old_string, new_string)
        } else {
            original_content.replacen(old_string, new_string, 1)
        };
        std::fs::write(&resolved, updated_content.as_bytes())
            .map_err(|e| ToolError::ExecutionFailed(format!("Failed to write file: {e}")))?;
        update_file_state_cache(&ctx, &resolved, &updated_content);

        Ok(tool_result(
            "Edit",
            json!({
                "file_path": rel,
                "operation": "edit",
                "bytes_written": updated_content.len(),
                "replacements": matches,
            }),
        ))
    }
}
