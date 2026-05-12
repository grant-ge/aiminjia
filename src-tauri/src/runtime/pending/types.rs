//! Pending queue data types — see spec §4.1.

use serde::{Deserialize, Serialize};

use crate::runtime::chat::ChatTurnRequest;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PendingItem {
    pub id: String,
    pub source: PendingSource,
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sender_nick: Option<String>,
    #[serde(default)]
    pub attachments: Vec<PendingAttachment>,
    pub received_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PendingAttachment {
    pub id: String,
    pub file_path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mime: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size_bytes: Option<u64>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum PendingSource {
    App,
    ImDingtalk,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PendingFileFormat {
    #[serde(default)]
    pub schema_version: u32,
    #[serde(default)]
    pub items: Vec<PendingItem>,
}

/// Result of `enqueue_or_send`.
#[derive(Debug)]
pub enum EnqueueOutcome {
    /// Session was idle — caller should consume the request.
    SentDirectly { request: ChatTurnRequest },
    /// Session was busy — item buffered.
    Queued { snapshot: Vec<PendingItem> },
    /// Item refused.
    Rejected { reason: EnqueueRejection },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EnqueueRejection {
    QueueFull { limit: usize },
    SessionArchived,
}

#[derive(Debug, Clone)]
pub struct PendingConfig {
    pub debounce_window: std::time::Duration,
    pub max_queue_per_session: usize,
    pub recently_drained_ttl: std::time::Duration,
}

impl Default for PendingConfig {
    fn default() -> Self {
        Self {
            debounce_window: std::time::Duration::from_millis(1200),
            max_queue_per_session: 50,
            recently_drained_ttl: std::time::Duration::from_secs(600),
        }
    }
}
