use crate::storage::file_manager::FileManager;
use crate::storage::file_store::RuntimeRepositoryFacade;
use crate::storage::workspace::WorkspaceManager;
use std::sync::Arc;
use tauri::State;

/// Select workspace directory.
/// Validates the path, ensures directory structure, and saves to settings.
#[tauri::command]
pub async fn select_workspace(
    facade: State<'_, Arc<RuntimeRepositoryFacade>>,
    path: String,
) -> Result<(), String> {
    let manager = WorkspaceManager::new(&path);

    // Ensure the directory structure exists
    manager.ensure_structure().map_err(|e| e.to_string())?;

    // Save to settings
    facade
        .settings_store()
        .set("workspacePath", &path)
        .map_err(|e| e.to_string())?;

    Ok(())
}

/// Get workspace information (sizes, directory structure).
#[tauri::command]
pub async fn get_workspace_info(
    facade: State<'_, Arc<RuntimeRepositoryFacade>>,
) -> Result<String, String> {
    let path = facade
        .settings_store()
        .get("workspacePath")
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

/// Open the logs directory in the system file manager.
#[tauri::command]
pub async fn open_logs_directory(file_mgr: State<'_, Arc<FileManager>>) -> Result<(), String> {
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
    let file_size = std::fs::metadata(&dest_path).map(|m| m.len()).unwrap_or(0);

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

/// Authorize a local directory for tool access within a session.
///
/// Validates that the path is an existing directory, then persists an
/// `AuthorizedWorkspace` record (single-value per session, overwrites any
/// previous authorization).
#[tauri::command]
pub async fn authorize_local_directory(
    facade: tauri::State<'_, std::sync::Arc<RuntimeRepositoryFacade>>,
    path: String,
    session_id: String,
) -> Result<serde_json::Value, String> {
    // canonicalize：解析符号链接、消除 ..，得到真实绝对路径
    let root = std::path::PathBuf::from(&path)
        .canonicalize()
        .map_err(|e| format!("Cannot resolve path '{}': {}", path, e))?;
    if !root.is_dir() {
        return Err(format!("Path is not a directory: {}", root.display()));
    }
    // display_name：优先用最后一个路径组件，fallback 到完整路径字符串
    let display_name = root
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| root.to_string_lossy().to_string());
    let ws = crate::runtime::store::AuthorizedWorkspace {
        id: uuid::Uuid::new_v4().to_string(),
        session_id: crate::runtime::ids::SessionId::new(session_id),
        root_path: root.clone(),
        display_name: display_name.clone(),
        authorized_at: chrono::Utc::now().to_rfc3339(),
    };
    facade
        .authorized_workspace_store()
        .replace_for_session(&ws)
        .map_err(|e| e.to_string())?;
    Ok(serde_json::json!({
        "id": ws.id,
        "rootPath": root.to_string_lossy(),
        "displayName": display_name,
    }))
}

/// Get the currently authorized workspace for a session, if any.
#[tauri::command]
pub async fn get_authorized_workspace(
    facade: tauri::State<'_, std::sync::Arc<RuntimeRepositoryFacade>>,
    session_id: String,
) -> Result<Option<serde_json::Value>, String> {
    let sid = crate::runtime::ids::SessionId::new(session_id);
    match facade
        .authorized_workspace_store()
        .get_current_for_session(&sid)
        .map_err(|e| e.to_string())?
    {
        Some(ws) => Ok(Some(serde_json::json!({
            "id": ws.id,
            "rootPath": ws.root_path.to_string_lossy(),
            "displayName": ws.display_name,
        }))),
        None => Ok(None),
    }
}

/// Revoke (clear) the authorized workspace for a session.
#[tauri::command]
pub async fn revoke_authorized_workspace(
    facade: tauri::State<'_, std::sync::Arc<RuntimeRepositoryFacade>>,
    session_id: String,
) -> Result<(), String> {
    let sid = crate::runtime::ids::SessionId::new(session_id);
    facade
        .authorized_workspace_store()
        .clear_for_session(&sid)
        .map_err(|e| e.to_string())
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
