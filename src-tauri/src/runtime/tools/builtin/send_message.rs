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
use std::path::Path;

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
pub(crate) fn append_team_chat_entry(
    conv_dir: Option<&std::path::Path>,
    team_name: Option<&str>,
    from: &str,
    to: &str,
    message: &StructuredMessage,
) {
    let Some(dir) = conv_dir else { return };
    let Some(team_name) = team_name else { return };
    let body = message.as_text().unwrap_or("").to_string();
    let mut entry = serde_json::json!({
        "ts": chrono::Utc::now().to_rfc3339(),
        "from": from,
        "to": to,
        "text": body,
        "variant": message.variant_name(),
    });
    // X 方案：协议握手变体把 approve / reason / feedback 透传进 jsonl。
    // 这样前端可以区分"同意退出"vs"拒绝退出"。omit 缺失字段以保持 jsonl 行紧凑。
    match message {
        StructuredMessage::ShutdownRequest { reason } => {
            if let Some(r) = reason {
                entry["reason"] = Value::String(r.clone());
            }
        }
        StructuredMessage::ShutdownResponse {
            approve, reason, ..
        } => {
            entry["approve"] = Value::Bool(*approve);
            if let Some(r) = reason {
                entry["reason"] = Value::String(r.clone());
            }
        }
        StructuredMessage::PlanApprovalResponse {
            approve, feedback, ..
        } => {
            entry["approve"] = Value::Bool(*approve);
            if let Some(f) = feedback {
                entry["feedback"] = Value::String(f.clone());
            }
        }
        _ => {}
    }
    use crate::runtime::agent::team_paths::TeamPaths;
    let path = TeamPaths::for_team(dir, team_name).team_chat_jsonl();
    let line = match serde_json::to_string(&entry) {
        Ok(s) => s,
        Err(e) => {
            log::warn!("[team_chat.jsonl] serialize failed: {e}");
            return;
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
                log::warn!(
                    "[team_chat.jsonl] write failed path={}: {e}",
                    path.display()
                );
            }
        }
        Err(e) => {
            log::warn!("[team_chat.jsonl] open failed path={}: {e}", path.display());
        }
    }
}

fn lead_has_pending_outbound_to_recipient(
    conv_dir: Option<&Path>,
    team_name: Option<&str>,
    from: &str,
    to: &str,
) -> bool {
    let Some(dir) = conv_dir else { return false };
    let Some(team_name) = team_name else {
        return false;
    };
    use crate::runtime::agent::team_paths::TeamPaths;
    let path = TeamPaths::for_team(dir, team_name).team_chat_jsonl();
    let Ok(raw) = std::fs::read_to_string(&path) else {
        return false;
    };

    let mut pending = false;
    for line in raw.lines().filter(|line| !line.trim().is_empty()) {
        let Ok(entry) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        let entry_from = entry.get("from").and_then(Value::as_str);
        let entry_to = entry.get("to").and_then(Value::as_str);
        if entry_from == Some(from) && entry_to == Some(to) {
            pending = true;
        } else if entry_from == Some(to) && entry_to == Some(from) {
            pending = false;
        }
    }
    pending
}

pub struct SendMessageRuntimeTool;

fn decode_structured_message(
    message_value: Value,
) -> std::result::Result<StructuredMessage, String> {
    match serde_json::from_value::<StructuredMessage>(message_value.clone()) {
        Ok(message) => Ok(message),
        Err(primary_err) => {
            let Some(raw) = message_value.as_str() else {
                return Err(primary_err.to_string());
            };
            match serde_json::from_str::<StructuredMessage>(raw) {
                Ok(message) => Ok(message),
                Err(secondary_err) => {
                    if let Some(message) = decode_lenient_text_message_string(raw) {
                        return Ok(message);
                    }
                    Err(format!(
                        "{primary_err}; also failed to parse JSON-string payload: {secondary_err}"
                    ))
                }
            }
        }
    }
}

fn decode_lenient_text_message_string(raw: &str) -> Option<StructuredMessage> {
    if !raw.contains("\"type\"") || !raw.contains("\"text\"") || !raw.contains("\"content\"") {
        return None;
    }
    let content_key = raw.find("\"content\"")?;
    let after_key = &raw[content_key + "\"content\"".len()..];
    let colon = after_key.find(':')?;
    let after_colon = after_key[colon + 1..].trim_start();
    let after_open_quote = after_colon.strip_prefix('"')?;
    let close_quote = after_open_quote.rfind('"')?;
    let content = &after_open_quote[..close_quote];
    Some(StructuredMessage::text(
        content
            .replace("\\n", "\n")
            .replace("\\\"", "\"")
            .replace("\\\\", "\\"),
    ))
}

#[async_trait]
impl RuntimeTool for SendMessageRuntimeTool {
    fn id(&self) -> &str {
        "SendMessage"
    }

    async fn definition(
        &self,
        _ctx: &crate::runtime::tools::ToolDescriptionContext,
    ) -> ToolDefinition {
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
            .ok_or_else(|| ToolError::ExecutionFailed("missing required string field `to`".into()))?
            .to_string();

        let message_value = input.get("message").cloned().ok_or_else(|| {
            ToolError::ExecutionFailed("missing required object field `message`".into())
        })?;
        let message: StructuredMessage = decode_structured_message(message_value).map_err(|e| {
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
            if names
                .resolve(&session, team_name, LEAD_NAME)
                .await
                .is_some()
            {
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
                            append_team_chat_entry(
                                ctx.conv_dir.as_deref(),
                                ctx.active_team_name.as_deref(),
                                caller_name.as_deref().unwrap_or("system"),
                                name,
                                &message,
                            );
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
                DiagnosticEvent::new(
                    "tool.send_message.broadcast.completed",
                    DiagnosticSource::Backend,
                )
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

        let caller_label = caller_name.as_deref().unwrap_or("system");
        if caller_label == LEAD_NAME
            && to != LEAD_NAME
            && lead_has_pending_outbound_to_recipient(
                ctx.conv_dir.as_deref(),
                ctx.active_team_name.as_deref(),
                caller_label,
                &to,
            )
        {
            record_diagnostic(
                &ws,
                DiagnosticEvent::new(
                    "tool.send_message.duplicate_pending_suppressed",
                    DiagnosticSource::Backend,
                )
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
            return Ok(ToolResult::new(
                "SendMessage",
                format!(
                    "suppressed duplicate {} to `{to}`; previous Lead message is still pending",
                    message.variant_name()
                ),
                Some(json!({
                    "delivered_to": to,
                    "variant": message.variant_name(),
                    "duplicate_suppressed": true,
                    "reason": "pending_outbound_without_reply",
                })),
            ));
        }

        inbox
            .send(InboxItem::ChatMessage {
                message: message.clone(),
                source,
            })
            .await
            .map_err(|_| {
                ToolError::ExecutionFailed(format!("agent `{to}` inbox closed; message dropped"))
            })?;

        append_team_chat_entry(
            ctx.conv_dir.as_deref(),
            ctx.active_team_name.as_deref(),
            caller_label,
            &to,
            &message,
        );

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
            if let (Some(sup), Some(names_reg)) = (ctx.lead_idle.as_ref(), ctx.agent_names.as_ref())
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::agent::team_paths::TeamPaths;
    use tempfile::tempdir;

    /// 读出 jsonl 的所有行，按 `\n` 拆分并 parse 成 Value。
    fn read_jsonl(path: &std::path::Path) -> Vec<Value> {
        let raw = std::fs::read_to_string(path).unwrap();
        raw.lines()
            .filter(|l| !l.trim().is_empty())
            .map(|l| serde_json::from_str::<Value>(l).unwrap())
            .collect()
    }

    #[test]
    fn append_team_chat_entry_writes_protocol_fields() {
        let dir = tempdir().unwrap();
        let conv_dir = dir.path();
        let paths = TeamPaths::for_team(conv_dir, "alpha");
        let jsonl = paths.team_chat_jsonl();

        // 4 个 variant 各一条，覆盖 X 方案三个字段全部分支。
        let cases = vec![
            (
                "team-lead",
                "pro",
                StructuredMessage::ShutdownRequest {
                    reason: Some("task done".into()),
                },
            ),
            (
                "pro",
                "team-lead",
                StructuredMessage::ShutdownResponse {
                    request_id: "rid-1".into(),
                    approve: true,
                    reason: None,
                },
            ),
            (
                "con",
                "team-lead",
                StructuredMessage::ShutdownResponse {
                    request_id: "rid-2".into(),
                    approve: false,
                    reason: Some("still working".into()),
                },
            ),
            (
                "team-lead",
                "con",
                StructuredMessage::PlanApprovalResponse {
                    request_id: "rid-3".into(),
                    approve: false,
                    feedback: Some("missed edge".into()),
                },
            ),
        ];
        for (from, to, msg) in &cases {
            append_team_chat_entry(Some(conv_dir), Some("alpha"), from, to, msg);
        }

        let entries = read_jsonl(&jsonl);
        assert_eq!(entries.len(), 4);

        assert_eq!(entries[0]["variant"], "shutdown_request");
        assert_eq!(entries[0]["reason"], "task done");
        assert!(entries[0].get("approve").is_none());
        assert!(entries[0].get("feedback").is_none());

        assert_eq!(entries[1]["variant"], "shutdown_response");
        assert_eq!(entries[1]["approve"], true);
        assert!(entries[1].get("reason").is_none());

        assert_eq!(entries[2]["variant"], "shutdown_response");
        assert_eq!(entries[2]["approve"], false);
        assert_eq!(entries[2]["reason"], "still working");

        assert_eq!(entries[3]["variant"], "plan_approval_response");
        assert_eq!(entries[3]["approve"], false);
        assert_eq!(entries[3]["feedback"], "missed edge");
    }

    #[test]
    fn append_team_chat_entry_omits_extra_fields_for_text() {
        let dir = tempdir().unwrap();
        let conv_dir = dir.path();
        let paths = TeamPaths::for_team(conv_dir, "alpha");
        let jsonl = paths.team_chat_jsonl();

        append_team_chat_entry(
            Some(conv_dir),
            Some("alpha"),
            "team-lead",
            "pro",
            &StructuredMessage::text("hi"),
        );

        let entries = read_jsonl(&jsonl);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0]["variant"], "text");
        assert_eq!(entries[0]["text"], "hi");
        // text variant 不应该泄漏协议字段。
        assert!(entries[0].get("approve").is_none());
        assert!(entries[0].get("reason").is_none());
        assert!(entries[0].get("feedback").is_none());
    }

    #[test]
    fn pending_outbound_tracks_reply_from_recipient() {
        let dir = tempdir().unwrap();
        let conv_dir = dir.path();

        assert!(!lead_has_pending_outbound_to_recipient(
            Some(conv_dir),
            Some("alpha"),
            LEAD_NAME,
            "growth-hacker"
        ));

        append_team_chat_entry(
            Some(conv_dir),
            Some("alpha"),
            LEAD_NAME,
            "growth-hacker",
            &StructuredMessage::text("请发言"),
        );
        assert!(lead_has_pending_outbound_to_recipient(
            Some(conv_dir),
            Some("alpha"),
            LEAD_NAME,
            "growth-hacker"
        ));

        append_team_chat_entry(
            Some(conv_dir),
            Some("alpha"),
            "growth-hacker",
            LEAD_NAME,
            &StructuredMessage::text("观点如下"),
        );
        assert!(!lead_has_pending_outbound_to_recipient(
            Some(conv_dir),
            Some("alpha"),
            LEAD_NAME,
            "growth-hacker"
        ));
    }

    #[test]
    fn decode_structured_message_accepts_json_object() {
        let value = json!({ "type": "text", "content": "hello" });

        let message = decode_structured_message(value).unwrap();

        assert_eq!(message, StructuredMessage::text("hello"));
    }

    #[test]
    fn decode_structured_message_accepts_json_string_object() {
        let value = Value::String(r#"{"type":"text","content":"hello"}"#.to_string());

        let message = decode_structured_message(value).unwrap();

        assert_eq!(message, StructuredMessage::text("hello"));
    }

    #[test]
    fn decode_structured_message_accepts_json_string_with_raw_newlines() {
        let value =
            Value::String("{\"type\":\"text\",\"content\":\"第一行\n\n第二行\"}".to_string());

        let message = decode_structured_message(value).unwrap();

        assert_eq!(message, StructuredMessage::text("第一行\n\n第二行"));
    }

    #[test]
    fn decode_structured_message_rejects_plain_text_string() {
        let err = decode_structured_message(Value::String("hello".to_string())).unwrap_err();

        assert!(err.contains("failed to parse JSON-string payload"));
    }
}
