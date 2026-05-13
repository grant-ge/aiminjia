//! 群聊持久化（v0.4 group chat persistence）。
//!
//! 与 v0.3 "反扫 transcript" 路径不同：本模块是**一等公民的群聊存储**。每个
//! agent（Lead + teammate）每次 LLM turn 结束时由调用方追加一条或多条 entry
//! 到 `<conv_dir>/team-chat.jsonl`。前端 `team_view_for_conversation` 直接读
//! 这个文件作为消息时间线的唯一来源。
//!
//! 为什么要独立文件而非反扫：
//! - assistant 在 transcript 里直接写正文却没用 SendMessage 时，反扫看不见
//! - SendMessage 的 message 字段 LLM 偶尔会写成字符串，反扫得做格式兼容
//! - 反扫每次重新解析 jsonl，消息越多越慢，无法做"未读"等增量语义
//!
//! 一行一条 JSON envelope（snake_case，向后兼容靠 `#[serde(default)]`）：
//!
//! ```json
//! {
//!   "ts": "2026-05-13T03:00:00Z",
//!   "sender": "investor",
//!   "to": "team-lead",
//!   "content": "...",
//!   "source": "send_message"
//! }
//! ```
//!
//! `source` 区分 entry 来源：
//! - `send_message`：来自 SendMessage 工具调用（明确广播）
//! - `assistant_text`：来自 assistant 在 turn 里输出的纯文本（隐式广播给 Lead）
//! - `lead_reply`：Lead 自己的 assistant 文本，UI 渲染为 Lead 发言

use std::fs::OpenOptions;
use std::io::{self, BufRead, BufReader, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

pub const FILE_NAME: &str = "team-chat.jsonl";

/// Decide whether a conversation has a team — only then do we persist team chat.
/// Cheap stat (no JSON parse) so it's safe to call per turn.
pub fn team_exists(conv_dir: &Path) -> bool {
    conv_dir.join("team.json").exists()
}

/// Record one turn's output into `team-chat.jsonl` and emit matching
/// `RuntimeEvent::TeamMessage` events. Caller passes:
/// - the speaker's display name (`team-lead` or teammate name)
/// - whether the speaker is the Lead (controls EntrySource for plain text)
/// - the assistant text content (may be empty)
/// - the tool calls in this turn (we only extract `SendMessage`)
/// - the parent bus for live UI mirroring (optional)
///
/// Returns the entries that were written (for tests/logging).
pub async fn record_turn(
    conv_dir: &Path,
    speaker: &str,
    is_lead: bool,
    assistant_text: &str,
    tool_calls: &[crate::llm::streaming::ToolCall],
    bus: Option<&crate::runtime::event_bus::RuntimeEventBus>,
    session_id: &crate::runtime::ids::SessionId,
    run_id: &crate::runtime::ids::RunId,
) -> Vec<Entry> {
    if !team_exists(conv_dir) {
        return Vec::new();
    }
    let mut entries: Vec<Entry> = Vec::new();
    let now = chrono::Utc::now();

    if !assistant_text.trim().is_empty() {
        entries.push(Entry {
            ts: now,
            sender: speaker.to_string(),
            // Plain assistant text has no explicit recipient. For Lead, target
            // the team broadcast slot ("*"); for teammate, default to Lead so
            // the user can read what the worker is "thinking aloud".
            to: if is_lead { "*".to_string() } else { "team-lead".to_string() },
            content: assistant_text.to_string(),
            source: if is_lead {
                EntrySource::LeadReply
            } else {
                EntrySource::AssistantText
            },
        });
    }

    for (i, tc) in tool_calls.iter().enumerate() {
        if tc.name != "SendMessage" {
            continue;
        }
        if let Some((to, body)) = extract_send_message_body(&tc.arguments) {
            entries.push(Entry {
                // Stagger ts within a turn so sort order is stable.
                ts: now + chrono::Duration::milliseconds((i + 1) as i64),
                sender: speaker.to_string(),
                to,
                content: body,
                source: EntrySource::SendMessage,
            });
        }
    }

    if entries.is_empty() {
        return entries;
    }

    if let Err(e) = append_entries(conv_dir, &entries) {
        log::warn!(
            "[team_chat] append_entries failed conv={} err={}",
            conv_dir.display(),
            e
        );
    }

    if let Some(bus) = bus {
        for entry in &entries {
            let event = crate::runtime::events::RuntimeEvent::new(
                session_id.clone(),
                run_id.clone(),
                crate::runtime::events::RuntimeEventKind::TeamMessage {
                    ts: entry.ts,
                    from: entry.sender.clone(),
                    to: entry.to.clone(),
                    body: entry.content.clone(),
                },
            );
            if let Err(e) = bus.emit(event).await {
                log::warn!("[team_chat] bus emit failed: {e}");
            }
        }
    }

    entries
}

/// Variant of `record_turn` that accepts tool calls in their JSON-serialized
/// shape `{id, name, arguments}` — convenient for callers (like Lead's
/// chat_turn_driver) that have already normalized into Value form.
pub async fn record_turn_json(
    conv_dir: &Path,
    speaker: &str,
    is_lead: bool,
    assistant_text: &str,
    tool_calls_json: &[serde_json::Value],
    bus: Option<&crate::runtime::event_bus::RuntimeEventBus>,
    session_id: &crate::runtime::ids::SessionId,
    run_id: &crate::runtime::ids::RunId,
) -> Vec<Entry> {
    let tool_calls: Vec<crate::llm::streaming::ToolCall> = tool_calls_json
        .iter()
        .filter_map(|v| {
            Some(crate::llm::streaming::ToolCall {
                id: v.get("id").and_then(|x| x.as_str())?.to_string(),
                name: v.get("name").and_then(|x| x.as_str())?.to_string(),
                arguments: v.get("arguments").cloned().unwrap_or(serde_json::Value::Null),
            })
        })
        .collect();
    record_turn(
        conv_dir,
        speaker,
        is_lead,
        assistant_text,
        &tool_calls,
        bus,
        session_id,
        run_id,
    )
    .await
}

/// Extract `(to, body)` from a SendMessage `arguments` Value. Handles the
/// LLM's two observed shapes — a `{type, content}` object, or a JSON-encoded
/// string carrying the same envelope — and always returns a printable body
/// (never a raw JSON dump).
fn extract_send_message_body(arguments: &serde_json::Value) -> Option<(String, String)> {
    let to = arguments.get("to").and_then(|x| x.as_str())?.to_string();
    let raw = arguments.get("message")?;
    let msg = match raw {
        serde_json::Value::String(s) => serde_json::from_str::<serde_json::Value>(s)
            .unwrap_or_else(|_| serde_json::json!({ "content": s })),
        other => other.clone(),
    };
    let body = msg
        .get("content")
        .and_then(|x| x.as_str())
        .map(str::to_string)
        .or_else(|| {
            msg.get("text")
                .and_then(|x| x.as_str())
                .map(str::to_string)
        })
        .unwrap_or_else(|| serde_json::to_string(&msg).unwrap_or_default());
    Some((to, body))
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum EntrySource {
    /// 显式 SendMessage 工具调用。
    SendMessage,
    /// teammate 在 turn 里的 assistant 文本（默认收件人 = team-lead）。
    AssistantText,
    /// Lead 在 turn 里的 assistant 文本（默认 sender = team-lead，to = team）。
    LeadReply,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Entry {
    pub ts: chrono::DateTime<chrono::Utc>,
    pub sender: String,
    pub to: String,
    pub content: String,
    pub source: EntrySource,
}

fn jsonl_path(conv_dir: &Path) -> PathBuf {
    conv_dir.join(FILE_NAME)
}

/// Append 一组 entries（同一 turn 多条，调用方一次性传入避免多次 open）。
/// 调用方应在 `team.json` 存在时才调；本函数不做这个检查（避免每条都 stat）。
pub fn append_entries(conv_dir: &Path, entries: &[Entry]) -> io::Result<()> {
    if entries.is_empty() {
        return Ok(());
    }
    std::fs::create_dir_all(conv_dir)?;
    let path = jsonl_path(conv_dir);
    let mut file = OpenOptions::new().create(true).append(true).open(&path)?;
    for entry in entries {
        let mut line = serde_json::to_vec(entry)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        line.push(b'\n');
        file.write_all(&line)?;
    }
    Ok(())
}

/// 读全部 entries（按 file order，等价于 ts 单调递增）。文件缺失 → 空数组。
/// 解析失败的行跳过 + warn，避免单行损坏让整个时间线没法看。
pub fn read_all(conv_dir: &Path) -> io::Result<Vec<Entry>> {
    let path = jsonl_path(conv_dir);
    let file = match std::fs::File::open(&path) {
        Ok(f) => f,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(e),
    };
    let mut out = Vec::new();
    for (idx, line) in BufReader::new(file).lines().enumerate() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        match serde_json::from_str::<Entry>(&line) {
            Ok(entry) => out.push(entry),
            Err(e) => log::warn!(
                "[team_chat] skip malformed line {} in {}: {}",
                idx + 1,
                path.display(),
                e
            ),
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn ent(sender: &str, to: &str, body: &str, src: EntrySource) -> Entry {
        Entry {
            ts: chrono::Utc::now(),
            sender: sender.into(),
            to: to.into(),
            content: body.into(),
            source: src,
        }
    }

    #[test]
    fn append_then_read_round_trip() {
        let tmp = TempDir::new().unwrap();
        append_entries(
            tmp.path(),
            &[
                ent("investor", "team-lead", "hi", EntrySource::SendMessage),
                ent("investor", "team-lead", "more", EntrySource::AssistantText),
            ],
        )
        .unwrap();
        let all = read_all(tmp.path()).unwrap();
        assert_eq!(all.len(), 2);
        assert_eq!(all[0].sender, "investor");
        assert_eq!(all[1].source, EntrySource::AssistantText);
    }

    #[test]
    fn missing_file_returns_empty() {
        let tmp = TempDir::new().unwrap();
        let entries = read_all(tmp.path()).unwrap();
        assert!(entries.is_empty());
    }

    #[test]
    fn malformed_line_skipped() {
        let tmp = TempDir::new().unwrap();
        append_entries(tmp.path(), &[ent("x", "y", "ok", EntrySource::SendMessage)]).unwrap();
        let path = jsonl_path(tmp.path());
        let mut f = OpenOptions::new().append(true).open(&path).unwrap();
        f.write_all(b"{not json\n").unwrap();
        append_entries(tmp.path(), &[ent("x", "y", "ok2", EntrySource::SendMessage)]).unwrap();
        let entries = read_all(tmp.path()).unwrap();
        assert_eq!(entries.len(), 2);
    }

    #[test]
    fn empty_entries_is_noop() {
        let tmp = TempDir::new().unwrap();
        append_entries(tmp.path(), &[]).unwrap();
        assert!(!jsonl_path(tmp.path()).exists());
    }
}
