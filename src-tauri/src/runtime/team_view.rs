//! Read-only view over a conversation's team activity.
//!
//! Given a conversation directory, reconstruct:
//!   • the team session (TeamCreate → TeamDelete window, members)
//!   • the chronological event stream (SendMessage, peer-messages, spawn/stop)
//!
//! The data sources are entirely on disk — no runtime state required:
//!   • `messages.jsonl`           → main lead transcript (tool calls + XML)
//!   • `teammates/{id}.meta.json` → per-teammate identity
//!   • `teammates/{id}.jsonl`     → per-teammate transcript (for drill-down)

use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::storage::file_store::AppStorage;

const PEER_MESSAGES_OPEN: &str = "<peer-messages>";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TeamOverview {
    pub conversation_id: String,
    /// One TeamCreate → TeamDelete window. Multiple if the user re-created
    /// a team after deleting one (rare but legal).
    pub teams: Vec<TeamSession>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TeamSession {
    pub team_id: String,
    pub team_name: Option<String>,
    pub created_at: String,
    pub deleted_at: Option<String>,
    pub members: Vec<TeamAgent>,
    pub events: Vec<TeamEvent>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TeamAgent {
    pub agent_id: String,
    pub agent_name: String,
    pub spawned_at: String,
    #[serde(default)]
    pub is_async: bool,
    /// True when this teammate has its own transcript file on disk.
    /// Frontend uses this to decide whether the "drill into details" affordance
    /// should be enabled.
    pub has_transcript: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TeamEvent {
    TeamCreate {
        ts: String,
        team_name: Option<String>,
    },
    TeamDelete {
        ts: String,
    },
    AgentSpawn {
        ts: String,
        agent_id: String,
        agent_name: String,
    },
    AgentStop {
        ts: String,
        agent_name: String,
    },
    SendMessage {
        ts: String,
        from: String,
        to: String,
        text: String,
        is_error: bool,
        tool_call_id: String,
    },
    PeerMessage {
        ts: String,
        from: String,
        to: String,
        text: String,
        variant: String,
    },
}

impl TeamEvent {
    #[allow(dead_code)]
    fn ts(&self) -> &str {
        match self {
            TeamEvent::TeamCreate { ts, .. } => ts,
            TeamEvent::TeamDelete { ts } => ts,
            TeamEvent::AgentSpawn { ts, .. } => ts,
            TeamEvent::AgentStop { ts, .. } => ts,
            TeamEvent::SendMessage { ts, .. } => ts,
            TeamEvent::PeerMessage { ts, .. } => ts,
        }
    }
}

// ─── Public API ──────────────────────────────────────────────────────────────

/// Build a team overview from the conversation's on-disk state.
///
/// Always returns `Ok(TeamOverview)` — even for conversations that never had
/// a team (in that case `teams` is empty). Errors only when the disk read
/// itself fails.
pub fn build_team_overview(
    storage: &AppStorage,
    conversation_id: &str,
) -> anyhow::Result<TeamOverview> {
    let base_dir = storage.base_dir();
    let messages = storage.get_messages(conversation_id)?;
    let conv_dir = base_dir
        .join("conversations")
        .join(conversation_id);

    let members = load_teammates(&conv_dir.join("teammates"));
    let events = extract_events(&messages, &members);
    let teams = group_events_into_teams(events, members, conversation_id);

    Ok(TeamOverview {
        conversation_id: conversation_id.to_string(),
        teams,
    })
}

/// Read one teammate's full transcript jsonl. Returns the parsed entries
/// (already JSON-shaped, frontend can render however).
pub fn read_teammate_transcript(
    storage: &AppStorage,
    conversation_id: &str,
    agent_id: &str,
) -> anyhow::Result<Vec<Value>> {
    let path = storage
        .base_dir()
        .join("conversations")
        .join(conversation_id)
        .join("teammates")
        .join(format!("{agent_id}.jsonl"));

    if !path.exists() {
        return Ok(Vec::new());
    }

    let raw = fs::read_to_string(&path)?;
    let entries = raw
        .lines()
        .filter_map(|line| {
            let trimmed = strip_trailing_marker(line.trim());
            if trimmed.is_empty() {
                return None;
            }
            serde_json::from_str::<Value>(trimmed).ok()
        })
        .collect();
    Ok(entries)
}

// ─── Internals ───────────────────────────────────────────────────────────────

/// Strip a trailing `\t<checkmark>` integrity marker if present.
/// On-disk lines may look like `{...json...}\t✓` — we want just the JSON.
fn strip_trailing_marker(line: &str) -> &str {
    match line.find('\t') {
        Some(idx) => &line[..idx],
        None => line,
    }
}

fn load_teammates(teammates_dir: &Path) -> Vec<TeamAgent> {
    let mut out: Vec<TeamAgent> = Vec::new();
    let Ok(entries) = fs::read_dir(teammates_dir) else {
        return out;
    };

    for entry in entries.flatten() {
        let path = entry.path();
        let Some(name) = path.file_name().and_then(|s| s.to_str()) else {
            continue;
        };
        if !name.ends_with(".meta.json") {
            continue;
        }
        let Ok(raw) = fs::read_to_string(&path) else {
            continue;
        };
        let Ok(meta) = serde_json::from_str::<Value>(&raw) else {
            continue;
        };
        let agent_id = meta
            .get("agent_id")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string();
        if agent_id.is_empty() {
            continue;
        }
        let agent_name = meta
            .get("agent_name")
            .and_then(|v| v.as_str())
            .unwrap_or("?")
            .to_string();
        let spawned_at = meta
            .get("spawned_at")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let is_async = meta.get("is_async").and_then(|v| v.as_bool()).unwrap_or(false);
        let transcript_path = teammates_dir.join(format!("{agent_id}.jsonl"));
        let has_transcript = transcript_path.exists();
        out.push(TeamAgent {
            agent_id,
            agent_name,
            spawned_at,
            is_async,
            has_transcript,
        });
    }

    // Stable order: by spawn time ascending.
    out.sort_by(|a, b| a.spawned_at.cmp(&b.spawned_at));
    out
}

/// Scan the lead's main message timeline and extract every team-relevant
/// event. Returns events in the order they appear in the conversation
/// (which is already chronological — see file_store::messages sort).
fn extract_events(messages: &[Value], members: &[TeamAgent]) -> Vec<TeamEvent> {
    let mut events: Vec<TeamEvent> = Vec::new();

    for msg in messages {
        let role = msg.get("role").and_then(|v| v.as_str()).unwrap_or("");
        let ts = msg
            .get("createdAt")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        match role {
            "assistant" => {
                if let Some(tool_calls) = msg.get("toolCalls").and_then(|v| v.as_array()) {
                    for tc in tool_calls {
                        push_event_for_assistant_tool_call(&mut events, tc, &ts);
                    }
                }
            }
            "tool" => {
                if let Some(tr) = msg.get("toolResult") {
                    push_event_for_tool_result(&mut events, tr, &ts, members);
                }
            }
            "user" => {
                // <peer-messages> XML payload — teammate replies funneled to lead.
                if let Some(text) = msg
                    .get("content")
                    .and_then(|c| c.get("text"))
                    .and_then(|v| v.as_str())
                {
                    push_events_from_peer_xml(&mut events, text, &ts);
                }
            }
            _ => {}
        }
    }

    events
}

fn push_event_for_assistant_tool_call(out: &mut Vec<TeamEvent>, tc: &Value, ts: &str) {
    let name = tc.get("name").and_then(|v| v.as_str()).unwrap_or("");
    let args = tc.get("arguments").cloned().unwrap_or(Value::Null);
    let tool_call_id = tc
        .get("id")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    match name {
        "SendMessage" => {
            let to = args
                .get("to")
                .and_then(|v| v.as_str())
                .unwrap_or("?")
                .to_string();
            // `message` can be a string OR an object `{type, content}`.
            let text = extract_send_message_text(args.get("message"));
            out.push(TeamEvent::SendMessage {
                ts: ts.to_string(),
                from: "team-lead".to_string(),
                to,
                text,
                is_error: false, // refined by tool_result below
                tool_call_id,
            });
        }
        "TeamCreate" => {
            let team_name = args
                .get("team_name")
                .and_then(|v| v.as_str())
                .map(|s| s.to_string());
            // TeamCreate's authoritative team_name is set by the tool itself
            // (often derived from session id); we'll overwrite from tool_result.
            out.push(TeamEvent::TeamCreate {
                ts: ts.to_string(),
                team_name,
            });
        }
        "TeamDelete" => {
            out.push(TeamEvent::TeamDelete { ts: ts.to_string() });
        }
        _ => {}
    }
}

fn push_event_for_tool_result(
    out: &mut Vec<TeamEvent>,
    tr: &Value,
    ts: &str,
    members: &[TeamAgent],
) {
    let name = tr.get("name").and_then(|v| v.as_str()).unwrap_or("");
    let is_error = tr.get("isError").and_then(|v| v.as_bool()).unwrap_or(false);
    let tool_call_id = tr
        .get("toolCallId")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let content_str = tr
        .get("content")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    match name {
        "SendMessage" => {
            // Annotate the previously-pushed SendMessage event with is_error.
            if let Some(TeamEvent::SendMessage {
                is_error: ref mut e,
                tool_call_id: ref tcid,
                ..
            }) = out.iter_mut().rev().find(|ev| {
                matches!(
                    ev,
                    TeamEvent::SendMessage { tool_call_id: t, .. } if !t.is_empty() && *t == tool_call_id
                )
            }) {
                if *tcid == tool_call_id {
                    *e = is_error;
                }
            }
        }
        "TeamCreate" => {
            // Backfill team_name from the structured result (best-effort: also
            // emit a fallback to the textual `Team \`name\` created ...`
            // sentence). Looks for the most recent TeamCreate event.
            let parsed: Option<String> = serde_json::from_str::<Value>(&content_str)
                .ok()
                .and_then(|v| {
                    v.get("team_name")
                        .and_then(|t| t.as_str())
                        .map(|s| s.to_string())
                });
            let resolved = parsed.or_else(|| parse_team_name_from_text(&content_str));
            if let Some(name) = resolved {
                for ev in out.iter_mut().rev() {
                    if let TeamEvent::TeamCreate { team_name, .. } = ev {
                        if team_name.is_none() {
                            *team_name = Some(name);
                        }
                        break;
                    }
                }
            }
        }
        "Agent" => {
            // Spawn signal — content_str is `{agent_id, name, status:"teammate_spawned"}`
            if let Ok(v) = serde_json::from_str::<Value>(&content_str) {
                if v.get("status").and_then(|s| s.as_str()) == Some("teammate_spawned") {
                    let agent_id = v
                        .get("agent_id")
                        .and_then(|s| s.as_str())
                        .unwrap_or("")
                        .to_string();
                    let agent_name = v
                        .get("name")
                        .and_then(|s| s.as_str())
                        .unwrap_or("?")
                        .to_string();
                    if !agent_id.is_empty() {
                        out.push(TeamEvent::AgentSpawn {
                            ts: ts.to_string(),
                            agent_id,
                            agent_name,
                        });
                    }
                }
            }
        }
        "TeammateStop" => {
            // content is plain text: "Teammate `<name>` cancelled"
            if let Some(name) = parse_teammate_name_from_stop_text(&content_str) {
                out.push(TeamEvent::AgentStop {
                    ts: ts.to_string(),
                    agent_name: name,
                });
            }
        }
        _ => {}
    }
    let _ = members; // reserved for future enrichment (e.g. resolve agent_id → name)
}

fn extract_send_message_text(value: Option<&Value>) -> String {
    let Some(v) = value else {
        return String::new();
    };
    if let Some(s) = v.as_str() {
        return s.to_string();
    }
    if let Some(obj) = v.as_object() {
        if let Some(content) = obj.get("content").and_then(|c| c.as_str()) {
            return content.to_string();
        }
    }
    // Fallback: stringify whatever was passed.
    v.to_string()
}

fn parse_team_name_from_text(text: &str) -> Option<String> {
    // "Team `<name>` created for session ..."
    let prefix = "Team `";
    let suffix = "` created";
    let start = text.find(prefix)? + prefix.len();
    let end = text[start..].find(suffix)?;
    Some(text[start..start + end].to_string())
}

fn parse_teammate_name_from_stop_text(text: &str) -> Option<String> {
    // "Teammate `<name>` cancelled"
    let prefix = "Teammate `";
    let suffix = "` cancelled";
    let start = text.find(prefix)? + prefix.len();
    let end = text[start..].find(suffix)?;
    Some(text[start..start + end].to_string())
}

fn push_events_from_peer_xml(out: &mut Vec<TeamEvent>, xml: &str, ts: &str) {
    let trimmed = xml.trim();
    if !trimmed.starts_with(PEER_MESSAGES_OPEN) {
        return;
    }
    // Lazy, non-regex parse: find each `<peer-message from="X" variant="Y">...</peer-message>`.
    let mut cursor = 0usize;
    while let Some(open_idx) = trimmed[cursor..].find("<peer-message ") {
        let abs_open = cursor + open_idx;
        // Find end of opening tag
        let Some(rel_close_open) = trimmed[abs_open..].find('>') else {
            break;
        };
        let abs_close_open = abs_open + rel_close_open;
        let open_tag = &trimmed[abs_open..=abs_close_open];

        let from = extract_attr(open_tag, "from").unwrap_or_else(|| "?".to_string());
        let variant = extract_attr(open_tag, "variant").unwrap_or_else(|| "text".to_string());

        let body_start = abs_close_open + 1;
        let Some(rel_end) = trimmed[body_start..].find("</peer-message>") else {
            break;
        };
        let abs_end = body_start + rel_end;
        let body = trimmed[body_start..abs_end].trim().to_string();

        out.push(TeamEvent::PeerMessage {
            ts: ts.to_string(),
            from,
            to: "team-lead".to_string(),
            text: body,
            variant,
        });

        cursor = abs_end + "</peer-message>".len();
    }
}

fn extract_attr(open_tag: &str, attr: &str) -> Option<String> {
    let key = format!("{attr}=\"");
    let start = open_tag.find(&key)? + key.len();
    let end_rel = open_tag[start..].find('"')?;
    Some(open_tag[start..start + end_rel].to_string())
}

/// Group the flat event stream into TeamCreate→TeamDelete windows. Events
/// before the first TeamCreate (defensive: shouldn't happen) and any tail
/// events after a TeamDelete with no subsequent TeamCreate are also kept.
fn group_events_into_teams(
    events: Vec<TeamEvent>,
    members: Vec<TeamAgent>,
    conversation_id: &str,
) -> Vec<TeamSession> {
    let mut sessions: Vec<TeamSession> = Vec::new();
    let mut current: Option<TeamSession> = None;
    let mut team_seq = 0u32;

    for ev in events {
        match &ev {
            TeamEvent::TeamCreate { ts, team_name } => {
                if let Some(prev) = current.take() {
                    sessions.push(prev);
                }
                team_seq += 1;
                current = Some(TeamSession {
                    team_id: format!("{conversation_id}#{team_seq}"),
                    team_name: team_name.clone(),
                    created_at: ts.clone(),
                    deleted_at: None,
                    members: Vec::new(),
                    events: vec![ev],
                });
            }
            TeamEvent::TeamDelete { ts } => {
                if let Some(mut session) = current.take() {
                    session.deleted_at = Some(ts.clone());
                    session.events.push(ev);
                    sessions.push(session);
                }
            }
            _ => {
                if let Some(session) = current.as_mut() {
                    session.events.push(ev);
                }
            }
        }
    }

    if let Some(session) = current {
        sessions.push(session);
    }

    // Assign members to whichever session was open at their spawned_at time.
    // Members are kept simple: each one appears in the team they were spawned into.
    for member in members {
        if let Some(session) = sessions
            .iter_mut()
            .find(|s| member.spawned_at >= s.created_at
                && match &s.deleted_at {
                    Some(end) => &member.spawned_at <= end,
                    None => true,
                })
        {
            session.members.push(member);
        } else if let Some(last) = sessions.last_mut() {
            last.members.push(member);
        }
    }

    sessions
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_jsonl_integrity_marker() {
        assert_eq!(strip_trailing_marker("{\"a\":1}\t✓"), "{\"a\":1}");
        assert_eq!(strip_trailing_marker("{\"a\":1}"), "{\"a\":1}");
    }

    #[test]
    fn extracts_team_name_from_textual_result() {
        let text = "Team `debate-team` created for session `abc` with Lead `team-lead`";
        assert_eq!(parse_team_name_from_text(text), Some("debate-team".into()));
    }

    #[test]
    fn parses_teammate_stop_text() {
        let text = "Teammate `host` cancelled";
        assert_eq!(
            parse_teammate_name_from_stop_text(text),
            Some("host".into())
        );
    }

    #[test]
    fn send_message_text_from_string_or_object() {
        assert_eq!(
            extract_send_message_text(Some(&serde_json::json!("hi"))),
            "hi".to_string()
        );
        assert_eq!(
            extract_send_message_text(Some(
                &serde_json::json!({ "type": "text", "content": "hello" })
            )),
            "hello".to_string()
        );
    }

    #[test]
    fn peer_message_xml_parse() {
        let xml = r#"<peer-messages>
  <peer-message from="pro" variant="text">立论完成</peer-message>
  <peer-message from="con" variant="text">收到</peer-message>
</peer-messages>"#;
        let mut out = Vec::new();
        push_events_from_peer_xml(&mut out, xml, "2026-05-13T00:00:00Z");
        assert_eq!(out.len(), 2);
        match &out[0] {
            TeamEvent::PeerMessage { from, text, .. } => {
                assert_eq!(from, "pro");
                assert_eq!(text, "立论完成");
            }
            _ => panic!("expected PeerMessage"),
        }
    }

    #[test]
    fn groups_events_into_team_sessions() {
        let conv = "conv-1";
        let events = vec![
            TeamEvent::TeamCreate {
                ts: "2026-01-01T00:00:00Z".into(),
                team_name: Some("alpha".into()),
            },
            TeamEvent::SendMessage {
                ts: "2026-01-01T00:00:01Z".into(),
                from: "team-lead".into(),
                to: "pro".into(),
                text: "go".into(),
                is_error: false,
                tool_call_id: "t1".into(),
            },
            TeamEvent::TeamDelete {
                ts: "2026-01-01T00:01:00Z".into(),
            },
            TeamEvent::TeamCreate {
                ts: "2026-01-01T00:02:00Z".into(),
                team_name: Some("beta".into()),
            },
        ];
        let sessions = group_events_into_teams(events, vec![], conv);
        assert_eq!(sessions.len(), 2);
        assert_eq!(sessions[0].team_name.as_deref(), Some("alpha"));
        assert_eq!(sessions[0].events.len(), 3);
        assert!(sessions[0].deleted_at.is_some());
        assert_eq!(sessions[1].team_name.as_deref(), Some("beta"));
        assert!(sessions[1].deleted_at.is_none());
    }
}
