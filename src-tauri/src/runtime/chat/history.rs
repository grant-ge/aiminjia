use std::collections::HashSet;

use anyhow::Result;

use crate::llm::streaming::{ChatMessage, ToolCall};
use crate::runtime::chat::compaction::{CompactBoundaryRecord, CompactTrigger, PreservedSegment};
use crate::storage::file_store::types::StoredMessage;

#[derive(Debug, Clone)]
pub struct HistoryConfig {
    pub char_budget: usize,
    pub max_rounds: usize,
    pub include_uploaded_file_hints: bool,
    pub has_authorized_workspace: bool,
    pub trim_to_budget: bool,
}

impl Default for HistoryConfig {
    fn default() -> Self {
        Self {
            char_budget: 120_000,
            max_rounds: 30,
            include_uploaded_file_hints: true,
            has_authorized_workspace: false,
            trim_to_budget: true,
        }
    }
}

#[derive(Debug, Clone)]
struct HistoryChatMessage {
    id: Option<String>,
    conversation_id: Option<String>,
    created_at: Option<String>,
    chat: ChatMessage,
    subtype: Option<String>,
    compact_metadata: Option<serde_json::Value>,
    is_compact_summary: Option<bool>,
}

pub fn build_chat_history(
    stored: &[StoredMessage],
    boundary: Option<&CompactBoundaryRecord>,
    config: &HistoryConfig,
    claude_md_content: Option<&str>,
) -> Result<Vec<ChatMessage>> {
    let messages = build_history_messages(stored, boundary, config, claude_md_content)?;
    Ok(messages.into_iter().map(|message| message.chat).collect())
}

/// Build model-facing JSON messages while retaining transcript metadata.
///
/// The driver needs `id`/`conversationId`/`createdAt` during auto-compact so a
/// successful boundary can anchor to a real tail message. The LLM gateway later
/// deserializes these values into `ChatMessage`, which ignores the metadata.
pub fn build_chat_history_values(
    stored: &[StoredMessage],
    boundary: Option<&CompactBoundaryRecord>,
    config: &HistoryConfig,
    claude_md_content: Option<&str>,
) -> Result<Vec<serde_json::Value>> {
    let messages = build_history_messages(stored, boundary, config, claude_md_content)?;
    Ok(messages.into_iter().map(history_message_to_value).collect())
}

fn build_history_messages(
    stored: &[StoredMessage],
    boundary: Option<&CompactBoundaryRecord>,
    config: &HistoryConfig,
    claude_md_content: Option<&str>,
) -> Result<Vec<HistoryChatMessage>> {
    let transcript_boundary = compact_boundary_from_transcript(stored);
    let merged_boundary = boundary.and_then(|sidecar| {
        transcript_boundary
            .as_ref()
            .map(|t| merge_boundary(sidecar, t, stored))
    });
    let effective_boundary = merged_boundary
        .as_ref()
        .or(boundary)
        .or(transcript_boundary.as_ref());
    let relevant = apply_boundary(stored, effective_boundary);

    // PR2: 过滤掉 error.is_some() 的消息（避免错误气泡回灌给 LLM）。
    // 守卫规则等价 claude-code-best `isApiErrorMessage:true` 过滤。
    // spec §3.2。
    let filtered: Vec<&StoredMessage> = relevant
        .iter()
        .filter(|m| m.error.is_none())
        .filter(|m| effective_boundary.is_none() || !is_stored_compact_artifact(m))
        .collect();

    let mut messages: Vec<HistoryChatMessage> = filtered
        .iter()
        .map(|message| stored_to_history_chat(message, config))
        .collect();

    messages = filter_invalid_tool_pairs_history(messages);
    messages = reorder_tool_results_after_assistant_history(messages);
    if config.trim_to_budget {
        messages = trim_to_budget_history(messages, config);
    }
    messages = collapse_trailing_consecutive_user_history(messages);

    if let Some(boundary) = effective_boundary {
        if !boundary.summary_text.is_empty() {
            let context_text = if let Some(claude_md) = claude_md_content {
                format!(
                    "<context>\n{}\n</context>\n\n<project_context>\n{}\n</project_context>",
                    boundary.summary_text, claude_md
                )
            } else {
                format!("<context>\n{}\n</context>", boundary.summary_text)
            };
            messages.insert(
                0,
                HistoryChatMessage {
                    id: None,
                    conversation_id: Some(boundary.conversation_id.clone()),
                    created_at: Some(boundary.created_at.clone()),
                    chat: ChatMessage::text("user", context_text),
                    subtype: None,
                    compact_metadata: None,
                    is_compact_summary: None,
                },
            );
        }
    }

    Ok(messages)
}

fn merge_boundary(
    sidecar: &CompactBoundaryRecord,
    transcript: &CompactBoundaryRecord,
    stored: &[StoredMessage],
) -> CompactBoundaryRecord {
    let mut merged = sidecar.clone();

    if merged.summary_text.trim().is_empty() {
        merged.summary_text = transcript.summary_text.clone();
    }
    if merged
        .tail_message_id
        .as_deref()
        .map_or(true, str::is_empty)
        || !boundary_tail_exists(stored, merged.tail_message_id.as_deref())
    {
        merged.tail_message_id = transcript.tail_message_id.clone();
    }
    if merged.preserved_segment.is_none() {
        merged.preserved_segment = transcript.preserved_segment.clone();
    }
    if merged.pre_tokens == 0 {
        merged.pre_tokens = transcript.pre_tokens;
    }
    if merged.post_tokens == 0 {
        merged.post_tokens = transcript.post_tokens;
    }
    if merged.messages_summarized == 0 {
        merged.messages_summarized = transcript.messages_summarized;
    }

    merged
}

fn boundary_tail_exists(stored: &[StoredMessage], tail_id: Option<&str>) -> bool {
    let Some(tail_id) = tail_id.filter(|value| !value.is_empty()) else {
        return false;
    };
    stored.iter().any(|message| message.id == tail_id)
}

fn compact_boundary_from_transcript(stored: &[StoredMessage]) -> Option<CompactBoundaryRecord> {
    let (index, boundary) = stored.iter().enumerate().rev().find(|(_, message)| {
        message.role == "system" && message.subtype.as_deref() == Some("compact_boundary")
    })?;

    let metadata = boundary.compact_metadata.as_ref();
    let trigger = metadata
        .and_then(|value| metadata_str(value, &["trigger"]))
        .map(|value| {
            if value.eq_ignore_ascii_case("manual") {
                CompactTrigger::Manual
            } else {
                CompactTrigger::Auto
            }
        })
        .unwrap_or(CompactTrigger::Auto);

    Some(CompactBoundaryRecord {
        id: boundary.id.clone(),
        conversation_id: boundary.conversation_id.clone(),
        trigger,
        pre_tokens: metadata
            .and_then(|value| metadata_u64(value, &["preTokens", "pre_tokens"]))
            .unwrap_or(0),
        post_tokens: metadata
            .and_then(|value| metadata_u64(value, &["postTokens", "post_tokens"]))
            .unwrap_or(0),
        messages_summarized: metadata
            .and_then(|value| metadata_u64(value, &["messagesSummarized", "messages_summarized"]))
            .unwrap_or(0) as usize,
        created_at: boundary.created_at.clone(),
        summary_text: compact_summary_text_after(stored, index),
        tail_message_id: metadata
            .and_then(|value| metadata_str(value, &["tailMessageId", "tail_message_id"]))
            .filter(|value| !value.is_empty()),
        preserved_segment: metadata.and_then(preserved_segment_from_metadata),
    })
}

fn metadata_u64(metadata: &serde_json::Value, keys: &[&str]) -> Option<u64> {
    keys.iter()
        .find_map(|key| metadata.get(*key).and_then(serde_json::Value::as_u64))
}

fn metadata_str(metadata: &serde_json::Value, keys: &[&str]) -> Option<String> {
    keys.iter().find_map(|key| {
        metadata
            .get(*key)
            .and_then(serde_json::Value::as_str)
            .map(str::to_string)
    })
}

fn preserved_segment_from_metadata(metadata: &serde_json::Value) -> Option<PreservedSegment> {
    let preserved = metadata.get("preservedSegment")?;
    let first_preserved_message_id =
        metadata_str(preserved, &["firstPreservedMessageId", "headUuid"])?;
    let anchor_message_id = metadata_str(preserved, &["anchorMessageId", "anchorUuid"])?;
    let tail_message_id =
        metadata_str(preserved, &["tailMessageId", "tailUuid"]).unwrap_or_else(|| {
            // Legacy lotus-app records used anchor_message_id as the tail.
            anchor_message_id.clone()
        });
    let preserved_token_count =
        metadata_u64(preserved, &["preservedTokenCount", "preserved_token_count"]).unwrap_or(0);

    Some(PreservedSegment {
        first_preserved_message_id,
        anchor_message_id,
        tail_message_id,
        preserved_token_count,
    })
}

fn compact_summary_text_after(stored: &[StoredMessage], boundary_index: usize) -> String {
    stored
        .iter()
        .skip(boundary_index + 1)
        .find(|message| message.is_compact_summary == Some(true))
        .map(|message| unwrap_context_envelope(message.text()))
        .unwrap_or_default()
}

fn unwrap_context_envelope(text: &str) -> String {
    let trimmed = text.trim();
    if let Some(rest) = trimmed.strip_prefix("<context>") {
        if let Some((inner, _)) = rest.split_once("</context>") {
            return inner.trim().to_string();
        }
    }
    trimmed.to_string()
}

/// Return the slice of stored messages after the compact boundary.
///
/// When a boundary exists and its `tail_message_id` matches a stored message,
/// returns `stored[index..]` (inclusive of the tail message). Falls back to
/// the full `stored` slice when the boundary is missing or the tail ID doesn't
/// match any stored message.
pub fn apply_boundary<'a>(
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
        name: message.name.clone().or_else(|| {
            message
                .content
                .get("name")
                .and_then(|v| v.as_str())
                .map(String::from)
        }),
        thinking: None,
        thinking_blocks: extract_thinking_blocks(message),
        anthropic_multimodal_turn: None,
    }
}

fn is_stored_compact_artifact(message: &StoredMessage) -> bool {
    message.is_compact_summary == Some(true)
        || (message.role == "system" && message.subtype.as_deref() == Some("compact_boundary"))
}

fn stored_to_history_chat(message: &StoredMessage, config: &HistoryConfig) -> HistoryChatMessage {
    HistoryChatMessage {
        id: Some(message.id.clone()),
        conversation_id: Some(message.conversation_id.clone()),
        created_at: Some(message.created_at.clone()),
        chat: stored_to_chat(message, config),
        subtype: message.subtype.clone(),
        compact_metadata: message.compact_metadata.clone(),
        is_compact_summary: message.is_compact_summary,
    }
}

fn history_message_to_value(message: HistoryChatMessage) -> serde_json::Value {
    let mut value = serde_json::json!({
        "role": message.chat.role,
        "content": message.chat.content,
    });
    if let Some(id) = message.id {
        value["id"] = id.into();
    }
    if let Some(conversation_id) = message.conversation_id {
        value["conversationId"] = conversation_id.into();
    }
    if let Some(created_at) = message.created_at {
        value["createdAt"] = created_at.into();
    }
    if let Some(tool_calls) = message.chat.tool_calls {
        if let Ok(serialized) = serde_json::to_value(tool_calls) {
            value["toolCalls"] = serialized;
        }
    }
    if let Some(thinking_blocks) = message.chat.thinking_blocks {
        value["thinkingBlocks"] = serde_json::Value::Array(thinking_blocks);
    }
    if let Some(tool_call_id) = message.chat.tool_call_id {
        value["toolCallId"] = tool_call_id.into();
    }
    if let Some(name) = message.chat.name {
        value["name"] = name.into();
    }
    if let Some(subtype) = message.subtype {
        value["subtype"] = subtype.into();
    }
    if let Some(compact_metadata) = message.compact_metadata {
        value["compactMetadata"] = compact_metadata;
    }
    if let Some(is_compact_summary) = message.is_compact_summary {
        value["isCompactSummary"] = is_compact_summary.into();
    }
    value
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
        .clone()
        .filter(|s| !s.is_empty())
        .or_else(|| {
            message
                .content
                .get("toolCallId")
                .and_then(|v| v.as_str())
                .filter(|s| !s.is_empty())
                .map(String::from)
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
    let id = value.get("id").and_then(|v| v.as_str())?.to_string();

    if let (Some(name), Some(arguments)) = (
        value.get("name").and_then(|v| v.as_str()),
        value.get("arguments"),
    ) {
        return Some(ToolCall {
            id,
            name: name.to_string(),
            arguments: arguments.clone(),
        });
    }

    let function = value.get("function")?;
    let name = function.get("name").and_then(|v| v.as_str())?;
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

    Some(ToolCall {
        id,
        name: name.to_string(),
        arguments,
    })
}

#[allow(dead_code)]
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
                    .map(|id| declared_ids.contains(id))
                    .unwrap_or(false);
            }
            true
        })
        .map(|mut message| {
            if message.role == "assistant" {
                if let Some(tool_calls) = message.tool_calls.clone() {
                    let all_responded = tool_calls
                        .iter()
                        .all(|tool_call| responded_ids.contains(&tool_call.id));
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
#[allow(dead_code)]
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

#[allow(dead_code)]
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

#[allow(dead_code)]
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
#[allow(dead_code)]
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

fn filter_invalid_tool_pairs_history(messages: Vec<HistoryChatMessage>) -> Vec<HistoryChatMessage> {
    let has_legacy_tool_messages = messages
        .iter()
        .any(|m| m.chat.role == "tool" && m.chat.tool_call_id.is_none());
    if has_legacy_tool_messages {
        return messages;
    }

    let responded_ids: HashSet<String> = messages
        .iter()
        .filter(|message| message.chat.role == "tool")
        .filter_map(|message| message.chat.tool_call_id.clone())
        .collect();

    let declared_ids: HashSet<String> = messages
        .iter()
        .filter(|message| message.chat.role == "assistant")
        .flat_map(|message| {
            message
                .chat
                .tool_calls
                .iter()
                .flatten()
                .map(|tool_call| tool_call.id.clone())
        })
        .collect();

    messages
        .into_iter()
        .filter(|message| {
            if message.chat.role == "tool" {
                return message
                    .chat
                    .tool_call_id
                    .as_ref()
                    .map(|id| declared_ids.contains(id))
                    .unwrap_or(false);
            }
            true
        })
        .map(|mut message| {
            if message.chat.role == "assistant" {
                if let Some(tool_calls) = message.chat.tool_calls.clone() {
                    let all_responded = tool_calls
                        .iter()
                        .all(|tool_call| responded_ids.contains(&tool_call.id));
                    if !all_responded {
                        message.chat.tool_calls = None;
                    }
                }
            }
            message
        })
        .collect()
}

fn reorder_tool_results_after_assistant_history(
    messages: Vec<HistoryChatMessage>,
) -> Vec<HistoryChatMessage> {
    use std::collections::HashMap;

    let mut tool_pool: HashMap<String, HistoryChatMessage> = HashMap::new();
    let mut non_tool: Vec<HistoryChatMessage> = Vec::with_capacity(messages.len());

    for message in messages {
        if message.chat.role == "tool" {
            if let Some(id) = message
                .chat
                .tool_call_id
                .clone()
                .filter(|value| !value.is_empty())
            {
                tool_pool.insert(id, message);
            }
            continue;
        }
        non_tool.push(message);
    }

    let mut out: Vec<HistoryChatMessage> = Vec::with_capacity(non_tool.len() + tool_pool.len());
    for message in non_tool {
        let tool_call_ids: Vec<String> = message
            .chat
            .tool_calls
            .as_ref()
            .map(|calls| calls.iter().map(|call| call.id.clone()).collect())
            .unwrap_or_default();
        out.push(message);
        for id in tool_call_ids {
            if let Some(tool_msg) = tool_pool.remove(&id) {
                out.push(tool_msg);
            }
        }
    }

    out
}

fn trim_to_budget_history(
    messages: Vec<HistoryChatMessage>,
    config: &HistoryConfig,
) -> Vec<HistoryChatMessage> {
    let rounds = split_into_rounds_history(&messages);
    let mut kept: Vec<&[HistoryChatMessage]> =
        rounds.iter().map(|round| round.as_slice()).collect();

    loop {
        let total_chars: usize = kept
            .iter()
            .flat_map(|round| round.iter())
            .map(|message| message.chat.content.len())
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

fn split_into_rounds_history(messages: &[HistoryChatMessage]) -> Vec<Vec<HistoryChatMessage>> {
    let mut rounds = Vec::new();
    let mut current = Vec::new();

    for message in messages {
        if message.chat.role == "user" && !current.is_empty() {
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

fn collapse_trailing_consecutive_user_history(
    mut messages: Vec<HistoryChatMessage>,
) -> Vec<HistoryChatMessage> {
    if messages.len() < 2 {
        return messages;
    }

    let mut start = messages.len();
    for (index, message) in messages.iter().enumerate().rev() {
        if message.chat.role == "user" {
            start = index;
        } else {
            break;
        }
    }

    let run_len = messages.len() - start;
    if run_len < 2 {
        return messages;
    }

    let tail = messages.split_off(start);
    let mut combined = String::new();
    for (index, message) in tail.iter().enumerate() {
        if index > 0 {
            combined.push_str("\n\n");
        }
        combined.push_str(&message.chat.content);
    }

    let mut collapsed = tail.last().cloned().unwrap_or_else(|| HistoryChatMessage {
        id: None,
        conversation_id: None,
        created_at: None,
        chat: ChatMessage::text("user", ""),
        subtype: None,
        compact_metadata: None,
        is_compact_summary: None,
    });
    collapsed.chat = ChatMessage::text("user", combined);
    messages.push(collapsed);
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
            subtype: None,
            compact_metadata: None,
            is_compact_summary: None,
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
}
