use std::sync::Arc;

use crate::storage::file_manager::FileManager;
use crate::storage::file_store::AppStorage;
use crate::storage::workspace::WorkspaceManager;

#[derive(Clone)]
pub struct TauriWorkspaceCommandAdapter {
    db: Arc<AppStorage>,
    file_mgr: Arc<FileManager>,
}

impl TauriWorkspaceCommandAdapter {
    pub fn new(db: Arc<AppStorage>, file_mgr: Arc<FileManager>) -> Self {
        Self { db, file_mgr }
    }

    pub fn select_workspace(&self, path: String) -> Result<(), String> {
        let manager = WorkspaceManager::new(&path);
        manager.ensure_structure().map_err(|e| e.to_string())?;
        self.db
            .set_setting("workspacePath", &path)
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    pub fn get_workspace_info(&self) -> Result<String, String> {
        let path = self
            .db
            .get_setting("workspacePath")
            .map_err(|e| e.to_string())?
            .unwrap_or_default();

        if path.is_empty() {
            return Ok(serde_json::json!({
                "path": "",
                "exists": false,
                "totalSize": 0,
                "fileCount": 0,
                "subdirectories": []
            })
            .to_string());
        }

        let manager = WorkspaceManager::new(&path);
        let info = manager.get_info().map_err(|e| e.to_string())?;
        serde_json::to_string(&info).map_err(|e| e.to_string())
    }

    pub fn open_logs_directory(&self) -> Result<(), String> {
        let logs_dir = self.file_mgr.workspace_path().join("logs");
        std::fs::create_dir_all(&logs_dir).map_err(|e| e.to_string())?;

        #[cfg(target_os = "macos")]
        {
            std::process::Command::new("open")
                .arg(&logs_dir)
                .spawn()
                .map_err(|e| e.to_string())?;
        }
        #[cfg(target_os = "windows")]
        {
            std::process::Command::new("explorer")
                .arg(&logs_dir)
                .spawn()
                .map_err(|e| e.to_string())?;
        }
        #[cfg(target_os = "linux")]
        {
            std::process::Command::new("xdg-open")
                .arg(&logs_dir)
                .spawn()
                .map_err(|e| e.to_string())?;
        }

        Ok(())
    }

    pub fn export_metrics(&self, dest_path: String) -> Result<serde_json::Value, String> {
        let workspace = self.file_mgr.workspace_path();
        let (json_content, entry_count) = crate::telemetry::export_all(&workspace)?;

        std::fs::write(&dest_path, json_content.as_bytes()).map_err(|e| e.to_string())?;
        let file_size = std::fs::metadata(&dest_path).map(|m| m.len()).unwrap_or(0);

        Ok(serde_json::json!({
            "path": dest_path,
            "entryCount": entry_count,
            "fileSize": file_size,
        }))
    }

    pub fn clear_metrics(&self) -> Result<serde_json::Value, String> {
        let workspace = self.file_mgr.workspace_path();
        let deleted = crate::telemetry::clear_all(&workspace)?;

        Ok(serde_json::json!({
            "deletedFiles": deleted,
        }))
    }

    pub fn get_metrics_info(&self) -> Result<serde_json::Value, String> {
        let workspace = self.file_mgr.workspace_path();
        let (entry_count, total_bytes) = crate::telemetry::get_info(&workspace)?;

        Ok(serde_json::json!({
            "entryCount": entry_count,
            "totalBytes": total_bytes,
        }))
    }

    pub fn open_workspace_directory(&self) -> Result<(), String> {
        let ws_dir = self.file_mgr.workspace_path();
        if !ws_dir.exists() {
            std::fs::create_dir_all(&ws_dir).map_err(|e| e.to_string())?;
        }

        #[cfg(target_os = "macos")]
        {
            std::process::Command::new("open")
                .arg(&ws_dir)
                .spawn()
                .map_err(|e| e.to_string())?;
        }
        #[cfg(target_os = "windows")]
        {
            std::process::Command::new("explorer")
                .arg(&ws_dir)
                .spawn()
                .map_err(|e| e.to_string())?;
        }
        #[cfg(target_os = "linux")]
        {
            std::process::Command::new("xdg-open")
                .arg(&ws_dir)
                .spawn()
                .map_err(|e| e.to_string())?;
        }

        Ok(())
    }
}
