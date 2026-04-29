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

    let mut messages: Vec<ChatMessage> = relevant
        .iter()
        .map(|message| stored_to_chat(message, config))
        .collect();

    messages = filter_invalid_tool_pairs(messages);
    messages = reorder_tool_results_after_assistant(messages);
    messages = trim_to_budget(messages, config);

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
        name: message.name.clone().or_else(|| {
            message
                .content
                .get("name")
                .and_then(|v| v.as_str())
                .map(String::from)
        }),
    }
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
                    let attachments: Vec<crate::runtime::chat::chat_turn_driver::ChatAttachmentRef> = files
                        .iter()
                        .map(|file| crate::runtime::chat::chat_turn_driver::ChatAttachmentRef {
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
                            file_size: file.get("fileSize").and_then(|value| value.as_u64()).unwrap_or(0),
                            file_type: file
                                .get("fileType")
                                .and_then(|value| value.as_str())
                                .unwrap_or("unknown")
                                .to_string(),
                            mime_type: file
                                .get("mimeType")
                                .and_then(|value| value.as_str())
                                .map(ToString::to_string),
                        })
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
