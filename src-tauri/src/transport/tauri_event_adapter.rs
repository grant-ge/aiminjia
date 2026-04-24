use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;
use serde_json::json;

use crate::runtime::chat::ChatTurnOutcome;
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
        RuntimeEventKind::StreamRetryReset => Some(LegacyEvent {
            name: "streaming:retry-reset".to_string(),
            payload: json!({
                "conversationId": conversation_id,
                "runId": event.run_id.as_str(),
            }),
        }),
        RuntimeEventKind::StreamError {
            ref error,
            ref raw_error,
        } => Some(LegacyEvent {
            name: "streaming:error".to_string(),
            payload: serde_json::json!({
                "conversationId": conversation_id,
                "error": error,
                "rawError": raw_error,
                "runId": event.run_id.as_str(),
            }),
        }),
        RuntimeEventKind::ToolCallExecuting {
            tool_call_id,
            tool_name,
            input,
        } => Some(LegacyEvent {
            name: "tool:executing".to_string(),
            payload: json!({
                "conversationId": conversation_id,
                "toolId": tool_call_id.as_str(),
                "toolName": tool_name,
                "runId": event.run_id.as_str(),
                "input": input,
            }),
        }),
        RuntimeEventKind::ToolCallCompleted {
            tool_call_id,
            tool_name,
            is_error,
            content,
            msg_id,
            duration_ms,
        } => Some(LegacyEvent {
            name: "tool:completed".to_string(),
            payload: json!({
                "id": msg_id,
                "conversationId": conversation_id,
                "role": "tool",
                "createdAt": chrono::Utc::now().to_rfc3339(),
                "content": {},
                "toolResult": {
                    "toolCallId": tool_call_id.as_str(),
                    "name": tool_name,
                    "content": content,
                    "isError": is_error,
                    "durationMs": duration_ms,
                },
                "runId": event.run_id.as_str(),
                // legacy compat: keep success field for any consumers that still rely on it
                "success": !is_error,
            }),
        }),
        RuntimeEventKind::PermissionAskRequired {
            tool_call_id,
            tool_name,
            message,
            suggestions,
            mode,
            remember_options,
            default_destination,
        } => Some(LegacyEvent {
            name: "permission:ask".to_string(),
            payload: json!({
                "conversationId": conversation_id,
                "runId": event.run_id.as_str(),
                "toolCallId": tool_call_id.as_str(),
                "toolName": tool_name,
                "message": message,
                "suggestions": suggestions,
                "mode": match mode {
                    crate::runtime::tools::permission::PermissionMode::Default => "default",
                    crate::runtime::tools::permission::PermissionMode::Plan => "plan",
                    crate::runtime::tools::permission::PermissionMode::DontAsk => "dontAsk",
                },
                "rememberOptions": remember_options.iter().map(|destination| match destination {
                    crate::runtime::tools::permission::PermissionDestination::Session => "session",
                    crate::runtime::tools::permission::PermissionDestination::Workspace => "workspace",
                    crate::runtime::tools::permission::PermissionDestination::User => "user",
                }).collect::<Vec<_>>(),
                "defaultDestination": default_destination.as_ref().map(|destination| match destination {
                    crate::runtime::tools::permission::PermissionDestination::Session => "session",
                    crate::runtime::tools::permission::PermissionDestination::Workspace => "workspace",
                    crate::runtime::tools::permission::PermissionDestination::User => "user",
                }),
            }),
        }),
        RuntimeEventKind::MessagePersisted {
            message_id,
            role,
            content,
            client_message_id,
        } => {
            let mut payload = json!({
                "conversationId": conversation_id,
                "messageId": message_id,
                "id": message_id,
                "role": role,
                "content": crate::runtime::conversation_service::transform_message_json_for_frontend(json!({
                    "content": content,
                }))["content"].clone(),
                "createdAt": chrono::Utc::now().to_rfc3339(),
                "runId": event.run_id.as_str(),
            });
            if let Some(client_message_id) = client_message_id {
                payload["clientMessageId"] = json!(client_message_id);
            }
            Some(LegacyEvent {
                name: "message:updated".to_string(),
                payload,
            })
        }
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
        RuntimeEventKind::TaskStatusChanged {
            task_id,
            status,
            subject,
            active_form,
            owner_agent_id,
        } => Some(LegacyEvent {
            name: "task:status-changed".to_string(),
            payload: json!({
                "conversationId": conversation_id,
                "taskId": task_id.as_str(),
                "status": status,
                "runId": event.run_id.as_str(),
                "subject": subject,
                "activeForm": active_form,
                "owner": owner_agent_id.as_ref().map(|id| id.as_str()),
            }),
        }),
        RuntimeEventKind::StopHookPreventedContinuation { reason } => Some(LegacyEvent {
            name: "stop:prevented-continuation".to_string(),
            payload: json!({
                "conversationId": conversation_id,
                "runId": event.run_id.as_str(),
                "reason": reason,
            }),
        }),
        RuntimeEventKind::TurnCompleted {
            outcome,
            total_input_tokens,
            total_output_tokens,
            total_cost_usd,
            permission_denial_count,
        } => {
            let mut payload = json!({
                "conversationId": conversation_id,
                "runId": event.run_id.as_str(),
                "totalInputTokens": total_input_tokens,
                "totalOutputTokens": total_output_tokens,
                "totalCostUsd": total_cost_usd,
                "permissionDenialCount": permission_denial_count,
            });

            if let Some(object) = payload.as_object_mut() {
                match outcome {
                    ChatTurnOutcome::Success => {
                        object.insert("outcome".to_string(), json!("Success"));
                    }
                    ChatTurnOutcome::Cancelled => {
                        object.insert("outcome".to_string(), json!("Cancelled"));
                    }
                    ChatTurnOutcome::MaxIterationsReached { iterations } => {
                        object.insert("outcome".to_string(), json!("MaxIterationsReached"));
                        object.insert("iterations".to_string(), json!(iterations));
                    }
                    ChatTurnOutcome::BudgetExceeded { reason, .. } => {
                        object.insert("outcome".to_string(), json!("BudgetExceeded"));
                        object.insert("reason".to_string(), json!(reason));
                    }
                    ChatTurnOutcome::ExecutionError { message } => {
                        object.insert("outcome".to_string(), json!("ExecutionError"));
                        object.insert("message".to_string(), json!(message));
                    }
                }
            }

            Some(LegacyEvent {
                name: "turn:completed".to_string(),
                payload,
            })
        }
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
