//! Read-only view over a conversation's team activity.
//!
//! Per-team disk layout v2 (`docs/superpowers/specs/2026-05-14-...`):
//! a conversation can host multiple teams, each rooted at
//! `<conv>/teams/{name}/{config.json, team-chat.jsonl, tasks/, teammates/}`.
//!
//! `build_team_overview` returns one `TeamSession` per `teams/*` directory
//! on disk, in descending `created_at` order.  Empty list means "this
//! conversation has no teams yet".

use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::runtime::agent::team_paths::TeamPaths;
use crate::runtime::agent::TeamSnapshot;
use crate::storage::file_store::AppStorage;

fn default_variant() -> String {
    "text".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TeamOverview {
    pub conversation_id: String,
    /// One entry per `teams/{name}/` directory on disk.  Sorted by
    /// `created_at` ascending (oldest first, newest last) so the drawer
    /// timeline reads top-down chronologically and `MessageList` can pair
    /// TeamCreate turns with `teams[i]` by ordinal.
    pub teams: Vec<TeamSession>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TeamSession {
    /// Stable identifier: `{conversation_id}#{team_name}`.
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
    #[serde(rename_all = "camelCase")]
    TeamCreate {
        ts: String,
        team_name: Option<String>,
    },
    TeamDelete {
        ts: String,
    },
    #[serde(rename_all = "camelCase")]
    AgentSpawn {
        ts: String,
        agent_id: String,
        agent_name: String,
    },
    #[serde(rename_all = "camelCase")]
    AgentStop {
        ts: String,
        agent_name: String,
    },
    #[serde(rename_all = "camelCase")]
    SendMessage {
        ts: String,
        from: String,
        to: String,
        text: String,
        is_error: bool,
        tool_call_id: String,
        /// StructuredMessage 的 type 字段（snake_case）。老 jsonl 行没有
        /// 该字段时回退到 "text"，保证历史会话不出现"未知 variant"。
        #[serde(default = "default_variant")]
        variant: String,
        /// 仅 ShutdownResponse / PlanApprovalResponse 出现。
        #[serde(default, skip_serializing_if = "Option::is_none")]
        approve: Option<bool>,
        /// ShutdownRequest 或 ShutdownResponse 可选携带的说明。
        #[serde(default, skip_serializing_if = "Option::is_none")]
        reason: Option<String>,
        /// 仅 PlanApprovalResponse 可选携带的反馈。
        #[serde(default, skip_serializing_if = "Option::is_none")]
        feedback: Option<String>,
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
/// Returns `Ok(TeamOverview)` even for conversations that never had any
/// teams — `teams` is just empty in that case.  Errors only when the
/// `messages.jsonl` read itself fails (per-team config.json parse errors
/// are silently skipped so one bad team doesn't break the rest).
pub fn build_team_overview(
    storage: &AppStorage,
    conversation_id: &str,
) -> anyhow::Result<TeamOverview> {
    let base_dir = storage.base_dir();
    let conv_dir = base_dir.join("conversations").join(conversation_id);
    let messages = storage.get_messages(conversation_id)?;
    let lifecycle = extract_lifecycle_events(&messages);

    let mut teams = scan_teams_dir(&conv_dir, conversation_id, &lifecycle);
    // 按 created_at 升序：旧 team 在前、新 team 在后。这个顺序服务两条消费者：
    // ① 抽屉时间线（IM 风格：旧在上、新在下，默认滚到底就看到最新 team）；
    // ② 主聊天里 TeamProgressBlock 卡片按 turn ordinal 配 overview.teams[i]
    //    （MessageList::teamSessionForTurnIdx），turns 是时间正序，
    //    overview.teams 也得是正序才能让"第 N 个 TeamCreate turn ↔ 第 N 个
    //    team session"匹配上。
    teams.sort_by(|a, b| a.created_at.cmp(&b.created_at));

    Ok(TeamOverview {
        conversation_id: conversation_id.to_string(),
        teams,
    })
}

/// Read one teammate's full transcript jsonl.  Per-team layout v2: searches
/// every `teams/*/teammates/{agent_id}.jsonl` and returns the first hit.
pub fn read_teammate_transcript(
    storage: &AppStorage,
    conversation_id: &str,
    agent_id: &str,
) -> anyhow::Result<Vec<Value>> {
    let conv_dir = storage
        .base_dir()
        .join("conversations")
        .join(conversation_id);

    let teams_root = conv_dir.join("teams");
    let Ok(entries) = fs::read_dir(&teams_root) else {
        return Ok(Vec::new());
    };

    for entry in entries.flatten() {
        let team_dir = entry.path();
        let team_name = match entry.file_name().into_string() {
            Ok(s) => s,
            Err(_) => continue,
        };
        let path = TeamPaths::for_team(&conv_dir, &team_name).teammate_transcript(agent_id);
        if !path.exists() {
            // Defensive: tolerate teams/ entries that aren't directories.
            let _ = team_dir;
            continue;
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
        return Ok(entries);
    }

    Ok(Vec::new())
}

// ─── Internals ───────────────────────────────────────────────────────────────

/// Strip a trailing `\t<checkmark>` integrity marker if present.
fn strip_trailing_marker(line: &str) -> &str {
    match line.find('\t') {
        Some(idx) => &line[..idx],
        None => line,
    }
}

#[derive(Debug, Default, Clone)]
struct LifecycleByTeam {
    /// `team_name → Vec<TeamEvent>` for events we *can* attribute to a
    /// specific team from `messages.jsonl` alone (TeamCreate / TeamDelete /
    /// AgentSpawn — all carry `team_name` in their tool args or result).
    by_team: std::collections::HashMap<String, Vec<TeamEvent>>,
    /// Pending stop events keyed by `agent_name`.  Stop events come from
    /// `TeammateStop` tool results whose textual `content` only carries the
    /// agent name, not the team_name — so we can't bucket them upfront.
    /// `scan_teams_dir` resolves the owning team at render time using each
    /// team's member roster (a teammate name is unique within a team, and
    /// a teammate's whole life happens inside one team — so first-match by
    /// member roster is enough).
    stops_by_agent_name: std::collections::HashMap<String, Vec<TeamEvent>>,
}

impl LifecycleByTeam {
    /// Drain the team-attributed events plus any stop events whose
    /// `agent_name` is in the given member set.  `member_names` should be
    /// the names of teammates registered to this team (from the on-disk
    /// `config.json` snapshot).
    fn drain_for(&mut self, team_name: &str, member_names: &std::collections::HashSet<String>) -> Vec<TeamEvent> {
        let mut events = self.by_team.remove(team_name).unwrap_or_default();
        // Reattach stop events whose agent_name belongs to this team.
        // We mutate stops_by_agent_name in-place: each matching key is
        // drained so a subsequent team doesn't see the same stop twice.
        let matching_names: Vec<String> = self
            .stops_by_agent_name
            .keys()
            .filter(|n| member_names.contains(*n))
            .cloned()
            .collect();
        for n in matching_names {
            if let Some(mut stops) = self.stops_by_agent_name.remove(&n) {
                events.append(&mut stops);
            }
        }
        events
    }
}

fn extract_lifecycle_events(messages: &[Value]) -> LifecycleByTeam {
    let mut out = LifecycleByTeam::default();
    let mut pending_team_name: Option<String> = None;

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
                        let name = tc.get("name").and_then(|v| v.as_str()).unwrap_or("");
                        let args = tc.get("arguments").cloned().unwrap_or(Value::Null);
                        match name {
                            "TeamCreate" => {
                                let team_name = args
                                    .get("team_name")
                                    .and_then(|v| v.as_str())
                                    .map(|s| s.to_string());
                                pending_team_name = team_name.clone();
                            }
                            "TeamDelete" => {
                                let team_name = args
                                    .get("team_name")
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("")
                                    .to_string();
                                out.by_team
                                    .entry(team_name)
                                    .or_default()
                                    .push(TeamEvent::TeamDelete { ts: ts.clone() });
                            }
                            _ => {}
                        }
                    }
                }
            }
            "tool" => {
                if let Some(tr) = msg.get("toolResult") {
                    let name = tr.get("name").and_then(|v| v.as_str()).unwrap_or("");
                    let content_str = tr
                        .get("content")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();

                    match name {
                        "TeamCreate" => {
                            let parsed_team_name = serde_json::from_str::<Value>(&content_str)
                                .ok()
                                .and_then(|v| {
                                    v.get("team_name")
                                        .and_then(|t| t.as_str())
                                        .map(|s| s.to_string())
                                })
                                .or_else(|| parse_team_name_from_text(&content_str))
                                .or_else(|| pending_team_name.take());

                            if let Some(team_name) = parsed_team_name {
                                out.by_team.entry(team_name.clone()).or_default().push(
                                    TeamEvent::TeamCreate {
                                        ts: ts.clone(),
                                        team_name: Some(team_name),
                                    },
                                );
                            }
                        }
                        "Agent" => {
                            if let Ok(v) = serde_json::from_str::<Value>(&content_str) {
                                if v.get("status").and_then(|s| s.as_str())
                                    == Some("teammate_spawned")
                                {
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
                                    let team_name = v
                                        .get("team_name")
                                        .and_then(|s| s.as_str())
                                        .unwrap_or("")
                                        .to_string();
                                    if !agent_id.is_empty() {
                                        out.by_team.entry(team_name).or_default().push(
                                            TeamEvent::AgentSpawn {
                                                ts: ts.clone(),
                                                agent_id,
                                                agent_name,
                                            },
                                        );
                                    }
                                }
                            }
                        }
                        "TeammateStop" => {
                            if let Some(name) = parse_teammate_name_from_stop_text(&content_str) {
                                // Stop events can't be attributed to a
                                // team_name from the textual content alone.
                                // Stash them under agent_name; scan_teams_dir
                                // resolves the owning team via the team's
                                // on-disk member roster.
                                out.stops_by_agent_name
                                    .entry(name.clone())
                                    .or_default()
                                    .push(TeamEvent::AgentStop {
                                        ts: ts.clone(),
                                        agent_name: name,
                                    });
                            }
                        }
                        _ => {}
                    }
                }
            }
            _ => {}
        }
    }

    out
}

fn scan_teams_dir(
    conv_dir: &Path,
    conversation_id: &str,
    lifecycle: &LifecycleByTeam,
) -> Vec<TeamSession> {
    let teams_root = conv_dir.join("teams");
    let mut sessions = Vec::new();

    let Ok(entries) = fs::read_dir(&teams_root) else {
        return sessions;
    };

    for entry in entries.flatten() {
        let team_dir = entry.path();
        if !team_dir.is_dir() {
            continue;
        }
        let team_name = match entry.file_name().into_string() {
            Ok(s) => s,
            Err(_) => continue,
        };
        let paths = TeamPaths::for_team(conv_dir, &team_name);
        let config_path = paths.config_json();
        let Ok(bytes) = fs::read(&config_path) else {
            continue;
        };
        let snapshot: TeamSnapshot = match serde_json::from_slice(&bytes) {
            Ok(s) => s,
            Err(err) => {
                log::warn!(
                    "[team_view] failed to parse {}: {err}",
                    config_path.display()
                );
                continue;
            }
        };

        let mut lc = lifecycle.clone();
        let members = load_teammates_from_snapshot(&snapshot, &paths);
        let member_names: std::collections::HashSet<String> =
            members.iter().map(|m| m.agent_name.clone()).collect();
        let mut events = lc.drain_for(&team_name, &member_names);
        append_events_from_team_chat_jsonl(&mut events, &paths.team_chat_jsonl(), &team_name);
        events.sort_by(|a, b| a.ts().cmp(b.ts()));

        sessions.push(TeamSession {
            team_id: format!("{conversation_id}#{team_name}"),
            team_name: Some(team_name),
            created_at: snapshot.created_at.to_rfc3339(),
            // 软删除：TeamDelete 时由 mark_deleted_on_disk 写入；live team 为 None。
            deleted_at: snapshot.deleted_at.map(|t| t.to_rfc3339()),
            members,
            events,
        });
    }

    sessions
}

fn load_teammates_from_snapshot(snapshot: &TeamSnapshot, paths: &TeamPaths<'_>) -> Vec<TeamAgent> {
    let mut out: Vec<TeamAgent> = Vec::new();
    // Lead first.
    {
        let lead = &snapshot.lead;
        let transcript = paths.teammate_transcript(lead.agent_id.as_str());
        out.push(TeamAgent {
            agent_id: lead.agent_id.as_str().to_string(),
            agent_name: lead.name.clone(),
            spawned_at: lead.created_at.to_rfc3339(),
            is_async: false,
            has_transcript: transcript.exists(),
        });
    }
    for m in &snapshot.teammates {
        let transcript = paths.teammate_transcript(m.agent_id.as_str());
        out.push(TeamAgent {
            agent_id: m.agent_id.as_str().to_string(),
            agent_name: m.name.clone(),
            spawned_at: m.created_at.to_rfc3339(),
            is_async: false,
            has_transcript: transcript.exists(),
        });
    }
    out
}

/// Read `<team_dir>/team-chat.jsonl` (if present) and push every entry as a
/// `SendMessage` event scoped to `team_name`.
fn append_events_from_team_chat_jsonl(out: &mut Vec<TeamEvent>, path: &Path, _team_name: &str) {
    let Ok(raw) = fs::read_to_string(path) else {
        return;
    };
    for line in raw.lines() {
        let trimmed = strip_trailing_marker(line.trim());
        if trimmed.is_empty() {
            continue;
        }
        let Ok(v) = serde_json::from_str::<Value>(trimmed) else {
            continue;
        };
        let ts = v
            .get("ts")
            .and_then(|x| x.as_str())
            .unwrap_or("")
            .to_string();
        let from = v
            .get("from")
            .and_then(|x| x.as_str())
            .unwrap_or("system")
            .to_string();
        let to = v
            .get("to")
            .and_then(|x| x.as_str())
            .unwrap_or("?")
            .to_string();
        let text = v
            .get("text")
            .and_then(|x| x.as_str())
            .unwrap_or("")
            .to_string();
        let variant = v
            .get("variant")
            .and_then(|x| x.as_str())
            .unwrap_or("text")
            .to_string();
        let approve = v.get("approve").and_then(|x| x.as_bool());
        let reason = v
            .get("reason")
            .and_then(|x| x.as_str())
            .map(|s| s.to_string());
        let feedback = v
            .get("feedback")
            .and_then(|x| x.as_str())
            .map(|s| s.to_string());
        if ts.is_empty() {
            continue;
        }
        out.push(TeamEvent::SendMessage {
            ts,
            from,
            to,
            text,
            is_error: false,
            tool_call_id: String::new(),
            variant,
            approve,
            reason,
            feedback,
        });
    }
}

fn parse_team_name_from_text(text: &str) -> Option<String> {
    let prefix = "Team `";
    let suffix = "` created";
    let start = text.find(prefix)? + prefix.len();
    let end = text[start..].find(suffix)?;
    Some(text[start..start + end].to_string())
}

fn parse_teammate_name_from_stop_text(text: &str) -> Option<String> {
    let prefix = "Teammate `";
    let suffix = "` cancelled";
    let start = text.find(prefix)? + prefix.len();
    let end = text[start..].find(suffix)?;
    Some(text[start..start + end].to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::agent::{Member, MemberRole, TeamRegistry};
    use crate::runtime::ids::{AgentId, SessionId};
    use tempfile::tempdir;

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

    fn dummy_lead(name: &str) -> Member {
        Member {
            agent_id: AgentId::new(format!("lead-{name}")),
            name: name.to_string(),
            role: MemberRole::Lead,
            created_at: chrono::Utc::now(),
            last_active_at: chrono::Utc::now(),
        }
    }

    #[tokio::test]
    async fn scan_teams_dir_finds_two_teams() {
        let dir = tempdir().unwrap();
        let conv_dir = dir.path().join("conversations").join("conv-1");
        std::fs::create_dir_all(&conv_dir).unwrap();
        let session_id = SessionId::new("conv-1");

        let reg = TeamRegistry::new();
        reg.create(session_id.clone(), dummy_lead("a"), "alpha".to_string())
            .await
            .unwrap();
        reg.create(session_id.clone(), dummy_lead("b"), "beta".to_string())
            .await
            .unwrap();
        // Persist both to disk.
        reg.persist(&session_id, "alpha", &conv_dir).await.unwrap();
        reg.persist(&session_id, "beta", &conv_dir).await.unwrap();

        let sessions = scan_teams_dir(&conv_dir, "conv-1", &LifecycleByTeam::default());
        assert_eq!(sessions.len(), 2);
        let names: Vec<&str> = sessions
            .iter()
            .filter_map(|s| s.team_name.as_deref())
            .collect();
        assert!(names.contains(&"alpha"));
        assert!(names.contains(&"beta"));
        // Each team has exactly the lead as a member.
        for s in &sessions {
            assert_eq!(s.members.len(), 1);
            assert!(matches!(s.members[0].agent_name.as_str(), "a" | "b"));
            assert_eq!(s.team_id, format!("conv-1#{}", s.team_name.as_deref().unwrap()));
            // live team：deleted_at 仍是 None。
            assert!(s.deleted_at.is_none());
        }
    }

    #[tokio::test]
    async fn scan_teams_dir_propagates_deleted_at_after_soft_delete() {
        let dir = tempdir().unwrap();
        let conv_dir = dir.path().join("conversations").join("conv-deleted");
        std::fs::create_dir_all(&conv_dir).unwrap();
        let session_id = SessionId::new("conv-deleted");

        let reg = TeamRegistry::new();
        reg.create(session_id.clone(), dummy_lead("a"), "alpha".to_string())
            .await
            .unwrap();
        reg.persist(&session_id, "alpha", &conv_dir).await.unwrap();
        TeamRegistry::mark_deleted_on_disk(&conv_dir, "alpha").unwrap();

        let sessions = scan_teams_dir(&conv_dir, "conv-deleted", &LifecycleByTeam::default());
        assert_eq!(sessions.len(), 1, "soft-deleted team must still appear in overview");
        // RFC3339 字符串带 Z 时区，含 "T" 分隔——格式校验粗一点即可。
        let ts = sessions[0].deleted_at.as_deref().expect("deleted_at透传");
        assert!(ts.contains('T'), "deleted_at should be RFC3339 string, got {ts}");
    }

    #[test]
    fn scan_teams_dir_returns_empty_when_no_teams_dir() {
        let dir = tempdir().unwrap();
        let conv_dir = dir.path().join("conv-1");
        std::fs::create_dir_all(&conv_dir).unwrap();
        let sessions = scan_teams_dir(&conv_dir, "conv-1", &LifecycleByTeam::default());
        assert!(sessions.is_empty());
    }

    #[test]
    fn append_team_chat_jsonl_parses_send_messages() {
        let dir = tempdir().unwrap();
        let conv_dir = dir.path();
        let paths = crate::runtime::agent::team_paths::TeamPaths::for_team(conv_dir, "alpha");
        let path = paths.team_chat_jsonl();
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(
            &path,
            r#"{"ts":"2026-05-15T00:00:00Z","from":"team-lead","to":"pro","text":"go"}
{"ts":"2026-05-15T00:00:01Z","from":"pro","to":"team-lead","text":"ok","variant":"text"}
"#,
        )
        .unwrap();
        let mut out = Vec::new();
        append_events_from_team_chat_jsonl(&mut out, &path, "alpha");
        assert_eq!(out.len(), 2);
        match &out[0] {
            TeamEvent::SendMessage { from, to, text, variant, .. } => {
                assert_eq!(from, "team-lead");
                assert_eq!(to, "pro");
                assert_eq!(text, "go");
                // 老 jsonl 行没有 variant 字段时回退到 "text"，保证历史会话兼容。
                assert_eq!(variant, "text");
            }
            _ => panic!("expected SendMessage"),
        }
    }

    #[test]
    fn append_team_chat_jsonl_passes_through_protocol_fields() {
        let dir = tempdir().unwrap();
        let conv_dir = dir.path();
        let paths = crate::runtime::agent::team_paths::TeamPaths::for_team(conv_dir, "alpha");
        let path = paths.team_chat_jsonl();
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        // 三行：shutdown_request (带 reason) / shutdown_response approve=true / approve=false 带 reason
        std::fs::write(
            &path,
            r#"{"ts":"2026-05-15T00:00:00Z","from":"team-lead","to":"pro","text":"","variant":"shutdown_request","reason":"task done"}
{"ts":"2026-05-15T00:00:01Z","from":"pro","to":"team-lead","text":"","variant":"shutdown_response","approve":true}
{"ts":"2026-05-15T00:00:02Z","from":"con","to":"team-lead","text":"","variant":"shutdown_response","approve":false,"reason":"still working"}
{"ts":"2026-05-15T00:00:03Z","from":"team-lead","to":"con","text":"","variant":"plan_approval_response","approve":false,"feedback":"missed edge"}
"#,
        )
        .unwrap();
        let mut out = Vec::new();
        append_events_from_team_chat_jsonl(&mut out, &path, "alpha");
        assert_eq!(out.len(), 4);

        match &out[0] {
            TeamEvent::SendMessage { variant, reason, approve, feedback, .. } => {
                assert_eq!(variant, "shutdown_request");
                assert_eq!(reason.as_deref(), Some("task done"));
                assert!(approve.is_none());
                assert!(feedback.is_none());
            }
            _ => panic!("expected SendMessage[0]"),
        }
        match &out[1] {
            TeamEvent::SendMessage { variant, approve, reason, feedback, .. } => {
                assert_eq!(variant, "shutdown_response");
                assert_eq!(*approve, Some(true));
                assert!(reason.is_none());
                assert!(feedback.is_none());
            }
            _ => panic!("expected SendMessage[1]"),
        }
        match &out[2] {
            TeamEvent::SendMessage { variant, approve, reason, .. } => {
                assert_eq!(variant, "shutdown_response");
                assert_eq!(*approve, Some(false));
                assert_eq!(reason.as_deref(), Some("still working"));
            }
            _ => panic!("expected SendMessage[2]"),
        }
        match &out[3] {
            TeamEvent::SendMessage { variant, approve, feedback, .. } => {
                assert_eq!(variant, "plan_approval_response");
                assert_eq!(*approve, Some(false));
                assert_eq!(feedback.as_deref(), Some("missed edge"));
            }
            _ => panic!("expected SendMessage[3]"),
        }
    }

    #[test]
    fn drain_for_attributes_stop_events_by_agent_name() {
        let mut lc = LifecycleByTeam::default();
        // Two teams have spawn events; AgentStop events come without team
        // context (parsed from textual TeammateStop result).
        lc.by_team.insert(
            "alpha".to_string(),
            vec![TeamEvent::AgentSpawn {
                ts: "2026-05-15T00:00:00Z".into(),
                agent_id: "id-a".into(),
                agent_name: "researcher".into(),
            }],
        );
        lc.by_team.insert(
            "beta".to_string(),
            vec![TeamEvent::AgentSpawn {
                ts: "2026-05-15T00:00:01Z".into(),
                agent_id: "id-b".into(),
                agent_name: "analyst".into(),
            }],
        );
        lc.stops_by_agent_name.insert(
            "researcher".to_string(),
            vec![TeamEvent::AgentStop {
                ts: "2026-05-15T00:01:00Z".into(),
                agent_name: "researcher".into(),
            }],
        );
        lc.stops_by_agent_name.insert(
            "analyst".to_string(),
            vec![TeamEvent::AgentStop {
                ts: "2026-05-15T00:01:01Z".into(),
                agent_name: "analyst".into(),
            }],
        );

        // alpha owns "researcher", beta owns "analyst".
        let alpha_members: std::collections::HashSet<String> =
            ["researcher".to_string()].into_iter().collect();
        let beta_members: std::collections::HashSet<String> =
            ["analyst".to_string()].into_iter().collect();

        let alpha_events = lc.drain_for("alpha", &alpha_members);
        assert_eq!(alpha_events.len(), 2, "alpha gets spawn + researcher's stop");
        assert!(matches!(alpha_events[0], TeamEvent::AgentSpawn { .. }));
        assert!(matches!(
            alpha_events[1],
            TeamEvent::AgentStop { ref agent_name, .. } if agent_name == "researcher"
        ));

        let beta_events = lc.drain_for("beta", &beta_members);
        assert_eq!(beta_events.len(), 2, "beta gets spawn + analyst's stop");
        assert!(matches!(
            beta_events[1],
            TeamEvent::AgentStop { ref agent_name, .. } if agent_name == "analyst"
        ));

        // Stops bucket is fully drained — no leftover.
        assert!(lc.stops_by_agent_name.is_empty(), "stops bucket fully consumed");
    }

    #[test]
    fn drain_for_drops_stop_events_with_no_owning_team() {
        let mut lc = LifecycleByTeam::default();
        // Stop event whose agent_name doesn't belong to any team's roster
        // is silently dropped (the alternative — leaking it to a random
        // team — would be worse than losing it).
        lc.stops_by_agent_name.insert(
            "ghost".to_string(),
            vec![TeamEvent::AgentStop {
                ts: "2026-05-15T00:01:00Z".into(),
                agent_name: "ghost".into(),
            }],
        );
        let empty: std::collections::HashSet<String> = std::collections::HashSet::new();
        let events = lc.drain_for("alpha", &empty);
        assert!(events.is_empty());
        // The stop still sits in the bucket waiting for an owner — but
        // because no later drain_for call will match, it's effectively dropped
        // from output (correct behaviour: better silent drop than misattribute).
        assert_eq!(lc.stops_by_agent_name.len(), 1);
    }
}
