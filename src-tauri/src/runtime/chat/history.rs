use std::collections::HashSet;

use anyhow::Result;

use crate::llm::streaming::{ChatMessage, ToolCall};
use crate::runtime::chat::compaction::CompactBoundaryRecord;
use crate::storage::file_store::types::StoredMessage;

#[derive(Debug, Clone)]
pub struct HistoryConfig {
    pub char_budget: usize,
    pub max_rounds: usize,
    pub include_uploaded_file_hints: bool,
    pub has_authorized_workspace: bool,
}

impl Default for HistoryConfig {
    fn default() -> Self {
        Self {
            char_budget: 120_000,
            max_rounds: 30,
            include_uploaded_file_hints: true,
            has_authorized_workspace: false,
        }
    }
}

pub fn build_chat_history(
    stored: &[StoredMessage],
    boundary: Option<&CompactBoundaryRecord>,
    config: &HistoryConfig,
) -> Result<Vec<ChatMessage>> {
    let relevant = apply_boundary(stored, boundary);

    // PR2: 过滤掉 error.is_some() 的消息（避免错误气泡回灌给 LLM）。
    // 守卫规则等价 claude-code-best `isApiErrorMessage:true` 过滤。
    // spec §3.2。
    let filtered: Vec<&StoredMessage> = relevant.iter().filter(|m| m.error.is_none()).collect();

    let mut messages: Vec<ChatMessage> = filtered
        .iter()
        .map(|message| stored_to_chat(message, config))
        .collect();

    messages = filter_invalid_tool_pairs(messages);
    messages = reorder_tool_results_after_assistant(messages);
    messages = trim_to_budget(messages, config);
    messages = collapse_trailing_consecutive_user(messages);

    if let Some(boundary) = boundary {
        if !boundary.summary_text.is_empty() {
            messages.insert(
                0,
                ChatMessage::text(
                    "user",
                    format!("<context>\n{}\n</context>", boundary.summary_text),
                ),
            );
        }
    }

    Ok(messages)
}

fn apply_boundary<'a>(
    stored: &'a [StoredMessage],
    boundary: Option<&CompactBoundaryRecord>,
) -> &'a [StoredMessage] {
    if let Some(boundary) = boundary {
        if let Some(tail_id) = boundary
            .tail_message_id
            .as_deref()
            .filter(|value| !value.is_empty())
        {
            if let Some(index) = stored.iter().position(|message| message.id == tail_id) {
                return &stored[index..];
            }
            log::warn!(
                "[history] compact boundary tail_id='{}' not found, using full history",
                tail_id
            );
        }
    }

    stored
}

fn stored_to_chat(message: &StoredMessage, config: &HistoryConfig) -> ChatMessage {
    ChatMessage {
        role: message.role.clone(),
        content: build_chat_message_content(message, config),
        tool_calls: normalize_tool_calls(message.tool_calls.as_ref())
            .or_else(|| extract_content_tool_calls(message)),
        tool_call_id: extract_tool_call_id(message),
        name: message
            .name
            .as_deref()
            .and_then(non_empty_trimmed)
            .or_else(|| {
                message
                    .content
                    .get("name")
                    .and_then(|v| v.as_str())
                    .and_then(non_empty_trimmed)
            }),
        thinking: None,
        thinking_blocks: extract_thinking_blocks(message),
        anthropic_multimodal_turn: None,
    }
}

fn extract_thinking_blocks(message: &StoredMessage) -> Option<Vec<serde_json::Value>> {
    if message.role != "assistant" {
        return None;
    }
    let blocks = message
        .content
        .get("thinkingBlocks")
        .or_else(|| message.content.get("thinking_blocks"))?
        .as_array()?;
    let blocks: Vec<serde_json::Value> = blocks
        .iter()
        .filter(|block| block.is_object())
        .cloned()
        .collect();
    (!blocks.is_empty()).then_some(blocks)
}

/// Tool messages may have toolCallId at top-level (new schema) or nested in
/// `content.toolCallId` (legacy/embedded). Fall back to content so LLM gateway
/// always receives a non-null tool_use_id.
fn extract_tool_call_id(message: &StoredMessage) -> Option<String> {
    message
        .tool_call_id
        .as_deref()
        .and_then(non_empty_trimmed)
        .or_else(|| {
            message
                .content
                .get("toolCallId")
                .and_then(|v| v.as_str())
                .and_then(non_empty_trimmed)
        })
}

/// Assistant messages may carry tool_calls at top-level (new schema) or nested
/// in `content.toolCalls` (legacy). Fall back so we don't drop tool linkage.
fn extract_content_tool_calls(message: &StoredMessage) -> Option<Vec<ToolCall>> {
    if message.role != "assistant" {
        return None;
    }
    let arr = message.content.get("toolCalls")?.as_array()?;
    let calls: Vec<serde_json::Value> = arr.clone();
    normalize_tool_calls(Some(&calls))
}

fn build_chat_message_content(message: &StoredMessage, config: &HistoryConfig) -> String {
    if message.role == "user" && config.include_uploaded_file_hints {
        if let Some(text) = message.content.get("text").and_then(|value| value.as_str()) {
            if let Some(files) = message
                .content
                .get("files")
                .and_then(|value| value.as_array())
            {
                if !files.is_empty() {
                    let attachments: Vec<
                        crate::runtime::chat::chat_turn_driver::ChatAttachmentRef,
                    > = files
                        .iter()
                        .map(
                            |file| crate::runtime::chat::chat_turn_driver::ChatAttachmentRef {
                                id: file
                                    .get("id")
                                    .and_then(|value| value.as_str())
                                    .unwrap_or_default()
                                    .to_string(),
                                file_name: file
                                    .get("fileName")
                                    .or_else(|| file.get("originalName"))
                                    .and_then(|value| value.as_str())
                                    .unwrap_or("unknown")
                                    .to_string(),
                                file_path: file
                                    .get("filePath")
                                    .or_else(|| file.get("path"))
                                    .or_else(|| file.get("id"))
                                    .and_then(|value| value.as_str())
                                    .unwrap_or_default()
                                    .to_string(),
                                kind: file
                                    .get("kind")
                                    .and_then(|value| value.as_str())
                                    .unwrap_or("file")
                                    .to_string(),
                                file_size: file
                                    .get("fileSize")
                                    .and_then(|value| value.as_u64())
                                    .unwrap_or(0),
                                file_type: file
                                    .get("fileType")
                                    .and_then(|value| value.as_str())
                                    .unwrap_or("unknown")
                                    .to_string(),
                                mime_type: file
                                    .get("mimeType")
                                    .and_then(|value| value.as_str())
                                    .map(ToString::to_string),
                            },
                        )
                        .collect();
                    return crate::transport::tauri_commands::chat::chat_runtime_impl::build_llm_content(
                        text,
                        &attachments,
                        config.has_authorized_workspace,
                    );
                }
            }
            return text.to_string();
        }
    }

    message.text().to_string()
}

fn normalize_tool_calls(tool_calls: Option<&Vec<serde_json::Value>>) -> Option<Vec<ToolCall>> {
    let normalized: Vec<ToolCall> = tool_calls
        .into_iter()
        .flatten()
        .filter_map(normalize_tool_call)
        .collect();

    if normalized.is_empty() {
        None
    } else {
        Some(normalized)
    }
}

fn normalize_tool_call(value: &serde_json::Value) -> Option<ToolCall> {
    let id = non_empty_trimmed(value.get("id").and_then(|v| v.as_str())?)?;

    if let (Some(name), Some(arguments)) = (
        value
            .get("name")
            .and_then(|v| v.as_str())
            .and_then(non_empty_trimmed),
        value.get("arguments"),
    ) {
        return ToolCall {
            id,
            name,
            arguments: arguments.clone(),
        }
        .into_valid()
        .ok();
    }

    let function = value.get("function")?;
    let name = non_empty_trimmed(function.get("name").and_then(|v| v.as_str())?)?;
    let arguments = function
        .get("arguments")
        .cloned()
        .unwrap_or(serde_json::json!({}));

    let arguments = match arguments {
        serde_json::Value::String(raw) => {
            serde_json::from_str(&raw).unwrap_or_else(|_| serde_json::Value::String(raw))
        }
        other => other,
    };

    ToolCall {
        id,
        name,
        arguments,
    }
    .into_valid()
    .ok()
}

fn non_empty_trimmed(value: &str) -> Option<String> {
    let trimmed = value.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_string())
}

fn filter_invalid_tool_pairs(messages: Vec<ChatMessage>) -> Vec<ChatMessage> {
    // Legacy messages (pre-schemaVersion) have toolCallId=None on tool messages.
    // Skip filtering entirely for such conversations to keep them visible.
    let has_legacy_tool_messages = messages
        .iter()
        .any(|m| m.role == "tool" && m.tool_call_id.is_none());
    if has_legacy_tool_messages {
        return messages;
    }

    let responded_ids: HashSet<String> = messages
        .iter()
        .filter(|message| message.role == "tool")
        .filter_map(|message| message.tool_call_id.clone())
        .collect();

    let declared_ids: HashSet<String> = messages
        .iter()
        .filter(|message| message.role == "assistant")
        .flat_map(|message| {
            message
                .tool_calls
                .iter()
                .flatten()
                .map(|tool_call| tool_call.id.clone())
        })
        .collect();

    messages
        .into_iter()
        .filter(|message| {
            if message.role == "tool" {
                return message
                    .tool_call_id
                    .as_ref()
                    .map(|id| !id.trim().is_empty() && declared_ids.contains(id))
                    .unwrap_or(false);
            }
            true
        })
        .map(|mut message| {
            if message.role == "assistant" {
                if let Some(tool_calls) = message.tool_calls.clone() {
                    let all_responded = tool_calls.iter().all(|tool_call| {
                        !tool_call.id.trim().is_empty() && responded_ids.contains(&tool_call.id)
                    });
                    if !all_responded {
                        message.tool_calls = None;
                    }
                }
            }
            message
        })
        .collect()
}

/// Some legacy conversations stored tool messages BEFORE their owning assistant
/// message (because tool results were persisted as they streamed, while the
/// assistant's tool_use was finalized later). LLM gateways require strict
/// `assistant(tool_use) → tool(tool_result)` ordering, so reorder here.
///
/// Strategy: walk messages in order. Buffer any tool messages we encounter.
/// When we see an assistant message with tool_calls, emit it followed by the
/// matching tools (preserving the assistant's tool_call order). Drop tools
/// whose owning assistant cannot be found.
fn reorder_tool_results_after_assistant(messages: Vec<ChatMessage>) -> Vec<ChatMessage> {
    use std::collections::HashMap;

    // Index every tool message by tool_call_id. If duplicates exist, last wins.
    let mut tool_pool: HashMap<String, ChatMessage> = HashMap::new();
    let mut non_tool: Vec<ChatMessage> = Vec::with_capacity(messages.len());

    for m in messages {
        if m.role == "tool" {
            if let Some(id) = m.tool_call_id.clone().filter(|s| !s.is_empty()) {
                tool_pool.insert(id, m);
            }
            // tools without id were already filtered earlier; drop silently
            continue;
        }
        non_tool.push(m);
    }

    let mut out: Vec<ChatMessage> = Vec::with_capacity(non_tool.len() + tool_pool.len());
    for m in non_tool {
        let tool_call_ids: Vec<String> = m
            .tool_calls
            .as_ref()
            .map(|calls| calls.iter().map(|c| c.id.clone()).collect())
            .unwrap_or_default();
        out.push(m);
        for id in tool_call_ids {
            if let Some(tool_msg) = tool_pool.remove(&id) {
                out.push(tool_msg);
            }
        }
    }
    // Any remaining tool messages are orphans: their assistant declaration was
    // dropped by filter_invalid_tool_pairs. Discarding them is correct.
    out
}

fn trim_to_budget(messages: Vec<ChatMessage>, config: &HistoryConfig) -> Vec<ChatMessage> {
    let rounds = split_into_rounds(&messages);
    let mut kept: Vec<&[ChatMessage]> = rounds.iter().map(|round| round.as_slice()).collect();

    loop {
        let total_chars: usize = kept
            .iter()
            .flat_map(|round| round.iter())
            .map(|message| message.content.len())
            .sum();
        if kept.len() <= config.max_rounds && total_chars <= config.char_budget {
            break;
        }
        if kept.is_empty() {
            break;
        }
        kept.remove(0);
    }

    kept.into_iter()
        .flat_map(|round| round.iter().cloned())
        .collect()
}

fn split_into_rounds(messages: &[ChatMessage]) -> Vec<Vec<ChatMessage>> {
    let mut rounds = Vec::new();
    let mut current = Vec::new();

    for message in messages {
        if message.role == "user" && !current.is_empty() {
            rounds.push(current);
            current = Vec::new();
        }
        current.push(message.clone());
    }

    if !current.is_empty() {
        rounds.push(current);
    }

    rounds
}

/// Defensive: collapse trailing consecutive `user` messages into a single one.
///
/// Why: when a turn fails (LLM 4xx/5xx) the user message has already been
/// persisted to messages.jsonl but no assistant reply ever lands. The next
/// send appends another user message, and the request now has 2+ consecutive
/// user turns at the tail. Anthropic's `/v1/messages` endpoint is documented
/// to merge consecutive same-role turns server-side, but in practice some
/// gateways / model versions return 400 "Improperly formed request" when the
/// tail has stacked user messages.
///
/// The fix is to merge them ourselves before sending. This is loss-less for
/// the model (it sees the same content); it just collapses N tail messages
/// into one separated by "\n\n". Only TRAILING consecutive users are merged
/// — earlier stretches are left intact.
fn collapse_trailing_consecutive_user(mut messages: Vec<ChatMessage>) -> Vec<ChatMessage> {
    if messages.len() < 2 {
        return messages;
    }
    // Find the index where the trailing user run begins.
    let mut start = messages.len();
    for (i, m) in messages.iter().enumerate().rev() {
        if m.role == "user" {
            start = i;
        } else {
            break;
        }
    }
    let run_len = messages.len() - start;
    if run_len < 2 {
        return messages;
    }
    // Drain the tail run and stitch its content together.
    let tail = messages.split_off(start);
    let mut combined = String::new();
    for (idx, m) in tail.iter().enumerate() {
        if idx > 0 {
            combined.push_str("\n\n");
        }
        combined.push_str(&m.content);
    }
    messages.push(ChatMessage::text("user", combined));
    messages
}

#[cfg(test)]
mod collapse_trailing_tests {
    use super::*;

    fn msg(role: &str, text: &str) -> ChatMessage {
        ChatMessage::text(role, text)
    }

    #[test]
    fn empty_unchanged() {
        let out = collapse_trailing_consecutive_user(vec![]);
        assert!(out.is_empty());
    }

    #[test]
    fn single_unchanged() {
        let out = collapse_trailing_consecutive_user(vec![msg("user", "hi")]);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].content, "hi");
    }

    #[test]
    fn no_trailing_run_unchanged() {
        let input = vec![msg("user", "a"), msg("assistant", "b")];
        let out = collapse_trailing_consecutive_user(input);
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].role, "user");
        assert_eq!(out[0].content, "a");
        assert_eq!(out[1].role, "assistant");
        assert_eq!(out[1].content, "b");
    }

    #[test]
    fn merges_two_trailing_users() {
        let input = vec![
            msg("assistant", "ok"),
            msg("user", "first"),
            msg("user", "second"),
        ];
        let out = collapse_trailing_consecutive_user(input);
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].role, "assistant");
        assert_eq!(out[1].role, "user");
        assert_eq!(out[1].content, "first\n\nsecond");
    }

    #[test]
    fn merges_three_trailing_users() {
        let input = vec![
            msg("assistant", "ok"),
            msg("user", "a"),
            msg("user", "b"),
            msg("user", "c"),
        ];
        let out = collapse_trailing_consecutive_user(input);
        assert_eq!(out.len(), 2);
        assert_eq!(out[1].content, "a\n\nb\n\nc");
    }

    #[test]
    fn earlier_user_runs_left_intact() {
        // Only the trailing run is collapsed. An earlier user/user pair is
        // left as-is (Anthropic auto-merges those; trimming earlier turns
        // could remove tool-use/result pairs by accident).
        let input = vec![
            msg("user", "a"),
            msg("user", "b"),
            msg("assistant", "mid"),
            msg("user", "c"),
            msg("user", "d"),
        ];
        let out = collapse_trailing_consecutive_user(input);
        assert_eq!(out.len(), 4);
        assert_eq!(out[0].content, "a");
        assert_eq!(out[1].content, "b");
        assert_eq!(out[2].content, "mid");
        assert_eq!(out[3].content, "c\n\nd");
    }

    #[test]
    fn stored_assistant_preserves_thinking_blocks_from_content() {
        let stored = StoredMessage {
            id: "m1".to_string(),
            conversation_id: "c1".to_string(),
            role: "assistant".to_string(),
            content: serde_json::json!({
                "text": "",
                "thinkingBlocks": [{
                    "type": "thinking",
                    "thinking": "hidden",
                    "signature": "sig-1"
                }]
            }),
            created_at: "2026-05-28T00:00:00Z".to_string(),
            tool_calls: Some(vec![serde_json::json!({
                "id": "call_1",
                "name": "SearchMemory",
                "arguments": {"query": "x"}
            })]),
            tool_call_id: None,
            name: None,
            run_id: None,
            schema_version: Some(2),
            sequence: None,
            seq: None,
            rev: None,
            error: None,
        };

        let chat = stored_to_chat(&stored, &HistoryConfig::default());

        let blocks = chat.thinking_blocks.expect("thinking blocks");
        assert_eq!(blocks.len(), 1);
        assert_eq!(blocks[0]["thinking"], "hidden");
        assert_eq!(blocks[0]["signature"], "sig-1");
    }

    #[test]
    fn build_history_drops_empty_tool_call_and_matching_empty_tool_result() {
        let stored = vec![
            StoredMessage {
                id: "m1".to_string(),
                conversation_id: "c1".to_string(),
                role: "assistant".to_string(),
                content: serde_json::json!({"text": "checking"}),
                created_at: "2026-06-02T00:00:00Z".to_string(),
                tool_calls: Some(vec![serde_json::json!({
                    "id": "",
                    "name": "",
                    "arguments": null
                })]),
                tool_call_id: None,
                name: None,
                run_id: None,
                schema_version: Some(2),
                sequence: None,
                seq: None,
                rev: None,
                error: None,
            },
            StoredMessage {
                id: "m2".to_string(),
                conversation_id: "c1".to_string(),
                role: "tool".to_string(),
                content: serde_json::json!({"text": "bad result", "toolCallId": ""}),
                created_at: "2026-06-02T00:00:01Z".to_string(),
                tool_calls: None,
                tool_call_id: Some(String::new()),
                name: Some(String::new()),
                run_id: None,
                schema_version: Some(2),
                sequence: None,
                seq: None,
                rev: None,
                error: None,
            },
        ];

        let history = build_chat_history(&stored, None, &HistoryConfig::default()).unwrap();

        assert_eq!(history.len(), 1);
        assert_eq!(history[0].role, "assistant");
        assert!(history[0].tool_calls.is_none());
    }
}
