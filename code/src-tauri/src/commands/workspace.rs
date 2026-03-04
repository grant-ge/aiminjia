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

/// Export all metrics entries to a JSON file.
#[tauri::command]
pub async fn export_metrics(
    file_mgr: State<'_, Arc<FileManager>>,
    dest_path: String,
) -> Result<serde_json::Value, String> {
    let workspace = file_mgr.workspace_path();
    let (json_content, entry_count) = crate::telemetry::export_all(&workspace)?;

    std::fs::write(&dest_path, json_content.as_bytes()).map_err(|e| e.to_string())?;
    let file_size = std::fs::metadata(&dest_path)
        .map(|m| m.len())
        .unwrap_or(0);

    Ok(serde_json::json!({
        "path": dest_path,
        "entryCount": entry_count,
        "fileSize": file_size,
    }))
}

/// Clear all metrics JSONL files.
#[tauri::command]
pub async fn clear_metrics(
    file_mgr: State<'_, Arc<FileManager>>,
) -> Result<serde_json::Value, String> {
    let workspace = file_mgr.workspace_path();
    let deleted = crate::telemetry::clear_all(&workspace)?;

    Ok(serde_json::json!({
        "deletedFiles": deleted,
    }))
}

/// Get metrics file info (entry count + total bytes).
#[tauri::command]
pub async fn get_metrics_info(
    file_mgr: State<'_, Arc<FileManager>>,
) -> Result<serde_json::Value, String> {
    let workspace = file_mgr.workspace_path();
    let (entry_count, total_bytes) = crate::telemetry::get_info(&workspace)?;

    Ok(serde_json::json!({
        "entryCount": entry_count,
        "totalBytes": total_bytes,
    }))
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
