use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::Result;
use log::warn;

use crate::storage::file_manager::FileManager;
use crate::storage::file_store::AppStorage;

fn collect_upload_files(dir: &Path, root: &Path, files: &mut Vec<PathBuf>) -> Result<()> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            collect_upload_files(&path, root, files)?;
            continue;
        }
        if path.is_file() {
            let relative = path
                .strip_prefix(root)
                .map(|value| value.to_path_buf())
                .unwrap_or(path.clone());
            files.push(relative);
        }
    }
    Ok(())
}

fn normalize_upload_relative_path(stored_path: &str) -> Option<PathBuf> {
    let path = Path::new(stored_path);
    path.strip_prefix("uploads")
        .ok()
        .map(|value| value.to_path_buf())
}

pub fn gc_orphan_upload_files(db: &AppStorage, file_mgr: &FileManager) -> Result<usize> {
    let uploads_dir = file_mgr.workspace_path().join("uploads");
    if !uploads_dir.exists() {
        return Ok(0);
    }

    let conversations = db.get_conversations()?;
    let mut referenced_uploads = HashSet::new();

    for conversation in conversations {
        let Some(conversation_id) = conversation.get("id").and_then(|value| value.as_str()) else {
            continue;
        };

        let uploaded_files = match db.get_uploaded_files_for_conversation(conversation_id) {
            Ok(files) => files,
            Err(err) => {
                warn!(
                    "[upload_gc] failed to read file index for conversation {}: {}; skipping deletion round",
                    conversation_id, err
                );
                return Ok(0);
            }
        };

        for file in uploaded_files {
            if let Some(stored_path) = file.get("storedPath").and_then(|value| value.as_str()) {
                if let Some(relative) = normalize_upload_relative_path(stored_path) {
                    referenced_uploads.insert(relative);
                }
            }
        }
    }

    let mut physical_uploads = Vec::new();
    collect_upload_files(&uploads_dir, &uploads_dir, &mut physical_uploads)?;

    let mut deleted = 0usize;
    for relative_path in physical_uploads {
        if referenced_uploads.contains(&relative_path) {
            continue;
        }

        let full_path = uploads_dir.join(&relative_path);
        match fs::remove_file(&full_path) {
            Ok(_) => {
                deleted += 1;
                warn!(
                    "[upload_gc] deleted orphan upload file {}",
                    full_path.display()
                );
            }
            Err(err) => {
                warn!(
                    "[upload_gc] failed to delete orphan upload file {}: {}",
                    full_path.display(),
                    err
                );
            }
        }
    }

    Ok(deleted)
}
