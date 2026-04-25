use std::sync::Arc;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use tokio::time::{self, Duration};

use crate::runtime::schedule::{ScheduleRecord, ScheduleStore};

#[async_trait]
pub trait ScheduleRunDispatcher: Send + Sync {
    async fn dispatch_schedule_run(
        &self,
        schedule: ScheduleRecord,
        fire_at: DateTime<Utc>,
    ) -> anyhow::Result<()>;
}

pub fn spawn_schedule_runner(
    aijia_home: Arc<crate::storage::AiJiaHome>,
    dispatcher: Arc<dyn ScheduleRunDispatcher>,
) {
    tauri::async_runtime::spawn(async move {
        let store = ScheduleStore::new(aijia_home.root().to_path_buf());
        let mut interval = time::interval(Duration::from_secs(60));
        loop {
            interval.tick().await;
            match run_due_schedules_once(&store, dispatcher.as_ref(), Utc::now()).await {
                Ok(()) => {}
                Err(err) => log::warn!("schedule runner failed to scan schedules: {err}"),
            }
        }
    });
}

pub async fn run_due_schedules_once(
    store: &ScheduleStore,
    dispatcher: &dyn ScheduleRunDispatcher,
    now: DateTime<Utc>,
) -> anyhow::Result<()> {
    for due_schedule in store.take_due(now)? {
        dispatcher
            .dispatch_schedule_run(due_schedule.record, due_schedule.fire_at)
            .await?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    use crate::runtime::schedule::{CreateScheduleRequest, ScheduleStore};

    #[test]
    fn take_due_advances_next_run_once() {
        let dir = TempDir::new().unwrap();
        let store = ScheduleStore::new(dir.path().to_path_buf());
        let created = store
            .create(CreateScheduleRequest {
                title: "日报汇总".to_string(),
                prompt: "汇总昨日数据".to_string(),
                cron: "* * * * *".to_string(),
                timezone: None,
                enabled: Some(true),
            })
            .unwrap();

        let future = Utc::now() + chrono::Duration::minutes(2);
        let due = store.take_due(future).unwrap();
        assert_eq!(due.len(), 1);
        assert_eq!(due[0].record.id, created.id);

        let listed = store.list().unwrap();
        assert!(listed[0].next_run_at.unwrap() > future);
    }
}
