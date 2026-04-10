use std::sync::Arc;

use anyhow::Result;

use crate::runtime::store::TaskStore;
use crate::runtime::task::task_models::{TaskRecord, TaskStatus};

pub struct TaskRuntime {
    store: Arc<dyn TaskStore>,
}

impl TaskRuntime {
    pub fn new(store: Arc<dyn TaskStore>) -> Self {
        Self { store }
    }

    pub fn create_task(&self, record: TaskRecord) -> Result<()> {
        self.store.create_task(record)
    }

    pub fn set_status(
        &self,
        task_id: &crate::runtime::ids::TaskId,
        status: TaskStatus,
    ) -> Result<()> {
        self.store.update_task_status(task_id, status)
    }
}
