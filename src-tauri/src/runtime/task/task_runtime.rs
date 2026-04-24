use std::sync::Arc;

use anyhow::Result;

use crate::runtime::event_bus::RuntimeEventBus;
use crate::runtime::events::{RuntimeEvent, RuntimeEventKind};
use crate::runtime::ids::{RunId, SessionId};
use crate::runtime::store::TaskStore;
use crate::runtime::task::task_models::{TaskRecord, TaskStatus};

pub struct TaskRuntime {
    store: Arc<dyn TaskStore>,
    event_bus: Option<RuntimeEventBus>,
}

impl TaskRuntime {
    pub fn new(store: Arc<dyn TaskStore>) -> Self {
        Self {
            store,
            event_bus: None,
        }
    }

    pub fn with_event_bus(store: Arc<dyn TaskStore>, event_bus: RuntimeEventBus) -> Self {
        Self {
            store,
            event_bus: Some(event_bus),
        }
    }

    pub fn create_task(&self, record: TaskRecord) -> Result<()> {
        self.store.create_task(record)
    }

    pub fn list_for_session(
        &self,
        session_id: &crate::runtime::ids::SessionId,
    ) -> Result<Vec<TaskRecord>> {
        self.store.list_for_session(session_id)
    }

    pub fn set_status(
        &self,
        task_id: &crate::runtime::ids::TaskId,
        status: TaskStatus,
    ) -> Result<()> {
        // Look up the task record to get real context before mutating
        let task_record = self.store.get_task(task_id)?;

        self.store.update_task_status(task_id, status.clone())?;

        if let Some(bus) = &self.event_bus {
            let status_str = match &status {
                TaskStatus::Pending => "pending",
                TaskStatus::InProgress => "in_progress",
                TaskStatus::Completed => "completed",
                TaskStatus::Failed => "failed",
                TaskStatus::Cancelled => "cancelled",
            };

            // Use the task's real parent_run_id as context
            let run_id = task_record
                .as_ref()
                .map(|r| r.parent_run_id.clone())
                .unwrap_or_else(|| RunId::new("unknown"));
            let session_id = SessionId::new(run_id.as_str());

            let event = RuntimeEvent::new(
                session_id,
                run_id,
                RuntimeEventKind::TaskStatusChanged {
                    task_id: task_id.clone(),
                    status: status_str.to_string(),
                    subject: task_record
                        .as_ref()
                        .map(|r| r.subject.clone())
                        .unwrap_or_default(),
                    active_form: task_record
                        .as_ref()
                        .and_then(|r| r.active_form.clone()),
                    owner_agent_id: task_record
                        .as_ref()
                        .and_then(|r| r.owner_agent_id.clone()),
                },
            );
            let bus = bus.clone();
            let _ = std::thread::spawn(move || {
                let rt = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .unwrap();
                rt.block_on(bus.emit(event))
            })
            .join();
        }

        Ok(())
    }
}
