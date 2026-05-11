//! `CompactSummaryClient` trait — pure compaction interface, isolated from
//! the streaming-oriented `RuntimeLlmExecutor`.
//!
//! Splitting compaction out of `RuntimeLlmExecutor` lets Teammate idle loops
//! run compaction without holding a streaming executor (P0.2, LTR plan §2.1).

use async_trait::async_trait;

use crate::runtime::chat::turn_config::TurnError;

/// A compaction backend that can summarise a conversation slice into a
/// replacement "compact summary" message.
///
/// The default production implementation is `NoopCompactSummaryClient` (warn
/// log + empty string).  A real LLM-backed implementation can be injected via
/// `RuntimeChatTurnDriver::with_compact_client`.
#[async_trait]
pub trait CompactSummaryClient: Send + Sync {
    async fn compact_summary(
        &self,
        conversation_id: &str,
        messages: &[serde_json::Value],
    ) -> Result<String, TurnError>;
}
