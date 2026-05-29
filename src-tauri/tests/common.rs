// src-tauri/tests/common.rs
//
// Shared test helpers for integration tests.
// Imported via `mod common;` in each test file.

use app_lib::runtime::events::RuntimeEventKind;

#[allow(dead_code)]
pub fn kind_label(kind: &RuntimeEventKind) -> &'static str {
    match kind {
        RuntimeEventKind::RunStarted => "RunStarted",
        RuntimeEventKind::StreamStarted => "StreamStarted",
        RuntimeEventKind::StreamDelta { .. } => "StreamDelta",
        RuntimeEventKind::StreamDone => "StreamDone",
        RuntimeEventKind::StreamRetryReset { .. } => "StreamRetryReset",
        RuntimeEventKind::StreamError { .. } => "StreamError",
        RuntimeEventKind::MessagePersisted { .. } => "MessagePersisted",
        RuntimeEventKind::ToolCallExecuting { .. } => "ToolCallExecuting",
        RuntimeEventKind::ToolCallCompleted { .. } => "ToolCallCompleted",
        RuntimeEventKind::ToolProgress { .. } => "ToolProgress",
        RuntimeEventKind::PermissionAskRequired { .. } => "PermissionAskRequired",
        RuntimeEventKind::UserInteractionRequired { .. } => "UserInteractionRequired",
        RuntimeEventKind::UserInteractionResolved { .. } => "UserInteractionResolved",
        RuntimeEventKind::AgentIdle { .. } => "AgentIdle",
        RuntimeEventKind::LeadHasPendingMessages { .. } => "LeadHasPendingMessages",
        RuntimeEventKind::TaskStatusChanged { .. } => "TaskStatusChanged",
        RuntimeEventKind::StopHookPreventedContinuation { .. } => "StopHookPreventedContinuation",
        RuntimeEventKind::OrphanedPermissionDetected { .. } => "OrphanedPermissionDetected",
        RuntimeEventKind::TurnCompleted { .. } => "TurnCompleted",
        RuntimeEventKind::RunCancelled => "RunCancelled",
        RuntimeEventKind::RunCompleted => "RunCompleted",
        RuntimeEventKind::PendingSnapshot { .. } => "PendingSnapshot",
        RuntimeEventKind::PendingQueued { .. } => "PendingQueued",
        RuntimeEventKind::PendingDrained { .. } => "PendingDrained",
        RuntimeEventKind::PendingRemoved { .. } => "PendingRemoved",
        RuntimeEventKind::TurnStageChanged { .. } => "TurnStageChanged",
        RuntimeEventKind::TurnHeartbeat { .. } => "TurnHeartbeat",
    }
}

#[allow(dead_code)]
pub fn event_labels(events: &[app_lib::runtime::events::RuntimeEvent]) -> Vec<&'static str> {
    events.iter().map(|e| kind_label(&e.kind)).collect()
}

use app_lib::storage::file_store::types::StoredMessage;

#[allow(dead_code)]
pub fn make_user(id: &str, text: &str) -> StoredMessage {
    StoredMessage {
        id: id.into(),
        conversation_id: "c1".into(),
        role: "user".into(),
        content: serde_json::json!({"text": text}),
        created_at: format!("2026-04-24T00:00:{:02}Z", id.parse::<u64>().unwrap_or(0)),
        tool_calls: None,
        tool_call_id: None,
        name: None,
        run_id: None,
        schema_version: Some(2),
        sequence: None,
        seq: None,
        rev: None,
        error: None,
    }
}

#[allow(dead_code)]
pub fn make_assistant_with_tc(id: &str, tc_id: &str, tool: &str) -> StoredMessage {
    StoredMessage {
        id: id.into(),
        conversation_id: "c1".into(),
        role: "assistant".into(),
        content: serde_json::json!({"text": ""}),
        created_at: format!("2026-04-24T00:00:{:02}Z", id.parse::<u64>().unwrap_or(0)),
        tool_calls: Some(vec![serde_json::json!({
            "id": tc_id,
            "type": "function",
            "function": {"name": tool, "arguments": "{}"}
        })]),
        tool_call_id: None,
        name: None,
        run_id: None,
        schema_version: Some(2),
        sequence: None,
        seq: None,
        rev: None,
        error: None,
    }
}

#[allow(dead_code)]
pub fn make_tool_result(id: &str, tc_id: &str, tool: &str, content: &str) -> StoredMessage {
    StoredMessage {
        id: id.into(),
        conversation_id: "c1".into(),
        role: "tool".into(),
        content: serde_json::json!({"text": content}),
        created_at: format!("2026-04-24T00:00:{:02}Z", id.parse::<u64>().unwrap_or(0)),
        tool_calls: None,
        tool_call_id: Some(tc_id.into()),
        name: Some(tool.into()),
        run_id: None,
        schema_version: Some(2),
        sequence: None,
        seq: None,
        rev: None,
        error: None,
    }
}

#[allow(dead_code)]
pub fn make_assistant(id: &str, text: &str) -> StoredMessage {
    StoredMessage {
        id: id.into(),
        conversation_id: "c1".into(),
        role: "assistant".into(),
        content: serde_json::json!({"text": text}),
        created_at: format!("2026-04-24T00:00:{:02}Z", id.parse::<u64>().unwrap_or(0)),
        tool_calls: None,
        tool_call_id: None,
        name: None,
        run_id: None,
        schema_version: Some(2),
        sequence: None,
        seq: None,
        rev: None,
        error: None,
    }
}

/// A minimal in-process Skill stub for testing.
struct MockSkill {
    id: String,
    description: String,
}

impl app_lib::plugin::Skill for MockSkill {
    fn id(&self) -> &str {
        &self.id
    }
    fn display_name(&self) -> &str {
        &self.id
    }
    fn description(&self) -> &str {
        &self.description
    }
    fn system_prompt(&self, _state: &app_lib::plugin::skill_trait::SkillState) -> String {
        String::new()
    }
    fn tool_filter(
        &self,
        _state: &app_lib::plugin::skill_trait::SkillState,
    ) -> app_lib::plugin::skill_trait::ToolFilter {
        app_lib::plugin::skill_trait::ToolFilter::All
    }
}

/// Register a lightweight mock skill into the given registry (for tests).
#[allow(dead_code)]
pub async fn register_mock_skill(
    registry: &std::sync::Arc<app_lib::plugin::SkillRegistry>,
    id: &str,
    description: &str,
) {
    let skill = std::sync::Arc::new(MockSkill {
        id: id.to_string(),
        description: description.to_string(),
    });
    registry.register(skill, "test").await;
}
