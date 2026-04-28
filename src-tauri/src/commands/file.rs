use crate::storage::file_manager::FileManager;
use crate::storage::file_store::RuntimeRepositoryFacade;
use crate::storage::AiJiaHome;
use std::collections::HashSet;
use std::path::Path;
use std::sync::Arc;
use tauri::State;

/// Look up a file's stored_path from both uploaded_files and generated_files tables.
/// Returns the stored_path string if found.
fn resolve_stored_path(
    facade: &RuntimeRepositoryFacade,
    file_id: &str,
    conversation_id: &str,
) -> Result<String, String> {
    let store = facade.file_record_store();

    // Try uploaded_files first
    if let Some(record) = store
        .get_uploaded_file_for_conversation(file_id, conversation_id)
        .map_err(|e| e.to_string())?
    {
        if let Some(path) = record.get("storedPath").and_then(|v| v.as_str()) {
            return Ok(path.to_string());
        }
    }

    // Fall back to generated_files
    if let Some(record) = store
        .get_generated_file_for_conversation(file_id, conversation_id)
        .map_err(|e| e.to_string())?
    {
        if let Some(path) = record.get("storedPath").and_then(|v| v.as_str()) {
            return Ok(path.to_string());
        }
    }

    Err("File not found or does not belong to this conversation".to_string())
}

/// Maximum upload file size: 200 MB
const MAX_UPLOAD_SIZE: u64 = 200 * 1024 * 1024;

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SavedClipboardAttachment {
    pub file_name: String,
    pub path: String,
    pub file_size: u64,
    pub mime_type: String,
}

/// Upload a file to the workspace.
/// Copies the file to workspace/uploads/ and records it in the database.
/// Returns a JSON object with fileId and fileSize.
#[tauri::command]
pub async fn upload_file(
    facade: State<'_, Arc<RuntimeRepositoryFacade>>,
    file_mgr: State<'_, Arc<FileManager>>,
    file_path: String,
    conversation_id: String,
) -> Result<serde_json::Value, String> {
    let source = Path::new(&file_path);

    // Check file size before copying
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

    // Store in workspace
    let info = file_mgr.store_upload(source).map_err(|e| e.to_string())?;

    // Record in database with conversation ownership
    let file_id = uuid::Uuid::new_v4().to_string();
    let file_size = info.file_size;
    if let Err(e) = facade.file_record_store().insert_uploaded_file(
        &file_id,
        &conversation_id,
        &info.file_name,
        &info.stored_path,
        &info.file_type,
        file_size as i64,
        None,
    ) {
        // Rollback: delete the physical file if DB insert fails
        log::error!(
            "DB insert failed for uploaded file, rolling back physical file: {}",
            e
        );
        let _ = file_mgr.delete_file(&info.stored_path);
        return Err(e.to_string());
    }

    Ok(serde_json::json!({
        "fileId": file_id,
        "fileSize": file_size,
    }))
}

fn clipboard_extension_for_mime(mime_type: &str) -> &'static str {
    match mime_type {
        "image/jpeg" => "jpg",
        "image/webp" => "webp",
        "image/gif" => "gif",
        "image/bmp" => "bmp",
        "image/svg+xml" => "svg",
        _ => "png",
    }
}

pub(crate) fn save_clipboard_image_attachment_to_home(
    aijia_home: &AiJiaHome,
    conversation_id: &str,
    bytes: &[u8],
    mime_type: &str,
) -> Result<SavedClipboardAttachment, String> {
    let ext = clipboard_extension_for_mime(mime_type);
    let file_name = format!(
        "clipboard-{}-{}.{}",
        chrono::Utc::now().timestamp_millis(),
        &uuid::Uuid::new_v4().simple().to_string()[..8],
        ext
    );
    let dir = aijia_home
        .root()
        .join("conversations")
        .join(conversation_id)
        .join("attachments")
        .join("clipboard");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let full_path = dir.join(&file_name);
    std::fs::write(&full_path, bytes).map_err(|e| e.to_string())?;
    Ok(SavedClipboardAttachment {
        file_name,
        path: full_path.to_string_lossy().to_string(),
        file_size: bytes.len() as u64,
        mime_type: mime_type.to_string(),
    })
}

#[tauri::command]
pub async fn save_clipboard_image_attachment(
    aijia_home: State<'_, Arc<AiJiaHome>>,
    conversation_id: String,
    bytes: Vec<u8>,
    mime_type: String,
) -> Result<SavedClipboardAttachment, String> {
    save_clipboard_image_attachment_to_home(
        aijia_home.inner().as_ref(),
        &conversation_id,
        &bytes,
        &mime_type,
    )
}

#[cfg(target_os = "macos")]
fn read_clipboard_file_paths_platform() -> Result<Vec<String>, String> {
    use objc2_app_kit::{NSPasteboard, NSPasteboardItem, NSPasteboardTypeFileURL};
    use objc2_foundation::NSURL;

    let pasteboard = NSPasteboard::generalPasteboard();
    let mut paths = Vec::new();
    let mut seen = HashSet::new();

    if let Some(items) = pasteboard.pasteboardItems() {
        for item in items.iter() {
            let item: &NSPasteboardItem = &item;
            if let Some(url_text) = item.stringForType(unsafe { NSPasteboardTypeFileURL }) {
                if let Some(url) = NSURL::URLWithString(&url_text) {
                    if url.isFileURL() {
                        let normalized = url.filePathURL().unwrap_or(url);
                        if let Some(path) = normalized.to_file_path() {
                            let path_string = path.to_string_lossy().to_string();
                            if path.is_absolute() && seen.insert(path_string.clone()) {
                                paths.push(path_string);
                            }
                        }
                    } else if let Some(path) = url.to_file_path() {
                        let path_string = path.to_string_lossy().to_string();
                        if path.is_absolute() && seen.insert(path_string.clone()) {
                            paths.push(path_string);
                        }
                    }
                }
            }
        }
    }

    Ok(paths)
}

#[cfg(not(target_os = "macos"))]
fn read_clipboard_file_paths_platform() -> Result<Vec<String>, String> {
    Ok(Vec::new())
}

#[tauri::command]
pub async fn read_clipboard_file_paths() -> Result<Vec<String>, String> {
    read_clipboard_file_paths_platform()
}

/// Open a generated file with system default application.
/// Searches both uploaded_files and generated_files tables.
#[tauri::command]
pub async fn open_generated_file(
    facade: State<'_, Arc<RuntimeRepositoryFacade>>,
    file_mgr: State<'_, Arc<FileManager>>,
    file_id: String,
    conversation_id: String,
) -> Result<(), String> {
    let stored_path = resolve_stored_path(&facade, &file_id, &conversation_id)?;
    let full_path = file_mgr.full_path(&stored_path);

    // Open with system default application
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

/// Reveal a file in the OS file manager (Finder / Explorer / file manager).
/// Searches both uploaded_files and generated_files tables.
#[tauri::command]
pub async fn reveal_file_in_folder(
    facade: State<'_, Arc<RuntimeRepositoryFacade>>,
    file_mgr: State<'_, Arc<FileManager>>,
    file_id: String,
    conversation_id: String,
) -> Result<(), String> {
    let stored_path = resolve_stored_path(&facade, &file_id, &conversation_id)?;
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
    facade: State<'_, Arc<RuntimeRepositoryFacade>>,
    file_mgr: State<'_, Arc<FileManager>>,
    file_id: String,
    conversation_id: String,
) -> Result<String, String> {
    let file_record = facade
        .file_record_store()
        .get_uploaded_file_for_conversation(&file_id, &conversation_id)
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "File not found or does not belong to this conversation".to_string())?;

    let stored_path = file_record
        .get("storedPath")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "Invalid file record".to_string())?;

    let full_path = file_mgr.full_path(stored_path);

    // For HTML files, return the file path for WebView preview
    // For other files, return basic info
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

/// Search workspace subdirectories for a file by name and open it with the default app.
#[tauri::command]
pub async fn open_file_by_name(
    file_mgr: State<'_, Arc<FileManager>>,
    file_name: String,
) -> Result<(), String> {
    let full_path = find_file_in_workspace(&file_mgr, &file_name)?;

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

/// Search workspace subdirectories for a file by name and reveal it in the OS file manager.
#[tauri::command]
pub async fn reveal_file_by_name(
    file_mgr: State<'_, Arc<FileManager>>,
    file_name: String,
) -> Result<(), String> {
    let full_path = find_file_in_workspace(&file_mgr, &file_name)?;

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

/// Search workspace subdirectories for a file matching the given name.
/// Checks: reports/, analysis/, uploads/, scripts/, temp/ and workspace root.
fn find_file_in_workspace(
    file_mgr: &FileManager,
    file_name: &str,
) -> Result<std::path::PathBuf, String> {
    let ws = file_mgr.workspace_path();
    let subdirs = [
        "reports", "analysis", "uploads", "scripts", "temp", "charts", "exports",
    ];

    // First: exact match in subdirectories
    for subdir in &subdirs {
        let candidate = ws.join(subdir).join(file_name);
        if candidate.exists() {
            return Ok(candidate);
        }
    }

    // Second: exact match in workspace root
    let root_candidate = ws.join(file_name);
    if root_candidate.exists() {
        return Ok(root_candidate);
    }

    // Third: substring match — file name on disk contains the search term
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

/// Delete a file.
#[tauri::command]
pub async fn delete_file(
    facade: State<'_, Arc<RuntimeRepositoryFacade>>,
    file_mgr: State<'_, Arc<FileManager>>,
    file_id: String,
    conversation_id: String,
) -> Result<(), String> {
    let store = facade.file_record_store();
    // Get file info, verified against conversation
    let file_record = store
        .get_uploaded_file_for_conversation(&file_id, &conversation_id)
        .map_err(|e| e.to_string())?;

    if let Some(record) = file_record {
        if let Some(stored_path) = record.get("storedPath").and_then(|v| v.as_str()) {
            // Delete from filesystem
            file_mgr
                .delete_file(stored_path)
                .map_err(|e| e.to_string())?;
        }
    }

    // Delete from database
    store
        .delete_uploaded_file(&file_id, &conversation_id)
        .map_err(|e| e.to_string())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn save_clipboard_image_attachment_writes_to_conversation_clipboard_dir() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let home = AiJiaHome::from_path(tmp.path().to_path_buf());

        let saved = save_clipboard_image_attachment_to_home(
            &home,
            "conv-1",
            &[1, 2, 3, 4],
            "image/png",
        )
        .expect("save clipboard image");

        assert!(saved.path.contains("/conversations/conv-1/attachments/clipboard/"));
        assert!(saved.file_name.ends_with(".png"));
        assert_eq!(saved.file_size, 4);
        assert_eq!(saved.mime_type, "image/png");
        assert!(std::path::Path::new(&saved.path).exists());
    }
}
