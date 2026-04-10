use std::collections::HashMap;
use std::sync::Mutex;

use anyhow::Result;

use crate::runtime::ids::{RunId, ToolCallId};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ToolCallStatus {
    Pending,
    Running,
    Completed,
    Failed,
}

#[derive(Clone, Debug)]
pub struct ToolCallRecord {
    pub tool_call_id: ToolCallId,
    pub run_id: RunId,
    pub tool_name: String,
    pub status: ToolCallStatus,
}

pub trait ToolCallStore: Send + Sync {
    fn create_tool_call(&self, record: ToolCallRecord) -> Result<()>;
    fn get_tool_call(&self, tool_call_id: &ToolCallId) -> Result<Option<ToolCallRecord>>;
    fn update_tool_call_status(
        &self,
        tool_call_id: &ToolCallId,
        status: ToolCallStatus,
    ) -> Result<()>;
}

#[derive(Default)]
pub struct InMemoryToolCallStore {
    tool_calls: Mutex<HashMap<String, ToolCallRecord>>,
}

impl InMemoryToolCallStore {
    pub fn new() -> Self {
        Self::default()
    }
}

impl ToolCallStore for InMemoryToolCallStore {
    fn create_tool_call(&self, record: ToolCallRecord) -> Result<()> {
        self.tool_calls
            .lock()
            .unwrap()
            .insert(record.tool_call_id.as_str().to_string(), record);
        Ok(())
    }

    fn get_tool_call(&self, tool_call_id: &ToolCallId) -> Result<Option<ToolCallRecord>> {
        Ok(self
            .tool_calls
            .lock()
            .unwrap()
            .get(tool_call_id.as_str())
            .cloned())
    }

    fn update_tool_call_status(
        &self,
        tool_call_id: &ToolCallId,
        status: ToolCallStatus,
    ) -> Result<()> {
        if let Some(record) = self
            .tool_calls
            .lock()
            .unwrap()
            .get_mut(tool_call_id.as_str())
        {
            record.status = status;
        }
        Ok(())
    }
}
