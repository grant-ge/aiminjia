//! File lifecycle management — register, store, delete, cleanup, path resolution.
#![allow(dead_code)]

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{anyhow, Context, Result};
use serde::Serialize;

pub struct FileManager {
    workspace_path: PathBuf,
}

#[derive(Debug, Serialize)]
pub struct FileInfo {
    pub file_name: String,
    pub stored_path: String,
    pub file_size: u64,
    pub file_type: String,
}

impl FileManager {
    pub fn new(workspace_path: impl AsRef<Path>) -> Self {
        Self {
            workspace_path: workspace_path.as_ref().to_path_buf(),
        }
    }

    /// Get the workspace root directory path.
    pub fn workspace_path(&self) -> &Path {
        &self.workspace_path
    }

    /// Resolve a stored_path to a full path and verify it stays within the workspace.
    /// Returns an error if the resolved path escapes the workspace directory.
    fn safe_resolve(&self, stored_path: &str) -> Result<PathBuf> {
        let joined = self.workspace_path.join(stored_path);
        // Canonicalize to resolve ../ sequences. If the file doesn't exist yet,
        // canonicalize the parent directory instead.
        let canonical = if joined.exists() {
            joined.canonicalize()?
        } else {
            let parent = joined.parent().unwrap_or(&joined);
            fs::create_dir_all(parent).ok();
            if parent.exists() {
                let canon_parent = parent.canonicalize()?;
                let file_name = joined.file_name().unwrap_or_default();
                canon_parent.join(file_name)
            } else {
                joined.clone()
            }
        };
        let workspace_canonical = self
            .workspace_path
            .canonicalize()
            .unwrap_or_else(|_| self.workspace_path.clone());
        if !canonical.starts_with(&workspace_canonical) {
            return Err(anyhow!(
                "Path traversal rejected: '{}' resolves outside workspace",
                stored_path
            ));
        }
        Ok(canonical)
    }

    /// Copy an uploaded file to workspace/uploads/ and return its stored info.
    pub fn store_upload(&self, source_path: &Path) -> Result<FileInfo> {
        let file_name = source_path
            .file_name()
            .context("No filename")?
            .to_string_lossy()
            .to_string();

        let ext = source_path
            .extension()
            .map(|e| e.to_string_lossy().to_lowercase())
            .unwrap_or_default();

        let file_type = match ext.as_str() {
            "xlsx" | "xls" => "excel",
            "csv" | "tsv" => "csv",
            "docx" | "doc" => "word",
            "pdf" => "pdf",
            "pptx" | "ppt" => "ppt",
            "json" | "jsonl" => "json",
            "parquet" => "parquet",
            "txt" | "log" => "text",
            _ => "other",
        }
        .to_string();

        let dest_dir = self.workspace_path.join("uploads");
        fs::create_dir_all(&dest_dir)?;

        // Add UUID prefix to avoid name collisions
        let stored_name = format!(
            "{}_{}",
            uuid::Uuid::new_v4().to_string().split('-').next().unwrap(),
            file_name
        );
        let dest_path = dest_dir.join(&stored_name);

        fs::copy(source_path, &dest_path)
            .with_context(|| format!("Failed to copy {} to uploads", file_name))?;

        let file_size = fs::metadata(&dest_path)?.len();
        let stored_path = format!("uploads/{}", stored_name);

        Ok(FileInfo {
            file_name,
            stored_path,
            file_size,
            file_type,
        })
    }

    /// Write content to a file in the workspace. Returns the stored_path relative to workspace.
    pub fn write_file(&self, subdir: &str, file_name: &str, content: &[u8]) -> Result<FileInfo> {
        let dest_dir = self.workspace_path.join(subdir);
        fs::create_dir_all(&dest_dir)?;
        let dest_path = dest_dir.join(file_name);
        fs::write(&dest_path, content)
            .with_context(|| format!("Failed to write {}", dest_path.display()))?;

        let ext = Path::new(file_name)
            .extension()
            .map(|e| e.to_string_lossy().to_lowercase())
            .unwrap_or_default();
        let file_type = match ext.as_str() {
            "xlsx" | "xls" => "excel",
            "html" => "html",
            "pdf" => "pdf",
            "pptx" => "pptx",
            "docx" => "docx",
            "md" => "markdown",
            "csv" => "csv",
            "json" => "json",
            "png" => "png",
            "jpg" | "jpeg" => "jpeg",
            "webp" => "webp",
            "gif" => "gif",
            "bmp" => "bmp",
            "svg" => "svg",
            "py" => "py",
            _ => "csv",
        }
        .to_string();

        Ok(FileInfo {
            file_name: file_name.to_string(),
            stored_path: format!("{}/{}", subdir, file_name),
            file_size: content.len() as u64,
            file_type,
        })
    }

    /// Delete a file from workspace by its stored_path (relative to workspace root).
    pub fn delete_file(&self, stored_path: &str) -> Result<()> {
        let full_path = self.safe_resolve(stored_path)?;
        if full_path.exists() {
            fs::remove_file(&full_path)
                .with_context(|| format!("Failed to delete {}", full_path.display()))?;
        }
        Ok(())
    }

    pub fn resolve_existing_file(&self, stored_path: &str) -> Result<PathBuf> {
        let path = self.safe_resolve(stored_path)?;
        if !path.is_file() {
            return Err(anyhow!("Stored file does not exist: {}", stored_path));
        }
        Ok(path)
    }

    /// Get full absolute path for a stored_path.
    /// Validates that the path stays within the workspace.
    pub fn full_path(&self, stored_path: &str) -> PathBuf {
        match self.safe_resolve(stored_path) {
            Ok(p) => p,
            Err(_) => {
                // Log the rejection but return workspace root join (not the raw stored_path join)
                // This is a defensive fallback: callers should use safe_resolve directly for security-critical paths
                log::warn!("[FileManager::full_path] path traversal rejected for '{}', returning workspace root", stored_path);
                self.workspace_path.clone()
            }
        }
    }

    /// Clean up expired temp files older than `retention_days`.
    pub fn cleanup_temp_files(&self, retention_days: u32) -> Result<Vec<String>> {
        let temp_dir = self.workspace_path.join("temp");
        if !temp_dir.exists() {
            return Ok(vec![]);
        }

        let cutoff = chrono::Utc::now() - chrono::Duration::days(retention_days as i64);
        let mut deleted = Vec::new();

        for entry in fs::read_dir(&temp_dir)? {
            let entry = entry?;
            let meta = entry.metadata()?;
            if meta.is_file() {
                if let Ok(modified) = meta.modified() {
                    let modified_dt: chrono::DateTime<chrono::Utc> = modified.into();
                    if modified_dt < cutoff {
                        let name = entry.file_name().to_string_lossy().to_string();
                        fs::remove_file(entry.path()).ok();
                        deleted.push(name);
                    }
                }
            }
        }

        if !deleted.is_empty() {
            log::info!("Cleaned up {} temp files", deleted.len());
        }
        Ok(deleted)
    }

    /// Check if a file exists in the workspace.
    pub fn file_exists(&self, stored_path: &str) -> bool {
        self.safe_resolve(stored_path)
            .map(|p| p.exists())
            .unwrap_or(false)
    }
}

/// 通过 containment 校验解析相对路径到授权根目录下的绝对路径
pub fn resolve_local_reference(
    root_path: &std::path::Path,
    rel_path: &str,
) -> anyhow::Result<std::path::PathBuf> {
    use anyhow::anyhow;
    let joined = root_path.join(rel_path);

    fn canonicalize_existing_ancestor(
        path: &std::path::Path,
    ) -> anyhow::Result<std::path::PathBuf> {
        let mut current = path;
        while !current.exists() {
            current = current
                .parent()
                .ok_or_else(|| anyhow!("Path traversal rejected: ancestor missing"))?;
        }
        let canonical = current.canonicalize()?;
        let suffix = path
            .strip_prefix(current)
            .unwrap_or(std::path::Path::new(""));
        Ok(canonical.join(suffix))
    }

    // 如果路径存在则 canonicalize，否则 canonicalize 最近的已存在祖先
    let canonical = if joined.exists() {
        joined.canonicalize()?
    } else {
        canonicalize_existing_ancestor(&joined)?
    };
    let root_canonical = root_path
        .canonicalize()
        .unwrap_or_else(|_| root_path.to_path_buf());
    if !canonical.starts_with(&root_canonical) {
        return Err(anyhow!(
            "Path traversal rejected: '{}' escapes authorized workspace",
            rel_path
        ));
    }
    Ok(canonical)
}

/// 检查路径是否在授权目录内（containment check）
pub fn is_within_authorized_workspace(path: &std::path::Path, root: &std::path::Path) -> bool {
    let path_canonical = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    let root_canonical = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
    path_canonical.starts_with(&root_canonical)
}

#[cfg(test)]
mod tests {
    use super::FileManager;
    use tempfile::TempDir;

    #[test]
    fn write_file_preserves_image_file_types() {
        let tmp = TempDir::new().expect("tempdir");
        let file_mgr = FileManager::new(tmp.path());

        let jpg = file_mgr
            .write_file("generated", "photo.jpg", &[1, 2, 3])
            .expect("write jpg");
        let webp = file_mgr
            .write_file("generated", "preview.webp", &[1, 2, 3])
            .expect("write webp");

        assert_eq!(jpg.file_type, "jpeg");
        assert_eq!(webp.file_type, "webp");
    }

    #[test]
    #[cfg(unix)]
    fn test_symlink_escape_rejected() {
        let root = std::env::temp_dir().join("lotus_ws_symlink_test");
        std::fs::create_dir_all(&root).unwrap();
        let outside = std::env::temp_dir().join("lotus_ws_outside");
        std::fs::create_dir_all(&outside).unwrap();
        let link = root.join("escape_link");
        // 清理旧 link（测试可能重复跑）
        let _ = std::fs::remove_file(&link);
        std::os::unix::fs::symlink(&outside, &link).unwrap();
        let result = super::resolve_local_reference(&root, "escape_link");
        assert!(
            result.is_err(),
            "Symlink to outside root should be rejected"
        );
        // 清理
        let _ = std::fs::remove_file(&link);
    }
}
