use std::path::{Path, PathBuf};
use std::sync::Mutex;

use super::item::AgendaItemId;

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
}
