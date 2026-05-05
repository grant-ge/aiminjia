use std::sync::Arc;

use tauri::{AppHandle, Manager};

use crate::runtime::employee::inbox::{InboxEntry, InboxStore};
use crate::runtime::employee::store::{
    CreateEmployeeRequest, EmployeeRecord, EmployeeStore, UpdateEmployeeRequest,
};
use crate::storage::{CurrentUserStorage, UserScopedPathResolver};

// ─── helpers ─────────────────────────────────────────────────────────────────

fn employee_store(app: &AppHandle) -> Result<EmployeeStore, String> {
    let cus = app.state::<Arc<CurrentUserStorage>>();
    let paths = cus.require_paths().map_err(|e| e.to_string())?;
    Ok(EmployeeStore::new(paths.employees_dir()))
}

fn inbox_store(app: &AppHandle) -> Result<InboxStore, String> {
    let cus = app.state::<Arc<CurrentUserStorage>>();
    let paths = cus.require_paths().map_err(|e| e.to_string())?;
    Ok(InboxStore::new(paths.employees_dir()))
}

// ─── employee CRUD ────────────────────────────────────────────────────────────

#[tauri::command]
pub async fn employee_list(app: AppHandle) -> Result<Vec<EmployeeRecord>, String> {
    employee_store(&app)?.list().map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn employee_get(app: AppHandle, id: String) -> Result<EmployeeRecord, String> {
    employee_store(&app)?.get(&id).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn employee_create(
    app: AppHandle,
    request: CreateEmployeeRequest,
) -> Result<EmployeeRecord, String> {
    employee_store(&app)?
        .create(request)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn employee_update(
    app: AppHandle,
    id: String,
    request: UpdateEmployeeRequest,
) -> Result<EmployeeRecord, String> {
    employee_store(&app)?
        .update(&id, request)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn employee_delete(app: AppHandle, id: String) -> Result<bool, String> {
    employee_store(&app)?
        .delete(&id)
        .map_err(|e| e.to_string())
}

// ─── trigger ─────────────────────────────────────────────────────────────────

/// Manually trigger an employee run (on-demand dispatch).
/// Returns the conversation_id created for this run. The agent loop runs in a
/// detached background task; the caller should immediately route to the chat
/// view to observe streaming output.
#[tauri::command]
pub async fn employee_trigger(
    app: AppHandle,
    id: String,
    prompt_override: Option<String>,
    attachments: Vec<crate::runtime::chat::chat_turn_driver::ChatAttachmentRef>,
) -> Result<String, String> {
    use crate::transport::tauri_commands::chat::TauriChatCommandAdapter;
    use crate::runtime::employee::runner::{EmployeeRunDispatcher, TriggerKind};
    use chrono::Utc;

    let store = employee_store(&app)?;
    let record = store.get(&id).map_err(|e| e.to_string())?;

    let adapter = app
        .state::<Arc<TauriChatCommandAdapter>>()
        .inner()
        .clone();

    let conversation_id = adapter
        .dispatch_employee_run(
            record,
            Utc::now(),
            prompt_override,
            None,
            TriggerKind::OnDemand,
            attachments,
        )
        .await
        .map_err(|e| e.to_string())?;

    // record_run is called synchronously inside dispatch_employee_run before
    // the detached agent task starts, so we do not call it here.

    Ok(conversation_id)
}

// ─── inbox ────────────────────────────────────────────────────────────────────

#[tauri::command]
pub async fn inbox_list(
    app: AppHandle,
    employee_id: Option<String>,
    limit: Option<usize>,
) -> Result<Vec<InboxEntry>, String> {
    let store = inbox_store(&app)?;
    let limit = limit.unwrap_or(100);
    let entries = match employee_id.as_deref() {
        Some(id) => store.list_for(id, limit),
        None => store.list_all(limit),
    }
    .map_err(|e| e.to_string())?;
    Ok(entries)
}

#[tauri::command]
pub async fn inbox_mark_read(
    app: AppHandle,
    employee_id: String,
    entry_id: String,
) -> Result<bool, String> {
    inbox_store(&app)?
        .mark_read(&employee_id, &entry_id)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn inbox_mark_all_read(
    app: AppHandle,
    employee_id: String,
) -> Result<u32, String> {
    inbox_store(&app)?
        .mark_all_read(&employee_id)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn inbox_unread_count(
    app: AppHandle,
    employee_id: Option<String>,
) -> Result<u32, String> {
    inbox_store(&app)?
        .unread_count(employee_id.as_deref())
        .map_err(|e| e.to_string())
}
