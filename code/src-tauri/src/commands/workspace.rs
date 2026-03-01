use std::sync::Arc;
use tauri::State;
use crate::storage::file_store::AppStorage;
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
