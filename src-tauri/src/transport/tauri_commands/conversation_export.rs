use std::path::Path;
use std::sync::Arc;

use tauri::{AppHandle, State};

use crate::runtime::export::conversation_exporter::{
    ConversationExportRequest, ConversationExportResult, ConversationExporter, ExportPaths,
};
use crate::storage::file_store::AppStorage;

#[tauri::command]
pub async fn export_conversation(
    app: AppHandle,
    storage: State<'_, Arc<AppStorage>>,
    conversation_id: String,
) -> Result<ConversationExportResult, String> {
    let app_home = storage.base_dir().to_path_buf();
    let export_root = app_home.join("exports").join("conversations");
    let exporter = ConversationExporter::new(ExportPaths {
        app_home,
        export_root,
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
pub async fn reveal_export_in_folder(path: String) -> Result<(), String> {
    let full_path = Path::new(&path);
    if !full_path.exists() {
        return Err("导出文件不存在或已被移动。".to_string());
    }

    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .arg("-R")
            .arg(full_path)
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
        let parent = full_path.parent().unwrap_or(full_path);
        std::process::Command::new("xdg-open")
            .arg(parent)
            .spawn()
            .map_err(|error| error.to_string())?;
    }

    Ok(())
}
