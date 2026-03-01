//! Workspace directory structure creation and validation.
#![allow(dead_code)]

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::Serialize;

/// Subdirectories within the workspace
const WORKSPACE_DIRS: &[&str] = &["uploads", "analysis", "reports", "scripts", "temp"];

pub struct WorkspaceManager {
    path: PathBuf,
}

#[derive(Debug, Serialize)]
pub struct WorkspaceInfo {
    pub path: String,
    pub exists: bool,
    pub total_size: u64,
    pub file_count: u32,
    pub subdirectories: Vec<SubdirInfo>,
}

#[derive(Debug, Serialize)]
pub struct SubdirInfo {
    pub name: String,
    pub file_count: u32,
    pub total_size: u64,
}

impl WorkspaceManager {
    /// Create a WorkspaceManager for the given path.
    /// Does NOT create the directory — call ensure_structure() for that.
    pub fn new(path: impl AsRef<Path>) -> Self {
        Self {
            path: path.as_ref().to_path_buf(),
        }
    }

    /// Get the workspace root path.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Ensure the workspace directory structure exists.
    /// Creates: uploads/, analysis/, reports/, scripts/, temp/
    pub fn ensure_structure(&self) -> Result<()> {
        for dir in WORKSPACE_DIRS {
            let dir_path = self.path.join(dir);
            fs::create_dir_all(&dir_path)
                .with_context(|| format!("Failed to create {}", dir_path.display()))?;
        }
        log::info!("Workspace structure ensured at {}", self.path.display());
        Ok(())
    }

    /// Check if the workspace path is valid and writable.
    pub fn validate(&self) -> Result<bool> {
        if !self.path.exists() {
            return Ok(false);
        }
        if !self.path.is_dir() {
            anyhow::bail!("Workspace path is not a directory");
        }
        // Test writability by creating and removing a temp file
        let test_file = self.path.join(".aijia_write_test");
        fs::write(&test_file, "test").context("Workspace is not writable")?;
        fs::remove_file(&test_file).ok();
        Ok(true)
    }

    /// Get full path for a subdirectory (e.g., "uploads", "reports").
    pub fn subdir(&self, name: &str) -> PathBuf {
        self.path.join(name)
    }

    /// Get workspace information: path, sizes, file counts per subdirectory.
    pub fn get_info(&self) -> Result<WorkspaceInfo> {
        let mut subdirectories = Vec::new();
        let mut total_size = 0u64;
        let mut file_count = 0u32;

        for dir_name in WORKSPACE_DIRS {
            let dir_path = self.path.join(dir_name);
            let (count, size) = if dir_path.exists() {
                Self::dir_stats(&dir_path)?
            } else {
                (0, 0)
            };
            total_size += size;
            file_count += count;
            subdirectories.push(SubdirInfo {
                name: dir_name.to_string(),
                file_count: count,
                total_size: size,
            });
        }

        Ok(WorkspaceInfo {
            path: self.path.to_string_lossy().to_string(),
            exists: self.path.exists(),
            total_size,
            file_count,
            subdirectories,
        })
    }

    /// Calculate file count and total size for a directory (non-recursive simple version).
    fn dir_stats(dir: &Path) -> Result<(u32, u64)> {
        let mut count = 0u32;
        let mut size = 0u64;
        for entry in fs::read_dir(dir)? {
            let entry = entry?;
            let meta = entry.metadata()?;
            if meta.is_file() {
                count += 1;
                size += meta.len();
            }
        }
        Ok((count, size))
    }
}
