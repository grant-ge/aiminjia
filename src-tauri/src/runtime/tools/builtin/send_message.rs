//! `SendMessage` builtin tool (P2.2).
//!
//! Routes a [`StructuredMessage`] from the calling agent to one peer (by name)
//! or to every Teammate in the Team (`to: "*"` broadcast).
//!
//! Resolution chain:
//! 1. `to` -> `AgentNameRegistry::resolve` -> `AgentId`
//! 2. `AgentId` -> `InboxRegistry::get` -> `Arc<AgentInbox>`
//! 3. push `InboxItem::ChatMessage { message, source }` into the inbox
//!
//! `source` is derived from a reverse-lookup of the caller's `AgentId` in
//! `AgentNameRegistry`: `team-lead` is the canonical Lead name (P1.7), any
//! other resolved name is treated as a Teammate, and a missing reverse
//! mapping degrades to `MessageSource::System`.

use anyhow::Result;
use async_trait::async_trait;
use serde_json::{json, Value};

use crate::runtime::agent::inbox::{InboxItem, MessageSource};
use crate::runtime::messaging::StructuredMessage;
use crate::runtime::tools::builtin::team_tools::LEAD_NAME;
use crate::runtime::tools::catalog::TOOL_CATALOG;
use crate::runtime::tools::context::ToolExecutionContext;
use crate::runtime::tools::definition::ToolDefinition;
use crate::runtime::tools::executor::{ToolError, ToolResult};
use crate::runtime::tools::RuntimeTool;

pub const BROADCAST_TOKEN: &str = "*";

pub struct SendMessageRuntimeTool;

#[async_trait]
impl RuntimeTool for SendMessageRuntimeTool {
    fn definition(&self) -> ToolDefinition {
        TOOL_CATALOG.get("SendMessage").unwrap_or_else(|| {
            ToolDefinition::new("SendMessage", "Send a structured message to another agent.")
        })
    }

    fn is_concurrency_safe(&self, _input: &Value) -> bool {
        true
    }

    async fn execute(
        &self,
        input: Value,
        ctx: ToolExecutionContext,
    ) -> Result<ToolResult, ToolError> {
        let to = input
            .get("to")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .ok_or_else(|| {
                ToolError::ExecutionFailed("missing required string field `to`".into())
            })?
            .to_string();

        let message_value = input.get("message").cloned().ok_or_else(|| {
            ToolError::ExecutionFailed("missing required object field `message`".into())
        })?;
        let message: StructuredMessage = serde_json::from_value(message_value).map_err(|e| {
            ToolError::ExecutionFailed(format!(
                "invalid `message` payload: {e}. Expected a StructuredMessage \
                 ({{type:'text'|'shutdown_request'|...}})"
            ))
        })?;

        let session = ctx.session_id.clone();
        let inbox_reg = ctx.inbox_registry.clone().ok_or_else(|| {
            ToolError::ExecutionFailed(
                "inbox_registry not available — SendMessage requires LTR runtime wiring".into(),
            )
        })?;
        let names = ctx.agent_names.clone().ok_or_else(|| {
            ToolError::ExecutionFailed(
                "agent_names not available — SendMessage requires LTR runtime wiring".into(),
            )
        })?;

        // Derive caller name (for source label + self-send guard) by reverse-
        // looking up the agent_id.  Unnamed callers (no entry in registry) get
        // MessageSource::System and bypass the self-send check.
        let caller_name = if let Some(aid) = ctx.agent_id.as_ref() {
            names.name_for(&session, aid).await
        } else {
            None
        };
        let source = match caller_name.as_deref() {
            Some(LEAD_NAME) => MessageSource::Lead,
            Some(other) => MessageSource::Teammate(other.to_string()),
            None => MessageSource::System,
        };

        // ── Broadcast path ──────────────────────────────────────────────
        if to == BROADCAST_TOKEN {
            let team_reg = ctx.team_registry.clone().ok_or_else(|| {
                ToolError::ExecutionFailed(
                    "team_registry not available — broadcast requires Team mode".into(),
                )
            })?;
            let team_handle = team_reg.get(&session).await.ok_or_else(|| {
                ToolError::ExecutionFailed(
                    "no team in this session — TeamCreate must be called first".into(),
                )
            })?;

            // Snapshot recipient list under the Team lock then drop it before
            // doing async sends, to avoid holding the lock across awaits that
            // touch other registries.
            let mut recipients: Vec<String> = {
                let team = team_handle.lock().await;
                team.teammates.iter().map(|m| m.name.clone()).collect()
            };
            // Don't echo the broadcast back to the sender.
            if let Some(ref name) = caller_name {
                recipients.retain(|n| n != name);
            }

            let mut delivered = 0usize;
            let mut missing: Vec<String> = Vec::new();
            for name in &recipients {
                if let Some(target_id) = names.resolve(&session, name).await {
                    if let Some(inbox) = inbox_reg.get(&session, &target_id).await {
                        // mpsc full → record but keep going; this is best-effort
                        // broadcast, not transactional.
                        if inbox
                            .send(InboxItem::ChatMessage {
                                message: message.clone(),
                                source: source.clone(),
                            })
                            .await
                            .is_ok()
                        {
                            delivered += 1;
                        } else {
                            missing.push(format!("{name} (inbox closed)"));
                        }
                    } else {
                        missing.push(format!("{name} (no inbox)"));
                    }
                } else {
                    missing.push(format!("{name} (unresolved name)"));
                }
            }

            return Ok(ToolResult::new(
                "SendMessage",
                format!(
                    "broadcast {} delivered to {delivered} recipient(s){}",
                    message.variant_name(),
                    if missing.is_empty() {
                        String::new()
                    } else {
                        format!("; skipped: {}", missing.join(", "))
                    }
                ),
                Some(json!({
                    "broadcast": true,
                    "delivered": delivered,
                    "skipped": missing,
                    "variant": message.variant_name(),
                })),
            ));
        }

        // ── Single-recipient path ───────────────────────────────────────
        if caller_name.as_deref() == Some(to.as_str()) {
            return Err(ToolError::ExecutionFailed(format!(
                "self-send rejected: caller `{to}` cannot SendMessage to itself"
            )));
        }

        let target_id = names.resolve(&session, &to).await.ok_or_else(|| {
            ToolError::ExecutionFailed(format!(
                "no agent named `{to}` in this session — call TeamCreate first or check the spelling"
            ))
        })?;
        let inbox = inbox_reg.get(&session, &target_id).await.ok_or_else(|| {
            ToolError::ExecutionFailed(format!(
                "agent `{to}` is registered but has no inbox — likely already exited or not a Teammate"
            ))
        })?;

        inbox
            .send(InboxItem::ChatMessage {
                message: message.clone(),
                source,
            })
            .await
            .map_err(|_| {
                ToolError::ExecutionFailed(format!("agent `{to}` inbox closed; message dropped"))
            })?;

        Ok(ToolResult::new(
            "SendMessage",
            format!("delivered {} to `{to}`", message.variant_name()),
            Some(json!({
                "delivered_to": to,
                "variant": message.variant_name(),
            })),
        ))
    }
}
