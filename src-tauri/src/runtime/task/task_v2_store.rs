//! File-backed Task V2 store.
//!
//! Mirrors claude-code-best src/tasks.ts:
//! - task files live under ~/.renlijia/tasks/<taskListId>/<id>.json
//! - .highwatermark tracks max assigned id
//! - writes use temp file + rename to avoid partial writes

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use anyhow::{anyhow, Context, Result};

use crate::runtime::task::task_models::TaskRecord;

const HIGH_WATER_MARK_FILE: &str = ".highwatermark";

#[derive(Debug)]
pub struct FileTaskV2Store {
    root: PathBuf,
    lock: Mutex<()>,
}

impl FileTaskV2Store {
    pub fn new(aijia_home: PathBuf) -> Self {
        Self {
            root: aijia_home.join("tasks"),
            lock: Mutex::new(()),
        }
    }

    fn sanitize(input: &str) -> String {
        input
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '-' })
            .collect()
    }

    fn list_dir(&self, task_list_id: &str) -> PathBuf {
        self.root.join(Self::sanitize(task_list_id))
    }

    fn task_path(&self, task_list_id: &str, task_id: &str) -> PathBuf {
        self.list_dir(task_list_id)
            .join(format!("{}.json", Self::sanitize(task_id)))
    }

    fn highwatermark_path(&self, task_list_id: &str) -> PathBuf {
        self.list_dir(task_list_id).join(HIGH_WATER_MARK_FILE)
    }

    fn ensure_dir(&self, task_list_id: &str) -> Result<()> {
        fs::create_dir_all(self.list_dir(task_list_id))?;
        Ok(())
    }

    fn read_highwatermark(&self, task_list_id: &str) -> Result<u64> {
        let path = self.highwatermark_path(task_list_id);
        if !path.exists() {
            return Ok(0);
        }
        let s = fs::read_to_string(path)?;
        Ok(s.trim().parse::<u64>().unwrap_or(0))
    }

    fn write_highwatermark(&self, task_list_id: &str, value: u64) -> Result<()> {
        self.atomic_write(&self.highwatermark_path(task_list_id), value.to_string().as_bytes())
    }

    fn atomic_write(&self, path: &Path, bytes: &[u8]) -> Result<()> {
        let tmp = path.with_extension("tmp");
        fs::write(&tmp, bytes)?;
        fs::rename(tmp, path)?;
        Ok(())
    }

    pub fn next_id(&self, task_list_id: &str) -> Result<String> {
        let _guard = self.lock.lock().unwrap();
        self.ensure_dir(task_list_id)?;
        let next = self.read_highwatermark(task_list_id)? + 1;
        self.write_highwatermark(task_list_id, next)?;
        Ok(next.to_string())
    }

    pub fn create(&self, task_list_id: &str, task: &TaskRecord) -> Result<()> {
        let _guard = self.lock.lock().unwrap();
        self.ensure_dir(task_list_id)?;
        let path = self.task_path(task_list_id, &task.id);
        if path.exists() {
            return Err(anyhow!("task already exists: {}", task.id));
        }
        let bytes = serde_json::to_vec_pretty(task)?;
        self.atomic_write(&path, &bytes)
    }

    pub fn get(&self, task_list_id: &str, task_id: &str) -> Result<Option<TaskRecord>> {
        let path = self.task_path(task_list_id, task_id);
        if !path.exists() {
            return Ok(None);
        }
        let content = fs::read_to_string(&path)
            .with_context(|| format!("failed to read task file {}", path.display()))?;
        let task: TaskRecord = serde_json::from_str(&content)?;
        Ok(Some(task))
    }

    pub fn list(&self, task_list_id: &str) -> Result<Vec<TaskRecord>> {
        let dir = self.list_dir(task_list_id);
        if !dir.exists() {
            return Ok(vec![]);
        }
        let mut tasks = vec![];
        for entry in fs::read_dir(dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.extension().and_then(|s| s.to_str()) != Some("json") {
                continue;
            }
            let content = fs::read_to_string(&path)?;
            let task: TaskRecord = serde_json::from_str(&content)?;
            tasks.push(task);
        }
        tasks.sort_by_key(|t| t.id.parse::<u64>().unwrap_or(u64::MAX));
        Ok(tasks)
    }

    pub fn update(&self, task_list_id: &str, task: &TaskRecord) -> Result<()> {
        let _guard = self.lock.lock().unwrap();
        self.ensure_dir(task_list_id)?;
        let path = self.task_path(task_list_id, &task.id);
        if !path.exists() {
            return Err(anyhow!("task not found: {}", task.id));
        }
        let bytes = serde_json::to_vec_pretty(task)?;
        self.atomic_write(&path, &bytes)
    }

    pub fn delete(&self, task_list_id: &str, task_id: &str) -> Result<bool> {
        let _guard = self.lock.lock().unwrap();
        let path = self.task_path(task_list_id, task_id);
        if !path.exists() {
            return Ok(false);
        }
        fs::remove_file(path)?;
        Ok(true)
    }
}
