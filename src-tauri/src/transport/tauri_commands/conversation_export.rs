use std::path::{Path, PathBuf};
use std::sync::Arc;

use tauri::{AppHandle, State};

use crate::runtime::export::conversation_exporter::{
    ConversationExportRequest, ConversationExportResult, ConversationExporter, ExportPaths,
};
use crate::storage::file_store::AppStorage;
use crate::storage::{AiJiaHome, CurrentUserStorage};

pub fn active_export_storage(
    current_user_storage: &Arc<CurrentUserStorage>,
    root_storage: &Arc<AppStorage>,
) -> Arc<AppStorage> {
    current_user_storage.get_or(root_storage)
}

pub fn conversation_export_root(home: &AiJiaHome) -> PathBuf {
    home.root().join("exports").join("conversations")
}

pub fn validate_export_zip_path(home: &AiJiaHome, path: &Path) -> Result<PathBuf, String> {
    if path.extension().and_then(|value| value.to_str()) != Some("zip") {
        return Err("只能打开导出的 zip 文件。".to_string());
    }

    let export_root = conversation_export_root(home);
    let canonical_root = export_root
        .canonicalize()
        .map_err(|_| "导出目录不存在。".to_string())?;
    let canonical_path = path
        .canonicalize()
        .map_err(|_| "导出文件不存在或已被移动。".to_string())?;

    if !canonical_path.starts_with(&canonical_root) {
        return Err("只能打开导出目录中的 zip 文件。".to_string());
    }

    Ok(canonical_path)
}

#[tauri::command]
pub async fn export_conversation(
    app: AppHandle,
    current_user_storage: State<'_, Arc<CurrentUserStorage>>,
    root_storage: State<'_, Arc<AppStorage>>,
    home: State<'_, Arc<AiJiaHome>>,
    conversation_id: String,
) -> Result<ConversationExportResult, String> {
    let storage = active_export_storage(&current_user_storage, &root_storage);
    let exporter = ConversationExporter::new(ExportPaths {
        app_home: home.root().to_path_buf(),
        export_root: conversation_export_root(&home),
    });

    exporter
        .export(
            &storage,
            ConversationExportRequest {
                conversation_id,
                app_version: app.package_info().version.to_string(),
                platform: std::env::consts::OS.to_string(),
                arch: std::env::consts::ARCH.to_string(),
            },
        )
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn reveal_export_in_folder(
    home: State<'_, Arc<AiJiaHome>>,
    path: String,
) -> Result<(), String> {
    let full_path = validate_export_zip_path(&home, Path::new(&path))?;

    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .arg("-R")
            .arg(&full_path)
            .spawn()
            .map_err(|error| error.to_string())?;
    }

    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("explorer")
            .arg(format!("/select,{}", full_path.display()))
            .spawn()
            .map_err(|error| error.to_string())?;
    }

    #[cfg(target_os = "linux")]
    {
        let parent = full_path.parent().unwrap_or(&full_path);
        std::process::Command::new("xdg-open")
            .arg(parent)
            .spawn()
            .map_err(|error| error.to_string())?;
    }

    Ok(())
}
