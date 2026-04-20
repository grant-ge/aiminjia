use std::sync::Arc;

use tauri::{Manager, State};

use crate::runtime::project_memory::{ProjectMemoryEntryDraft, ProjectMemoryService};
use crate::storage::file_store::RuntimeRepositoryFacade;

#[tauri::command]
pub async fn save_project_memory(
    app: tauri::AppHandle,
    facade: State<'_, Arc<RuntimeRepositoryFacade>>,
    workspace_path: String,
    memory: ProjectMemoryEntryDraft,
) -> Result<String, String> {
    let app_data_dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
    let service = ProjectMemoryService::new(app_data_dir, workspace_path);
    let saved = service.save_memory(memory).map_err(|e| e.to_string())?;

    let _ = facade;
    Ok(saved.relative_path.display().to_string())
}

#[tauri::command]
pub async fn distill_project_memory(
    app: tauri::AppHandle,
    facade: State<'_, Arc<RuntimeRepositoryFacade>>,
    workspace_path: String,
) -> Result<u32, String> {
    let app_data_dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
    let service = ProjectMemoryService::new(app_data_dir, workspace_path);
    let rebuilt = service.distill_index().map_err(|e| e.to_string())?;

    let _ = facade;
    Ok(rebuilt as u32)
}
