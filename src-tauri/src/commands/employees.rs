use std::sync::Arc;

use tauri::{AppHandle, Manager, State};

use crate::runtime::employee::inbox::{InboxEntry, InboxStore};
use crate::runtime::employee::store::{
    CreateEmployeeRequest, EmployeeLifecycle, EmployeeRecord, EmployeeStore, UpdateEmployeeRequest,
};
use crate::runtime::employee::template_store::{
    bootstrap_templates, ensure_cached, fetch_catalog, merge_catalog, TemplateSnapshot,
};
use crate::storage::file_store::AppStorage;
use crate::storage::{AiJiaHome, CurrentUserStorage, UserScopedPathResolver};

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

/// Returns an `AgendaStore` scoped to the current user, or `None` when the
/// user session is not available (logged out, state not yet registered, etc.).
///
/// Returning `Option` keeps the orphan hook non-fatal: callers log a warning
/// and continue — agenda orphaning is a secondary side-effect that must not
/// block the primary employee lifecycle transition.
fn agenda_store_for(app: &AppHandle) -> Option<crate::runtime::agenda::AgendaStore> {
    let cus = app.try_state::<Arc<CurrentUserStorage>>()?;
    let paths = cus.require_paths().ok()?;
    Some(crate::runtime::agenda::AgendaStore::new(paths.base_dir()))
}

// ─── employee CRUD ────────────────────────────────────────────────────────────

/// Returns the catalog of templates the new-hire wizard should display.
///
/// Sources merged (last write wins on `template_id`, by version string):
///   1. Embedded bootstrap (always available, ~11 entries at v1.0.0)
///   2. `~/.renlijia/employee-templates-cache/` — versions previously
///      downloaded from lotus OPS via `employee_template_refresh` or
///      `ensure_cached`.
///
/// This command never hits the network. Call `employee_template_refresh`
/// to update the cache.
#[tauri::command]
pub async fn employee_template_catalog() -> Result<Vec<TemplateSnapshot>, String> {
    let bootstrap = bootstrap_templates().map_err(|e| e.to_string())?;
    let cache_dir = AiJiaHome::from_home().employee_templates_cache_dir();
    Ok(merge_catalog(bootstrap, &cache_dir))
}

/// Sync the local template cache from lotus ops-portal.
///
/// 1. `GET {OPS}/api/public/employee-templates` — list of currently-published
///    templates (latest version per `template_id`, `tenant_scope=global`).
/// 2. For each entry whose version is newer than the cache (or missing),
///    fetch its manifest, download the snapshot, verify sha256, and write
///    to `~/.renlijia/employee-templates-cache/{tid}/{version}.json`.
///
/// Returns the count of templates downloaded this call.
///
/// Failures on individual templates are logged and skipped — a partial
/// refresh is better than a hard failure that leaves the user without any
/// catalog at all (bootstrap stays available regardless).
#[tauri::command]
pub async fn employee_template_refresh() -> Result<u32, String> {
    let cache_dir = AiJiaHome::from_home().employee_templates_cache_dir();
    let client = reqwest::Client::builder()
        .build()
        .map_err(|e| e.to_string())?;

    let catalog = fetch_catalog(&client).await.map_err(|e| e.to_string())?;
    let mut downloaded = 0u32;

    for entry in catalog {
        let template_id = entry
            .get("template_id")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        let version = entry
            .get("version")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());
        let (Some(template_id), Some(version)) = (template_id, version) else {
            continue;
        };

        match ensure_cached(&cache_dir, &client, &template_id, &version).await {
            Ok(_) => downloaded += 1,
            Err(e) => log::warn!(
                "[employee_template_refresh] {template_id}@{version}: {e}"
            ),
        }
    }

    Ok(downloaded)
}

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
    // Detect whether this update is archiving the employee so we can cascade
    // the orphan hook. We check the request *before* applying it — if the
    // caller explicitly sets lifecycle=Archived we treat it the same as
    // employee_delete (which is the canonical path, but the store does not
    // reject lifecycle changes via update).
    let archiving = request.lifecycle == Some(EmployeeLifecycle::Archived);

    let record = employee_store(&app)?
        .update(&id, request)
        .map_err(|e| e.to_string())?;

    if archiving {
        if let Some(agenda_store) = agenda_store_for(&app) {
            if let Err(e) = agenda_store.mark_orphaned_by_organizer(&id) {
                log::warn!(
                    "[employee_update] mark_orphaned_by_organizer({}) failed: {}",
                    id,
                    e
                );
            }
        }
    }

    Ok(record)
}

/// Soft-delete: set lifecycle = Archived. The employee is hidden from the
/// main grid but recoverable via `employee_restore` for 7 days. After 7
/// days, the scheduler's purge sweep hard-deletes the directory.
///
/// Errors when the record is already Archived so the caller can surface a
/// clear "员工已处于解雇状态" message instead of silently no-op'ing.
#[tauri::command]
pub async fn employee_delete(app: AppHandle, id: String) -> Result<bool, String> {
    let store = employee_store(&app)?;
    let current = store.get(&id).map_err(|e| e.to_string())?;
    if current.lifecycle == EmployeeLifecycle::Archived {
        return Err("员工已处于解雇状态".to_string());
    }
    store
        .update(
            &id,
            UpdateEmployeeRequest {
                lifecycle: Some(EmployeeLifecycle::Archived),
                ..Default::default()
            },
        )
        .map_err(|e| e.to_string())?;

    // 联动：把这个 employee 作 organizer 的 agenda items 转 Orphaned。
    // 失败仅 log，不阻塞软删除——孤儿 item 调度器不会再 dispatch（runner 只接 Active）。
    if let Some(agenda_store) = agenda_store_for(&app) {
        if let Err(e) = agenda_store.mark_orphaned_by_organizer(&id) {
            log::warn!(
                "[employee_delete] mark_orphaned_by_organizer({}) failed: {}",
                id,
                e
            );
        }
    }

    Ok(true)
}

/// Restore an archived employee: lifecycle Archived -> Active.
///
/// Errors when the record is not Archived so the caller doesn't silently
/// no-op (e.g. a stale UI button click).
#[tauri::command]
pub async fn employee_restore(app: AppHandle, id: String) -> Result<bool, String> {
    let store = employee_store(&app)?;
    let current = store.get(&id).map_err(|e| e.to_string())?;
    if current.lifecycle != EmployeeLifecycle::Archived {
        return Err("员工未处于解雇状态，无需恢复".to_string());
    }
    store
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
///
/// Refuses to purge a non-Archived employee — the only legitimate path to
/// permanent deletion is via the recycle bin (or the scheduler's age sweep,
/// which uses `purge_if_archived_older_than` directly on the store).
#[tauri::command]
pub async fn employee_purge(app: AppHandle, id: String) -> Result<bool, String> {
    let store = employee_store(&app)?;
    let current = store.get(&id).map_err(|e| e.to_string())?;
    if current.lifecycle != EmployeeLifecycle::Archived {
        return Err("只能永久删除已解雇的员工".to_string());
    }
    store.purge(&id).map_err(|e| e.to_string())?;

    // 幂等地 mark_orphaned：archive 时已触发过；purge 后再确保一次，避免
    // 用户手动恢复 agenda 后再走 purge 路径造成残留 Active 孤儿。
    // purge 后 employee 目录已删除，AgendaStore 操作的是独立的 agenda/ 目录，
    // 所以此时调用仍然有效。
    if let Some(agenda_store) = agenda_store_for(&app) {
        if let Err(e) = agenda_store.mark_orphaned_by_organizer(&id) {
            log::warn!(
                "[employee_purge] mark_orphaned_by_organizer({}) failed: {}",
                id,
                e
            );
        }
    }

    Ok(true)
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

    // Authoritative lifecycle gate. The drawer disables this button for
    // paused / archived, but the command must enforce it independently —
    // skills, scripts, and stale UI clicks can all reach this entry point.
    match record.lifecycle {
        EmployeeLifecycle::Archived => {
            return Err("员工已解雇，恢复后才能派活".to_string());
        }
        EmployeeLifecycle::Paused => {
            return Err("员工已暂停，恢复员工后才能派活".to_string());
        }
        EmployeeLifecycle::Active => {}
    }

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

// ─── knowledge indexing ───────────────────────────────────────────────────────

/// Arguments for `employee_index_knowledge_async`.
///
/// `sources` is a list of `(absolute_path, original_name)` pairs.
/// The caller must have already added the corresponding entries to
/// `resource_config.knowledgeSources` with `status = "pending"` before
/// invoking this command.
#[derive(serde::Deserialize)]
pub struct IndexKnowledgeArgs {
    pub employee_id: String,
    /// (absolute_path, original_name) pairs.
    pub sources: Vec<(String, String)>,
}

/// Kick off async background indexing for the listed knowledge-source files.
///
/// Returns immediately — progress is tracked per-file via `knowledgeSources[*].status`
/// on the `EmployeeRecord` (poll `employee_get` to observe `"indexing"` → `"done"`/`"failed"`).
#[tauri::command]
pub async fn employee_index_knowledge_async(
    args: IndexKnowledgeArgs,
    app: AppHandle,
    root_db: State<'_, Arc<AppStorage>>,
) -> Result<(), String> {
    use crate::runtime::employee::knowledge::spawn_index_all;
    use std::path::PathBuf;

    let store = Arc::new(employee_store(&app)?);
    let app_storage = root_db.inner().clone();

    let sources: Vec<(PathBuf, String)> = args
        .sources
        .into_iter()
        .map(|(p, name)| (PathBuf::from(p), name))
        .collect();

    spawn_index_all(store, app_storage, args.employee_id, sources);
    Ok(())
}
