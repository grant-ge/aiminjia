use std::sync::Arc;

use tauri::{AppHandle, Manager, State};

use crate::runtime::employee::inbox::{InboxEntry, InboxStore};
use crate::runtime::employee::store::{
    CreateEmployeeRequest, EmployeeLifecycle, EmployeeRecord, EmployeeStore, UpdateEmployeeRequest,
};
use crate::runtime::employee::template_store::{
    ensure_cached, ensure_instance_snapshot, fetch_catalog, find_latest_for_template,
    merge_catalog, read_instance_snapshot, TemplateSnapshot,
};
use crate::storage::file_store::AppStorage;
use crate::storage::{AiJiaHome, CurrentUserStorage, UserScopedPathResolver};

// ─── helpers ─────────────────────────────────────────────────────────────────

fn employee_store(app: &AppHandle) -> Result<Arc<EmployeeStore>, String> {
    if let Some(state) = app.try_state::<Arc<EmployeeStore>>() {
        return Ok(state.inner().clone());
    }
    // Fallback for early-boot paths or tests where lib.rs hasn't yet
    // installed the singleton. Bypasses the AgentRegistry sync hook —
    // production logged-in flow always sees the singleton.
    let cus = app.state::<Arc<CurrentUserStorage>>();
    let paths = cus.require_paths().map_err(|e| e.to_string())?;
    Ok(Arc::new(EmployeeStore::new(paths.employees_dir())))
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
/// 来源：`~/.renlijia/employee-templates-cache/` —— 由 `employee_template_refresh`
/// 从 lotus OPS 推下来的 snapshot。**没有 embedded bootstrap 兜底**——参见
/// `runtime/employee/template_store.rs` 模块注释里的删除理由。
///
/// 这个命令不发网络请求。Cache 为空时返回 []；前端应先调
/// `employee_template_refresh` 触发一次拉取。
#[tauri::command]
pub async fn employee_template_catalog() -> Result<Vec<TemplateSnapshot>, String> {
    let cache_dir = AiJiaHome::from_home().employee_templates_cache_dir();
    // merge_catalog 用空 Vec 作为起点：等同于"只读 cache 目录"。
    Ok(merge_catalog(Vec::new(), &cache_dir))
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
            Err(e) => log::warn!("[employee_template_refresh] {template_id}@{version}: {e}"),
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

/// Hard-delete an employee. PR-7: the soft-delete / 7-day recycle bin /
/// scheduler purge sweep model was retired — it added complexity (three
/// IPC commands + a tick + race-safe `purge_if_archived_older_than`) for a
/// scenario users almost never hit (mis-deleting). Re-hiring takes ~30
/// seconds via the wizard, so we replaced it with an immediate hard
/// delete + a frontend confirmation dialog.
///
/// Errors only when the record is missing.
#[tauri::command]
pub async fn employee_delete(app: AppHandle, id: String) -> Result<bool, String> {
    let store = employee_store(&app)?;
    // Probe existence so the caller gets a clear error instead of a no-op.
    let _current = store.get(&id).map_err(|e| e.to_string())?;
    store.purge(&id).map_err(|e| e.to_string())?;

    // 联动：把这个 employee 作 organizer 的 agenda items 转 Orphaned。
    // 失败仅 log，不阻塞删除——孤儿 item 调度器不会再 dispatch。
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
    use crate::runtime::employee::runner::{EmployeeRunDispatcher, TriggerKind};
    use crate::transport::tauri_commands::chat::TauriChatCommandAdapter;
    use chrono::Utc;

    let store = employee_store(&app)?;
    let record = store.get(&id).map_err(|e| e.to_string())?;

    // Authoritative lifecycle gate. The drawer disables this button for
    // archived employees, but the command must enforce it independently —
    // skills, scripts, and stale UI clicks can all reach this entry point.
    // PR-6: `Paused` was retired; legacy records are canonicalized to Active
    // when `EmployeeStore::get` returns them, so we only need to gate on
    // Archived here.
    match record.lifecycle {
        EmployeeLifecycle::Archived => {
            return Err("员工已解雇，恢复后才能派活".to_string());
        }
        EmployeeLifecycle::Active | EmployeeLifecycle::Paused => {}
    }

    let adapter = app.state::<Arc<TauriChatCommandAdapter>>().inner().clone();

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
pub async fn inbox_mark_all_read(app: AppHandle, employee_id: String) -> Result<u32, String> {
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

    let adapter = app.state::<Arc<TauriChatCommandAdapter>>().inner().clone();
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

    let store = employee_store(&app)?;
    let app_storage = root_db.inner().clone();

    let sources: Vec<(PathBuf, String)> = args
        .sources
        .into_iter()
        .map(|(p, name)| (PathBuf::from(p), name))
        .collect();

    spawn_index_all(store, app_storage, args.employee_id, sources);
    Ok(())
}

// ─── PR-12: manual template upgrade ──────────────────────────────────────────

#[derive(serde::Serialize)]
pub struct TemplateUpgradeCheck {
    pub current_version: Option<String>,
    pub latest_version: Option<String>,
    pub has_upgrade: bool,
    /// Human-readable field names that will change if the upgrade proceeds.
    /// Empty when has_upgrade is false. Frontend uses this to render the
    /// confirmation dialog body.
    pub changed_fields: Vec<&'static str>,
}

/// Diff the employee's current snapshot against the latest available
/// (bootstrap ∪ cache) version. Returns metadata for the frontend to
/// decide whether to surface the 升级模板 button.
///
/// "Latest" follows the same lexicographic version comparison as
/// `merge_catalog` (works for `1.0` < `1.1` < `1.2` patterns; `1.10`
/// vs `1.2` is a known weakness but not in current play).
#[tauri::command]
pub async fn employee_template_check_upgrade(
    app: AppHandle,
    id: String,
) -> Result<TemplateUpgradeCheck, String> {
    let store = employee_store(&app)?;
    let record = store.get(&id).map_err(|e| e.to_string())?;

    let employees_dir = current_employees_dir(&app)?;
    let instance_dir = employees_dir.join(&record.id);
    let current = read_instance_snapshot(&instance_dir).ok().flatten();

    let template_id = match record.template_id.as_deref() {
        Some(t) => t,
        None => {
            return Ok(TemplateUpgradeCheck {
                current_version: current.as_ref().map(|s| s.version.clone()),
                latest_version: None,
                has_upgrade: false,
                changed_fields: vec![],
            });
        }
    };

    let cache_dir = AiJiaHome::from_home().employee_templates_cache_dir();
    let latest = find_latest_for_template(&cache_dir, template_id);

    let (Some(cur), Some(lat)) = (current.as_ref(), latest.as_ref()) else {
        return Ok(TemplateUpgradeCheck {
            current_version: current.as_ref().map(|s| s.version.clone()),
            latest_version: latest.as_ref().map(|s| s.version.clone()),
            has_upgrade: false,
            changed_fields: vec![],
        });
    };

    if lat.version <= cur.version {
        return Ok(TemplateUpgradeCheck {
            current_version: Some(cur.version.clone()),
            latest_version: Some(lat.version.clone()),
            has_upgrade: false,
            changed_fields: vec![],
        });
    }

    let mut changed: Vec<&'static str> = Vec::new();
    if cur.system_prompt_extra != lat.system_prompt_extra {
        changed.push("职责说明");
    }
    if cur.role != lat.role {
        changed.push("角色名");
    }
    if cur.description != lat.description {
        changed.push("简介");
    }
    if cur.avatar != lat.avatar {
        changed.push("头像");
    }
    if cur.default_skill_id != lat.default_skill_id {
        changed.push("默认技能");
    }
    if cur.requires_attachment != lat.requires_attachment {
        changed.push("附件需求");
    }
    if cur.requires_dingtalk != lat.requires_dingtalk {
        changed.push("钉钉授权");
    }

    Ok(TemplateUpgradeCheck {
        current_version: Some(cur.version.clone()),
        latest_version: Some(lat.version.clone()),
        has_upgrade: true,
        changed_fields: changed,
    })
}

/// Rewrite the employee's snapshot to the latest available version and
/// rebuild the derived fields on the record.
///
/// **Overwrites** (template-owned facts):
///   role, description, avatar, system_prompt_extra, default_skill_id,
///   skill_ids
///
/// **Preserves** (user-tuned state):
///   name, cron, cron_enabled, timezone, resource_config, lifecycle,
///   tool_whitelist (vestigial post-PR-11), last_run_at, next_run_at
///
/// Errors when: employee not found, no template_id, no upgrade
/// available, or snapshot write fails.
#[tauri::command]
pub async fn employee_template_upgrade(
    app: AppHandle,
    id: String,
) -> Result<EmployeeRecord, String> {
    let store = employee_store(&app)?;
    let record = store.get(&id).map_err(|e| e.to_string())?;

    let template_id = record
        .template_id
        .as_deref()
        .ok_or_else(|| "员工没有模板关联，无法升级".to_string())?;

    let employees_dir = current_employees_dir(&app)?;
    let instance_dir = employees_dir.join(&record.id);
    let current = read_instance_snapshot(&instance_dir).map_err(|e| e.to_string())?;

    let cache_dir = AiJiaHome::from_home().employee_templates_cache_dir();
    let latest = find_latest_for_template(&cache_dir, template_id)
        .ok_or_else(|| "未找到任何可用模板版本，请先刷新模板".to_string())?;

    if let Some(ref cur) = current {
        if latest.version <= cur.version {
            return Err(format!("当前已是最新版本 (v{})，无需升级", cur.version));
        }
    }

    // Rewrite the per-instance snapshot dir.
    let source = format!(
        "upgrade:{}→{}",
        current.as_ref().map(|s| s.version.as_str()).unwrap_or(""),
        latest.version
    );
    ensure_instance_snapshot(&instance_dir, &latest, &source)
        .map_err(|e| format!("写入新 snapshot 失败: {e}"))?;

    // Derive record fields from the new snapshot. `name` is intentionally
    // preserved — users may have renamed the employee in the wizard or
    // drawer. Same for resource_config / cron / lifecycle.
    let updated = store
        .update(
            &id,
            UpdateEmployeeRequest {
                role: Some(latest.role.clone()),
                description: Some(latest.description.clone()),
                avatar: Some(latest.avatar.clone()),
                system_prompt_extra: Some(if latest.system_prompt_extra.is_empty() {
                    None
                } else {
                    Some(latest.system_prompt_extra.clone())
                }),
                default_skill_id: Some(if latest.default_skill_id.is_empty() {
                    None
                } else {
                    Some(latest.default_skill_id.clone())
                }),
                skill_ids: Some(latest.skill_ids.clone()),
                // PR-11: tool_whitelist is vestigial. Pass an empty vec so
                // a record that still carries a stale legacy whitelist gets
                // cleared on upgrade.
                tool_whitelist: Some(Vec::new()),
                // Preserve everything else (None = don't touch).
                ..Default::default()
            },
        )
        .map_err(|e| e.to_string())?;

    log::info!(
        "[employee_template_upgrade] {} : v{} -> v{} (source={})",
        id,
        current.as_ref().map(|s| s.version.as_str()).unwrap_or("?"),
        latest.version,
        source
    );

    Ok(updated)
}

fn current_employees_dir(app: &AppHandle) -> Result<std::path::PathBuf, String> {
    let cus = app
        .try_state::<Arc<CurrentUserStorage>>()
        .ok_or_else(|| "CurrentUserStorage not registered".to_string())?;
    let paths = cus.require_paths().map_err(|e| e.to_string())?;
    Ok(paths.employees_dir())
}
