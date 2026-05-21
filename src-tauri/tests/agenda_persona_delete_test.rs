//! Persona-deletion → agenda Orphan linkage (spec §9 + §1.8).
//!
//! Direct store-level test that mirrors what `TauriPersonaCommandAdapter::
//! delete_persona` invokes: `AgendaStore::mark_orphaned_by_organizer(employee_id)`.
//! When a persona is deleted, items they organized must flip to `Orphaned`
//! (so runner's take_due skips them — only Active is fireable) and remain
//! recoverable by reassigning organizer to a different employee.
//!
//! NOTE: Field renamed from `organizer_persona_id` → `organizer_employee_id`
//! and `persona_id` → `employee_id` in Task 3 of PR-5. Legacy JSON still
//! deserialises via `#[serde(alias)]`; the struct uses the new names.

use chrono::Utc;
use tempfile::TempDir;

use app_lib::runtime::agenda::{AgendaItem, AgendaItemId, AgendaStore, ItemStatus, Participant};

fn make(employee_id: &str) -> AgendaItem {
    let now = Utc::now();
    AgendaItem {
        id: AgendaItemId::new(),
        title: "T".into(),
        prompt: "P".into(),
        organizer_employee_id: employee_id.into(),
        participants: vec![Participant {
            employee_id: employee_id.into(),
            joined_at: now,
        }],
        start_at: now + chrono::Duration::days(1),
        timezone: "UTC".into(),
        rule: None,
        skip_dates: vec![],
        next_fire_at: Some(now + chrono::Duration::days(1)),
        occurrence_count: 0,
        status: ItemStatus::Active,
        override_of: None,
        workspace_path: None,
        created_at: now,
        updated_at: now,
    }
}

#[test]
fn deleting_persona_orphans_their_active_items() {
    let dir = TempDir::new().unwrap();
    let store = AgendaStore::new(dir.path());
    let alice = store.create(make("alice")).unwrap();
    let bob = store.create(make("bob")).unwrap();

    let count = store.mark_orphaned_by_organizer("alice").unwrap();
    assert_eq!(count, 1);
    assert_eq!(store.get(&alice.id).unwrap().status, ItemStatus::Orphaned);
    assert_eq!(store.get(&bob.id).unwrap().status, ItemStatus::Active);
}

#[test]
fn orphaned_items_can_be_revived_by_assigning_new_organizer() {
    let dir = TempDir::new().unwrap();
    let store = AgendaStore::new(dir.path());
    let item = store.create(make("alice")).unwrap();
    store.mark_orphaned_by_organizer("alice").unwrap();

    let mut revived = store.get(&item.id).unwrap();
    revived.organizer_employee_id = "carol".into();
    revived.participants = vec![Participant {
        employee_id: "carol".into(),
        joined_at: Utc::now(),
    }];
    revived.status = ItemStatus::Active;

    let updated = store.update(revived).unwrap();
    assert_eq!(updated.organizer_employee_id, "carol");
    assert_eq!(updated.status, ItemStatus::Active);
}
