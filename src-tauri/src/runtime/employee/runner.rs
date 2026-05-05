use std::sync::Arc;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use tokio::time::{self, Duration};

use crate::runtime::employee::store::{DueEmployee, EmployeeRecord, EmployeeStore};
use crate::storage::UserScopedPathResolver;

/// Source of an employee run trigger.
///
/// Used to control side effects that should differ between user-initiated and
/// scheduler-initiated runs (e.g. desktop notifications, missed-tick catchup
/// labels).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TriggerKind {
    /// User clicked "现在派活" — they are watching the chat view.
    OnDemand,
    /// Cron scheduler fired the run (real-time or catchup).
    Cron,
}

/// Implemented by the transport layer (TauriChatCommandAdapter) to actually
/// dispatch an employee run into a new conversation.
///
/// Returns the `conversation_id` synchronously after creating the conversation
/// and writing a `Running` inbox entry; the agent loop runs in a detached task
/// so the caller (UI or scheduler) can return to the event loop immediately.
#[async_trait]
pub trait EmployeeRunDispatcher: Send + Sync {
    async fn dispatch_employee_run(
        &self,
        employee: EmployeeRecord,
        fire_at: DateTime<Utc>,
        prompt_override: Option<String>,
        catchup_info: Option<String>,
        trigger_kind: TriggerKind,
    ) -> anyhow::Result<String>;
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
            .dispatch_employee_run(due.record, fire_at, None, catchup_info, TriggerKind::Cron)
            .await
        {
            Ok(_conversation_id) => {
                // record_run is called synchronously inside dispatch_employee_run.
            }
            Err(err) => {
                log::warn!("[EmployeeScheduler] dispatch failed for {employee_id}: {err}");
            }
        }
    }
    Ok(())
}
