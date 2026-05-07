use chrono::{Duration, TimeZone, Utc};
use tempfile::TempDir;

use app_lib::runtime::agenda::{
    AgendaItem, AgendaItemId, AgendaStore, EndCondition, Freq, ItemStatus, Occurrence,
    OccurrenceStatus, Participant, RecurrenceRule, TriggerSource,
};
use app_lib::runtime::ids::{RunId, SessionId};

fn make_item(persona: &str, start_at: chrono::DateTime<chrono::Utc>) -> AgendaItem {
    let now = Utc::now();
    AgendaItem {
        id: AgendaItemId::new(),
        title: "测试日程".into(),
        prompt: "做点事".into(),
        organizer_persona_id: persona.into(),
        participants: vec![Participant { persona_id: persona.into(), joined_at: now }],
        start_at,
        timezone: "Asia/Shanghai".into(),
        rule: None,
        skip_dates: vec![],
        next_fire_at: Some(start_at),
        occurrence_count: 0,
        status: ItemStatus::Active,
        override_of: None,
        created_at: now,
        updated_at: now,
    }
}

#[test]
fn create_then_list_includes_item() {
    let dir = TempDir::new().unwrap();
    let store = AgendaStore::new(dir.path());
    let saved = store.create(make_item("p1", Utc::now() + Duration::hours(1))).unwrap();
    let listed = store.list().unwrap();
    assert!(listed.iter().any(|i| i.id == saved.id));
}

#[test]
fn delete_then_list_excludes_item() {
    let dir = TempDir::new().unwrap();
    let store = AgendaStore::new(dir.path());
    let saved = store.create(make_item("p1", Utc::now() + Duration::hours(1))).unwrap();
    assert!(store.delete(&saved.id).unwrap());
    assert!(store.list().unwrap().iter().all(|i| i.id != saved.id));
}

#[test]
fn skip_then_unskip_round_trip() {
    let dir = TempDir::new().unwrap();
    let store = AgendaStore::new(dir.path());
    let mut item = make_item("p1", Utc::now() + Duration::hours(1));
    item.rule = Some(RecurrenceRule {
        freq: Freq::Daily, interval: 1, end_condition: EndCondition::Never,
        by_day: vec![], by_month_day: vec![],
    });
    store.create(item.clone()).unwrap();
    let target = Utc::now() + Duration::days(2);
    let after_skip = store.set_skip(&item.id, target).unwrap();
    assert!(after_skip.skip_dates.contains(&target));
    let after_unskip = store.unset_skip(&item.id, target).unwrap();
    assert!(!after_unskip.skip_dates.contains(&target));
}

#[test]
fn append_occurrence_then_list_returns_running() {
    let dir = TempDir::new().unwrap();
    let store = AgendaStore::new(dir.path());
    let item = store.create(make_item("p1", Utc::now())).unwrap();
    let occ = Occurrence {
        id: Occurrence::new_id(),
        agenda_item_id: item.id.clone(),
        fired_at: Utc::now(),
        planned_fire_at: Utc::now(),
        started_at: Utc::now(),
        finished_at: None,
        primary_persona_id: "p1".into(),
        conversation_id: "conv-1".into(),
        session_id: SessionId::new("conv-1"),
        run_id: RunId::new("run-1"),
        status: OccurrenceStatus::Running,
        error_summary: None,
        trigger_source: TriggerSource::Scheduled,
    };
    store.append_occurrence(&occ).unwrap();
    let listed = store.list_occurrences(&item.id, 10).unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].status, OccurrenceStatus::Running);
}

#[test]
fn append_occurrence_succeeded_overrides_running() {
    let dir = TempDir::new().unwrap();
    let store = AgendaStore::new(dir.path());
    let item = store.create(make_item("p1", Utc::now())).unwrap();
    let id = Occurrence::new_id();
    let mut occ = Occurrence {
        id: id.clone(),
        agenda_item_id: item.id.clone(),
        fired_at: Utc::now(),
        planned_fire_at: Utc::now(),
        started_at: Utc::now(),
        finished_at: None,
        primary_persona_id: "p1".into(),
        conversation_id: "conv-1".into(),
        session_id: SessionId::new("conv-1"),
        run_id: RunId::new("run-1"),
        status: OccurrenceStatus::Running,
        error_summary: None,
        trigger_source: TriggerSource::Scheduled,
    };
    store.append_occurrence(&occ).unwrap();
    occ.status = OccurrenceStatus::Succeeded;
    occ.finished_at = Some(Utc::now());
    store.append_occurrence(&occ).unwrap();
    let listed = store.list_occurrences(&item.id, 10).unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].status, OccurrenceStatus::Succeeded);
}
