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

use crate::llm::streaming::ChatMessage;
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
    pub threshold_chars: usize,
    pub max_output_chars: usize,
    pub consecutive_failure_limit: u32,
}

impl Default for AutoCompactConfig {
    fn default() -> Self {
        Self {
            threshold_chars: 480_000,
            max_output_chars: 80_000,
            consecutive_failure_limit: 3,
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

pub fn compact_messages_via_llm(
    messages: Vec<serde_json::Value>,
    summary_text: String,
) -> CompactLlmOutput {
    let pre_tokens = (estimate_total_chars(&messages) / 4) as u64;

    let latest_user = messages
        .iter()
        .rev()
        .find(|message| {
            message.get("role").and_then(|value| value.as_str()) == Some("user")
                && message
                    .get("isCompactSummary")
                    .and_then(|value| value.as_bool())
                    != Some(true)
        })
        .cloned();

    let boundary = serde_json::json!({
        "role": "system",
        "subtype": "compact_boundary",
        "content": "Conversation compacted",
        "compactMetadata": {
            "trigger": "auto",
            "preTokens": pre_tokens,
            "messagesSummarized": messages.len(),
        },
        "createdAt": chrono::Utc::now().to_rfc3339(),
    });

    let summary_message = serde_json::json!({
        "role": "user",
        "content": format!("<context>\n{}\n</context>", summary_text),
        "isCompactSummary": true,
    });

    let mut new_messages = vec![boundary, summary_message];
    if let Some(latest_user) = latest_user {
        new_messages.push(latest_user);
    }

    let post_tokens = (estimate_total_chars(&new_messages) / 4) as u64;

    CompactLlmOutput {
        new_messages,
        pre_tokens,
        post_tokens,
        messages_summarized: messages.len(),
    }
}

// TODO(T15): extract compress_context_if_needed here once LlmGateway is
// injectable via RuntimeLlmExecutor or a similar seam. For now the
// implementation lives in transport/tauri_commands/chat.rs (the legacy agent
// loop) and is not yet callable from the runtime layer without introducing a
// forbidden dependency on the transport / gateway infrastructure.
