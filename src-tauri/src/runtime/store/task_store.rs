use std::collections::HashMap;
use std::sync::Mutex;

use anyhow::Result;

use crate::runtime::ids::{SessionId, TaskId};
use crate::runtime::task::task_models::{TaskRecord, TaskStatus};

pub trait TaskStore: Send + Sync {
    fn create_task(&self, record: TaskRecord) -> Result<()>;
    fn get_task(&self, task_id: &TaskId) -> Result<Option<TaskRecord>>;
    fn update_task_status(&self, task_id: &TaskId, status: TaskStatus) -> Result<()>;
    fn list_for_session(&self, session_id: &SessionId) -> Result<Vec<TaskRecord>>;
}

#[derive(Default)]
pub struct InMemoryTaskStore {
    tasks: Mutex<HashMap<String, TaskRecord>>,
}

impl InMemoryTaskStore {
    pub fn new() -> Self {
        Self::default()
    }
}

impl TaskStore for InMemoryTaskStore {
    fn create_task(&self, record: TaskRecord) -> Result<()> {
        self.tasks
            .lock()
            .unwrap()
            .insert(record.task_id.as_str().to_string(), record);
        Ok(())
    }

    fn get_task(&self, task_id: &TaskId) -> Result<Option<TaskRecord>> {
        Ok(self.tasks.lock().unwrap().get(task_id.as_str()).cloned())
    }

    fn update_task_status(&self, task_id: &TaskId, status: TaskStatus) -> Result<()> {
        let mut tasks = self.tasks.lock().unwrap();
        let record = tasks
            .get_mut(task_id.as_str())
            .ok_or_else(|| anyhow::anyhow!("task not found: {}", task_id.as_str()))?;
        record.status = status;
        Ok(())
    }

    fn list_for_session(&self, session_id: &SessionId) -> Result<Vec<TaskRecord>> {
        let tasks = self.tasks.lock().unwrap();
        let result = tasks
            .values()
            .filter(|r| r.session_id.as_str() == session_id.as_str())
            .cloned()
            .collect();
        Ok(result)
    }
}
