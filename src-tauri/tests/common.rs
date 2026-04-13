// src-tauri/tests/common.rs
//
// Shared test helpers for integration tests.
// Imported via `mod common;` in each test file.

use std::sync::Mutex;

use app_lib::runtime::{ChatTurnRequest, RuntimeTurnExecutor};
use app_lib::runtime::events::RuntimeEventKind;
use async_trait::async_trait;

pub fn kind_label(kind: &RuntimeEventKind) -> &'static str {
    match kind {
        RuntimeEventKind::RunStarted => "RunStarted",
        RuntimeEventKind::StreamStarted => "StreamStarted",
        RuntimeEventKind::StreamDelta { .. } => "StreamDelta",
        RuntimeEventKind::StreamDone => "StreamDone",
        RuntimeEventKind::MessagePersisted { .. } => "MessagePersisted",
        RuntimeEventKind::ToolCallExecuting { .. } => "ToolCallExecuting",
        RuntimeEventKind::ToolCallCompleted { .. } => "ToolCallCompleted",
        RuntimeEventKind::AgentIdle { .. } => "AgentIdle",
        RuntimeEventKind::TaskStatusChanged { .. } => "TaskStatusChanged",
        RuntimeEventKind::RunCancelled => "RunCancelled",
        RuntimeEventKind::RunCompleted => "RunCompleted",
    }
}

pub fn event_labels(events: &[app_lib::runtime::events::RuntimeEvent]) -> Vec<&'static str> {
    events.iter().map(|e| kind_label(&e.kind)).collect()
}

#[derive(Default)]
pub struct SilentExecutor {
    pub called: Mutex<bool>,
}

#[async_trait]
impl RuntimeTurnExecutor for SilentExecutor {
    async fn run_chat_turn(&self, _request: ChatTurnRequest) -> Result<(), String> {
        *self.called.lock().unwrap() = true;
        Ok(())
    }
}
