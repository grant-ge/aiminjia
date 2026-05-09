//! Integration test: switching scope dirs (tenant / user) between agenda
//! runner ticks must surface the new scope's items, not stale ones.
//!
//! This is the runtime end of the architecture rule asserted by
//! `tests/review_agenda_runner_scope.rs` (which inspects spawn_agenda_runner
//! source). Here we drive `run_due_once` directly with two fresh `AgendaStore`
//! instances backed by different temp dirs, simulating what
//! `spawn_agenda_runner` does on every tick: re-resolve scope, build a new
//! store, dispatch only the items in the now-active scope.

use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use chrono::{DateTime, TimeZone, Utc};
use tempfile::TempDir;

use app_lib::runtime::agenda::{
    run_due_once, AgendaItem, AgendaItemId, AgendaRunDispatcher, AgendaStore, ItemStatus,
    Participant, TriggerSource,
};

struct CountingDispatcher {
    count: Mutex<usize>,
}

#[async_trait]
impl AgendaRunDispatcher for CountingDispatcher {
    async fn dispatch(
        &self,
        _item: AgendaItem,
        _planned: DateTime<Utc>,
        _src: TriggerSource,
        _now: DateTime<Utc>,
    ) -> anyhow::Result<String> {
        *self.count.lock().unwrap() += 1;
        Ok("occ-x".into())
    }
}

fn make(persona: &str, when: DateTime<Utc>) -> AgendaItem {
    AgendaItem {
        id: AgendaItemId::new(),
        title: "T".into(),
        prompt: "P".into(),
        organizer_persona_id: persona.into(),
        participants: vec![Participant {
            persona_id: persona.into(),
            joined_at: when,
        }],
        start_at: when,
        timezone: "UTC".into(),
        rule: None,
        skip_dates: vec![],
        next_fire_at: Some(when),
        occurrence_count: 0,
        status: ItemStatus::Active,
        override_of: None,
        workspace_path: None,
        created_at: when,
        updated_at: when,
    }
}

#[tokio::test]
async fn switching_scope_dirs_picks_up_new_items() {
    let dispatcher = Arc::new(CountingDispatcher {
        count: Mutex::new(0),
    });
    let now = Utc.with_ymd_and_hms(2026, 5, 7, 9, 0, 0).unwrap();
    let due_at = Utc.with_ymd_and_hms(2026, 5, 7, 8, 0, 0).unwrap();

    let scope_a = TempDir::new().unwrap();
    let scope_b = TempDir::new().unwrap();
    let store_a = AgendaStore::new(scope_a.path());
    let store_b = AgendaStore::new(scope_b.path());

    store_a.create(make("alice", due_at)).unwrap();
    store_b.create(make("bob", due_at)).unwrap();

    // tick 1: runner sees scope A, dispatches alice's item only
    run_due_once(&store_a, dispatcher.as_ref(), now).await.unwrap();
    assert_eq!(*dispatcher.count.lock().unwrap(), 1);

    // tick 2: runner re-resolves to scope B, dispatches bob's item only
    run_due_once(&store_b, dispatcher.as_ref(), now).await.unwrap();
    assert_eq!(*dispatcher.count.lock().unwrap(), 2);
}
