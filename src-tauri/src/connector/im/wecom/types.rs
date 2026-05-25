//! Wecom-specific persisted types.
//!
//! 企微 aibot 凭证只有两件套：`bot_id` + `secret`（不需要 CorpID / CorpSecret /
//! AgentID 三件套）。`secret` 走 SecureStorage 加密，跟飞书 `app_secret` /
//! 钉钉 `app_secret` 一样的路径。
//!
//! Display name 是用户给账号起的别名（"销售群机器人" 之类），可空；UI 列表上展示。

use serde::{Deserialize, Serialize};

use crate::connector::im::types::{Platform, SecretStorageKind};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WecomStoredCredentials {
    pub bot_id: String,
    pub secret_encrypted: String,
    pub secret_storage: SecretStorageKind,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WecomStoredMetadata {
    pub created_at: String,
    pub updated_at: String,
}

/// users/<scope>/channels/wecom/config.json schema. schema_version=1 for PR6a onwards.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WecomStoredConfig {
    pub schema_version: u32,
    pub platform: Platform,
    pub configured: bool,
    pub enabled: bool,
    pub credentials: WecomStoredCredentials,
    /// 用户填的账号别名（"销售群机器人" 等），可空。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    pub metadata: WecomStoredMetadata,
}

/// Stable identifier surfaced in `ChannelConfigView.source` for wecom — symmetric
/// with `FEISHU_DEVICE_CODE_SOURCE` / `OPEN_CLAW_SOURCE`. aibot 凭证靠用户手填，
/// 不走 OAuth / device code，所以 source = "WECOM_AIBOT_MANUAL"。
pub const WECOM_AIBOT_SOURCE: &str = "WECOM_AIBOT_MANUAL";

/// Reply credentials per session_id, populated by manager worker when a
/// message arrives and consumed by `WecomConnector::send` / `WecomReplyForwarder`.
///
/// aibot 没有 feishu CardKit 那种 `receive_id_type`/`receive_id` 双字段——发主动
/// 消息时只需要一个 chatid（群 ID 或个人 userid，路由从入站 `from.userid` /
/// `chatid` 还原，见 `wecom::parser::parse_inbound`）。Sender 内部的 `SessionMap`
/// 同时缓存了 `req_id`：5 分钟窗口内优先走被动 `respond_msg`（=openclaw 的
/// `replyStream`）；过期 / 没记账则走主动 `send_msg`（=`sendMessage`），
/// 这一切都由 `Sender::send_markdown` 内部决定，外部只需提供 chatid。
#[derive(Debug, Clone)]
pub struct WecomSessionTarget {
    /// 入站消息的 `chatid`（group）或 `from.userid`（private）。对应
    /// `ReplyTarget::external_conversation_key`。
    pub chat_id: String,
}
