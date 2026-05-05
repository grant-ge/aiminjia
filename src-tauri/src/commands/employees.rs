use std::sync::Arc;

use tauri::{AppHandle, Manager};

use crate::runtime::employee::inbox::{InboxEntry, InboxStore};
use crate::runtime::employee::store::{
    CreateEmployeeRequest, EmployeeLifecycle, EmployeeRecord, EmployeeStore, UpdateEmployeeRequest,
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

/// Soft-delete: set lifecycle = Archived. The employee is hidden from the
/// main grid but recoverable via `employee_restore` for 7 days. After 7
/// days, the scheduler's purge sweep hard-deletes the directory.
#[tauri::command]
pub async fn employee_delete(app: AppHandle, id: String) -> Result<bool, String> {
    employee_store(&app)?
        .update(
            &id,
            UpdateEmployeeRequest {
                lifecycle: Some(EmployeeLifecycle::Archived),
                ..Default::default()
            },
        )
        .map(|_| true)
        .map_err(|e| e.to_string())
}

/// Restore an archived employee: lifecycle Archived -> Active.
#[tauri::command]
pub async fn employee_restore(app: AppHandle, id: String) -> Result<bool, String> {
    employee_store(&app)?
        .update(
            &id,
            UpdateEmployeeRequest {
                lifecycle: Some(EmployeeLifecycle::Active),
                ..Default::default()
            },
        )
        .map(|_| true)
        .map_err(|e| e.to_string())
}

/// Hard-delete an employee directory. Skips the 7-day recovery window —
/// used for the "永久删除" UI action and by the scheduler's auto-purge.
#[tauri::command]
pub async fn employee_purge(app: AppHandle, id: String) -> Result<bool, String> {
    employee_store(&app)?.purge(&id).map_err(|e| e.to_string())
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

// ─── active run / stop ───────────────────────────────────────────────────────

/// Stop an employee's currently running dispatch (if any).
/// Returns Ok(true) if a run was found and cancellation was requested,
/// Ok(false) if no active run exists for this employee.
#[tauri::command]
pub async fn employee_stop_run(app: AppHandle, id: String) -> Result<bool, String> {
    use crate::transport::tauri_commands::chat::TauriChatCommandAdapter;

    let active_runs = app
        .state::<Arc<crate::runtime::employee::EmployeeActiveRuns>>()
        .inner()
        .clone();
    let Some(run) = active_runs.lookup(&id) else {
        return Ok(false);
    };

    let adapter = app
        .state::<Arc<TauriChatCommandAdapter>>()
        .inner()
        .clone();
    adapter
        .stop_streaming(run.conversation_id.clone())
        .await
        .map_err(|e| format!("stop_streaming failed: {e}"))?;
    // The active_runs entry is cleaned up by the dispatch's spawn block (via
    // ActiveRunGuard's Drop) when the agent loop terminates; we don't
    // unregister here to avoid double-frees and racing the natural cleanup.
    Ok(true)
}

/// Returns the current ActiveRun for the employee (if any).
/// Polled by the UI to drive Activity-dimension state derivation.
#[tauri::command]
pub async fn employee_active_run(
    app: AppHandle,
    id: String,
) -> Result<Option<crate::runtime::employee::ActiveRun>, String> {
    let active_runs = app
        .state::<Arc<crate::runtime::employee::EmployeeActiveRuns>>()
        .inner()
        .clone();
    Ok(active_runs.lookup(&id))
}
