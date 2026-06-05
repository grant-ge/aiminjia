use crate::storage::file_manager::FileManager;
use crate::storage::file_store::RuntimeRepositoryFacade;
use crate::storage::AiJiaHome;
use base64::{engine::general_purpose::STANDARD, Engine as _};
use std::collections::HashSet;
use std::io::Read;
use std::path::Path;
use std::sync::Arc;
use tauri::State;

const MAX_PREVIEW_BYTES: u64 = 5 * 1024 * 1024;

#[derive(Debug, Clone, serde::Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum FilePreview {
    Markdown {
        #[serde(rename = "fileName")]
        file_name: String,
        content: String,
        #[serde(rename = "mimeType")]
        mime_type: String,
    },
    Text {
        #[serde(rename = "fileName")]
        file_name: String,
        content: String,
        #[serde(rename = "mimeType")]
        mime_type: String,
    },
    Json {
        #[serde(rename = "fileName")]
        file_name: String,
        content: String,
        #[serde(rename = "mimeType")]
        mime_type: String,
    },
    Csv {
        #[serde(rename = "fileName")]
        file_name: String,
        content: String,
        #[serde(rename = "mimeType")]
        mime_type: String,
    },
    Html {
        #[serde(rename = "fileName")]
        file_name: String,
        content: String,
        #[serde(rename = "mimeType")]
        mime_type: String,
        sandbox: bool,
    },
    Image {
        #[serde(rename = "fileName")]
        file_name: String,
        #[serde(rename = "mimeType")]
        mime_type: String,
        #[serde(rename = "dataUrl")]
        data_url: String,
    },
    Unsupported {
        #[serde(rename = "fileName")]
        file_name: String,
        reason: String,
    },
}

struct ResolvedFileRecord {
    file_name: String,
    stored_path: String,
    file_type: String,
    file_size: u64,
}

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

fn resolve_file_record(
    facade: &RuntimeRepositoryFacade,
    file_id: &str,
    conversation_id: &str,
) -> Result<ResolvedFileRecord, String> {
    let store = facade.file_record_store();

    if let Some(record) = store
        .get_uploaded_file_for_conversation(file_id, conversation_id)
        .map_err(|e| e.to_string())?
    {
        return record_to_resolved_file(record, true);
    }

    if let Some(record) = store
        .get_generated_file_for_conversation(file_id, conversation_id)
        .map_err(|e| e.to_string())?
    {
        return record_to_resolved_file(record, false);
    }

    Err("File not found or does not belong to this conversation".to_string())
}

fn record_to_resolved_file(
    record: serde_json::Value,
    is_uploaded: bool,
) -> Result<ResolvedFileRecord, String> {
    let stored_path = record
        .get("storedPath")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "Invalid file record: missing storedPath".to_string())?
        .to_string();
    let file_type = record
        .get("fileType")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown")
        .to_string();
    let file_size = record.get("fileSize").and_then(|v| v.as_u64()).unwrap_or(0);
    let file_name = if is_uploaded {
        record
            .get("originalName")
            .and_then(|v| v.as_str())
            .or_else(|| record.get("fileName").and_then(|v| v.as_str()))
    } else {
        record.get("fileName").and_then(|v| v.as_str())
    }
    .or_else(|| Path::new(&stored_path).file_name().and_then(|v| v.to_str()))
    .unwrap_or("unknown")
    .to_string();

    Ok(ResolvedFileRecord {
        file_name,
        stored_path,
        file_type,
        file_size,
    })
}

fn preview_mime_type(kind: &str) -> &'static str {
    match kind {
        "markdown" => "text/markdown",
        "html" => "text/html",
        "json" => "application/json",
        "csv" => "text/csv",
        "text" => "text/plain",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "webp" => "image/webp",
        "gif" => "image/gif",
        "bmp" => "image/bmp",
        "svg" => "image/svg+xml",
        _ => "application/octet-stream",
    }
}

fn normalize_preview_kind(file_name: &str, file_type: &str) -> Option<&'static str> {
    let lower_type = file_type.to_ascii_lowercase();
    match lower_type.as_str() {
        "markdown" | "md" => return Some("markdown"),
        "html" => return Some("html"),
        "text" | "txt" => return Some("text"),
        "json" => return Some("json"),
        "csv" => return Some("csv"),
        "png" => return Some("png"),
        "jpg" | "jpeg" => return Some("jpeg"),
        "webp" => return Some("webp"),
        "gif" => return Some("gif"),
        "bmp" => return Some("bmp"),
        "svg" => return Some("svg"),
        _ => {}
    }

    let ext = Path::new(file_name)
        .extension()
        .and_then(|v| v.to_str())
        .map(|v| v.to_ascii_lowercase());
    match ext.as_deref() {
        Some("md" | "markdown") => Some("markdown"),
        Some("html") => Some("html"),
        Some("txt") => Some("text"),
        Some("json") => Some("json"),
        Some("csv") => Some("csv"),
        Some("png") => Some("png"),
        Some("jpg" | "jpeg") => Some("jpeg"),
        Some("webp") => Some("webp"),
        Some("gif") => Some("gif"),
        Some("bmp") => Some("bmp"),
        Some("svg") => Some("svg"),
        _ => None,
    }
}

fn unsupported_preview(file_name: &str, reason: impl Into<String>) -> FilePreview {
    FilePreview::Unsupported {
        file_name: file_name.to_string(),
        reason: reason.into(),
    }
}

fn preview_from_bytes(file_name: &str, file_type: &str, bytes: Vec<u8>) -> FilePreview {
    if bytes.len() as u64 > MAX_PREVIEW_BYTES {
        return unsupported_preview(file_name, "File is too large to preview");
    }

    let Some(kind) = normalize_preview_kind(file_name, file_type) else {
        return unsupported_preview(
            file_name,
            format!("File type '{}' is not supported", file_type),
        );
    };

    if matches!(kind, "png" | "jpeg" | "webp" | "gif" | "bmp" | "svg") {
        let file_name = file_name.to_string();
        let mime_type = preview_mime_type(kind).to_string();
        let data_url = format!("data:{};base64,{}", mime_type, STANDARD.encode(bytes));
        return FilePreview::Image {
            file_name,
            mime_type,
            data_url,
        };
    }

    let content = match String::from_utf8(bytes) {
        Ok(content) => content,
        Err(_) => return unsupported_preview(file_name, "File is not valid UTF-8"),
    };
    let file_name = file_name.to_string();
    let mime_type = preview_mime_type(kind).to_string();

    match kind {
        "markdown" => FilePreview::Markdown {
            file_name,
            content,
            mime_type,
        },
        "html" => FilePreview::Html {
            file_name,
            content,
            mime_type,
            sandbox: true,
        },
        "json" => FilePreview::Json {
            file_name,
            content,
            mime_type,
        },
        "csv" => FilePreview::Csv {
            file_name,
            content,
            mime_type,
        },
        _ => FilePreview::Text {
            file_name,
            content,
            mime_type,
        },
    }
}

fn preview_from_record(file_mgr: &FileManager, record: ResolvedFileRecord) -> FilePreview {
    preview_from_record_with_reader(file_mgr, record, read_preview_file_bounded)
}

fn read_preview_file_bounded(path: &Path) -> std::io::Result<Vec<u8>> {
    let metadata = std::fs::metadata(path)?;
    if metadata.len() > MAX_PREVIEW_BYTES {
        return Ok(vec![0; (MAX_PREVIEW_BYTES as usize) + 1]);
    }

    let mut file = std::fs::File::open(path)?;
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    file.by_ref()
        .take(MAX_PREVIEW_BYTES + 1)
        .read_to_end(&mut bytes)?;
    Ok(bytes)
}

fn preview_from_record_with_reader(
    file_mgr: &FileManager,
    record: ResolvedFileRecord,
    read_file: impl FnOnce(&Path) -> std::io::Result<Vec<u8>>,
) -> FilePreview {
    let full_path = match file_mgr.resolve_existing_file(&record.stored_path) {
        Ok(path) => path,
        Err(_) => return unsupported_preview(&record.file_name, "File is unavailable"),
    };
    let bytes = if record.file_size > MAX_PREVIEW_BYTES {
        vec![0; (MAX_PREVIEW_BYTES as usize) + 1]
    } else {
        match read_file(&full_path) {
            Ok(bytes) => bytes,
            Err(_) => return unsupported_preview(&record.file_name, "File is unavailable"),
        }
    };

    preview_from_bytes(&record.file_name, &record.file_type, bytes)
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

pub(crate) fn save_clipboard_image_to_tmp(
    aijia_home: &AiJiaHome,
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
    let dir = aijia_home.root().join("tmpImage");
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
pub async fn save_clipboard_image_to_tmp_dir(
    aijia_home: State<'_, Arc<AiJiaHome>>,
    bytes: Vec<u8>,
    mime_type: String,
) -> Result<SavedClipboardAttachment, String> {
    save_clipboard_image_to_tmp(aijia_home.inner().as_ref(), &bytes, &mime_type)
}

/// Save a clipboard image into the AIjia home tmp directory
/// (`~/.renlijia/tmp/clipboard/`). Files here are user-visible but logically
/// "throwaway": they're regeneratable (user can re-paste) and get reaped on
/// startup by `cleanup_workspace_clipboard_staging`.
pub(crate) fn save_clipboard_image_to_tmp_clipboard_impl(
    dir: &Path,
    bytes: &[u8],
    mime_type: &str,
) -> Result<SavedClipboardAttachment, String> {
    if !dir.is_absolute() {
        return Err("tmp clipboard dir must be absolute".to_string());
    }
    let ext = clipboard_extension_for_mime(mime_type);
    let file_name = format!(
        "clipboard-{}-{}.{}",
        chrono::Utc::now().timestamp_millis(),
        &uuid::Uuid::new_v4().simple().to_string()[..8],
        ext
    );
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
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
pub async fn save_clipboard_image_to_workspace_staging(
    aijia_home: State<'_, Arc<AiJiaHome>>,
    bytes: Vec<u8>,
    mime_type: String,
) -> Result<SavedClipboardAttachment, String> {
    save_clipboard_image_to_tmp_clipboard_impl(&aijia_home.tmp_clipboard_dir(), &bytes, &mime_type)
}

/// Best-effort cleanup: remove files older than `max_age_days` from the
/// given tmp clipboard directory. Errors are swallowed.
pub fn cleanup_workspace_clipboard_staging(dir: &Path, max_age_days: u64) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let cutoff = std::time::SystemTime::now()
        .checked_sub(std::time::Duration::from_secs(max_age_days * 86_400));
    for entry in entries.flatten() {
        let Ok(meta) = entry.metadata() else { continue };
        if !meta.is_file() {
            continue;
        }
        let modified = meta.modified().ok();
        if let (Some(modified), Some(cutoff)) = (modified, cutoff) {
            if modified < cutoff {
                let _ = std::fs::remove_file(entry.path());
            }
        }
    }
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
    let full_path = file_mgr
        .resolve_existing_file(&stored_path)
        .map_err(|e| e.to_string())?;

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
    let full_path = file_mgr
        .resolve_existing_file(&stored_path)
        .map_err(|e| e.to_string())?;

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

#[tauri::command]
pub async fn get_file_preview(
    facade: State<'_, Arc<RuntimeRepositoryFacade>>,
    file_mgr: State<'_, Arc<FileManager>>,
    file_id: String,
    conversation_id: String,
) -> Result<FilePreview, String> {
    let record = resolve_file_record(&facade, &file_id, &conversation_id)?;

    if record.file_size > MAX_PREVIEW_BYTES {
        return Ok(unsupported_preview(
            &record.file_name,
            "File is too large to preview",
        ));
    }

    if normalize_preview_kind(&record.file_name, &record.file_type).is_none() {
        return Ok(unsupported_preview(
            &record.file_name,
            format!("File type '{}' is not supported", record.file_type),
        ));
    }

    Ok(preview_from_record(&file_mgr, record))
}

/// Preview a local file by absolute path (used for user-attached files that
/// were never uploaded to the workspace, e.g. drag/drop or paste).
#[tauri::command]
pub async fn get_local_file_preview(path: String) -> Result<FilePreview, String> {
    let p = Path::new(&path);
    let file_name = p
        .file_name()
        .and_then(|v| v.to_str())
        .unwrap_or("unknown")
        .to_string();

    let metadata = match std::fs::metadata(p) {
        Ok(m) => m,
        Err(e) => {
            return Ok(unsupported_preview(
                &file_name,
                format!("File is unavailable: {}", e),
            ));
        }
    };
    if !metadata.is_file() {
        return Ok(unsupported_preview(&file_name, "Not a regular file"));
    }
    if metadata.len() > MAX_PREVIEW_BYTES {
        return Ok(unsupported_preview(
            &file_name,
            "File is too large to preview",
        ));
    }

    let file_type = p
        .extension()
        .and_then(|v| v.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();

    if normalize_preview_kind(&file_name, &file_type).is_none() {
        return Ok(unsupported_preview(
            &file_name,
            format!("File type '{}' is not supported", file_type),
        ));
    }

    let bytes = match std::fs::read(p) {
        Ok(b) => b,
        Err(e) => {
            return Ok(unsupported_preview(
                &file_name,
                format!("File is unavailable: {}", e),
            ));
        }
    };

    Ok(preview_from_bytes(&file_name, &file_type, bytes))
}

/// Open a local file by absolute path with the system default application.
#[tauri::command]
pub async fn open_local_file(path: String) -> Result<(), String> {
    let p = Path::new(&path);
    if !p.exists() {
        return Err(format!("Path does not exist: {}", path));
    }
    #[cfg(target_os = "macos")]
    std::process::Command::new("open")
        .arg(p)
        .spawn()
        .map_err(|e| e.to_string())?;
    #[cfg(target_os = "windows")]
    std::process::Command::new("explorer")
        .arg(p)
        .spawn()
        .map_err(|e| e.to_string())?;
    #[cfg(target_os = "linux")]
    std::process::Command::new("xdg-open")
        .arg(p)
        .spawn()
        .map_err(|e| e.to_string())?;
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
    fn save_clipboard_image_writes_to_tmp_image_dir() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let home = AiJiaHome::from_path(tmp.path().to_path_buf());

        let saved = save_clipboard_image_to_tmp(&home, &[1, 2, 3, 4], "image/png")
            .expect("save clipboard image");

        assert!(saved.path.contains("/tmpImage/"));
        assert!(!saved.path.contains("/conversations/"));
        assert!(saved.file_name.starts_with("clipboard-"));
        assert!(saved.file_name.ends_with(".png"));
        assert_eq!(saved.file_size, 4);
        assert_eq!(saved.mime_type, "image/png");
        assert!(std::path::Path::new(&saved.path).exists());

        let parent = std::path::Path::new(&saved.path).parent().expect("parent");
        assert_eq!(parent, home.root().join("tmpImage").as_path());
    }
}

#[cfg(test)]
mod preview_tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn classify_markdown_preview() {
        let preview = preview_from_bytes("summary.md", "markdown", b"# Hello".to_vec());

        match preview {
            FilePreview::Markdown {
                file_name, content, ..
            } => {
                assert_eq!(file_name, "summary.md");
                assert_eq!(content, "# Hello");
            }
            other => panic!("expected markdown preview, got {:?}", other),
        }
    }

    #[test]
    fn html_preview_serializes_sandbox_flag() {
        let preview = preview_from_bytes("page.html", "html", b"<h1>Hello</h1>".to_vec());
        let json = serde_json::to_value(preview).expect("serialize preview");

        assert_eq!(json["kind"], "html");
        assert_eq!(json["fileName"], "page.html");
        assert_eq!(json["mimeType"], "text/html");
        assert_eq!(json["content"], "<h1>Hello</h1>");
        assert_eq!(json["sandbox"], true);
    }

    #[test]
    fn png_preview_serializes_data_url_without_utf8_decoding() {
        let preview = preview_from_bytes("chart.png", "png", vec![0x89, b'P', b'N', b'G']);
        let json = serde_json::to_value(preview).expect("serialize preview");

        assert_eq!(json["kind"], "image");
        assert_eq!(json["fileName"], "chart.png");
        assert_eq!(json["mimeType"], "image/png");
        assert_eq!(json["dataUrl"], "data:image/png;base64,iVBORw==");
    }

    #[test]
    fn image_preview_uses_extension_when_file_type_is_missing() {
        let preview = preview_from_bytes("chart.webp", "", vec![1, 2, 3]);
        let json = serde_json::to_value(preview).expect("serialize preview");

        assert_eq!(json["kind"], "image");
        assert_eq!(json["mimeType"], "image/webp");
        assert_eq!(json["dataUrl"], "data:image/webp;base64,AQID");
    }

    #[test]
    fn unsupported_binary_type_returns_reason() {
        let preview = preview_from_bytes("sheet.xlsx", "excel", b"binary".to_vec());

        match preview {
            FilePreview::Unsupported { reason, .. } => {
                assert!(
                    reason.contains("not supported"),
                    "unexpected reason: {reason}"
                );
            }
            other => panic!("expected unsupported preview, got {:?}", other),
        }
    }

    #[test]
    fn python_file_type_is_not_previewable_text() {
        let preview = preview_from_bytes("script.py", "py", b"print('secret')".to_vec());

        match preview {
            FilePreview::Unsupported { reason, .. } => {
                assert!(
                    reason.contains("not supported"),
                    "unexpected reason: {reason}"
                );
            }
            other => panic!("expected unsupported preview, got {:?}", other),
        }
    }

    #[test]
    fn oversized_file_returns_reason() {
        let preview = preview_from_bytes(
            "large.txt",
            "text",
            vec![b'a'; (MAX_PREVIEW_BYTES as usize) + 1],
        );

        match preview {
            FilePreview::Unsupported { reason, .. } => {
                assert!(reason.contains("too large"), "unexpected reason: {reason}");
            }
            other => panic!("expected unsupported preview, got {:?}", other),
        }
    }

    #[test]
    fn non_utf8_file_returns_reason() {
        let preview = preview_from_bytes("bad.txt", "text", vec![0xff, 0xfe]);

        match preview {
            FilePreview::Unsupported { reason, .. } => {
                assert!(
                    reason.contains("valid UTF-8"),
                    "unexpected reason: {reason}"
                );
            }
            other => panic!("expected unsupported preview, got {:?}", other),
        }
    }

    #[test]
    fn unavailable_file_preview_uses_controlled_reason_without_leaking_path() {
        let tmp = TempDir::new().expect("tempdir");
        let file_mgr = FileManager::new(tmp.path());
        let stored_path = "generated/private/missing.md";
        let record = ResolvedFileRecord {
            file_name: "missing.md".to_string(),
            stored_path: stored_path.to_string(),
            file_type: "markdown".to_string(),
            file_size: 10,
        };

        let preview = preview_from_record(&file_mgr, record);

        match preview {
            FilePreview::Unsupported { reason, .. } => {
                assert_eq!(reason, "File is unavailable");
                assert!(!reason.contains(stored_path), "reason leaked stored path");
                assert!(
                    !reason.contains(&tmp.path().display().to_string()),
                    "reason leaked absolute path"
                );
            }
            other => panic!("expected unsupported preview, got {:?}", other),
        }
    }

    #[test]
    fn traversal_preview_error_uses_controlled_reason_without_leaking_path() {
        let tmp = TempDir::new().expect("tempdir");
        let file_mgr = FileManager::new(tmp.path());
        let stored_path = "../outside.md";
        let record = ResolvedFileRecord {
            file_name: "outside.md".to_string(),
            stored_path: stored_path.to_string(),
            file_type: "markdown".to_string(),
            file_size: 10,
        };

        let preview = preview_from_record(&file_mgr, record);

        match preview {
            FilePreview::Unsupported { reason, .. } => {
                assert_eq!(reason, "File is unavailable");
                assert!(!reason.contains(stored_path), "reason leaked stored path");
                assert!(
                    !reason.contains(&tmp.path().display().to_string()),
                    "reason leaked absolute path"
                );
            }
            other => panic!("expected unsupported preview, got {:?}", other),
        }
    }

    #[test]
    fn read_error_preview_uses_controlled_reason_without_leaking_path() {
        let tmp = TempDir::new().expect("tempdir");
        let stored_path = "generated/private/secret.md";
        let full_path = tmp.path().join(stored_path);
        std::fs::create_dir_all(full_path.parent().expect("parent")).expect("create parent");
        std::fs::write(&full_path, "# secret").expect("write file");
        let file_mgr = FileManager::new(tmp.path());
        let record = ResolvedFileRecord {
            file_name: "secret.md".to_string(),
            stored_path: stored_path.to_string(),
            file_type: "markdown".to_string(),
            file_size: 8,
        };

        let preview = preview_from_record_with_reader(&file_mgr, record, |path| {
            Err(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                format!("cannot read {}", path.display()),
            ))
        });

        match preview {
            FilePreview::Unsupported { reason, .. } => {
                assert_eq!(reason, "File is unavailable");
                assert!(!reason.contains(stored_path), "reason leaked stored path");
                assert!(
                    !reason.contains(&tmp.path().display().to_string()),
                    "reason leaked absolute path"
                );
            }
            other => panic!("expected unsupported preview, got {:?}", other),
        }
    }

    #[test]
    fn oversized_metadata_skips_file_read() {
        let tmp = TempDir::new().expect("tempdir");
        let stored_path = "generated/large.txt";
        let full_path = tmp.path().join(stored_path);
        std::fs::create_dir_all(full_path.parent().expect("parent")).expect("create parent");
        std::fs::write(&full_path, "small").expect("write file");
        let file_mgr = FileManager::new(tmp.path());
        let record = ResolvedFileRecord {
            file_name: "large.txt".to_string(),
            stored_path: stored_path.to_string(),
            file_type: "text".to_string(),
            file_size: MAX_PREVIEW_BYTES + 1,
        };

        let preview = preview_from_record_with_reader(&file_mgr, record, |_path| {
            panic!("oversized metadata should skip file reads")
        });

        match preview {
            FilePreview::Unsupported { reason, .. } => {
                assert!(reason.contains("too large"), "unexpected reason: {reason}");
            }
            other => panic!("expected unsupported preview, got {:?}", other),
        }
    }

    #[test]
    fn bounded_reader_uses_actual_file_size_before_reading() {
        let tmp = TempDir::new().expect("tempdir");
        let full_path = tmp.path().join("large.txt");
        std::fs::write(&full_path, vec![b'a'; (MAX_PREVIEW_BYTES as usize) + 2])
            .expect("write large file");

        let bytes = read_preview_file_bounded(&full_path).expect("read preview bytes");

        assert_eq!(bytes.len(), (MAX_PREVIEW_BYTES as usize) + 1);
    }

    #[test]
    fn resolve_existing_file_rejects_path_traversal() {
        let tmp = TempDir::new().expect("tempdir");
        let file_mgr = FileManager::new(tmp.path());

        let err = file_mgr
            .resolve_existing_file("../outside.txt")
            .expect_err("path traversal should fail");

        assert!(err.to_string().contains("Path traversal rejected"));
    }

    #[test]
    fn resolve_existing_file_rejects_directory() {
        let tmp = TempDir::new().expect("tempdir");
        let generated_dir = tmp.path().join("generated");
        std::fs::create_dir_all(&generated_dir).expect("create generated dir");
        let file_mgr = FileManager::new(tmp.path());

        let err = file_mgr
            .resolve_existing_file("generated")
            .expect_err("directory should fail");

        assert!(
            err.to_string().contains("Stored file does not exist"),
            "unexpected error: {err}"
        );
    }
}
