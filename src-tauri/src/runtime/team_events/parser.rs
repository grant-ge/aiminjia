//! 群聊事件解析器
//!
//! 从一个 conversation 的 `messages.jsonl` + `teammates/*.meta.json` +
//! `teammates/*.jsonl` 解析出"群聊视图"消费的事件流。
//!
//! 设计原则：
//! - **不**改动现有 conversation 存储。所有数据从已有 jsonl + meta 派生。
//! - 数据格式参考 `~/.renlijia/users/{scope}/conversations/{id}/`：
//!   - `conv.json`：元数据
//!   - `messages.jsonl`：主对话流，每行一条 message，字段 `role / content / toolCalls / createdAt`
//!   - `teammates/agent-{id}.meta.json`：teammate 元数据（agent_name / team_id / spawned_at / boot_system_prompt）
//!   - `teammates/agent-{id}.jsonl`：teammate 的 transcript（用于"工具计数"等增强信息，v1 不读）
//!
//! ## 事件来源
//!
//! 1. **TeamCreated**：主流里 `toolCalls[].name == "TeamCreate"` 的 assistant 消息
//! 2. **MemberJoined**：teammates/*.meta.json 文件（每个文件 = 一个成员）
//! 3. **TaskCreated / TaskUpdated**：主流里 `toolCalls[].name in ("TaskCreate", "TaskUpdate")`
//! 4. **MessageSent (lead → member)**：主流里 `toolCalls[].name == "SendMessage"` 的 input
//! 5. **MessageSent (member → lead)**：主流里 `role=user content.text` 中的 `<peer-messages>` XML
//!    （teammate 通过 inbox 把回复注入主对话上下文，包装成 peer-message XML）
//!
//! ## 不包含
//!
//! 任何非 team 工具（Bash/Read/Edit/Grep/…）的调用——这些在群视图里折叠成"工具 (N)"按钮
//! 由前端从同一条 toolCalls 数组计数即可，不需要解析器处理。

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fs;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

// ─── Event types ──────────────────────────────────────────────────────────────

/// 一条"群聊事件"——前端按时间顺序渲染成系统消息或聊天气泡。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TeamEvent {
    /// `TeamCreate` 调用——群被创建。
    TeamCreated {
        ts: DateTime<Utc>,
        team_name: String,
        description: Option<String>,
    },
    /// 一个 teammate 被 spawn 进群。
    MemberJoined {
        ts: DateTime<Utc>,
        agent_id: String,
        name: String,
        subagent_type: Option<String>,
        description: Option<String>,
        /// 引用的数字员工（Phase 2 `Agent({employee_id})`）。v1 总是 None。
        employee_id: Option<String>,
    },
    /// `TaskCreate` 调用。
    TaskCreated {
        ts: DateTime<Utc>,
        task_id: String,
        subject: String,
    },
    /// `TaskUpdate` 调用——只在 owner 或 status 实际变化时发事件。
    TaskUpdated {
        ts: DateTime<Utc>,
        task_id: String,
        owner: Option<String>,
        status: Option<String>,
    },
    /// 群聊气泡：sender 给 to 发了一条消息。
    /// - lead → member：来自主流的 `SendMessage` toolCall
    /// - member → lead：来自主流 user content 里的 `<peer-message from="...">` XML
    MessageSent {
        ts: DateTime<Utc>,
        sender: String,
        to: String,
        content: String,
        /// 这条消息所属的主流 message id（前端做"工具(N)"挂靠用）。
        /// peer-message 的话指向那条 user 消息。
        anchor_message_id: Option<String>,
    },
}

impl TeamEvent {
    pub fn ts(&self) -> DateTime<Utc> {
        match self {
            TeamEvent::TeamCreated { ts, .. }
            | TeamEvent::MemberJoined { ts, .. }
            | TeamEvent::TaskCreated { ts, .. }
            | TeamEvent::TaskUpdated { ts, .. }
            | TeamEvent::MessageSent { ts, .. } => *ts,
        }
    }
}

// ─── Roster ───────────────────────────────────────────────────────────────────

/// 群活成员名册（live 派生，不持久化）。前端用来渲染头部 + inline card。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TeamRoster {
    pub team_name: Option<String>,
    pub description: Option<String>,
    pub created_at: Option<DateTime<Utc>>,
    pub members: Vec<MemberInfo>,
    pub task_count_total: usize,
    pub task_count_completed: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MemberInfo {
    pub agent_id: String,
    pub name: String,
    pub spawned_at: Option<DateTime<Utc>>,
    /// 引用员工 id（Phase 2）。v1 总是 None。
    pub employee_id: Option<String>,
}

// ─── Parser entry ─────────────────────────────────────────────────────────────

/// 解析一个 conversation 目录，返回事件流（按时间升序）和当前名册。
///
/// `conversation_dir` 形如：
/// `~/.renlijia/users/{scope}/conversations/{conv_id}/`
pub fn parse_team_view(conversation_dir: &Path) -> Result<TeamView> {
    let messages_path = conversation_dir.join("messages.jsonl");
    let teammates_dir = conversation_dir.join("teammates");

    let mut events: Vec<TeamEvent> = Vec::new();
    let mut roster = TeamRoster {
        team_name: None,
        description: None,
        created_at: None,
        members: Vec::new(),
        task_count_total: 0,
        task_count_completed: 0,
    };
    // task_id → completed?  用于避免重复计数 task 完成
    let mut task_completed: std::collections::HashMap<String, bool> =
        std::collections::HashMap::new();

    // ─ Step 1: 扫主对话 messages.jsonl ─
    if messages_path.exists() {
        let file = fs::File::open(&messages_path)
            .with_context(|| format!("open {}", messages_path.display()))?;
        let reader = BufReader::new(file);
        for raw in reader.lines() {
            let raw = raw?;
            let trimmed = strip_atomic_marker(&raw);
            if trimmed.is_empty() {
                continue;
            }
            let msg: serde_json::Value = match serde_json::from_str(trimmed) {
                Ok(v) => v,
                Err(_) => continue, // 容错：损坏的行跳过
            };
            extract_events_from_message(&msg, &mut events, &mut roster, &mut task_completed);
        }
    }

    // ─ Step 2: 扫 teammates/*.meta.json ─
    if teammates_dir.exists() {
        for entry in fs::read_dir(&teammates_dir).context("read teammates dir")? {
            let entry = entry?;
            let p = entry.path();
            if p.extension().and_then(|s| s.to_str()) != Some("json") {
                continue;
            }
            if !p.file_name().and_then(|s| s.to_str()).unwrap_or("").ends_with(".meta.json") {
                continue;
            }
            let bytes = fs::read(&p).with_context(|| format!("read {}", p.display()))?;
            let meta: TeammateMeta = match serde_json::from_slice(&bytes) {
                Ok(m) => m,
                Err(e) => {
                    log::warn!("skip malformed meta {}: {}", p.display(), e);
                    continue;
                }
            };
            roster.members.push(MemberInfo {
                agent_id: meta.agent_id.clone(),
                name: meta.agent_name.clone(),
                spawned_at: meta.spawned_at,
                employee_id: meta.employee_id.clone(),
            });
            events.push(TeamEvent::MemberJoined {
                ts: meta.spawned_at.unwrap_or_else(Utc::now),
                agent_id: meta.agent_id,
                name: meta.agent_name,
                subagent_type: None,
                description: None,
                employee_id: meta.employee_id,
            });
        }
    }

    // ─ Step 3: 按 ts 升序 ─
    events.sort_by_key(|e| e.ts());
    roster.members.sort_by_key(|m| m.spawned_at.unwrap_or_else(Utc::now));

    Ok(TeamView { events, roster })
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TeamView {
    pub events: Vec<TeamEvent>,
    pub roster: TeamRoster,
}

// ─── Internals ────────────────────────────────────────────────────────────────

#[derive(Deserialize)]
struct TeammateMeta {
    agent_id: String,
    agent_name: String,
    #[serde(default)]
    spawned_at: Option<DateTime<Utc>>,
    /// Phase 2 Agent 工具加 employee_id 时透传到 meta；v1 字段不存在。
    #[serde(default)]
    employee_id: Option<String>,
}

/// 去掉 file_store atomic-write 写入的 `\t✓` 尾标记（参考 io.rs::append_jsonl）。
fn strip_atomic_marker(line: &str) -> &str {
    line.trim_end_matches(|c: char| c == '\n' || c == '\r' || c == '✓' || c == '\t')
}

fn extract_events_from_message(
    msg: &serde_json::Value,
    events: &mut Vec<TeamEvent>,
    roster: &mut TeamRoster,
    task_completed: &mut std::collections::HashMap<String, bool>,
) {
    let role = msg.get("role").and_then(|v| v.as_str()).unwrap_or("");
    let ts = msg
        .get("createdAt")
        .or_else(|| msg.get("timestamp"))
        .and_then(|v| v.as_str())
        .and_then(|s| DateTime::parse_from_rfc3339(s).ok().map(|t| t.with_timezone(&Utc)))
        .unwrap_or_else(Utc::now);
    let msg_id = msg.get("id").and_then(|v| v.as_str()).map(String::from);

    match role {
        "assistant" => {
            if let Some(tcs) = msg.get("toolCalls").and_then(|v| v.as_array()) {
                for tc in tcs {
                    let name = tc.get("name").and_then(|v| v.as_str()).unwrap_or("");
                    let args = tc.get("arguments").cloned().unwrap_or(serde_json::Value::Null);
                    match name {
                        "TeamCreate" => {
                            let team_name = args
                                .get("team_name")
                                .and_then(|v| v.as_str())
                                .unwrap_or("team")
                                .to_string();
                            let description = args
                                .get("description")
                                .and_then(|v| v.as_str())
                                .map(String::from);
                            roster.team_name = Some(team_name.clone());
                            roster.description = description.clone();
                            roster.created_at = Some(ts);
                            events.push(TeamEvent::TeamCreated { ts, team_name, description });
                        }
                        "TaskCreate" => {
                            let subject = args
                                .get("subject")
                                .and_then(|v| v.as_str())
                                .unwrap_or("(no subject)")
                                .to_string();
                            // task_id 在 tool_result 里返回，这里先用 tool_call_id 占位
                            let task_id = tc
                                .get("id")
                                .and_then(|v| v.as_str())
                                .unwrap_or("")
                                .to_string();
                            roster.task_count_total += 1;
                            task_completed.insert(task_id.clone(), false);
                            events.push(TeamEvent::TaskCreated { ts, task_id, subject });
                        }
                        "TaskUpdate" => {
                            let task_id = args
                                .get("taskId")
                                .and_then(|v| v.as_str())
                                .unwrap_or("")
                                .to_string();
                            let owner = args
                                .get("owner")
                                .and_then(|v| v.as_str())
                                .map(String::from);
                            let status = args
                                .get("status")
                                .and_then(|v| v.as_str())
                                .map(String::from);
                            if matches!(status.as_deref(), Some("completed")) {
                                let already = task_completed.insert(task_id.clone(), true);
                                if !already.unwrap_or(false) {
                                    roster.task_count_completed += 1;
                                }
                            }
                            events.push(TeamEvent::TaskUpdated { ts, task_id, owner, status });
                        }
                        "SendMessage" => {
                            let to = args
                                .get("to")
                                .and_then(|v| v.as_str())
                                .unwrap_or("?")
                                .to_string();
                            let content = extract_send_message_content(&args);
                            events.push(TeamEvent::MessageSent {
                                ts,
                                sender: "team-lead".to_string(),
                                to,
                                content,
                                anchor_message_id: msg_id.clone(),
                            });
                        }
                        _ => {} // 其他工具不入流
                    }
                }
            }
        }
        "user" => {
            // 看 content.text 里有没有 <peer-messages> XML，提取每条 peer-message 作为 member→lead
            if let Some(text) = msg
                .get("content")
                .and_then(|c| c.get("text"))
                .and_then(|v| v.as_str())
            {
                for pm in parse_peer_messages(text) {
                    events.push(TeamEvent::MessageSent {
                        ts,
                        sender: pm.from,
                        to: "team-lead".to_string(),
                        content: pm.content,
                        anchor_message_id: msg_id.clone(),
                    });
                }
            }
        }
        _ => {}
    }
}

/// `SendMessage.message` 参数的 schema 真实情况（来自 840ba884 fixture）：
///   • 大多数情况是字符串，内容是 Python repr 风格的 dict 字面量：
///     `"{'content': '...', 'type': 'text'}"`（**单引号**——非 valid JSON）
///   • 偶尔是 valid JSON 对象 `{ "content": "...", "type": "text" }`
///   • 偶尔就是纯字符串
///
/// 提取顺序：
///   1. JSON 解析成功且有 `content` 字段 → 取 `content`
///   2. 字符串里 regex 匹配 `'content': '...'` 单引号 dict → 提取 + 反转义
///   3. 都失败 → 返回原始字符串（保留 fallback，避免空白）
fn extract_send_message_content(args: &serde_json::Value) -> String {
    let raw = args.get("message");
    match raw {
        Some(serde_json::Value::String(s)) => {
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(s) {
                if let Some(c) = v.get("content").and_then(|v| v.as_str()) {
                    return c.to_string();
                }
            }
            if let Some(c) = extract_python_repr_content(s) {
                return c;
            }
            s.clone()
        }
        Some(serde_json::Value::Object(_)) => raw
            .and_then(|v| v.get("content"))
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string(),
        _ => String::new(),
    }
}

/// 从形如 `"{'content': '...', ...}"` 的 Python repr 字符串里提取 `content` 值。
/// 处理转义：`\\n` → `\n`，`\\'` → `'`，`\\\\` → `\\` 等。找不到返回 None；
/// 这函数刻意只支持这一种 pattern，不试图做完整 Python literal eval。
fn extract_python_repr_content(s: &str) -> Option<String> {
    let needle = "'content':";
    let start = s.find(needle)?;
    let after_key = &s[start + needle.len()..];
    let trimmed = after_key.trim_start();
    let bytes = trimmed.as_bytes();
    if bytes.first() != Some(&b'\'') {
        return None;
    }
    let mut out = String::new();
    let mut iter = trimmed[1..].chars();
    while let Some(c) = iter.next() {
        if c == '\\' {
            match iter.next() {
                Some('n') => out.push('\n'),
                Some('t') => out.push('\t'),
                Some('r') => out.push('\r'),
                Some('\'') => out.push('\''),
                Some('"') => out.push('"'),
                Some('\\') => out.push('\\'),
                Some(other) => {
                    out.push('\\');
                    out.push(other);
                }
                None => break,
            }
        } else if c == '\'' {
            return Some(out);
        } else {
            out.push(c);
        }
    }
    Some(out)
}

struct PeerMessage {
    from: String,
    content: String,
}

/// 极简 XML 解析：从 `<peer-messages>...<peer-message from="X" variant="...">CONTENT</peer-message>...</peer-messages>`
/// 提取每条 peer-message 的 from + content。容错优先（XML 不严格、不递归嵌套 peer-message）。
fn parse_peer_messages(text: &str) -> Vec<PeerMessage> {
    let mut out = Vec::new();
    let mut rest = text;
    while let Some(start_idx) = rest.find("<peer-message ") {
        let after_start = &rest[start_idx..];
        // 找 tag 结束 >
        let Some(gt) = after_start.find('>') else { break };
        let opening = &after_start[..gt + 1]; // 包含 >
        let body_start_abs = start_idx + gt + 1;
        // 提取 from 属性
        let from = extract_attr(opening, "from").unwrap_or_else(|| "?".to_string());
        // 找结束 tag
        let body_str = &rest[body_start_abs..];
        let Some(end_rel) = body_str.find("</peer-message>") else { break };
        let content = body_str[..end_rel].trim().to_string();
        out.push(PeerMessage { from, content });
        rest = &body_str[end_rel + "</peer-message>".len()..];
    }
    out
}

fn extract_attr(opening_tag: &str, key: &str) -> Option<String> {
    // 找 `key="..."`
    let needle = format!("{}=\"", key);
    let idx = opening_tag.find(&needle)?;
    let after = &opening_tag[idx + needle.len()..];
    let end = after.find('"')?;
    Some(after[..end].to_string())
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_team_created_from_tool_call() {
        let mut events = Vec::new();
        let mut roster = TeamRoster {
            team_name: None,
            description: None,
            created_at: None,
            members: vec![],
            task_count_total: 0,
            task_count_completed: 0,
        };
        let mut tc = std::collections::HashMap::new();
        let msg = serde_json::json!({
            "role": "assistant",
            "createdAt": "2026-05-13T03:10:39Z",
            "id": "msg-1",
            "toolCalls": [{
                "name": "TeamCreate",
                "id": "tc1",
                "arguments": { "team_name": "debate-team", "description": "辩论赛" }
            }]
        });
        extract_events_from_message(&msg, &mut events, &mut roster, &mut tc);
        assert_eq!(events.len(), 1);
        assert!(matches!(&events[0], TeamEvent::TeamCreated { team_name, .. } if team_name == "debate-team"));
        assert_eq!(roster.team_name.as_deref(), Some("debate-team"));
    }

    #[test]
    fn extract_python_repr_handles_real_send_message() {
        // 真实 fixture 里看到的 schema：单引号 dict，含 \n 转义
        let raw = r#"{'content': '辩论赛即将开始！\n你的身份是【正方辩手】。', 'type': 'text'}"#;
        let got = extract_python_repr_content(raw).expect("should match");
        assert_eq!(got, "辩论赛即将开始！\n你的身份是【正方辩手】。");
    }

    #[test]
    fn extract_python_repr_returns_none_when_not_python_repr() {
        assert!(extract_python_repr_content("plain text").is_none());
        assert!(extract_python_repr_content(r#"{"content": "json"}"#).is_none()); // 双引号 → JSON 路径处理
    }

    #[test]
    fn send_message_extracts_python_repr_in_full_pipeline() {
        let mut events = Vec::new();
        let mut roster = roster_empty();
        let mut tc = std::collections::HashMap::new();
        let msg = serde_json::json!({
            "role": "assistant",
            "createdAt": "2026-05-13T03:11:00Z",
            "toolCalls": [{
                "name": "SendMessage",
                "id": "tcX",
                "arguments": {
                    "to": "moderator",
                    // 模拟真实 fixture 的 Python repr 字符串
                    "message": "{'content': '请你担任主持人，按以下流程组织：\\n\\n辩题：AI 是否应该取代初级程序员', 'type': 'text'}"
                }
            }]
        });
        extract_events_from_message(&msg, &mut events, &mut roster, &mut tc);
        match &events[0] {
            TeamEvent::MessageSent { content, .. } => {
                assert!(content.contains("请你担任主持人"));
                assert!(content.contains("\n\n辩题"));
                assert!(!content.starts_with("{"), "should not contain raw repr braces");
            }
            _ => panic!("expected MessageSent"),
        }
    }

    #[test]
    fn parses_send_message_string_arg() {
        let mut events = Vec::new();
        let mut roster = roster_empty();
        let mut tc = std::collections::HashMap::new();
        let msg = serde_json::json!({
            "role": "assistant",
            "createdAt": "2026-05-13T03:11:00Z",
            "toolCalls": [{
                "name": "SendMessage",
                "id": "tc2",
                "arguments": {
                    "to": "moderator",
                    "message": "{\"content\": \"开始辩论\", \"type\": \"text\"}"
                }
            }]
        });
        extract_events_from_message(&msg, &mut events, &mut roster, &mut tc);
        match &events[0] {
            TeamEvent::MessageSent { sender, to, content, .. } => {
                assert_eq!(sender, "team-lead");
                assert_eq!(to, "moderator");
                assert_eq!(content, "开始辩论");
            }
            _ => panic!("expected MessageSent"),
        }
    }

    #[test]
    fn parses_peer_messages_xml() {
        let xml = r#"<peer-messages>
            <peer-message from="affirmative" variant="text">你好，正方就位</peer-message>
            <peer-message from="negative" variant="text">反方就位</peer-message>
        </peer-messages>"#;
        let pms = parse_peer_messages(xml);
        assert_eq!(pms.len(), 2);
        assert_eq!(pms[0].from, "affirmative");
        assert_eq!(pms[0].content, "你好，正方就位");
        assert_eq!(pms[1].from, "negative");
    }

    #[test]
    fn extract_events_user_peer_messages() {
        let mut events = Vec::new();
        let mut roster = roster_empty();
        let mut tc = std::collections::HashMap::new();
        let msg = serde_json::json!({
            "role": "user",
            "createdAt": "2026-05-13T03:11:30Z",
            "id": "msg-9",
            "content": {
                "text": "<peer-messages><peer-message from=\"judge\" variant=\"text\">就位</peer-message></peer-messages>"
            }
        });
        extract_events_from_message(&msg, &mut events, &mut roster, &mut tc);
        assert_eq!(events.len(), 1);
        match &events[0] {
            TeamEvent::MessageSent { sender, to, content, anchor_message_id, .. } => {
                assert_eq!(sender, "judge");
                assert_eq!(to, "team-lead");
                assert_eq!(content, "就位");
                assert_eq!(anchor_message_id.as_deref(), Some("msg-9"));
            }
            _ => panic!("expected MessageSent"),
        }
    }

    #[test]
    fn task_completion_dedupes_count() {
        let mut events = Vec::new();
        let mut roster = roster_empty();
        let mut tc = std::collections::HashMap::new();
        // 同一个 task#2 被 update 两次为 completed，只计 1 次
        for _ in 0..2 {
            let msg = serde_json::json!({
                "role": "assistant",
                "createdAt": "2026-05-13T03:20:00Z",
                "toolCalls": [{
                    "name": "TaskUpdate",
                    "id": "tcU",
                    "arguments": { "taskId": "2", "status": "completed" }
                }]
            });
            extract_events_from_message(&msg, &mut events, &mut roster, &mut tc);
        }
        assert_eq!(roster.task_count_completed, 1, "double-completion must count once");
    }

    #[test]
    fn strips_atomic_marker() {
        assert_eq!(strip_atomic_marker("hello\t✓\n"), "hello");
        assert_eq!(strip_atomic_marker("hello"), "hello");
        assert_eq!(strip_atomic_marker("hello\t\t✓"), "hello");
    }

    fn roster_empty() -> TeamRoster {
        TeamRoster {
            team_name: None,
            description: None,
            created_at: None,
            members: vec![],
            task_count_total: 0,
            task_count_completed: 0,
        }
    }
}
