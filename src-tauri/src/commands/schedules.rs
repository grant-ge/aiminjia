use std::sync::Arc;

use tauri::{AppHandle, Manager};

use crate::runtime::schedule::{CreateScheduleRequest, ScheduleRecord, ScheduleStore};
use crate::storage::{CurrentUserStorage, UserScopedPathResolver};

fn schedule_store(app: &AppHandle) -> Result<ScheduleStore, String> {
    let cus = app.state::<Arc<CurrentUserStorage>>();
    let paths = cus.require_paths().map_err(|e| e.to_string())?;
    Ok(ScheduleStore::new(paths.base_dir()))
}

#[tauri::command]
pub async fn list_schedules(app: AppHandle) -> Result<Vec<ScheduleRecord>, String> {
    schedule_store(&app)?.list().map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn create_schedule(
    app: AppHandle,
    request: CreateScheduleRequest,
) -> Result<ScheduleRecord, String> {
    schedule_store(&app)?
        .create(request)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn delete_schedule(app: AppHandle, id: String) -> Result<bool, String> {
    schedule_store(&app)?.delete(&id).map_err(|e| e.to_string())
}
