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
        tool_calls: normalize_tool_calls(message.tool_calls.as_ref()),
        tool_call_id: message.tool_call_id.clone(),
        name: message.name.clone(),
        thinking: None,
        thinking_blocks: None,
    }
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
