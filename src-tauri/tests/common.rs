// src-tauri/tests/common.rs
//
// Shared test helpers for integration tests.
// Imported via `mod common;` in each test file.

use app_lib::runtime::events::RuntimeEventKind;

pub fn kind_label(kind: &RuntimeEventKind) -> &'static str {
    match kind {
        RuntimeEventKind::RunStarted => "RunStarted",
        RuntimeEventKind::StreamStarted => "StreamStarted",
        RuntimeEventKind::StreamDelta { .. } => "StreamDelta",
        RuntimeEventKind::StreamDone => "StreamDone",
        RuntimeEventKind::StreamError { .. } => "StreamError",
        RuntimeEventKind::MessagePersisted { .. } => "MessagePersisted",
        RuntimeEventKind::ToolCallExecuting { .. } => "ToolCallExecuting",
        RuntimeEventKind::ToolCallCompleted { .. } => "ToolCallCompleted",
        RuntimeEventKind::PermissionAskRequired { .. } => "PermissionAskRequired",
        RuntimeEventKind::AgentIdle { .. } => "AgentIdle",
        RuntimeEventKind::TaskStatusChanged { .. } => "TaskStatusChanged",
        RuntimeEventKind::StopHookPreventedContinuation { .. } => "StopHookPreventedContinuation",
        RuntimeEventKind::OrphanedPermissionDetected { .. } => "OrphanedPermissionDetected",
        RuntimeEventKind::TurnCompleted { .. } => "TurnCompleted",
        RuntimeEventKind::RunCancelled => "RunCancelled",
        RuntimeEventKind::RunCompleted => "RunCompleted",
    }
}

pub fn event_labels(events: &[app_lib::runtime::events::RuntimeEvent]) -> Vec<&'static str> {
    events.iter().map(|e| kind_label(&e.kind)).collect()
}
