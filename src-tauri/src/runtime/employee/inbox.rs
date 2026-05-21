use std::fs::{self, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::runtime::employee::store::EmployeeLifecycle;

/// Returns true when the employee.json next to this inbox.jsonl exists and
/// has lifecycle=Archived. Used by `list_all` and `unread_count` to skip
/// archived employees from global aggregations — their reports / signals
/// shouldn't surface in the sidebar badge or 汇报中心 list. Returns false
/// on any read/parse failure (be permissive: don't lose data on a corrupt
/// employee.json).
fn is_archived_dir(employee_dir: &Path) -> bool {
    let employee_json = employee_dir.join("employee.json");
    let Ok(content) = fs::read_to_string(&employee_json) else {
        return false;
    };
    let Ok(value) = serde_json::from_str::<serde_json::Value>(&content) else {
        return false;
    };
    matches!(
        value.get("lifecycle").and_then(|v| v.as_str()),
        Some("archived")
    ) || matches!(
        // Defensive: also handle the typed enum form in case serde changes.
        serde_json::from_value::<EmployeeLifecycle>(
            value.get("lifecycle").cloned().unwrap_or_default(),
        ),
        Ok(EmployeeLifecycle::Archived)
    )
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum InboxKind {
    Report,
    Signal,
    Running,
    Error,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InboxEntry {
    pub id: String,
    pub employee_id: String,
    pub kind: InboxKind,
    pub title: String,
    pub summary: Option<String>,
    /// Relative path from employee dir to the report file, e.g. "reports/2026-04-30.md"
    pub report_path: Option<String>,
    pub conversation_id: Option<String>,
    pub read: bool,
    pub catchup_info: Option<String>,
    pub created_at: DateTime<Utc>,
}

/// Manages the inbox.jsonl for a single employee directory.
#[derive(Debug)]
pub struct InboxStore {
    employees_root: PathBuf,
    lock: Mutex<()>,
}

impl InboxStore {
    pub fn new(employees_root: PathBuf) -> Self {
        Self {
            employees_root,
            lock: Mutex::new(()),
        }
    }

    fn inbox_path(&self, employee_id: &str) -> PathBuf {
        self.employees_root.join(employee_id).join("inbox.jsonl")
    }

    pub fn append(&self, entry: InboxEntry) -> Result<InboxEntry> {
        let _guard = self.lock.lock().unwrap();
        let path = self.inbox_path(&entry.employee_id);
        fs::create_dir_all(path.parent().unwrap())?;

        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .with_context(|| format!("failed to open inbox: {}", path.display()))?;

        let line = serde_json::to_string(&entry)?;
        writeln!(file, "{}", line)?;
        Ok(entry)
    }

    /// Appends a new entry with a generated id and returns it.
    pub fn push(
        &self,
        employee_id: &str,
        kind: InboxKind,
        title: String,
        summary: Option<String>,
        report_path: Option<String>,
        conversation_id: Option<String>,
        catchup_info: Option<String>,
    ) -> Result<InboxEntry> {
        let entry = InboxEntry {
            id: format!("inbox-{}", Uuid::new_v4()),
            employee_id: employee_id.to_string(),
            kind,
            title,
            summary,
            report_path,
            conversation_id,
            read: false,
            catchup_info,
            created_at: Utc::now(),
        };
        self.append(entry)
    }

    /// Reads all entries for a specific employee, newest first.
    pub fn list_for(&self, employee_id: &str, limit: usize) -> Result<Vec<InboxEntry>> {
        let _guard = self.lock.lock().unwrap();
        let path = self.inbox_path(employee_id);
        self.read_entries_from(&path, limit)
    }

    /// Reads all entries across all employees, merged and sorted newest first.
    pub fn list_all(&self, limit: usize) -> Result<Vec<InboxEntry>> {
        let _guard = self.lock.lock().unwrap();
        if !self.employees_root.exists() {
            return Ok(Vec::new());
        }
        let mut all = Vec::new();
        for entry in fs::read_dir(&self.employees_root)? {
            let dir = entry?.path();
            if is_archived_dir(&dir) {
                continue;
            }
            let inbox_path = dir.join("inbox.jsonl");
            if !inbox_path.exists() {
                continue;
            }
            let entries = self.read_entries_from(&inbox_path, usize::MAX)?;
            all.extend(entries);
        }
        all.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        all.truncate(limit);
        Ok(all)
    }

    fn read_entries_from(&self, path: &PathBuf, limit: usize) -> Result<Vec<InboxEntry>> {
        if !path.exists() {
            return Ok(Vec::new());
        }
        let file = fs::File::open(path)?;
        let reader = BufReader::new(file);
        let mut entries: Vec<InboxEntry> = reader
            .lines()
            .filter_map(|line| {
                let line = line.ok()?;
                if line.trim().is_empty() {
                    return None;
                }
                match serde_json::from_str(&line) {
                    Ok(e) => Some(e),
                    Err(err) => {
                        log::warn!("[InboxStore] parse error in {}: {err}", path.display());
                        None
                    }
                }
            })
            .collect();
        // Newest first
        entries.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        entries.truncate(limit);
        Ok(entries)
    }

    pub fn mark_read(&self, employee_id: &str, entry_id: &str) -> Result<bool> {
        self.update_entry(employee_id, |e| {
            if e.id == entry_id {
                e.read = true;
                true
            } else {
                false
            }
        })
    }

    pub fn mark_all_read(&self, employee_id: &str) -> Result<u32> {
        let _guard = self.lock.lock().unwrap();
        let path = self.inbox_path(employee_id);
        if !path.exists() {
            return Ok(0);
        }
        let file = fs::File::open(&path)?;
        let reader = BufReader::new(file);
        let mut count = 0u32;
        let new_content: Vec<String> = reader
            .lines()
            .filter_map(|l| l.ok())
            .filter(|l| !l.trim().is_empty())
            .map(|line| match serde_json::from_str::<InboxEntry>(&line) {
                Ok(mut e) if !e.read => {
                    e.read = true;
                    count += 1;
                    serde_json::to_string(&e).unwrap_or(line)
                }
                _ => line,
            })
            .collect();
        fs::write(&path, new_content.join("\n") + "\n")?;
        Ok(count)
    }

    pub fn unread_count(&self, employee_id: Option<&str>) -> Result<u32> {
        let _guard = self.lock.lock().unwrap();
        if !self.employees_root.exists() {
            return Ok(0);
        }

        let paths: Vec<PathBuf> = if let Some(id) = employee_id {
            vec![self.inbox_path(id)]
        } else {
            // Skip archived employees so the sidebar badge doesn't keep
            // counting unread reports from已解雇员工 (recoverable for 7 days
            // but they shouldn't be visible in the global aggregation).
            fs::read_dir(&self.employees_root)?
                .filter_map(|e| e.ok())
                .map(|e| e.path())
                .filter(|p| !is_archived_dir(p))
                .map(|p| p.join("inbox.jsonl"))
                .collect()
        };

        let mut count = 0u32;
        for path in paths {
            if !path.exists() {
                continue;
            }
            let file = fs::File::open(&path)?;
            let reader = BufReader::new(file);
            for line in reader.lines().flatten() {
                if line.trim().is_empty() {
                    continue;
                }
                if let Ok(e) = serde_json::from_str::<InboxEntry>(&line) {
                    if !e.read {
                        count += 1;
                    }
                }
            }
        }
        Ok(count)
    }

    /// Rewrites the JSONL applying a mutation function. Returns true if any entry was changed.
    fn update_entry<F>(&self, employee_id: &str, mut mutate: F) -> Result<bool>
    where
        F: FnMut(&mut InboxEntry) -> bool,
    {
        let _guard = self.lock.lock().unwrap();
        let path = self.inbox_path(employee_id);
        if !path.exists() {
            return Ok(false);
        }
        let file = fs::File::open(&path)?;
        let reader = BufReader::new(file);
        let mut changed = false;
        let new_content: Vec<String> = reader
            .lines()
            .filter_map(|l| l.ok())
            .filter(|l| !l.trim().is_empty())
            .map(|line| match serde_json::from_str::<InboxEntry>(&line) {
                Ok(mut e) => {
                    if mutate(&mut e) {
                        changed = true;
                        serde_json::to_string(&e).unwrap_or(line)
                    } else {
                        serde_json::to_string(&e).unwrap_or(line)
                    }
                }
                Err(_) => line,
            })
            .collect();
        if changed {
            fs::write(&path, new_content.join("\n") + "\n")?;
        }
        Ok(changed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn make_store(dir: &TempDir) -> InboxStore {
        InboxStore::new(dir.path().to_path_buf())
    }

    #[test]
    fn push_and_list() {
        let dir = TempDir::new().unwrap();
        let store = make_store(&dir);

        store
            .push(
                "emp-1",
                InboxKind::Report,
                "周报".to_string(),
                None,
                None,
                None,
                None,
            )
            .unwrap();
        store
            .push(
                "emp-1",
                InboxKind::Signal,
                "异常信号".to_string(),
                None,
                None,
                None,
                None,
            )
            .unwrap();

        let entries = store.list_for("emp-1", 100).unwrap();
        assert_eq!(entries.len(), 2);
        // newest first
        assert_eq!(entries[0].kind, InboxKind::Signal);
    }

    #[test]
    fn mark_read_changes_flag() {
        let dir = TempDir::new().unwrap();
        let store = make_store(&dir);

        let entry = store
            .push(
                "emp-1",
                InboxKind::Report,
                "报告".to_string(),
                None,
                None,
                None,
                None,
            )
            .unwrap();
        assert_eq!(store.unread_count(Some("emp-1")).unwrap(), 1);

        store.mark_read("emp-1", &entry.id).unwrap();
        assert_eq!(store.unread_count(Some("emp-1")).unwrap(), 0);
    }

    #[test]
    fn mark_all_read() {
        let dir = TempDir::new().unwrap();
        let store = make_store(&dir);

        for _ in 0..3 {
            store
                .push(
                    "emp-2",
                    InboxKind::Report,
                    "r".to_string(),
                    None,
                    None,
                    None,
                    None,
                )
                .unwrap();
        }
        assert_eq!(store.unread_count(Some("emp-2")).unwrap(), 3);
        assert_eq!(store.mark_all_read("emp-2").unwrap(), 3);
        assert_eq!(store.unread_count(Some("emp-2")).unwrap(), 0);
    }

    #[test]
    fn list_all_merges_employees() {
        let dir = TempDir::new().unwrap();
        let store = make_store(&dir);

        store
            .push(
                "emp-a",
                InboxKind::Report,
                "a1".to_string(),
                None,
                None,
                None,
                None,
            )
            .unwrap();
        store
            .push(
                "emp-b",
                InboxKind::Signal,
                "b1".to_string(),
                None,
                None,
                None,
                None,
            )
            .unwrap();

        let all = store.list_all(100).unwrap();
        assert_eq!(all.len(), 2);
    }

    #[test]
    fn unread_count_global() {
        let dir = TempDir::new().unwrap();
        let store = make_store(&dir);

        store
            .push(
                "emp-x",
                InboxKind::Report,
                "r".to_string(),
                None,
                None,
                None,
                None,
            )
            .unwrap();
        store
            .push(
                "emp-y",
                InboxKind::Report,
                "r".to_string(),
                None,
                None,
                None,
                None,
            )
            .unwrap();
        assert_eq!(store.unread_count(None).unwrap(), 2);
    }

    #[test]
    fn list_all_and_unread_skip_archived_employees() {
        let dir = TempDir::new().unwrap();
        let store = make_store(&dir);

        store
            .push(
                "emp-active",
                InboxKind::Report,
                "active".into(),
                None,
                None,
                None,
                None,
            )
            .unwrap();
        store
            .push(
                "emp-archived",
                InboxKind::Report,
                "archived".into(),
                None,
                None,
                None,
                None,
            )
            .unwrap();

        assert_eq!(store.list_all(100).unwrap().len(), 2);
        assert_eq!(store.unread_count(None).unwrap(), 2);

        std::fs::write(
            dir.path().join("emp-archived/employee.json"),
            r#"{"id":"emp-archived","lifecycle":"archived"}"#,
        )
        .unwrap();
        std::fs::write(
            dir.path().join("emp-active/employee.json"),
            r#"{"id":"emp-active","lifecycle":"active"}"#,
        )
        .unwrap();

        let all = store.list_all(100).unwrap();
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].employee_id, "emp-active");
        assert_eq!(store.unread_count(None).unwrap(), 1);

        // Per-employee path still works.
        assert_eq!(store.unread_count(Some("emp-archived")).unwrap(), 1);
    }
}
