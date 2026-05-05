use std::sync::Arc;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use tokio::time::{self, Duration};

use crate::runtime::employee::store::{DueEmployee, EmployeeRecord, EmployeeStore};
use crate::storage::UserScopedPathResolver;

/// Implemented by the transport layer (TauriChatCommandAdapter) to actually
/// dispatch an employee run into a new conversation.
#[async_trait]
pub trait EmployeeRunDispatcher: Send + Sync {
    async fn dispatch_employee_run(
        &self,
        employee: EmployeeRecord,
        fire_at: DateTime<Utc>,
        prompt_override: Option<String>,
        catchup_info: Option<String>,
    ) -> anyhow::Result<String>; // returns conversation_id
}

/// Spawns a background task that checks for due employees every 60 seconds.
pub fn spawn_employee_scheduler(
    path_resolver: Arc<dyn UserScopedPathResolver>,
    dispatcher: Arc<dyn EmployeeRunDispatcher>,
) {
    tauri::async_runtime::spawn(async move {
        let mut interval = time::interval(Duration::from_secs(60));
        loop {
            interval.tick().await;
            let Some(paths) = path_resolver.resolve_paths() else {
                continue;
            };
            let store = EmployeeStore::new(paths.employees_dir());
            match run_due_employees_once(&store, dispatcher.as_ref(), Utc::now()).await {
                Ok(()) => {}
                Err(err) => log::warn!("[EmployeeScheduler] scan failed: {err}"),
            }
        }
    });
}

pub async fn run_due_employees_once(
    store: &EmployeeStore,
    dispatcher: &dyn EmployeeRunDispatcher,
    now: DateTime<Utc>,
) -> anyhow::Result<()> {
    let due_list = store.take_due(now)?;
    for due in due_list {
        let catchup_info = if due.missed_count > 1 {
            Some(format!(
                "（补跑，本次触发时间：{}，跳过了 {} 次）",
                due.fire_at.format("%Y-%m-%d %H:%M"),
                due.missed_count - 1
            ))
        } else {
            None
        };

        let employee_id = due.record.id.clone();
        let fire_at = due.fire_at;

        match dispatcher
            .dispatch_employee_run(due.record, fire_at, None, catchup_info)
            .await
        {
            Ok(_conversation_id) => {
                if let Err(e) = store.record_run(&employee_id, fire_at) {
                    log::warn!("[EmployeeScheduler] failed to record run for {employee_id}: {e}");
                }
            }
            Err(err) => {
                log::warn!("[EmployeeScheduler] dispatch failed for {employee_id}: {err}");
            }
        }
    }
    Ok(())
}
