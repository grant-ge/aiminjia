//! `CompactSummaryClient` trait — pure compaction interface, isolated from
//! the streaming-oriented `RuntimeLlmExecutor`.
//!
//! Splitting compaction out of `RuntimeLlmExecutor` lets Teammate idle loops
//! run compaction without holding a streaming executor (P0.2, LTR plan §2.1).

use async_trait::async_trait;

use crate::runtime::chat::turn_config::{ResolvedLlmSettings, TurnError};

/// A compaction backend that can summarise a conversation slice into a
/// replacement "compact summary" message.
///
/// When no client is configured (`None`), compaction requests warn-log and
/// return an empty string, which `prepare_messages_for_llm` treats as "skip
/// compaction".  Wire a real LLM-backed implementation via
/// `RuntimeChatTurnDriver::with_compact_client` to enable compaction.
#[async_trait]
pub trait CompactSummaryClient: Send + Sync {
    async fn compact_summary(
        &self,
        conversation_id: &str,
        messages: &[serde_json::Value],
        llm_settings: &ResolvedLlmSettings,
        trace_id: Option<&str>,
        run_id: Option<&str>,
    ) -> Result<String, TurnError>;
}
