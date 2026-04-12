use async_trait::async_trait;
use serde_json::{json, Value};
use crate::plugin::context::PluginContext;
use crate::plugin::tool_trait::{ToolOutput, ToolPlugin, ToolError};
use crate::storage::file_manager;

/// AuthorizedPathScope：路径边界校验辅助
pub struct AuthorizedPathScope {
    pub workspace_root: std::path::PathBuf,
}

impl AuthorizedPathScope {
    pub fn resolve(&self, rel_path: &str) -> anyhow::Result<std::path::PathBuf> {
        file_manager::resolve_local_reference(&self.workspace_root, rel_path)
    }
}

// ── 工具 1: list_directory ──────────────────────────────────────────────────

pub struct ListDirectoryTool;

#[async_trait]
impl ToolPlugin for ListDirectoryTool {
    fn name(&self) -> &str { "list_directory" }
    fn description(&self) -> &str { "列出授权工作目录中的文件和子目录" }
    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "相对于授权工作目录的路径，默认为根目录 '.'",
                    "default": "."
                }
            }
        })
    }
    async fn execute(&self, ctx: &PluginContext, input: Value) -> Result<ToolOutput, ToolError> {
        let aw = ctx.authorized_workspace.as_ref()
            .ok_or_else(|| ToolError::ExecutionFailed("No authorized workspace set. Please authorize a directory first.".into()))?;
        let scope = AuthorizedPathScope { workspace_root: aw.root_path.clone() };
        let rel_path = input.get("path").and_then(|v| v.as_str()).unwrap_or(".");
        let resolved = scope.resolve(rel_path)
            .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;
        if !resolved.exists() {
            return Err(ToolError::ExecutionFailed(format!("Directory does not exist: {}", rel_path)));
        }
        if !resolved.is_dir() {
            return Err(ToolError::ExecutionFailed(format!("Not a directory: {}", rel_path)));
        }
        let entries = std::fs::read_dir(&resolved)
            .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;
        let mut files = Vec::new();
        for entry in entries.flatten() {
            let meta = entry.metadata().ok();
            let is_dir = meta.as_ref().map(|m| m.is_dir()).unwrap_or(false);
            let size = meta.as_ref().map(|m| m.len()).unwrap_or(0);
            files.push(json!({
                "name": entry.file_name().to_string_lossy(),
                "type": if is_dir { "directory" } else { "file" },
                "size": size,
            }));
        }
        files.sort_by(|a, b| {
            let a_type = a["type"].as_str().unwrap_or("file");
            let b_type = b["type"].as_str().unwrap_or("file");
            // directories first, then files
            b_type.cmp(a_type).then(
                a["name"].as_str().unwrap_or("").cmp(b["name"].as_str().unwrap_or(""))
            )
        });
        let count = files.len();
        let result = json!({ "path": rel_path, "files": files, "count": count });
        Ok(ToolOutput::success(serde_json::to_string_pretty(&result).unwrap_or_default()))
    }
}

// ── 工具 2: read_workspace_file ──────────────────────────────────────────────

pub struct ReadWorkspaceFileTool;

#[async_trait]
impl ToolPlugin for ReadWorkspaceFileTool {
    fn name(&self) -> &str { "read_workspace_file" }
    fn description(&self) -> &str { "读取授权工作目录中的文本文件内容" }
    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "required": ["path"],
            "properties": {
                "path": { "type": "string", "description": "相对于授权工作目录的文件路径" },
                "max_bytes": { "type": "integer", "description": "最多读取字节数，默认 1048576 (1MB)", "default": 1048576 }
            }
        })
    }
    async fn execute(&self, ctx: &PluginContext, input: Value) -> Result<ToolOutput, ToolError> {
        let aw = ctx.authorized_workspace.as_ref()
            .ok_or_else(|| ToolError::ExecutionFailed("No authorized workspace set.".into()))?;
        let rel_path = input.get("path").and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::ExecutionFailed("Missing required parameter: path".into()))?;
        let max_bytes = input.get("max_bytes").and_then(|v| v.as_u64()).unwrap_or(1_048_576) as usize;
        let scope = AuthorizedPathScope { workspace_root: aw.root_path.clone() };
        let resolved = scope.resolve(rel_path)
            .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;
        if !resolved.exists() {
            return Err(ToolError::ExecutionFailed(format!("File does not exist: {}", rel_path)));
        }
        if !resolved.is_file() {
            return Err(ToolError::ExecutionFailed(format!("Not a file: {}", rel_path)));
        }
        let content = std::fs::read(&resolved)
            .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;
        let truncated = content.len() > max_bytes;
        let content_str = String::from_utf8_lossy(if truncated { &content[..max_bytes] } else { &content }).to_string();
        let total_size = content.len();
        let mut result = json!({
            "path": rel_path,
            "content": content_str,
            "size": total_size,
        });
        if truncated {
            result["truncated"] = json!(true);
            result["note"] = json!(format!("Content truncated to {} bytes", max_bytes));
        }
        Ok(ToolOutput::success(serde_json::to_string_pretty(&result).unwrap_or_default()))
    }
}

// ── 工具 3: search_files ──────────────────────────────────────────────────────

pub struct SearchFilesTool;

fn matches_glob(name: &str, pattern: &str) -> bool {
    // 简单实现：只支持 * 通配符
    let parts: Vec<&str> = pattern.split('*').collect();
    if parts.len() == 1 {
        return name == pattern;
    }
    let mut remaining = name;
    for (i, part) in parts.iter().enumerate() {
        if part.is_empty() { continue; }
        if i == 0 {
            if !remaining.starts_with(part) { return false; }
            remaining = &remaining[part.len()..];
        } else if i == parts.len() - 1 {
            if !remaining.ends_with(part) { return false; }
        } else {
            if let Some(pos) = remaining.find(part) {
                remaining = &remaining[pos + part.len()..];
            } else {
                return false;
            }
        }
    }
    true
}

fn walk_dir(dir: &std::path::Path, pattern: &str, results: &mut Vec<serde_json::Value>, max: usize, root_path: &std::path::Path) {
    if results.len() >= max { return; }
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            if results.len() >= max { break; }
            // 跳过 symlink：避免循环引用和越界
            let file_type = match entry.file_type() {
                Ok(ft) => ft,
                Err(_) => continue,
            };
            if file_type.is_symlink() { continue; }
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().to_string();
            if file_type.is_dir() {
                walk_dir(&path, pattern, results, max, root_path);
            } else if file_type.is_file() && matches_glob(&name, pattern) {
                let rel = path.strip_prefix(root_path)
                    .map(|p| p.to_string_lossy().to_string())
                    .unwrap_or_else(|_| path.to_string_lossy().to_string());
                results.push(serde_json::json!({
                    "name": name,
                    "path": rel,
                    "size": entry.metadata().map(|m| m.len()).unwrap_or(0),
                }));
            }
        }
    }
}

#[async_trait]
impl ToolPlugin for SearchFilesTool {
    fn name(&self) -> &str { "search_files" }
    fn description(&self) -> &str { "在授权工作目录中搜索匹配 glob 模式的文件" }
    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "required": ["pattern"],
            "properties": {
                "pattern": { "type": "string", "description": "仅匹配文件名部分，如 '*.csv'、'report_*.xlsx'。如需限定子目录，请使用 path 参数指定子路径。" },
                "path": { "type": "string", "description": "搜索的子目录（相对路径），默认为 '.'", "default": "." },
                "max_results": { "type": "integer", "description": "最多返回结果数，默认 100", "default": 100 }
            }
        })
    }
    async fn execute(&self, ctx: &PluginContext, input: Value) -> Result<ToolOutput, ToolError> {
        let aw = ctx.authorized_workspace.as_ref()
            .ok_or_else(|| ToolError::ExecutionFailed("No authorized workspace set.".into()))?;
        let pattern = input.get("pattern").and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::ExecutionFailed("Missing required parameter: pattern".into()))?;
        let sub_path = input.get("path").and_then(|v| v.as_str()).unwrap_or(".");
        let max_results = input.get("max_results").and_then(|v| v.as_u64()).unwrap_or(100) as usize;
        let scope = AuthorizedPathScope { workspace_root: aw.root_path.clone() };
        let base = scope.resolve(sub_path)
            .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;
        // 提取文件名部分 pattern（最后一段）
        let file_pattern = std::path::Path::new(pattern)
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| pattern.to_string());
        let mut matches = Vec::new();
        walk_dir(&base, &file_pattern, &mut matches, max_results, &aw.root_path);
        let count = matches.len();
        let result = json!({
            "pattern": pattern,
            "path": sub_path,
            "matches": matches,
            "count": count,
        });
        Ok(ToolOutput::success(serde_json::to_string_pretty(&result).unwrap_or_default()))
    }
}

// ── 工具 4: get_file_info ────────────────────────────────────────────────────

pub struct GetFileInfoTool;

#[async_trait]
impl ToolPlugin for GetFileInfoTool {
    fn name(&self) -> &str { "get_file_info" }
    fn description(&self) -> &str { "获取授权工作目录中文件或目录的详细元数据" }
    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "required": ["path"],
            "properties": {
                "path": { "type": "string", "description": "相对于授权工作目录的路径" }
            }
        })
    }
    async fn execute(&self, ctx: &PluginContext, input: Value) -> Result<ToolOutput, ToolError> {
        let aw = ctx.authorized_workspace.as_ref()
            .ok_or_else(|| ToolError::ExecutionFailed("No authorized workspace set.".into()))?;
        let rel_path = input.get("path").and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::ExecutionFailed("Missing required parameter: path".into()))?;
        let scope = AuthorizedPathScope { workspace_root: aw.root_path.clone() };
        let resolved = scope.resolve(rel_path)
            .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;
        if !resolved.exists() {
            return Err(ToolError::ExecutionFailed(format!("Path does not exist: {}", rel_path)));
        }
        let meta = std::fs::metadata(&resolved)
            .map_err(|e| ToolError::ExecutionFailed(e.to_string()))?;
        let is_dir = meta.is_dir();
        let size = meta.len();
        let modified = meta.modified().ok().and_then(|t| {
            t.duration_since(std::time::UNIX_EPOCH).ok().map(|d| d.as_secs())
        });
        let mut info = json!({
            "path": rel_path,
            "name": resolved.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default(),
            "type": if is_dir { "directory" } else { "file" },
            "size": size,
        });
        if let Some(ts) = modified {
            info["modified_unix"] = json!(ts);
        }
        if !is_dir {
            let ext = resolved.extension().map(|e| e.to_string_lossy().to_string()).unwrap_or_default();
            info["extension"] = json!(ext);
        }
        Ok(ToolOutput::success(serde_json::to_string_pretty(&info).unwrap_or_default()))
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_path_traversal_rejected() {
        // Create a temp dir to use as root so canonicalize works
        let root = std::env::temp_dir().join("lotus_ws_test_traversal");
        std::fs::create_dir_all(&root).unwrap();
        let scope = AuthorizedPathScope { workspace_root: root };
        let result = scope.resolve("../secret");
        assert!(result.is_err(), "Path traversal should be rejected");
        let err_msg = result.unwrap_err().to_string();
        assert!(err_msg.contains("escapes authorized workspace") || err_msg.contains("traversal"),
                "Error message should mention traversal: {}", err_msg);
    }

    #[test]
    fn test_valid_path_within_scope_accepted() {
        let root = std::env::temp_dir().join("lotus_ws_test_valid");
        std::fs::create_dir_all(&root).unwrap();
        // Create a test file inside the root
        let test_file = root.join("data.csv");
        std::fs::write(&test_file, b"col1,col2\n1,2\n").unwrap();
        let scope = AuthorizedPathScope { workspace_root: root.clone() };
        let result = scope.resolve("data.csv");
        assert!(result.is_ok(), "Valid path within workspace should be accepted");
        let resolved = result.unwrap();
        assert!(resolved.starts_with(root.canonicalize().unwrap()));
        // Cleanup
        std::fs::remove_file(test_file).ok();
    }
}
