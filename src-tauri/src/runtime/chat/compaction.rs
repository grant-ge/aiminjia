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

// TODO(T15): extract compress_context_if_needed here once LlmGateway is
// injectable via RuntimeLlmExecutor or a similar seam. For now the
// implementation lives in transport/tauri_commands/chat.rs (the legacy agent
// loop) and is not yet callable from the runtime layer without introducing a
// forbidden dependency on the transport / gateway infrastructure.
