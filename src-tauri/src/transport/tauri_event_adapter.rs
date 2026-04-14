use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;
use serde_json::json;

use crate::runtime::event_bus::RuntimeEventSubscriber;
use crate::runtime::events::{RuntimeEvent, RuntimeEventKind};
use crate::transport::runtime_host::RuntimeHost;

#[derive(Clone, Debug)]
pub struct LegacyEvent {
    pub name: String,
    pub payload: serde_json::Value,
}

pub fn map_runtime_event(event: &RuntimeEvent) -> Option<LegacyEvent> {
    let conversation_id = event.session_id.as_str().to_string();
    let payload = match &event.kind {
        RuntimeEventKind::StreamDelta { content } => Some(LegacyEvent {
            name: "streaming:delta".to_string(),
            payload: json!({
                "conversationId": conversation_id,
                "delta": content,
                "runId": event.run_id.as_str(),
            }),
        }),
        RuntimeEventKind::StreamDone => Some(LegacyEvent {
            name: "streaming:done".to_string(),
            payload: json!({
                "conversationId": conversation_id,
                "runId": event.run_id.as_str(),
            }),
        }),
        RuntimeEventKind::ToolCallExecuting {
            tool_call_id,
            tool_name,
        } => Some(LegacyEvent {
            name: "tool:executing".to_string(),
            payload: json!({
                "conversationId": conversation_id,
                "toolId": tool_call_id.as_str(),
                "toolName": tool_name,
                "runId": event.run_id.as_str(),
            }),
        }),
        RuntimeEventKind::ToolCallCompleted {
            tool_call_id,
            tool_name,
            is_error,
        } => Some(LegacyEvent {
            name: "tool:completed".to_string(),
            payload: json!({
                "conversationId": conversation_id,
                "toolId": tool_call_id.as_str(),
                "toolName": tool_name,
                "runId": event.run_id.as_str(),
                "success": !is_error,
            }),
        }),
        RuntimeEventKind::MessagePersisted {
            message_id,
            role,
            content,
        } => Some(LegacyEvent {
            name: "message:updated".to_string(),
            payload: json!({
                "conversationId": conversation_id,
                "messageId": message_id,
                "id": message_id,
                "role": role,
                "content": content,
                "runId": event.run_id.as_str(),
            }),
        }),
        RuntimeEventKind::AgentIdle { agent_id, scope } => Some(LegacyEvent {
            name: "agent:idle".to_string(),
            payload: json!({
                "conversationId": conversation_id,
                "agentId": agent_id.as_str(),
                "runId": event.run_id.as_str(),
                "scope": match scope {
                    crate::runtime::events::AgentIdleScope::Primary => "primary",
                    crate::runtime::events::AgentIdleScope::Child => "child",
                },
            }),
        }),
        RuntimeEventKind::TaskStatusChanged { task_id, status } => Some(LegacyEvent {
            name: "task:status-changed".to_string(),
            payload: json!({
                "conversationId": conversation_id,
                "taskId": task_id.as_str(),
                "status": status,
                "runId": event.run_id.as_str(),
            }),
        }),
        _ => None,
    };
    payload
}

pub struct TauriEventAdapter {
    host: Arc<dyn RuntimeHost>,
}

impl TauriEventAdapter {
    pub fn new(host: Arc<dyn RuntimeHost>) -> Self {
        Self { host }
    }
}

#[async_trait]
impl RuntimeEventSubscriber for TauriEventAdapter {
    async fn on_event(&self, event: &RuntimeEvent) -> Result<()> {
        if let Some(mapped) = map_runtime_event(event) {
            self.host.emit_legacy_event(&mapped.name, mapped.payload)?;
        }
        Ok(())
    }
}
