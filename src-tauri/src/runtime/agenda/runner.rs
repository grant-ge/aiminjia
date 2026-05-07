use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use chrono::{DateTime, Utc};
use tokio::time;

use super::dispatcher::AgendaRunDispatcher;
use super::occurrence::TriggerSource;
use super::store::AgendaStore;
use crate::storage::UserScopedPathResolver;

pub fn spawn_agenda_runner(
    path_resolver: Arc<dyn UserScopedPathResolver>,
    dispatcher: Arc<dyn AgendaRunDispatcher>,
) {
    tauri::async_runtime::spawn(async move {
        let mut interval = time::interval(Duration::from_secs(60));
        loop {
            interval.tick().await;
            let Some(paths) = path_resolver.resolve_paths() else { continue; };
            let store = AgendaStore::new(paths.base_dir());
            if let Err(e) = run_due_once(&store, dispatcher.as_ref(), Utc::now()).await {
                log::warn!("agenda runner tick failed: {e}");
            }
        }
    });
}

pub async fn run_due_once(
    store: &AgendaStore,
    dispatcher: &dyn AgendaRunDispatcher,
    now: DateTime<Utc>,
) -> Result<()> {
    let due = store.take_due(now)?;
    for item in due {
        let planned = item.next_fire_at.unwrap_or(now);
        if let Err(e) = dispatcher
            .dispatch(item.clone(), planned, TriggerSource::Scheduled, now)
            .await
        {
            log::warn!("agenda dispatch failed for {}: {e}", item.id.as_str());
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;
    use tempfile::TempDir;

    struct RecordingDispatcher {
        calls: std::sync::Mutex<Vec<String>>,
    }

    #[async_trait::async_trait]
    impl AgendaRunDispatcher for RecordingDispatcher {
        async fn dispatch(
            &self,
            item: super::super::item::AgendaItem,
            _planned: DateTime<Utc>,
            _src: TriggerSource,
            _now: DateTime<Utc>,
        ) -> anyhow::Result<String> {
            self.calls.lock().unwrap().push(item.id.as_str().to_string());
            Ok("occ-test".into())
        }
    }

    fn make_active_due_item(persona: &str, when: DateTime<Utc>) -> super::super::item::AgendaItem {
        use super::super::item::*;
        AgendaItem {
            id: AgendaItemId::new(),
            title: "T".into(),
            prompt: "P".into(),
            organizer_persona_id: persona.into(),
            participants: vec![Participant { persona_id: persona.into(), joined_at: when }],
            start_at: when,
            timezone: "UTC".into(),
            rule: None,
            skip_dates: vec![],
            next_fire_at: Some(when),
            occurrence_count: 0,
            status: ItemStatus::Active,
            override_of: None,
            created_at: when,
            updated_at: when,
        }
    }

    #[tokio::test]
    async fn run_due_once_dispatches_active_items() {
        let dir = TempDir::new().unwrap();
        let store = AgendaStore::new(dir.path());
        let due_at = Utc.with_ymd_and_hms(2026, 5, 7, 8, 0, 0).unwrap();
        store.create(make_active_due_item("p1", due_at)).unwrap();

        let dispatcher = RecordingDispatcher { calls: Default::default() };
        let now = Utc.with_ymd_and_hms(2026, 5, 7, 9, 0, 0).unwrap();
        run_due_once(&store, &dispatcher, now).await.unwrap();
        assert_eq!(dispatcher.calls.lock().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn run_due_once_skips_when_no_due() {
        let dir = TempDir::new().unwrap();
        let store = AgendaStore::new(dir.path());
        let dispatcher = RecordingDispatcher { calls: Default::default() };
        let now = Utc::now();
        run_due_once(&store, &dispatcher, now).await.unwrap();
        assert_eq!(dispatcher.calls.lock().unwrap().len(), 0);
    }
}
