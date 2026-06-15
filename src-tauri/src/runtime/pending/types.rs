//! Pending queue data types — see spec §4.1.

use serde::{Deserialize, Serialize};

use crate::runtime::chat::chat_turn_driver::SkillCommandRef;
use crate::runtime::chat::ChatTurnRequest;
use crate::runtime::human_interaction::{ImPlatform, OutputBinding, TurnOrigin};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PendingItem {
    pub id: String,
    #[serde(default)]
    pub source: PendingSource,
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sender_nick: Option<String>,
    #[serde(default)]
    pub attachments: Vec<PendingAttachment>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub skill_command: Option<SkillCommandRef>,
    pub received_at: String,
    #[serde(default)]
    pub origin: TurnOrigin,
    #[serde(default)]
    pub output_binding: OutputBinding,
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
    ImFeishu,
    ImWecom,
    ImTelegram,
    ImWechat,
    ImWhatsapp,
}

impl Default for PendingSource {
    fn default() -> Self {
        PendingSource::App
    }
}

impl PendingSource {
    pub fn im_platform(self) -> Option<ImPlatform> {
        match self {
            PendingSource::App => None,
            PendingSource::ImDingtalk => Some(ImPlatform::Dingtalk),
            PendingSource::ImFeishu => Some(ImPlatform::Feishu),
            PendingSource::ImWecom => Some(ImPlatform::Wecom),
            PendingSource::ImTelegram => Some(ImPlatform::Telegram),
            PendingSource::ImWechat => Some(ImPlatform::Wechat),
            PendingSource::ImWhatsapp => Some(ImPlatform::Whatsapp),
        }
    }
}

#[cfg(test)]
impl PendingItem {
    pub fn im_text_for_test(
        source: PendingSource,
        text: impl Into<String>,
        external_conversation_key: impl Into<String>,
    ) -> Self {
        let text = text.into();
        let external_conversation_key = external_conversation_key.into();
        let platform = source
            .im_platform()
            .expect("im_text_for_test requires an IM source");
        Self {
            id: "pending-test".into(),
            source,
            text,
            sender_nick: None,
            attachments: Vec::new(),
            skill_command: None,
            received_at: "2026-06-09T00:00:00Z".into(),
            origin: TurnOrigin::Im {
                platform,
                external_conversation_key: external_conversation_key.clone(),
                sender_id: None,
                sender_label: None,
                account_id: None,
                thread_id: None,
            },
            output_binding: OutputBinding::im(
                platform,
                "sess-test",
                external_conversation_key,
                true,
            ),
        }
    }
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
    /// Session is suspended for an active human interaction; caller should route
    /// the message as a reply/new-turn decision instead of queueing blindly.
    HeldForHumanInteraction { interaction_id: Option<String> },
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
