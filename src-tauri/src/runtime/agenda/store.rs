use std::path::{Path, PathBuf};
use std::sync::Mutex;

use super::item::{AgendaItem, AgendaItemId};
use super::occurrence::Occurrence;
use super::trigger_eval::compute_next_fire_at;
use crate::storage::file_store::io::atomic_write_json;
use std::io::Write;

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

    pub fn update(&self, item: AgendaItem) -> anyhow::Result<AgendaItem> {
        let _guard = self.lock.lock().unwrap();
        validate_item_id_for_path(&item.id)?;
        validate_phase1_constraints(&item)?;
        let path = self.item_path(&item.id);
        if !path.exists() {
            anyhow::bail!("agenda item not found: {}", item.id.as_str());
        }
        let prev: AgendaItem = serde_json::from_slice(&std::fs::read(&path)?)?;
        if prev.organizer_persona_id != item.organizer_persona_id
            && prev.status != super::item::ItemStatus::Orphaned
        {
            anyhow::bail!(
                "phase1 constraint: organizer can only change when status was Orphaned"
            );
        }
        atomic_write_json(&path, &item)?;
        Ok(item)
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
        // 顺手清理 atomic-write 留下的 .bak 备份与孤儿 occurrence 历史
        let bak = path.with_extension("json.bak");
        let _ = std::fs::remove_file(&bak);
        let occ_path = self.occurrences_dir().join(id.as_str());
        let _ = std::fs::remove_file(&occ_path);
        Ok(true)
    }

    /// 软删除：把 status 切到 Cancelled，保留磁盘数据；下次 list 仍能看到，
    /// 但 runner 的 take_due 只接 Active，不会再触发。
    pub fn cancel(
        &self,
        id: &AgendaItemId,
        now: chrono::DateTime<chrono::Utc>,
    ) -> anyhow::Result<AgendaItem> {
        use super::item::ItemStatus;
        let _guard = self.lock.lock().unwrap();
        validate_item_id_for_path(id)?;
        let path = self.item_path(id);
        let bytes = std::fs::read(&path)?;
        let mut item: AgendaItem = serde_json::from_slice(&bytes)?;
        item.status = ItemStatus::Cancelled;
        item.next_fire_at = None;
        item.updated_at = now;
        atomic_write_json(&path, &item)?;
        Ok(item)
    }

    /// 从 Cancelled 恢复到 Active，重算 next_fire_at。
    pub fn restore(
        &self,
        id: &AgendaItemId,
        now: chrono::DateTime<chrono::Utc>,
    ) -> anyhow::Result<AgendaItem> {
        use super::item::ItemStatus;
        let _guard = self.lock.lock().unwrap();
        validate_item_id_for_path(id)?;
        let path = self.item_path(id);
        let bytes = std::fs::read(&path)?;
        let mut item: AgendaItem = serde_json::from_slice(&bytes)?;
        if !matches!(item.status, ItemStatus::Cancelled) {
            anyhow::bail!("item is not cancelled, cannot restore");
        }
        item.status = ItemStatus::Active;
        item.updated_at = now;
        item.next_fire_at = compute_next_fire_at(&item, now);
        atomic_write_json(&path, &item)?;
        Ok(item)
    }

    pub fn mark_orphaned_by_organizer(&self, persona_id: &str) -> anyhow::Result<usize> {
        use super::item::ItemStatus;
        let _guard = self.lock.lock().unwrap();
        let mut count = 0;
        if !self.items_dir().exists() {
            return Ok(0);
        }
        for entry in std::fs::read_dir(self.items_dir())? {
            let path = entry?.path();
            if path.extension().and_then(|s| s.to_str()) != Some("json") {
                continue;
            }
            let bytes = std::fs::read(&path)?;
            let mut item: AgendaItem = serde_json::from_slice(&bytes)?;
            if item.organizer_persona_id != persona_id {
                continue;
            }
            if matches!(item.status, ItemStatus::Active | ItemStatus::Paused) {
                item.status = ItemStatus::Orphaned;
                item.updated_at = chrono::Utc::now();
                atomic_write_json(&path, &item)?;
                count += 1;
            }
        }
        Ok(count)
    }

    pub fn append_occurrence(&self, occ: &Occurrence) -> anyhow::Result<()> {
        let _guard = self.lock.lock().unwrap();
        validate_item_id_for_path(&occ.agenda_item_id)?;
        let path = self.occurrence_shard_path(&occ.agenda_item_id, occ.fired_at);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut file = std::fs::OpenOptions::new()
            .append(true)
            .create(true)
            .open(&path)?;
        let line = serde_json::to_string(occ)?;
        writeln!(file, "{}", line)?;
        Ok(())
    }

    pub fn list_occurrences(
        &self,
        item_id: &super::item::AgendaItemId,
        limit: usize,
    ) -> anyhow::Result<Vec<Occurrence>> {
        let _guard = self.lock.lock().unwrap();
        validate_item_id_for_path(item_id)?;
        let dir = self.occurrence_dir_for(item_id);
        if !dir.exists() {
            return Ok(vec![]);
        }
        let mut latest: std::collections::HashMap<String, Occurrence> = Default::default();
        let mut shards: Vec<_> = std::fs::read_dir(&dir)?
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| p.extension().and_then(|s| s.to_str()) == Some("jsonl"))
            .collect();
        shards.sort();
        for shard in shards {
            let bytes = std::fs::read(&shard)?;
            for line in bytes.split(|b| *b == b'\n') {
                if line.is_empty() {
                    continue;
                }
                let occ: Occurrence = serde_json::from_slice(line)?;
                latest.insert(occ.id.clone(), occ);
            }
        }
        let mut out: Vec<Occurrence> = latest.into_values().collect();
        out.sort_by(|a, b| b.fired_at.cmp(&a.fired_at));
        out.truncate(limit);
        Ok(out)
    }

    pub fn take_due(
        &self,
        now: chrono::DateTime<chrono::Utc>,
    ) -> anyhow::Result<Vec<AgendaItem>> {
        use super::item::ItemStatus;
        let _guard = self.lock.lock().unwrap();
        let mut out = Vec::new();
        if !self.items_dir().exists() {
            return Ok(vec![]);
        }
        for entry in std::fs::read_dir(self.items_dir())? {
            let path = entry?.path();
            if path.extension().and_then(|s| s.to_str()) != Some("json") {
                continue;
            }
            let bytes = std::fs::read(&path)?;
            let item: AgendaItem = serde_json::from_slice(&bytes)?;
            if !matches!(item.status, ItemStatus::Active) {
                continue;
            }
            if item.override_of.is_some() {
                continue;
            }
            if validate_phase1_constraints(&item).is_err() {
                continue;
            }
            if let Some(next) = item.next_fire_at {
                if next <= now {
                    out.push(item);
                }
            }
        }
        Ok(out)
    }

    pub fn advance_after_fire(
        &self,
        id: &super::item::AgendaItemId,
        now: chrono::DateTime<chrono::Utc>,
    ) -> anyhow::Result<AgendaItem> {
        use super::item::ItemStatus;
        let _guard = self.lock.lock().unwrap();
        validate_item_id_for_path(id)?;
        let path = self.item_path(id);
        if !path.exists() {
            anyhow::bail!("agenda item not found: {}", id.as_str());
        }
        let mut item: AgendaItem = serde_json::from_slice(&std::fs::read(&path)?)?;
        if !matches!(item.status, ItemStatus::Active) {
            anyhow::bail!("agenda item not active: {}", id.as_str());
        }
        validate_phase1_constraints(&item)?;
        item.occurrence_count += 1;
        item.next_fire_at = compute_next_fire_at(&item, now);
        if item.next_fire_at.is_none() {
            item.status = ItemStatus::Completed;
        }
        item.updated_at = chrono::Utc::now();
        atomic_write_json(&path, &item)?;
        Ok(item)
    }

    pub fn set_skip(
        &self,
        id: &super::item::AgendaItemId,
        at: chrono::DateTime<chrono::Utc>,
    ) -> anyhow::Result<AgendaItem> {
        let _guard = self.lock.lock().unwrap();
        validate_item_id_for_path(id)?;
        let path = self.item_path(id);
        if !path.exists() {
            anyhow::bail!("agenda item not found: {}", id.as_str());
        }
        let mut item: AgendaItem = serde_json::from_slice(&std::fs::read(&path)?)?;
        if item.rule.is_none() {
            anyhow::bail!("skip_dates only valid when rule is Some");
        }
        if !item.skip_dates.contains(&at) {
            item.skip_dates.push(at);
        }
        item.next_fire_at = compute_next_fire_at(&item, chrono::Utc::now());
        item.updated_at = chrono::Utc::now();
        atomic_write_json(&path, &item)?;
        Ok(item)
    }

    pub fn unset_skip(
        &self,
        id: &super::item::AgendaItemId,
        at: chrono::DateTime<chrono::Utc>,
    ) -> anyhow::Result<AgendaItem> {
        let _guard = self.lock.lock().unwrap();
        validate_item_id_for_path(id)?;
        let path = self.item_path(id);
        if !path.exists() {
            anyhow::bail!("agenda item not found: {}", id.as_str());
        }
        let mut item: AgendaItem = serde_json::from_slice(&std::fs::read(&path)?)?;
        item.skip_dates.retain(|d| d != &at);
        item.next_fire_at = compute_next_fire_at(&item, chrono::Utc::now());
        item.updated_at = chrono::Utc::now();
        atomic_write_json(&path, &item)?;
        Ok(item)
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
            workspace_path: None,
            created_at: now,
            updated_at: now,
        }
    }

    fn make_running_occurrence(item_id: &super::super::item::AgendaItemId) -> super::super::occurrence::Occurrence {
        use chrono::Utc;
        use super::super::occurrence::*;
        use crate::runtime::ids::{RunId, SessionId};
        let now = Utc::now();
        Occurrence {
            id: Occurrence::new_id(),
            agenda_item_id: item_id.clone(),
            fired_at: now,
            planned_fire_at: now,
            started_at: now,
            finished_at: None,
            primary_persona_id: "p1".into(),
            conversation_id: "conv-x".into(),
            session_id: SessionId::new("conv-x"),
            run_id: RunId::new("run-y"),
            status: OccurrenceStatus::Running,
            error_summary: None,
            trigger_source: TriggerSource::Scheduled,
        }
    }

    #[test]
    fn append_occurrence_creates_jsonl_shard() {
        let dir = TempDir::new().unwrap();
        let store = AgendaStore::new(dir.path());
        let item = store.create(make_valid_item("p1")).unwrap();
        let occ = make_running_occurrence(&item.id);
        store.append_occurrence(&occ).unwrap();
        assert!(store.occurrence_shard_path(&item.id, occ.fired_at).exists());
    }

    #[test]
    fn read_occurrences_returns_last_state_per_id() {
        use super::super::occurrence::OccurrenceStatus;
        use chrono::Utc;
        let dir = TempDir::new().unwrap();
        let store = AgendaStore::new(dir.path());
        let item = store.create(make_valid_item("p1")).unwrap();

        let running = make_running_occurrence(&item.id);
        store.append_occurrence(&running).unwrap();

        let mut completed = running.clone();
        completed.status = OccurrenceStatus::Succeeded;
        completed.finished_at = Some(Utc::now());
        store.append_occurrence(&completed).unwrap();

        let occs = store.list_occurrences(&item.id, 10).unwrap();
        assert_eq!(occs.len(), 1);
        assert_eq!(occs[0].status, OccurrenceStatus::Succeeded);
        assert!(occs[0].finished_at.is_some());
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
    fn update_persists_changes() {
        let dir = TempDir::new().unwrap();
        let store = AgendaStore::new(dir.path());
        let mut saved = store.create(make_valid_item("p1")).unwrap();
        saved.title = "new title".into();
        let updated = store.update(saved.clone()).unwrap();
        assert_eq!(updated.title, "new title");
        assert_eq!(store.get(&saved.id).unwrap().title, "new title");
    }

    #[test]
    fn update_rejects_organizer_change_when_not_orphaned() {
        use super::super::item::Participant;
        use chrono::Utc;
        let dir = TempDir::new().unwrap();
        let store = AgendaStore::new(dir.path());
        let saved = store.create(make_valid_item("p1")).unwrap();
        let mut modified = saved.clone();
        modified.organizer_persona_id = "p2".into();
        modified.participants = vec![Participant {
            persona_id: "p2".into(),
            joined_at: Utc::now(),
        }];
        let err = store.update(modified).unwrap_err();
        assert!(err.to_string().contains("organizer"));
    }

    #[test]
    fn update_allows_organizer_change_when_orphaned() {
        use super::super::item::{ItemStatus, Participant};
        use chrono::Utc;
        let dir = TempDir::new().unwrap();
        let store = AgendaStore::new(dir.path());
        let mut saved = store.create(make_valid_item("p1")).unwrap();
        saved.status = ItemStatus::Orphaned;
        store.update(saved.clone()).unwrap();

        let mut revived = saved.clone();
        revived.organizer_persona_id = "p2".into();
        revived.participants = vec![Participant {
            persona_id: "p2".into(),
            joined_at: Utc::now(),
        }];
        revived.status = ItemStatus::Active;
        let updated = store.update(revived).unwrap();
        assert_eq!(updated.organizer_persona_id, "p2");
        assert_eq!(updated.status, ItemStatus::Active);
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
    fn mark_orphaned_flips_status_for_matching_organizer() {
        use super::super::item::ItemStatus;
        let dir = TempDir::new().unwrap();
        let store = AgendaStore::new(dir.path());
        let i1 = store.create(make_valid_item("alice")).unwrap();
        let i2 = store.create(make_valid_item("bob")).unwrap();
        let count = store.mark_orphaned_by_organizer("alice").unwrap();
        assert_eq!(count, 1);
        assert_eq!(store.get(&i1.id).unwrap().status, ItemStatus::Orphaned);
        assert_eq!(store.get(&i2.id).unwrap().status, ItemStatus::Active);
    }

    #[test]
    fn mark_orphaned_skips_already_completed() {
        use super::super::item::ItemStatus;
        let dir = TempDir::new().unwrap();
        let store = AgendaStore::new(dir.path());
        let mut item = make_valid_item("alice");
        item.status = ItemStatus::Completed;
        store.create(item.clone()).unwrap();
        let count = store.mark_orphaned_by_organizer("alice").unwrap();
        assert_eq!(count, 0);
        assert_eq!(store.get(&item.id).unwrap().status, ItemStatus::Completed);
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

    #[test]
    fn append_occurrence_rejects_path_traversal_item_id_without_writing_outside_dir() {
        let dir = TempDir::new().unwrap();
        let store = AgendaStore::new(dir.path());
        let unsafe_id = super::super::item::AgendaItemId("../outside".into());
        let occ = make_running_occurrence(&unsafe_id);

        let err = store.append_occurrence(&occ).unwrap_err();
        assert!(err.to_string().contains("invalid agenda item id"));
        assert!(!store.root.join("outside").exists());
    }

    #[test]
    fn list_occurrences_rejects_path_traversal_item_id() {
        let dir = TempDir::new().unwrap();
        let store = AgendaStore::new(dir.path());
        let unsafe_id = super::super::item::AgendaItemId("../outside".into());

        let err = store.list_occurrences(&unsafe_id, 10).unwrap_err();
        assert!(err.to_string().contains("invalid agenda item id"));
    }

    #[test]
    fn take_due_returns_active_items_with_past_next_fire_at() {
        use chrono::{TimeZone, Utc};
        use super::super::item::ItemStatus;
        let dir = TempDir::new().unwrap();
        let store = AgendaStore::new(dir.path());
        let mut item = make_valid_item("p1");
        item.next_fire_at = Some(Utc.with_ymd_and_hms(2026, 5, 7, 8, 0, 0).unwrap());
        item.status = ItemStatus::Active;
        store.create(item.clone()).unwrap();

        let now = Utc.with_ymd_and_hms(2026, 5, 7, 9, 0, 0).unwrap();
        let due = store.take_due(now).unwrap();
        assert_eq!(due.len(), 1);
        assert_eq!(due[0].id, item.id);
    }

    #[test]
    fn take_due_skips_paused_completed_orphaned() {
        use chrono::{TimeZone, Utc};
        use super::super::item::ItemStatus;
        let dir = TempDir::new().unwrap();
        let store = AgendaStore::new(dir.path());
        let now = Utc.with_ymd_and_hms(2026, 5, 7, 9, 0, 0).unwrap();
        let past = Utc.with_ymd_and_hms(2026, 5, 7, 8, 0, 0).unwrap();
        for status in [ItemStatus::Paused, ItemStatus::Completed, ItemStatus::Orphaned] {
            let mut item = make_valid_item("p1");
            item.next_fire_at = Some(past);
            item.status = status;
            store.create(item).unwrap();
        }
        let due = store.take_due(now).unwrap();
        assert_eq!(due.len(), 0);
    }

    #[test]
    fn advance_after_fire_increments_count_and_recomputes() {
        use chrono::{TimeZone, Utc};
        use super::super::item::*;
        let dir = TempDir::new().unwrap();
        let store = AgendaStore::new(dir.path());
        let start = Utc.with_ymd_and_hms(2026, 5, 7, 9, 0, 0).unwrap();
        let mut item = make_valid_item("p1");
        item.start_at = start;
        item.next_fire_at = Some(start);
        item.rule = Some(RecurrenceRule {
            freq: Freq::Daily, interval: 1, end_condition: EndCondition::Never,
            by_day: vec![], by_month_day: vec![],
        });
        store.create(item.clone()).unwrap();

        let now = Utc.with_ymd_and_hms(2026, 5, 7, 9, 0, 1).unwrap();
        let updated = store.advance_after_fire(&item.id, now).unwrap();
        assert_eq!(updated.occurrence_count, 1);
        assert_eq!(
            updated.next_fire_at,
            Some(Utc.with_ymd_and_hms(2026, 5, 8, 9, 0, 0).unwrap())
        );
        assert_eq!(updated.status, ItemStatus::Active);
    }

    #[test]
    fn advance_after_fire_one_shot_marks_completed() {
        use chrono::{TimeZone, Utc};
        use super::super::item::ItemStatus;
        let dir = TempDir::new().unwrap();
        let store = AgendaStore::new(dir.path());
        let start = Utc.with_ymd_and_hms(2026, 5, 7, 9, 0, 0).unwrap();
        let mut item = make_valid_item("p1");
        item.start_at = start;
        item.next_fire_at = Some(start);
        store.create(item.clone()).unwrap();

        let now = Utc.with_ymd_and_hms(2026, 5, 7, 9, 0, 1).unwrap();
        let updated = store.advance_after_fire(&item.id, now).unwrap();
        assert_eq!(updated.occurrence_count, 1);
        assert_eq!(updated.next_fire_at, None);
        assert_eq!(updated.status, ItemStatus::Completed);
    }

    #[test]
    fn advance_after_fire_rejects_non_active_without_mutating() {
        use chrono::{TimeZone, Utc};
        use super::super::item::ItemStatus;
        let dir = TempDir::new().unwrap();
        let store = AgendaStore::new(dir.path());
        let start = Utc.with_ymd_and_hms(2026, 5, 7, 9, 0, 0).unwrap();
        let mut item = make_valid_item("p1");
        item.start_at = start;
        item.next_fire_at = Some(start);
        item.status = ItemStatus::Paused;
        store.create(item.clone()).unwrap();

        let now = Utc.with_ymd_and_hms(2026, 5, 7, 9, 0, 1).unwrap();
        let err = store.advance_after_fire(&item.id, now).unwrap_err();
        assert!(err.to_string().contains("not active"));
        let stored = store.get(&item.id).unwrap();
        assert_eq!(stored.occurrence_count, 0);
        assert_eq!(stored.next_fire_at, Some(start));
        assert_eq!(stored.status, ItemStatus::Paused);
    }

    #[test]
    fn set_skip_adds_to_skip_dates() {
        use chrono::{TimeZone, Utc};
        use super::super::item::*;
        let dir = TempDir::new().unwrap();
        let store = AgendaStore::new(dir.path());
        let mut item = make_valid_item("p1");
        item.rule = Some(RecurrenceRule {
            freq: Freq::Daily, interval: 1, end_condition: EndCondition::Never,
            by_day: vec![], by_month_day: vec![],
        });
        store.create(item.clone()).unwrap();
        let when = Utc.with_ymd_and_hms(2026, 5, 8, 9, 0, 0).unwrap();
        let updated = store.set_skip(&item.id, when).unwrap();
        assert!(updated.skip_dates.contains(&when));
    }

    #[test]
    fn unset_skip_removes_from_skip_dates() {
        use chrono::{TimeZone, Utc};
        use super::super::item::*;
        let dir = TempDir::new().unwrap();
        let store = AgendaStore::new(dir.path());
        let mut item = make_valid_item("p1");
        item.rule = Some(RecurrenceRule {
            freq: Freq::Daily, interval: 1, end_condition: EndCondition::Never,
            by_day: vec![], by_month_day: vec![],
        });
        let when = Utc.with_ymd_and_hms(2026, 5, 8, 9, 0, 0).unwrap();
        item.skip_dates.push(when);
        store.create(item.clone()).unwrap();
        let updated = store.unset_skip(&item.id, when).unwrap();
        assert!(!updated.skip_dates.contains(&when));
    }

    #[test]
    fn set_skip_rejects_one_shot() {
        use chrono::Utc;
        let dir = TempDir::new().unwrap();
        let store = AgendaStore::new(dir.path());
        let item = store.create(make_valid_item("p1")).unwrap();
        let err = store.set_skip(&item.id, Utc::now()).unwrap_err();
        assert!(err.to_string().contains("rule"));
    }

}
