use std::sync::Arc;
use tauri::State;
use crate::storage::file_store::AppStorage;
use crate::storage::file_manager::FileManager;
use crate::storage::workspace::WorkspaceManager;

/// Select workspace directory.
/// Validates the path, ensures directory structure, and saves to settings.
#[tauri::command]
pub async fn select_workspace(
    db: State<'_, Arc<AppStorage>>,
    path: String,
) -> Result<(), String> {
    let manager = WorkspaceManager::new(&path);

    // Ensure the directory structure exists
    manager.ensure_structure().map_err(|e| e.to_string())?;

    // Save to settings
    db.set_setting("workspacePath", &path).map_err(|e| e.to_string())?;

    Ok(())
}

/// Get workspace information (sizes, directory structure).
#[tauri::command]
pub async fn get_workspace_info(
    db: State<'_, Arc<AppStorage>>,
) -> Result<String, String> {
    let path = db.get_setting("workspacePath")
        .map_err(|e| e.to_string())?
        .unwrap_or_default();

    if path.is_empty() {
        return Ok(serde_json::json!({
            "path": "",
            "exists": false,
            "totalSize": 0,
            "fileCount": 0,
            "subdirectories": []
        }).to_string());
    }

    let manager = WorkspaceManager::new(&path);
    let info = manager.get_info().map_err(|e| e.to_string())?;
    serde_json::to_string(&info).map_err(|e| e.to_string())
}

/// Open the logs directory in the system file manager.
#[tauri::command]
pub async fn open_logs_directory(
    file_mgr: State<'_, Arc<FileManager>>,
) -> Result<(), String> {
    let logs_dir = file_mgr.workspace_path().join("logs");
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

/// Open the workspace root directory in the system file manager.
#[tauri::command]
pub async fn open_workspace_directory(
    file_mgr: State<'_, Arc<FileManager>>,
) -> Result<(), String> {
    let ws_dir = file_mgr.workspace_path();
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
