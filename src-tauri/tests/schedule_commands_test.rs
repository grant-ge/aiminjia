use tempfile::TempDir;

use app_lib::runtime::schedule::{CreateScheduleRequest, ScheduleStatus, ScheduleStore};
use app_lib::runtime::schedule_runner::{run_due_schedules_once, ScheduleRunDispatcher};
use chrono::Utc;
use std::sync::Mutex;

#[derive(Default)]
struct RecordingDispatcher {
    runs: Mutex<Vec<(String, String, chrono::DateTime<Utc>)>>,
}

#[async_trait::async_trait]
impl ScheduleRunDispatcher for RecordingDispatcher {
    async fn dispatch_schedule_run(
        &self,
        schedule: app_lib::runtime::schedule::ScheduleRecord,
        fire_at: chrono::DateTime<Utc>,
    ) -> anyhow::Result<()> {
        self.runs
            .lock()
            .unwrap()
            .push((schedule.id, schedule.prompt, fire_at));
        Ok(())
    }
}

#[test]
fn schedule_store_creates_lists_and_deletes_persistent_jobs() {
    let dir = TempDir::new().unwrap();
    let store = ScheduleStore::new(dir.path().to_path_buf());

    let created = store
        .create(CreateScheduleRequest {
            title: "日报汇总".to_string(),
            prompt: "汇总昨日数据".to_string(),
            cron: "0 9 * * *".to_string(),
            timezone: Some("Asia/Shanghai".to_string()),
            enabled: Some(true),
        })
        .unwrap();

    assert_eq!(created.title, "日报汇总");
    assert_eq!(created.cron, "0 9 * * *");
    assert_eq!(created.human_schedule, "每天 09:00");
    assert_eq!(created.status, ScheduleStatus::Enabled);
    assert!(created.next_run_at.is_some());

    let reloaded = ScheduleStore::new(dir.path().to_path_buf());
    let listed = reloaded.list().unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].id, created.id);

    assert!(reloaded.delete(&created.id).unwrap());
    assert!(reloaded.list().unwrap().is_empty());
}

#[test]
fn schedule_store_rejects_invalid_cron() {
    let dir = TempDir::new().unwrap();
    let store = ScheduleStore::new(dir.path().to_path_buf());

    let err = store
        .create(CreateScheduleRequest {
            title: "坏任务".to_string(),
            prompt: "noop".to_string(),
            cron: "bad cron".to_string(),
            timezone: None,
            enabled: None,
        })
        .unwrap_err();

    assert!(err.to_string().contains("invalid cron"));
}

#[test]
fn schedule_store_take_due_advances_next_run() {
    let dir = TempDir::new().unwrap();
    let store = ScheduleStore::new(dir.path().to_path_buf());

    let created = store
        .create(CreateScheduleRequest {
            title: "分钟任务".to_string(),
            prompt: "每分钟执行".to_string(),
            cron: "* * * * *".to_string(),
            timezone: None,
            enabled: Some(true),
        })
        .unwrap();

    let future = Utc::now() + chrono::Duration::minutes(2);
    let due = store.take_due(future).unwrap();

    assert_eq!(due.len(), 1);
    assert_eq!(due[0].record.id, created.id);
    assert!(store.list().unwrap()[0].next_run_at.unwrap() > future);
}

#[tokio::test]
async fn schedule_runner_dispatches_due_job_end_to_end() {
    let dir = TempDir::new().unwrap();
    let store = ScheduleStore::new(dir.path().to_path_buf());
    let dispatcher = RecordingDispatcher::default();

    let created = store
        .create(CreateScheduleRequest {
            title: "端到端任务".to_string(),
            prompt: "到点后进入调度器".to_string(),
            cron: "* * * * *".to_string(),
            timezone: Some("Asia/Shanghai".to_string()),
            enabled: Some(true),
        })
        .unwrap();

    let future = Utc::now() + chrono::Duration::minutes(2);
    run_due_schedules_once(&store, &dispatcher, future)
        .await
        .unwrap();

    let runs = dispatcher.runs.lock().unwrap();
    assert_eq!(runs.len(), 1);
    assert_eq!(runs[0].0, created.id);
    assert_eq!(runs[0].1, "到点后进入调度器");
    drop(runs);

    let listed = store.list().unwrap();
    assert_eq!(listed.len(), 1);
    assert!(listed[0].next_run_at.unwrap() > future);
}
