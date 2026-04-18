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
/// When `is_analysis` is `false` (daily mode) the messages are returned
/// unchanged — decay only applies inside analysis step loops.
///
/// Delegates to [`crate::llm::context_decay::apply_decay`].
pub fn apply_decay(messages: &[ChatMessage], is_analysis: bool) -> Vec<ChatMessage> {
    crate::llm::context_decay::apply_decay(messages, is_analysis)
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
    }
}

#[derive(Debug, Clone)]
pub struct MicrocompactConfig {
    pub trigger_chars: usize,
    pub keep_recent_tool_results: usize,
}

impl Default for MicrocompactConfig {
    fn default() -> Self {
        Self {
            trigger_chars: 120_000,
            keep_recent_tool_results: 2,
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
    messages.iter().map(|message| message.to_string().len()).sum()
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

    let mut freed_chars = 0usize;
    let rewritten_messages = messages
        .iter()
        .enumerate()
        .map(|(index, message)| {
            if !indices_to_clear.contains(&index) {
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

// TODO(T15): extract compress_context_if_needed here once LlmGateway is
// injectable via RuntimeLlmExecutor or a similar seam. For now the
// implementation lives in transport/tauri_commands/chat.rs (the legacy agent
// loop) and is not yet callable from the runtime layer without introducing a
// forbidden dependency on the transport / gateway infrastructure.
