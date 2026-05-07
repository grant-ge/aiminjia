use std::path::{Path, PathBuf};
use std::sync::Mutex;

use super::item::{AgendaItem, AgendaItemId};
use crate::storage::file_store::io::atomic_write_json;

pub struct AgendaStore {
    pub(crate) root: PathBuf,
    pub(crate) lock: Mutex<()>,
}

impl AgendaStore {
    pub fn new(base_dir: impl AsRef<Path>) -> Self {
        Self {
            root: base_dir.as_ref().join("agenda"),
            lock: Mutex::new(()),
        }
    }

    pub(crate) fn items_dir(&self) -> PathBuf {
        self.root.join("items")
    }

    pub(crate) fn occurrences_dir(&self) -> PathBuf {
        self.root.join("occurrences")
    }

    pub(crate) fn item_path(&self, id: &AgendaItemId) -> PathBuf {
        self.items_dir().join(format!("{}.json", id.as_str()))
    }

    pub(crate) fn occurrence_dir_for(&self, id: &AgendaItemId) -> PathBuf {
        self.occurrences_dir().join(id.as_str())
    }

    pub(crate) fn occurrence_shard_path(
        &self,
        id: &AgendaItemId,
        when: chrono::DateTime<chrono::Utc>,
    ) -> PathBuf {
        let yyyy_mm = when.format("%Y-%m").to_string();
        self.occurrence_dir_for(id).join(format!("{yyyy_mm}.jsonl"))
    }

    pub fn create(&self, item: AgendaItem) -> anyhow::Result<AgendaItem> {
        let _guard = self.lock.lock().unwrap();
        validate_item_id_for_path(&item.id)?;
        validate_phase1_constraints(&item)?;
        std::fs::create_dir_all(self.items_dir())?;
        atomic_write_json(&self.item_path(&item.id), &item)?;
        Ok(item)
    }

    pub fn get(&self, id: &AgendaItemId) -> anyhow::Result<AgendaItem> {
        let _guard = self.lock.lock().unwrap();
        validate_item_id_for_path(id)?;
        let path = self.item_path(id);
        if !path.exists() {
            anyhow::bail!("agenda item not found: {}", id.as_str());
        }
        let bytes = std::fs::read(&path)?;
        Ok(serde_json::from_slice(&bytes)?)
    }

    pub fn list(&self) -> anyhow::Result<Vec<AgendaItem>> {
        let _guard = self.lock.lock().unwrap();
        if !self.items_dir().exists() {
            return Ok(vec![]);
        }
        let mut out = Vec::new();
        for entry in std::fs::read_dir(self.items_dir())? {
            let path = entry?.path();
            if path.extension().and_then(|s| s.to_str()) != Some("json") {
                continue;
            }
            let bytes = std::fs::read(&path)?;
            out.push(serde_json::from_slice(&bytes)?);
        }
        Ok(out)
    }

    pub fn delete(&self, id: &AgendaItemId) -> anyhow::Result<bool> {
        let _guard = self.lock.lock().unwrap();
        validate_item_id_for_path(id)?;
        let path = self.item_path(id);
        if !path.exists() {
            return Ok(false);
        }
        std::fs::remove_file(&path)?;
        Ok(true)
    }
}

fn validate_item_id_for_path(id: &AgendaItemId) -> anyhow::Result<()> {
    let raw = id.as_str();
    if raw.is_empty() || raw == "." || raw == ".." || raw.contains('/') || raw.contains('\\') {
        anyhow::bail!("invalid agenda item id: {}", raw);
    }
    Ok(())
}

pub(crate) fn validate_phase1_constraints(item: &AgendaItem) -> anyhow::Result<()> {
    if item.participants.len() != 1 {
        anyhow::bail!("phase1 constraint: participants.len() must be 1");
    }
    if item.participants[0].persona_id != item.organizer_persona_id {
        anyhow::bail!("phase1 constraint: organizer must equal participants[0]");
    }
    if item.override_of.is_some() {
        anyhow::bail!("phase1 constraint: override_of must be None");
    }
    if let Some(rule) = &item.rule {
        if !rule.by_day.is_empty() || !rule.by_month_day.is_empty() {
            anyhow::bail!("phase1 constraint: rule.by_day / by_month_day must be empty");
        }
    } else if !item.skip_dates.is_empty() {
        anyhow::bail!("phase1 constraint: skip_dates only valid when rule is Some");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn store_paths_under_agenda_subdir() {
        let dir = TempDir::new().unwrap();
        let store = AgendaStore::new(dir.path());
        assert_eq!(store.root, dir.path().join("agenda"));
        assert_eq!(store.items_dir(), dir.path().join("agenda/items"));
        assert_eq!(
            store.occurrences_dir(),
            dir.path().join("agenda/occurrences")
        );
    }

    #[test]
    fn item_path_uses_id_as_filename() {
        let dir = TempDir::new().unwrap();
        let store = AgendaStore::new(dir.path());
        let id = AgendaItemId("agenda-abc".into());
        assert_eq!(
            store.item_path(&id),
            dir.path().join("agenda/items/agenda-abc.json")
        );
    }

    #[test]
    fn occurrence_shard_uses_yyyy_mm() {
        use chrono::TimeZone;
        let dir = TempDir::new().unwrap();
        let store = AgendaStore::new(dir.path());
        let id = AgendaItemId("agenda-x".into());
        let when = chrono::Utc.with_ymd_and_hms(2026, 5, 7, 1, 2, 3).unwrap();
        assert_eq!(
            store.occurrence_shard_path(&id, when),
            dir.path().join("agenda/occurrences/agenda-x/2026-05.jsonl")
        );
    }

    fn make_valid_item(persona: &str) -> super::super::item::AgendaItem {
        use super::super::item::*;
        use chrono::Utc;
        let now = Utc::now();
        AgendaItem {
            id: AgendaItemId::new(),
            title: "T".into(),
            prompt: "P".into(),
            organizer_persona_id: persona.into(),
            participants: vec![Participant {
                persona_id: persona.into(),
                joined_at: now,
            }],
            start_at: now,
            timezone: "Asia/Shanghai".into(),
            rule: None,
            skip_dates: vec![],
            next_fire_at: None,
            occurrence_count: 0,
            status: ItemStatus::Active,
            override_of: None,
            created_at: now,
            updated_at: now,
        }
    }

    #[test]
    fn create_persists_item() {
        let dir = TempDir::new().unwrap();
        let store = AgendaStore::new(dir.path());
        let item = make_valid_item("p1");
        let saved = store.create(item.clone()).unwrap();
        assert_eq!(saved, item);
        assert_eq!(saved.id, item.id);
        assert!(store.item_path(&item.id).exists());
        let persisted: super::super::item::AgendaItem =
            serde_json::from_str(&std::fs::read_to_string(store.item_path(&item.id)).unwrap())
                .unwrap();
        assert_eq!(persisted, item);
    }

    #[test]
    fn rejects_participants_len_not_one() {
        use super::super::item::Participant;
        use chrono::Utc;
        let dir = TempDir::new().unwrap();
        let store = AgendaStore::new(dir.path());
        let mut item = make_valid_item("p1");
        item.participants.push(Participant {
            persona_id: "p2".into(),
            joined_at: Utc::now(),
        });
        let err = store.create(item).unwrap_err();
        assert!(err.to_string().contains("participants"));
    }

    #[test]
    fn rejects_organizer_not_in_participants() {
        let dir = TempDir::new().unwrap();
        let store = AgendaStore::new(dir.path());
        let mut item = make_valid_item("p1");
        item.participants[0].persona_id = "other".into();
        let err = store.create(item).unwrap_err();
        assert!(err.to_string().contains("organizer"));
    }

    #[test]
    fn rejects_override_of_set() {
        use super::super::item::{AgendaItemId, OverrideRef};
        use chrono::Utc;
        let dir = TempDir::new().unwrap();
        let store = AgendaStore::new(dir.path());
        let mut item = make_valid_item("p1");
        item.override_of = Some(OverrideRef {
            series_item_id: AgendaItemId("agenda-x".into()),
            original_at: Utc::now(),
        });
        let err = store.create(item).unwrap_err();
        assert!(err.to_string().contains("override_of"));
    }

    #[test]
    fn rejects_rule_with_by_day() {
        use super::super::item::*;
        let dir = TempDir::new().unwrap();
        let store = AgendaStore::new(dir.path());
        let mut item = make_valid_item("p1");
        item.rule = Some(RecurrenceRule {
            freq: Freq::Weekly,
            interval: 1,
            end_condition: EndCondition::Never,
            by_day: vec![Weekday::Mon],
            by_month_day: vec![],
        });
        let err = store.create(item).unwrap_err();
        assert!(err.to_string().contains("by_day"));
    }

    #[test]
    fn rejects_rule_with_by_month_day() {
        use super::super::item::*;
        let dir = TempDir::new().unwrap();
        let store = AgendaStore::new(dir.path());
        let mut item = make_valid_item("p1");
        item.rule = Some(RecurrenceRule {
            freq: Freq::Monthly,
            interval: 1,
            end_condition: EndCondition::Never,
            by_day: vec![],
            by_month_day: vec![7],
        });
        let err = store.create(item).unwrap_err();
        assert!(err.to_string().contains("by_month_day"));
    }

    #[test]
    fn rejects_skip_dates_on_one_shot() {
        use chrono::Utc;
        let dir = TempDir::new().unwrap();
        let store = AgendaStore::new(dir.path());
        let mut item = make_valid_item("p1");
        item.skip_dates.push(Utc::now());
        let err = store.create(item).unwrap_err();
        assert!(err.to_string().contains("skip_dates"));
    }

    #[test]
    fn get_returns_saved_item() {
        let dir = TempDir::new().unwrap();
        let store = AgendaStore::new(dir.path());
        let saved = store.create(make_valid_item("p1")).unwrap();
        let fetched = store.get(&saved.id).unwrap();
        assert_eq!(fetched.id, saved.id);
    }

    #[test]
    fn get_missing_returns_err() {
        use super::super::item::AgendaItemId;
        let dir = TempDir::new().unwrap();
        let store = AgendaStore::new(dir.path());
        let result = store.get(&AgendaItemId("missing".into()));
        assert!(result.is_err());
    }

    #[test]
    fn list_returns_all() {
        let dir = TempDir::new().unwrap();
        let store = AgendaStore::new(dir.path());
        store.create(make_valid_item("p1")).unwrap();
        store.create(make_valid_item("p2")).unwrap();
        let all = store.list().unwrap();
        assert_eq!(all.len(), 2);
    }

    #[test]
    fn delete_removes_file_returns_true() {
        let dir = TempDir::new().unwrap();
        let store = AgendaStore::new(dir.path());
        let saved = store.create(make_valid_item("p1")).unwrap();
        assert!(store.delete(&saved.id).unwrap());
        assert!(!store.item_path(&saved.id).exists());
    }

    #[test]
    fn delete_missing_returns_false() {
        use super::super::item::AgendaItemId;
        let dir = TempDir::new().unwrap();
        let store = AgendaStore::new(dir.path());
        let result = store.delete(&AgendaItemId("missing".into())).unwrap();
        assert!(!result);
    }

    #[test]
    fn get_rejects_path_traversal_id() {
        use super::super::item::AgendaItemId;
        let dir = TempDir::new().unwrap();
        let store = AgendaStore::new(dir.path());
        let outside = store.root.join("outside.json");
        std::fs::create_dir_all(&store.root).unwrap();
        std::fs::write(&outside, "{}").unwrap();

        let err = store.get(&AgendaItemId("../outside".into())).unwrap_err();
        assert!(err.to_string().contains("invalid agenda item id"));
        assert!(outside.exists());
    }

    #[test]
    fn create_rejects_path_traversal_id_without_writing_outside_file() {
        let dir = TempDir::new().unwrap();
        let store = AgendaStore::new(dir.path());
        let mut item = make_valid_item("p1");
        item.id = super::super::item::AgendaItemId("../outside".into());

        let err = store.create(item).unwrap_err();
        assert!(err.to_string().contains("invalid agenda item id"));
        assert!(!store.root.join("outside.json").exists());
    }

    #[test]
    fn delete_rejects_path_traversal_id_without_removing_outside_file() {
        use super::super::item::AgendaItemId;
        let dir = TempDir::new().unwrap();
        let store = AgendaStore::new(dir.path());
        let outside = store.root.join("outside.json");
        std::fs::create_dir_all(&store.root).unwrap();
        std::fs::write(&outside, "{}").unwrap();

        let err = store.delete(&AgendaItemId("../outside".into())).unwrap_err();
        assert!(err.to_string().contains("invalid agenda item id"));
        assert!(outside.exists());
    }
}
