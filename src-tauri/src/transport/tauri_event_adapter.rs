use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;
use serde_json::json;

use crate::connector::channel::ask_coordinator::ChannelSessionRegistry;
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
            ..
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
                    crate::runtime::tools::permission::PermissionMode::AcceptEdits => "acceptEdits",
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
        RuntimeEventKind::UserInteractionRequired {
            interaction_id,
            tool_call_id,
            tool_name,
            kind,
            payload,
            ..
        } => Some(LegacyEvent {
            name: "interaction:required".to_string(),
            payload: json!({
                "conversationId": conversation_id,
                "runId": event.run_id.as_str(),
                "interactionId": interaction_id.as_str(),
                "toolCallId": tool_call_id.as_str(),
                "toolName": tool_name,
                "kind": kind,
                "payload": payload,
            }),
        }),
        RuntimeEventKind::UserInteractionResolved { interaction_id } => Some(LegacyEvent {
            name: "interaction:resolved".to_string(),
            payload: json!({
                "conversationId": conversation_id,
                "runId": event.run_id.as_str(),
                "interactionId": interaction_id.as_str(),
            }),
        }),
        RuntimeEventKind::MessagePersisted {
            message_id,
            role,
            content,
            client_message_id,
            tool_calls,
        } => {
            let skill_command = content.get("skillCommand");
            let command_text = content.get("commandText").and_then(|value| value.as_str());
            log::info!(
                "[skill-command][message-persisted-event] trace_id={} conversation_id={} run_id={} message_id={} role={} client_message_id={:?} has_skill_command={} command_text_len={}",
                client_message_id.as_deref().unwrap_or(event.run_id.as_str()),
                conversation_id,
                event.run_id.as_str(),
                message_id,
                role,
                client_message_id,
                skill_command.is_some(),
                command_text.map(str::len).unwrap_or(0)
            );
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
            if let Some(tool_calls) = tool_calls {
                payload["toolCalls"] = json!(tool_calls);
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
        RuntimeEventKind::LeadHasPendingMessages { agent_id } => Some(LegacyEvent {
            name: "lead:has-pending-messages".to_string(),
            payload: json!({
                "conversationId": conversation_id,
                "agentId": agent_id.as_str(),
                "runId": event.run_id.as_str(),
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
            total_cache_creation_input_tokens,
            total_cache_read_input_tokens,
            total_cost_usd,
            permission_denial_count,
        } => {
            let mut payload = json!({
                "conversationId": conversation_id,
                "runId": event.run_id.as_str(),
                "totalInputTokens": total_input_tokens,
                "totalOutputTokens": total_output_tokens,
                "totalCacheCreationInputTokens": total_cache_creation_input_tokens,
                "totalCacheReadInputTokens": total_cache_read_input_tokens,
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
        RuntimeEventKind::PendingSnapshot { items } => Some(LegacyEvent {
            name: "pending:snapshot".to_string(),
            payload: json!({
                "sessionId": conversation_id,
                "items": items,
            }),
        }),
        RuntimeEventKind::PendingQueued { item } => Some(LegacyEvent {
            name: "pending:queued".to_string(),
            payload: json!({
                "sessionId": conversation_id,
                "item": item,
            }),
        }),
        RuntimeEventKind::PendingDrained { drained_ids } => Some(LegacyEvent {
            name: "pending:drained".to_string(),
            payload: json!({
                "sessionId": conversation_id,
                "drainedIds": drained_ids,
            }),
        }),
        RuntimeEventKind::PendingRemoved { item_id } => Some(LegacyEvent {
            name: "pending:removed".to_string(),
            payload: json!({
                "sessionId": conversation_id,
                "itemId": item_id,
            }),
        }),
        RuntimeEventKind::TurnStageChanged {
            stage,
            stage_started_at_ms,
        } => Some(LegacyEvent {
            name: "turn:stage".to_string(),
            payload: json!({
                "conversationId": conversation_id,
                "runId": event.run_id.as_str(),
                "stage": stage,
                "stageStartedAtMs": stage_started_at_ms,
            }),
        }),
        RuntimeEventKind::TurnHeartbeat {
            stage_elapsed_ms,
            turn_elapsed_ms,
        } => Some(LegacyEvent {
            name: "turn:heartbeat".to_string(),
            payload: json!({
                "conversationId": conversation_id,
                "runId": event.run_id.as_str(),
                "stageElapsedMs": stage_elapsed_ms,
                "turnElapsedMs": turn_elapsed_ms,
            }),
        }),
        _ => None,
    };
    payload
}

pub struct TauriEventAdapter {
    host: Arc<dyn RuntimeHost>,
    /// When set, ask-style events for sessions registered in this registry are
    /// silently dropped — IMAskCoordinator owns those events for IM sessions.
    /// App-internal sessions (not in the registry) still flow through as before.
    channel_sessions: Option<Arc<dyn ChannelSessionRegistry>>,
}

impl TauriEventAdapter {
    pub fn new(host: Arc<dyn RuntimeHost>) -> Self {
        Self {
            host,
            channel_sessions: None,
        }
    }

    /// Constructor variant that wires up the shared channel-session registry.
    /// Use this in production (lib.rs) so that ask-style events for IM sessions
    /// are not forwarded to the desktop UI.
    pub fn with_channel_sessions(
        host: Arc<dyn RuntimeHost>,
        channel_sessions: Arc<dyn ChannelSessionRegistry>,
    ) -> Self {
        Self {
            host,
            channel_sessions: Some(channel_sessions),
        }
    }
}

#[async_trait]
impl RuntimeEventSubscriber for TauriEventAdapter {
    async fn on_event(&self, event: &RuntimeEvent) -> Result<()> {
        // Drop ask-style events for IM channel sessions — IMAskCoordinator owns those.
        // App-internal sessions still flow through to the desktop UI as before.
        if let (
            Some(registry),
            RuntimeEventKind::PermissionAskRequired { .. }
            | RuntimeEventKind::UserInteractionRequired { .. },
        ) = (self.channel_sessions.as_ref(), &event.kind)
        {
            if registry.is_channel_session(&event.session_id) {
                return Ok(());
            }
        }
        if let Some(mapped) = map_runtime_event(event) {
            self.host.emit_legacy_event(&mapped.name, mapped.payload)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod pending_event_tests {
    use super::*;
    use crate::runtime::ids::{RunId, SessionId};
    use crate::runtime::pending::{PendingItem, PendingSource};

    fn evt(kind: RuntimeEventKind) -> RuntimeEvent {
        RuntimeEvent::new(SessionId::new("conv-1"), RunId::new("run-1"), kind)
    }

    #[test]
    fn pending_snapshot_maps_to_legacy_event() {
        let e = evt(RuntimeEventKind::PendingSnapshot { items: vec![] });
        let m = map_runtime_event(&e).expect("mapped");
        assert_eq!(m.name, "pending:snapshot");
        assert_eq!(m.payload["sessionId"], "conv-1");
        assert!(m.payload["items"].is_array());
    }

    #[test]
    fn pending_queued_carries_item() {
        let item = PendingItem {
            id: "p-1".into(),
            source: PendingSource::App,
            text: "hi".into(),
            sender_nick: None,
            attachments: vec![],
            skill_command: None,
            received_at: "2026-05-11T03:21:00Z".into(),
        };
        let e = evt(RuntimeEventKind::PendingQueued { item });
        let m = map_runtime_event(&e).unwrap();
        assert_eq!(m.name, "pending:queued");
        assert_eq!(m.payload["item"]["id"], "p-1");
    }

    #[test]
    fn pending_drained_carries_ids() {
        let e = evt(RuntimeEventKind::PendingDrained {
            drained_ids: vec!["a".into(), "b".into()],
        });
        let m = map_runtime_event(&e).unwrap();
        assert_eq!(m.name, "pending:drained");
        assert_eq!(m.payload["drainedIds"][0], "a");
    }

    #[test]
    fn pending_removed_carries_item_id() {
        let e = evt(RuntimeEventKind::PendingRemoved {
            item_id: "p-1".into(),
        });
        let m = map_runtime_event(&e).unwrap();
        assert_eq!(m.name, "pending:removed");
        assert_eq!(m.payload["itemId"], "p-1");
    }
}
