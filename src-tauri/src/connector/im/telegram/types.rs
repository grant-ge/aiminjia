//! Telegram-specific persisted types.
//!
//! Bot token 走 SecureStorage 加密；bot_id / bot_username / bot_first_name 落明文，
//! UI 列表 + reply forwarder 都会用到。Allowlist 直接落 config.json 而不是单独
//! 文件，因为它的尺寸天然受限（用户能配对的人数 < 100），单文件原子写盘最简单。

use serde::{Deserialize, Serialize};

use crate::connector::im::types::{Platform, SecretStorageKind};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TelegramStoredCredentials {
    pub bot_token_encrypted: String,
    pub bot_token_storage: SecretStorageKind,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TelegramBotInfo {
    pub bot_id: String,
    pub bot_username: String,
    pub bot_first_name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AllowlistEntry {
    pub user_id: i64,
    pub first_name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub username: Option<String>,
    pub paired_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TelegramStoredMetadata {
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TelegramStoredConfig {
    pub schema_version: u32,
    pub platform: Platform,
    pub configured: bool,
    pub enabled: bool,
    pub credentials: TelegramStoredCredentials,
    pub bot: TelegramBotInfo,
    #[serde(default)]
    pub allowlist: Vec<AllowlistEntry>,
    pub metadata: TelegramStoredMetadata,
}

/// 给 ChannelConfigView.source 用，区分凭证来源。
pub const TELEGRAM_BOT_TOKEN_SOURCE: &str = "TELEGRAM_BOT_TOKEN";

/// 入站消息的发件人 + chat 元数据，缓存到 connector 的 session_targets，
/// 出站 reply 时用来还原 chat_id。
#[derive(Debug, Clone)]
pub struct TelegramSessionTarget {
    pub chat_id: i64,
    pub user_id: i64,
    /// 最近一条入站消息的 message_id；出站回复时用作 reply_to_message_id。
    pub last_inbound_message_id: Option<i64>,
}
