use std::sync::Arc;
use std::path::Path;
use tauri::State;
use crate::storage::file_store::AppStorage;
use crate::storage::file_manager::FileManager;

/// Look up a file's stored_path from both uploaded_files and generated_files tables.
/// Returns the stored_path string if found.
fn resolve_stored_path(
    db: &AppStorage,
    file_id: &str,
    conversation_id: &str,
) -> Result<String, String> {
    // Try uploaded_files first
    if let Some(record) = db.get_uploaded_file_for_conversation(file_id, conversation_id)
        .map_err(|e| e.to_string())?
    {
        if let Some(path) = record.get("storedPath").and_then(|v| v.as_str()) {
            return Ok(path.to_string());
        }
    }

    // Fall back to generated_files
    if let Some(record) = db.get_generated_file_for_conversation(file_id, conversation_id)
        .map_err(|e| e.to_string())?
    {
        if let Some(path) = record.get("storedPath").and_then(|v| v.as_str()) {
            return Ok(path.to_string());
        }
    }

    Err("File not found or does not belong to this conversation".to_string())
}

/// Upload a file to the workspace.
/// Copies the file to workspace/uploads/ and records it in the database.
/// Returns a JSON object with fileId and fileSize.
#[tauri::command]
pub async fn upload_file(
    db: State<'_, Arc<AppStorage>>,
    file_mgr: State<'_, Arc<FileManager>>,
    file_path: String,
    conversation_id: String,
) -> Result<serde_json::Value, String> {
    let source = Path::new(&file_path);

    // Store in workspace
    let info = file_mgr.store_upload(source).map_err(|e| e.to_string())?;

    // Record in database with conversation ownership
    let file_id = uuid::Uuid::new_v4().to_string();
    let file_size = info.file_size;
    db.insert_uploaded_file(
        &file_id,
        &conversation_id,
        &info.file_name,
        &info.stored_path,
        &info.file_type,
        file_size as i64,
        None,
    ).map_err(|e| e.to_string())?;

    Ok(serde_json::json!({
        "fileId": file_id,
        "fileSize": file_size,
    }))
}

/// Open a generated file with system default application.
/// Searches both uploaded_files and generated_files tables.
#[tauri::command]
pub async fn open_generated_file(
    db: State<'_, Arc<AppStorage>>,
    file_mgr: State<'_, Arc<FileManager>>,
    file_id: String,
    conversation_id: String,
) -> Result<(), String> {
    let stored_path = resolve_stored_path(&db, &file_id, &conversation_id)?;
    let full_path = file_mgr.full_path(&stored_path);

    // Open with system default application
    #[cfg(target_os = "macos")]
    std::process::Command::new("open").arg(&full_path).spawn().map_err(|e| e.to_string())?;
    #[cfg(target_os = "windows")]
    std::process::Command::new("explorer").arg(&full_path).spawn().map_err(|e| e.to_string())?;
    #[cfg(target_os = "linux")]
    std::process::Command::new("xdg-open").arg(&full_path).spawn().map_err(|e| e.to_string())?;

    Ok(())
}

/// Reveal a file in the OS file manager (Finder / Explorer / file manager).
/// Searches both uploaded_files and generated_files tables.
#[tauri::command]
pub async fn reveal_file_in_folder(
    db: State<'_, Arc<AppStorage>>,
    file_mgr: State<'_, Arc<FileManager>>,
    file_id: String,
    conversation_id: String,
) -> Result<(), String> {
    let stored_path = resolve_stored_path(&db, &file_id, &conversation_id)?;
    let full_path = file_mgr.full_path(&stored_path);

    // Reveal in OS file manager
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

/// Preview a file (returns preview content as string).
#[tauri::command]
pub async fn preview_file(
    db: State<'_, Arc<AppStorage>>,
    file_mgr: State<'_, Arc<FileManager>>,
    file_id: String,
    conversation_id: String,
) -> Result<String, String> {
    let file_record = db.get_uploaded_file_for_conversation(&file_id, &conversation_id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "File not found or does not belong to this conversation".to_string())?;

    let stored_path = file_record.get("storedPath")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "Invalid file record".to_string())?;

    let full_path = file_mgr.full_path(stored_path);

    // For HTML files, return the file path for WebView preview
    // For other files, return basic info
    let file_type = file_record.get("fileType")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown");

    let preview = serde_json::json!({
        "type": file_type,
        "path": full_path.to_string_lossy(),
        "name": file_record.get("originalName").and_then(|v| v.as_str()).unwrap_or("unknown"),
    });

    Ok(preview.to_string())
}

/// Delete a file.
#[tauri::command]
pub async fn delete_file(
    db: State<'_, Arc<AppStorage>>,
    file_mgr: State<'_, Arc<FileManager>>,
    file_id: String,
    conversation_id: String,
) -> Result<(), String> {
    // Get file info, verified against conversation
    let file_record = db.get_uploaded_file_for_conversation(&file_id, &conversation_id)
        .map_err(|e| e.to_string())?;

    if let Some(record) = file_record {
        if let Some(stored_path) = record.get("storedPath").and_then(|v| v.as_str()) {
            // Delete from filesystem
            file_mgr.delete_file(stored_path).map_err(|e| e.to_string())?;
        }
    }

    // TODO: delete from database (need delete_uploaded_file method)
    Ok(())
}
