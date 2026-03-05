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
        let workspace_canonical = self.workspace_path.canonicalize()
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
            uuid::Uuid::new_v4()
                .to_string()
                .split('-')
                .next()
                .unwrap(),
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
            "csv" => "csv",
            "json" => "json",
            "png" => "png",
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

    /// Get full absolute path for a stored_path.
    /// Validates that the path stays within the workspace.
    pub fn full_path(&self, stored_path: &str) -> PathBuf {
        self.safe_resolve(stored_path)
            .unwrap_or_else(|_| self.workspace_path.join(stored_path))
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
