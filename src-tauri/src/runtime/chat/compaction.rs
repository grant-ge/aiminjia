//! Context compaction helpers for the chat turn driver.
//!
//! This module is a thin forwarding layer over the existing compaction
//! implementations in `crate::llm`:
//!
//! - `apply_decay` — non-destructive progressive truncation of older tool
//!   outputs within a single agent turn. Delegates to
//!   [`crate::llm::context_decay::apply_decay`].
//!
//! - `compress_context_if_needed` — LLM-assisted summarisation of long daily
//!   conversations. This function depends on [`crate::llm::gateway::LlmGateway`]
//!   and [`crate::models::settings::AppSettings`], which are heavyweight
//!   infrastructure types that do not belong in the runtime layer. The wrapper
//!   is intentionally left as a **TODO** stub until T15 (legacy code cleanup)
//!   moves or re-exports the implementation through an appropriate seam (e.g.
//!   `RuntimeLlmExecutor`).

use std::path::{Path, PathBuf};

use crate::llm::streaming::ChatMessage;
use crate::runtime::chat::tool_result_artifact::is_persisted_tool_result_message;
use serde::{Deserialize, Serialize};

/// Non-destructive context decay: reduce token weight of older tool outputs.
///
/// Returns a new `Vec<ChatMessage>` with truncated tool result content for
/// iterations older than the most recent one. The caller's original slice is
/// never mutated, so checkpoint / auto-capture logic still sees full data.
///
/// Delegates to [`crate::llm::context_decay::apply_decay`].
pub fn apply_decay(messages: &[ChatMessage]) -> Vec<ChatMessage> {
    crate::llm::context_decay::apply_decay(messages)
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum CompactTrigger {
    Auto,
    Manual,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompactBoundaryRecord {
    pub id: String,
    pub conversation_id: String,
    pub trigger: CompactTrigger,
    pub pre_tokens: u64,
    pub post_tokens: u64,
    pub messages_summarized: usize,
    pub created_at: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub summary_text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tail_message_id: Option<String>,
    /// Metadata about the message segment preserved by this compaction.
    /// Records head/anchor message IDs and estimated token count of the
    /// preserved tail. Deserializes to `None` for legacy records.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preserved_segment: Option<PreservedSegment>,
}

/// Metadata about the segment of messages preserved after compaction.
///
/// Mirrors claude-code-best's `preservedSegment`:
/// head = first kept message, anchor = summary message, tail = last kept message.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PreservedSegment {
    /// UUID of the first message kept in the preserved tail.
    #[serde(default)]
    pub first_preserved_message_id: String,
    /// UUID of the summary message the preserved tail is attached after.
    #[serde(default)]
    pub anchor_message_id: String,
    /// UUID of the last message kept in the preserved tail.
    #[serde(default)]
    pub tail_message_id: String,
    /// Estimated token count of the preserved segment.
    #[serde(default)]
    pub preserved_token_count: u64,
}

pub fn build_compact_boundary_record(
    conversation_id: &str,
    trigger: CompactTrigger,
    pre_tokens: u64,
    post_tokens: u64,
    messages_summarized: usize,
) -> CompactBoundaryRecord {
    CompactBoundaryRecord {
        id: uuid::Uuid::new_v4().to_string(),
        conversation_id: conversation_id.to_string(),
        trigger,
        pre_tokens,
        post_tokens,
        messages_summarized,
        created_at: chrono::Utc::now().to_rfc3339(),
        summary_text: String::new(),
        tail_message_id: None,
        preserved_segment: None,
    }
}

#[derive(Debug, Clone)]
pub struct MicrocompactConfig {
    pub trigger_chars: usize,
    pub keep_recent_tool_results: usize,
    pub preserved_tool_names: std::collections::HashSet<String>,
}

impl Default for MicrocompactConfig {
    fn default() -> Self {
        let preserved_tool_names = crate::runtime::tools::catalog::TOOL_CATALOG
            .all_ids()
            .into_iter()
            .filter(|id| {
                crate::runtime::tools::catalog::TOOL_CATALOG
                    .get(id)
                    .map(|def| def.preserve_tool_use_results)
                    .unwrap_or(false)
            })
            .collect();
        Self {
            trigger_chars: 120_000,
            keep_recent_tool_results: 2,
            preserved_tool_names,
        }
    }
}

#[derive(Debug, Clone)]
pub struct MicrocompactResult {
    pub messages: Vec<serde_json::Value>,
    pub executed: bool,
    pub tokens_freed_estimate: usize,
}

fn estimate_total_chars(messages: &[serde_json::Value]) -> usize {
    messages
        .iter()
        .map(|message| message.to_string().len())
        .sum()
}

pub fn microcompact(
    messages: &[serde_json::Value],
    config: &MicrocompactConfig,
) -> MicrocompactResult {
    let total_chars = estimate_total_chars(messages);
    if total_chars < config.trigger_chars {
        return MicrocompactResult {
            messages: messages.to_vec(),
            executed: false,
            tokens_freed_estimate: 0,
        };
    }

    let tool_result_indices: Vec<usize> = messages
        .iter()
        .enumerate()
        .filter_map(|(index, message)| {
            (message.get("role").and_then(|value| value.as_str()) == Some("tool")).then_some(index)
        })
        .collect();

    if tool_result_indices.len() <= config.keep_recent_tool_results {
        return MicrocompactResult {
            messages: messages.to_vec(),
            executed: false,
            tokens_freed_estimate: 0,
        };
    }

    let keep_from = tool_result_indices.len() - config.keep_recent_tool_results;
    let indices_to_clear: std::collections::HashSet<usize> = tool_result_indices
        .iter()
        .take(keep_from)
        .copied()
        .collect();

    let mut tool_call_name_by_id = std::collections::HashMap::new();
    for message in messages {
        if message.get("role").and_then(|value| value.as_str()) != Some("assistant") {
            continue;
        }
        if let Some(tool_calls) = message.get("toolCalls").and_then(|value| value.as_array()) {
            for tool_call in tool_calls {
                if let (Some(id), Some(name)) = (
                    tool_call.get("id").and_then(|value| value.as_str()),
                    tool_call.get("name").and_then(|value| value.as_str()),
                ) {
                    tool_call_name_by_id.insert(id.to_string(), name.to_string());
                }
            }
        }
    }

    let mut freed_chars = 0usize;
    let rewritten_messages = messages
        .iter()
        .enumerate()
        .map(|(index, message)| {
            if !indices_to_clear.contains(&index) {
                return message.clone();
            }

            let tool_name = message
                .get("name")
                .and_then(|value| value.as_str())
                .map(|value| value.to_string())
                .or_else(|| {
                    message
                        .get("toolCallId")
                        .or_else(|| message.get("tool_call_id"))
                        .and_then(|value| value.as_str())
                        .and_then(|id| tool_call_name_by_id.get(id).cloned())
                });
            if tool_name
                .as_ref()
                .map(|name| config.preserved_tool_names.contains(name))
                .unwrap_or(false)
            {
                return message.clone();
            }

            let original_len = message
                .get("content")
                .and_then(|value| value.as_str())
                .map(str::len)
                .unwrap_or(0);
            if message
                .get("content")
                .and_then(|value| value.as_str())
                .map(is_persisted_tool_result_message)
                .unwrap_or(false)
            {
                return message.clone();
            }
            freed_chars += original_len;

            let mut cleared = message.clone();
            if let Some(object) = cleared.as_object_mut() {
                object.insert(
                    "content".to_string(),
                    serde_json::Value::String("[microcompacted]".to_string()),
                );
            }
            cleared
        })
        .collect();

    MicrocompactResult {
        messages: rewritten_messages,
        executed: freed_chars > 0,
        tokens_freed_estimate: freed_chars / 4,
    }
}

#[derive(Debug, Clone)]
pub struct AutoCompactConfig {
    /// Fixed-character threshold (deprecated in favour of dynamic).
    /// When `custom_context_window` is set, this is computed from
    /// `effective_auto_compact_threshold()` instead of hardcoded.
    pub threshold_chars: usize,
    pub max_output_chars: usize,
    pub consecutive_failure_limit: u32,
    /// Manual context window override for dynamic threshold computation.
    /// When `Some`, `threshold_chars` is set to
    /// `effective_auto_compact_threshold(Some(window))`. When `None`,
    /// the conservative fallback (64K) is used.
    pub custom_context_window: Option<usize>,
}

impl Default for AutoCompactConfig {
    fn default() -> Self {
        let threshold = crate::llm::context_decay::effective_auto_compact_threshold(None);
        Self {
            threshold_chars: threshold,
            max_output_chars: 80_000,
            consecutive_failure_limit: 3,
            custom_context_window: None,
        }
    }
}

impl AutoCompactConfig {
    /// Build an `AutoCompactConfig` with a context-window-aware threshold.
    pub fn with_context_window(context_window: usize) -> Self {
        let threshold =
            crate::llm::context_decay::effective_auto_compact_threshold(Some(context_window));
        Self {
            threshold_chars: threshold,
            max_output_chars: 80_000,
            consecutive_failure_limit: 3,
            custom_context_window: Some(context_window),
        }
    }
}

#[derive(Debug, Clone)]
pub struct CompactLlmOutput {
    pub new_messages: Vec<serde_json::Value>,
    pub pre_tokens: u64,
    pub post_tokens: u64,
    pub messages_summarized: usize,
}

#[derive(Debug, Clone, Default)]
pub struct AutoCompactState {
    pub compacted: bool,
    pub turn_counter: u32,
    pub consecutive_failures: u32,
}

impl AutoCompactState {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn is_circuit_broken(&self, config: &AutoCompactConfig) -> bool {
        self.consecutive_failures >= config.consecutive_failure_limit
    }

    pub fn record_success(&mut self) {
        self.compacted = true;
        self.turn_counter = 0;
        self.consecutive_failures = 0;
    }

    pub fn record_failure(&mut self) {
        self.consecutive_failures += 1;
    }

    pub fn increment_turn(&mut self) {
        self.turn_counter += 1;
    }
}

pub fn should_auto_compact(messages: &[serde_json::Value], config: &AutoCompactConfig) -> bool {
    estimate_total_chars(messages) >= config.threshold_chars
}

pub fn append_transcript_path_hint(summary_text: String, transcript_path: Option<&str>) -> String {
    let Some(path) = transcript_path
        .map(str::trim)
        .filter(|path| !path.is_empty())
    else {
        return summary_text;
    };
    if summary_text.contains(path) {
        return summary_text;
    }

    format!(
        "{}\n\n如果需要对摘要中的某个信息查证原文，完整的对话记录在：{}",
        summary_text.trim_end(),
        path
    )
}

pub fn compact_transcript_path_for_conversation_dir(conversation_dir: &Path) -> String {
    absolute_path(conversation_dir.join("messages.jsonl"))
        .to_string_lossy()
        .to_string()
}

fn absolute_path(path: PathBuf) -> PathBuf {
    if path.is_absolute() {
        return path;
    }

    match std::env::current_dir() {
        Ok(current_dir) => current_dir.join(path),
        Err(_) => path,
    }
}

fn is_ptl_retry_context_message(message: &serde_json::Value) -> bool {
    message.get("role").and_then(|value| value.as_str()) == Some("system")
        || message
            .get("isMeta")
            .and_then(|value| value.as_bool())
            .unwrap_or(false)
        || message
            .get("content")
            .and_then(|value| value.as_str())
            .map(|content| content.contains("protocol_path_anthropic"))
            .unwrap_or(false)
}

fn truncate_string_at_char_boundary(text: &str, keep_chars: usize) -> String {
    text.chars().take(keep_chars).collect()
}

fn reduce_message_content_for_ptl(message: &serde_json::Value) -> Option<serde_json::Value> {
    let content = message.get("content")?;
    let text = content.as_str()?;
    let char_count = text.chars().count();
    if char_count <= 256 {
        return None;
    }

    let keep_chars = ((char_count * 4) / 5).max(128).min(char_count - 1);
    let mut rewritten = message.clone();
    if let Some(object) = rewritten.as_object_mut() {
        let preview = truncate_string_at_char_boundary(text, keep_chars);
        object.insert(
            "content".to_string(),
            serde_json::Value::String(format!(
                "{}\n[truncated for prompt-too-long retry: original chars={}]",
                preview, char_count
            )),
        );
        Some(rewritten)
    } else {
        None
    }
}

fn truncate_single_round_for_ptl(
    prefix: &[serde_json::Value],
    body: &[serde_json::Value],
    original_len: usize,
) -> Vec<serde_json::Value> {
    let Some((index, reduced)) = body
        .iter()
        .enumerate()
        .filter_map(|(index, message)| reduce_message_content_for_ptl(message).map(|m| (index, m)))
        .max_by_key(|(_, message)| message.to_string().len())
    else {
        return prefix
            .iter()
            .cloned()
            .chain(std::iter::once(serde_json::json!({
                "role": "user",
                "content": "[conversation content truncated for prompt-too-long retry]"
            })))
            .collect();
    };

    let mut truncated = prefix.to_vec();
    truncated.extend(body.iter().enumerate().map(|(i, message)| {
        if i == index {
            reduced.clone()
        } else {
            message.clone()
        }
    }));

    if truncated
        .iter()
        .map(|message| message.to_string().len())
        .sum::<usize>()
        < original_len
    {
        truncated
    } else {
        prefix
            .iter()
            .cloned()
            .chain(std::iter::once(serde_json::json!({
                "role": "user",
                "content": "[conversation content truncated for prompt-too-long retry]"
            })))
            .collect()
    }
}

pub fn truncate_messages_for_ptl_retry(messages: &[serde_json::Value]) -> Vec<serde_json::Value> {
    if messages.len() <= 1 {
        return messages.to_vec();
    }
    let original_len = messages
        .iter()
        .map(|message| message.to_string().len())
        .sum();

    let prefix_len = messages
        .iter()
        .take_while(|message| is_ptl_retry_context_message(message))
        .count();
    let body = &messages[prefix_len..];
    if body.len() <= 1 {
        return messages.to_vec();
    }

    let mut rounds: Vec<Vec<serde_json::Value>> = Vec::new();
    for message in body {
        let starts_round = message.get("role").and_then(|value| value.as_str()) == Some("user");
        if starts_round || rounds.is_empty() {
            rounds.push(Vec::new());
        }
        rounds
            .last_mut()
            .expect("round exists")
            .push(message.clone());
    }

    if rounds.len() <= 1 {
        return truncate_single_round_for_ptl(&messages[..prefix_len], body, original_len);
    }

    let drop_rounds = rounds.len().div_ceil(5).clamp(1, rounds.len() - 1);
    let mut truncated = messages[..prefix_len].to_vec();
    for round in rounds.into_iter().skip(drop_rounds) {
        truncated.extend(round);
    }
    truncated
}

pub fn compact_messages_via_llm(
    messages: Vec<serde_json::Value>,
    summary_text: String,
) -> CompactLlmOutput {
    let pre_tokens = (estimate_total_chars(&messages) / 4) as u64;
    let tail_start = messages
        .iter()
        .rposition(|message| {
            message.get("role").and_then(|value| value.as_str()) == Some("user")
                && message
                    .get("isCompactSummary")
                    .and_then(|value| value.as_bool())
                    != Some(true)
        })
        .unwrap_or_else(|| messages.len().saturating_sub(1));
    let messages_summarized = tail_start;
    let tail_round = messages[tail_start..].to_vec();

    let boundary = serde_json::json!({
        "role": "system",
        "subtype": "compact_boundary",
        "content": "Conversation compacted",
        "compactMetadata": {
            "trigger": "auto",
            "preTokens": pre_tokens,
            "messagesSummarized": messages_summarized,
        },
        "createdAt": chrono::Utc::now().to_rfc3339(),
    });

    let summary_message = serde_json::json!({
        "id": format!("compact-summary-{}", uuid::Uuid::new_v4()),
        "role": "user",
        "content": format!("<context>\n{}\n</context>", summary_text),
        "isCompactSummary": true,
    });

    let mut new_messages = vec![boundary, summary_message];
    new_messages.extend(tail_round);

    let post_tokens = (estimate_total_chars(&new_messages) / 4) as u64;

    CompactLlmOutput {
        new_messages,
        pre_tokens,
        post_tokens,
        messages_summarized,
    }
}

// TODO(T15): extract compress_context_if_needed here once LlmGateway is
// injectable via RuntimeLlmExecutor or a similar seam. For now the
// implementation lives in transport/tauri_commands/chat.rs (the legacy agent
// loop) and is not yet callable from the runtime layer without introducing a
// forbidden dependency on the transport / gateway infrastructure.
