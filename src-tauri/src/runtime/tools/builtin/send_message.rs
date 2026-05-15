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
use crate::telemetry::{record_diagnostic, DiagnosticEvent, DiagnosticSource};

pub const BROADCAST_TOKEN: &str = "*";

/// Append one SendMessage delivery to `<conv_dir>/teams/{name}/team-chat.jsonl`.
/// Best-effort: any IO error is logged at warn but never surfaced — inbox
/// delivery is the authoritative path; this file is a UI-only mirror.
///
/// Returns the rendered entry (ts/from/to/text/variant) so the caller can
/// fan it out as a `team-chat:appended` `RuntimeEvent` (PR9).  Returns
/// `None` when the entry was not appended (no team_name / serialize fail /
/// IO error) so the caller can skip the emit.
fn append_team_chat_entry(
    conv_dir: Option<&std::path::Path>,
    team_name: Option<&str>,
    from: &str,
    to: &str,
    message: &StructuredMessage,
) -> Option<TeamChatAppended> {
    let dir = conv_dir?;
    let body = message.as_text().unwrap_or("").to_string();
    let ts = chrono::Utc::now().to_rfc3339();
    let variant = message.variant_name().to_string();
    let entry = serde_json::json!({
        "ts": ts,
        "from": from,
        "to": to,
        "text": body,
        "variant": variant,
    });
    use crate::runtime::agent::team_paths::TeamPaths;
    let team_name = team_name?;
    let path = TeamPaths::for_team(dir, team_name).team_chat_jsonl();
    let line = match serde_json::to_string(&entry) {
        Ok(s) => s,
        Err(e) => {
            log::warn!("[team_chat.jsonl] serialize failed: {e}");
            return None;
        }
    };
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    use std::io::Write;
    match std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
    {
        Ok(mut f) => {
            if let Err(e) = writeln!(f, "{line}") {
                log::warn!("[team_chat.jsonl] write failed path={}: {e}", path.display());
                return None;
            }
        }
        Err(e) => {
            log::warn!("[team_chat.jsonl] open failed path={}: {e}", path.display());
            return None;
        }
    }
    Some(TeamChatAppended {
        team_name: team_name.to_string(),
        ts,
        from: from.to_string(),
        to: to.to_string(),
        text: body,
        variant,
    })
}

/// Captured result of a successful `team-chat.jsonl` append.  Used by
/// callers (PR9) to fan the entry out as a `team-chat:appended` event.
struct TeamChatAppended {
    team_name: String,
    ts: String,
    from: String,
    to: String,
    text: String,
    variant: String,
}

/// PR9: fan a successful `team-chat.jsonl` append out as a
/// `RuntimeEventKind::TeamChatAppended` so the front-end's TeamChatPanel
/// (PR10) can append it live.  No-op when the tool execution context
/// didn't carry a runtime event bus (legacy/test paths).
async fn emit_team_chat_appended(
    ctx: &crate::runtime::tools::context::ToolExecutionContext,
    entry: TeamChatAppended,
) {
    let Some(bus) = ctx.runtime_event_bus.as_ref() else { return };
    let event = crate::runtime::events::RuntimeEvent::new(
        ctx.session_id.clone(),
        ctx.run_id.clone(),
        crate::runtime::events::RuntimeEventKind::TeamChatAppended {
            team_name: entry.team_name,
            ts: entry.ts,
            from: entry.from,
            to: entry.to,
            text: entry.text,
            variant: entry.variant,
        },
    );
    let _ = bus.emit(event).await;
}

pub struct SendMessageRuntimeTool;

#[async_trait]
impl RuntimeTool for SendMessageRuntimeTool {
    fn id(&self) -> &str { "SendMessage" }
    
    async fn definition(&self, _ctx: &crate::runtime::tools::ToolDescriptionContext) -> ToolDefinition {
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
        let ws = crate::telemetry::diagnostics_workspace();
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

        record_diagnostic(
            &ws,
            DiagnosticEvent::new("tool.send_message.entry", DiagnosticSource::Backend)
                .conversation_id(ctx.session_id.as_str())
                .run_id(ctx.run_id.as_str())
                .tool_call_id(ctx.tool_call_id.as_str())
                .team_name(ctx.active_team_name.as_deref().unwrap_or(""))
                .payload(serde_json::json!({
                    "to": to,
                    "variant": message.variant_name(),
                    "broadcast": to == BROADCAST_TOKEN,
                })),
        );

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
        let team_name = ctx.active_team_name.as_deref().unwrap_or("");
        let caller_name = if let Some(aid) = ctx.agent_id.as_ref() {
            names.name_for(&session, team_name, aid).await
        } else {
            // FALLBACK: when ctx.agent_id is None (e.g. Lead's user turn never
            // stamped its own agent_id onto the TurnState), assume Lead is the
            // caller IF a Lead is registered for this session.  Without this,
            // every Lead-originated SendMessage renders as `from="system"`.
            if names.resolve(&session, team_name, LEAD_NAME).await.is_some() {
                Some(LEAD_NAME.to_string())
            } else {
                None
            }
        };
        log::info!(
            "[SendMessage][diag] caller resolution: ctx.agent_id={:?} caller_name={:?} session={}",
            ctx.agent_id.as_ref().map(|a| a.as_str().to_string()),
            caller_name,
            session.as_str()
        );
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
            let team_handle = {
                // PR2 compat: use active_team_name if available, else fall back to first team.
                // PR3 will inject active_team_name properly.
                let teams = team_reg.list(&session).await;
                let first = teams.into_iter().next().ok_or_else(|| {
                    ToolError::ExecutionFailed(
                        "no team in this session — TeamCreate must be called first".into(),
                    )
                })?;
                first.1
            };

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
                if let Some(target_id) = names.resolve(&session, team_name, name).await {
                    if let Some(inbox) = inbox_reg.get(&session, team_name, &target_id).await {
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
                            if let Some(entry) = append_team_chat_entry(
                                ctx.conv_dir.as_deref(),
                                ctx.active_team_name.as_deref(),
                                caller_name.as_deref().unwrap_or("system"),
                                name,
                                &message,
                            ) {
                                emit_team_chat_appended(&ctx, entry).await;
                            }
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

            record_diagnostic(
                &ws,
                DiagnosticEvent::new("tool.send_message.broadcast.completed", DiagnosticSource::Backend)
                    .conversation_id(ctx.session_id.as_str())
                    .run_id(ctx.run_id.as_str())
                    .tool_call_id(ctx.tool_call_id.as_str())
                    .team_name(team_name)
                    .ok(missing.is_empty())
                    .payload(serde_json::json!({
                        "delivered": delivered,
                        "skipped_count": missing.len(),
                        "variant": message.variant_name(),
                    })),
            );
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

        let target_id = names.resolve(&session, team_name, &to).await.ok_or_else(|| {
            ToolError::ExecutionFailed(format!(
                "no agent named `{to}` in this session — call TeamCreate first or check the spelling"
            ))
        })?;
        let inbox = inbox_reg.get(&session, team_name, &target_id).await.ok_or_else(|| {
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

        if let Some(entry) = append_team_chat_entry(
            ctx.conv_dir.as_deref(),
            ctx.active_team_name.as_deref(),
            caller_name.as_deref().unwrap_or("system"),
            &to,
            &message,
        ) {
            emit_team_chat_appended(&ctx, entry).await;
        }

        record_diagnostic(
            &ws,
            DiagnosticEvent::new("tool.send_message.inbox_sent", DiagnosticSource::Backend)
                .conversation_id(ctx.session_id.as_str())
                .run_id(ctx.run_id.as_str())
                .tool_call_id(ctx.tool_call_id.as_str())
                .team_name(team_name)
                .ok(true)
                .payload(serde_json::json!({
                    "to_name": to,
                    "variant": message.variant_name(),
                })),
        );

        // P2.4 / B-gap1: if the recipient is the Lead, ask the supervisor
        // whether the Idle→Running CAS should fire.  Path A (turn-end
        // self-check in chat_turn_driver::run_chat_turn_s4) handles the
        // case where the Lead is currently running — pending is recorded
        // here and the driver emits `LeadHasPendingMessages` at turn end.
        // Path C: when `enqueue` returns true the supervisor itself invokes
        // the wake_fn previously installed by SessionRuntime, which
        // tokio::spawns a continuation turn.  This tool just logs the
        // outcome — no further work needed here.
        if to == LEAD_NAME {
            if let (Some(sup), Some(names_reg)) =
                (ctx.lead_idle.as_ref(), ctx.agent_names.as_ref())
            {
                if let Some(lead_id) = names_reg.resolve(&session, team_name, LEAD_NAME).await {
                    let key = (session.clone(), lead_id.clone());
                    let woke = sup.enqueue(&key, team_name.to_string()).await;
                    if woke {
                        log::info!(
                            "[SendMessage] Lead idle → Path C wake triggered \
                             (continuation turn spawned by supervisor)"
                        );
                        record_diagnostic(
                            &ws,
                            DiagnosticEvent::new("tool.send_message.path_c_enqueue", DiagnosticSource::Backend)
                                .conversation_id(ctx.session_id.as_str())
                                .run_id(ctx.run_id.as_str())
                                .tool_call_id(ctx.tool_call_id.as_str())
                                .agent_id(lead_id.as_str())
                                .team_name(team_name)
                                .ok(true)
                                .payload(serde_json::json!({ "transition": "idle_to_running", "wake_fired": true })),
                        );
                    } else {
                        log::info!(
                            "[SendMessage] Lead running → pending mark recorded for Path A session={} lead={}",
                            session.as_str(),
                            lead_id.as_str()
                        );
                        record_diagnostic(
                            &ws,
                            DiagnosticEvent::new("tool.send_message.path_c_enqueue", DiagnosticSource::Backend)
                                .conversation_id(ctx.session_id.as_str())
                                .run_id(ctx.run_id.as_str())
                                .tool_call_id(ctx.tool_call_id.as_str())
                                .agent_id(lead_id.as_str())
                                .team_name(team_name)
                                .ok(true)
                                .payload(serde_json::json!({ "transition": "already_running_pending_recorded", "wake_fired": false })),
                        );
                    }
                } else {
                    log::warn!(
                        "[SendMessage][diag] to=team-lead but agent_names.resolve(team-lead) returned None — Lead won't be woken"
                    );
                }
            } else {
                log::warn!(
                    "[SendMessage][diag] to=team-lead but ctx.lead_idle={} ctx.agent_names={} — Lead won't be woken",
                    ctx.lead_idle.is_some(),
                    ctx.agent_names.is_some()
                );
            }
        }

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
