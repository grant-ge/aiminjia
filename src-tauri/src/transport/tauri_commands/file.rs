use std::path::Path;
use std::sync::Arc;

use crate::storage::file_manager::FileManager;
use crate::storage::file_store::AppStorage;

/// Maximum upload file size: 200 MB
const MAX_UPLOAD_SIZE: u64 = 200 * 1024 * 1024;

/// Look up a file's stored_path from both uploaded_files and generated_files tables.
fn resolve_stored_path(
    db: &AppStorage,
    file_id: &str,
    conversation_id: &str,
) -> Result<String, String> {
    if let Some(record) = db
        .get_uploaded_file_for_conversation(file_id, conversation_id)
        .map_err(|e| e.to_string())?
    {
        if let Some(path) = record.get("storedPath").and_then(|v| v.as_str()) {
            return Ok(path.to_string());
        }
    }

    if let Some(record) = db
        .get_generated_file_for_conversation(file_id, conversation_id)
        .map_err(|e| e.to_string())?
    {
        if let Some(path) = record.get("storedPath").and_then(|v| v.as_str()) {
            return Ok(path.to_string());
        }
    }

    Err("File not found or does not belong to this conversation".to_string())
}

/// Search workspace subdirectories for a file matching the given name.
fn find_file_in_workspace(
    file_mgr: &FileManager,
    file_name: &str,
) -> Result<std::path::PathBuf, String> {
    let ws = file_mgr.workspace_path();
    let subdirs = [
        "reports", "analysis", "uploads", "scripts", "temp", "charts", "exports",
    ];

    for subdir in &subdirs {
        let candidate = ws.join(subdir).join(file_name);
        if candidate.exists() {
            return Ok(candidate);
        }
    }

    let root_candidate = ws.join(file_name);
    if root_candidate.exists() {
        return Ok(root_candidate);
    }

    for subdir in &subdirs {
        let dir = ws.join(subdir);
        if !dir.exists() {
            continue;
        }
        if let Ok(entries) = std::fs::read_dir(&dir) {
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().to_string();
                if name.contains(file_name) && entry.path().is_file() {
                    return Ok(entry.path());
                }
            }
        }
    }

    Err(format!("File '{}' not found in workspace", file_name))
}

#[derive(Clone)]
pub struct TauriFileCommandAdapter {
    db: Arc<AppStorage>,
    file_mgr: Arc<FileManager>,
}

impl TauriFileCommandAdapter {
    pub fn new(db: Arc<AppStorage>, file_mgr: Arc<FileManager>) -> Self {
        Self { db, file_mgr }
    }

    pub fn upload_file(
        &self,
        file_path: String,
        conversation_id: String,
    ) -> Result<serde_json::Value, String> {
        let source = Path::new(&file_path);

        let source_size = std::fs::metadata(source)
            .map(|m| m.len())
            .map_err(|e| format!("Cannot read file: {}", e))?;
        if source_size > MAX_UPLOAD_SIZE {
            return Err(format!(
                "File too large ({:.1} MB). Maximum allowed: {} MB.",
                source_size as f64 / (1024.0 * 1024.0),
                MAX_UPLOAD_SIZE / (1024 * 1024),
            ));
        }

        let info = self
            .file_mgr
            .store_upload(source)
            .map_err(|e| e.to_string())?;

        let file_id = uuid::Uuid::new_v4().to_string();
        let file_size = info.file_size;
        if let Err(e) = self.db.insert_uploaded_file(
            &file_id,
            &conversation_id,
            &info.file_name,
            &info.stored_path,
            &info.file_type,
            file_size as i64,
            None,
        ) {
            log::error!(
                "DB insert failed for uploaded file, rolling back physical file: {}",
                e
            );
            let _ = self.file_mgr.delete_file(&info.stored_path);
            return Err(e.to_string());
        }

        Ok(serde_json::json!({
            "fileId": file_id,
            "fileSize": file_size,
        }))
    }

    pub fn open_generated_file(
        &self,
        file_id: String,
        conversation_id: String,
    ) -> Result<(), String> {
        let stored_path = resolve_stored_path(&self.db, &file_id, &conversation_id)?;
        let full_path = self.file_mgr.full_path(&stored_path);

        #[cfg(target_os = "macos")]
        std::process::Command::new("open")
            .arg(&full_path)
            .spawn()
            .map_err(|e| e.to_string())?;
        #[cfg(target_os = "windows")]
        std::process::Command::new("explorer")
            .arg(&full_path)
            .spawn()
            .map_err(|e| e.to_string())?;
        #[cfg(target_os = "linux")]
        std::process::Command::new("xdg-open")
            .arg(&full_path)
            .spawn()
            .map_err(|e| e.to_string())?;

        Ok(())
    }

    pub fn reveal_file_in_folder(
        &self,
        file_id: String,
        conversation_id: String,
    ) -> Result<(), String> {
        let stored_path = resolve_stored_path(&self.db, &file_id, &conversation_id)?;
        let full_path = self.file_mgr.full_path(&stored_path);

        #[cfg(target_os = "macos")]
        std::process::Command::new("open")
            .arg("-R")
            .arg(&full_path)
            .spawn()
            .map_err(|e| e.to_string())?;

        #[cfg(target_os = "windows")]
        std::process::Command::new("explorer")
            .arg(format!("/select,{}", full_path.display()))
            .spawn()
            .map_err(|e| e.to_string())?;

        #[cfg(target_os = "linux")]
        {
            let parent = full_path.parent().unwrap_or(&full_path);
            std::process::Command::new("xdg-open")
                .arg(parent)
                .spawn()
                .map_err(|e| e.to_string())?;
        }

        Ok(())
    }

    pub fn preview_file(
        &self,
        file_id: String,
        conversation_id: String,
    ) -> Result<String, String> {
        let file_record = self
            .db
            .get_uploaded_file_for_conversation(&file_id, &conversation_id)
            .map_err(|e| e.to_string())?
            .ok_or_else(|| "File not found or does not belong to this conversation".to_string())?;

        let stored_path = file_record
            .get("storedPath")
            .and_then(|v| v.as_str())
            .ok_or_else(|| "Invalid file record".to_string())?;

        let full_path = self.file_mgr.full_path(stored_path);

        let file_type = file_record
            .get("fileType")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown");

        let preview = serde_json::json!({
            "type": file_type,
            "path": full_path.to_string_lossy(),
            "name": file_record.get("originalName").and_then(|v| v.as_str()).unwrap_or("unknown"),
        });

        Ok(preview.to_string())
    }

    pub fn open_file_by_name(&self, file_name: String) -> Result<(), String> {
        let full_path = find_file_in_workspace(&self.file_mgr, &file_name)?;

        #[cfg(target_os = "macos")]
        std::process::Command::new("open")
            .arg(&full_path)
            .spawn()
            .map_err(|e| e.to_string())?;
        #[cfg(target_os = "windows")]
        std::process::Command::new("explorer")
            .arg(&full_path)
            .spawn()
            .map_err(|e| e.to_string())?;
        #[cfg(target_os = "linux")]
        std::process::Command::new("xdg-open")
            .arg(&full_path)
            .spawn()
            .map_err(|e| e.to_string())?;

        Ok(())
    }

    pub fn reveal_file_by_name(&self, file_name: String) -> Result<(), String> {
        let full_path = find_file_in_workspace(&self.file_mgr, &file_name)?;

        #[cfg(target_os = "macos")]
        std::process::Command::new("open")
            .arg("-R")
            .arg(&full_path)
            .spawn()
            .map_err(|e| e.to_string())?;

        #[cfg(target_os = "windows")]
        std::process::Command::new("explorer")
            .arg(format!("/select,{}", full_path.display()))
            .spawn()
            .map_err(|e| e.to_string())?;

        #[cfg(target_os = "linux")]
        {
            let parent = full_path.parent().unwrap_or(&full_path);
            std::process::Command::new("xdg-open")
                .arg(parent)
                .spawn()
                .map_err(|e| e.to_string())?;
        }

        Ok(())
    }

    pub fn delete_file(&self, file_id: String, conversation_id: String) -> Result<(), String> {
        let file_record = self
            .db
            .get_uploaded_file_for_conversation(&file_id, &conversation_id)
            .map_err(|e| e.to_string())?;

        if let Some(record) = file_record {
            if let Some(stored_path) = record.get("storedPath").and_then(|v| v.as_str()) {
                self.file_mgr
                    .delete_file(stored_path)
                    .map_err(|e| e.to_string())?;
            }
        }

        self.db
            .delete_uploaded_file(&file_id, &conversation_id)
            .map_err(|e| e.to_string())?;
        Ok(())
    }
}
