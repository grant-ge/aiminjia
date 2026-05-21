//! WeChat (iLink) 持久化类型 + 运行态 target。
//!
//! 跟其他平台的关键差异：
//! - 凭证只是 `bot_token`（扫码登录拿到的）+ `ilink_bot_id` / `ilink_user_id`
//!   / `effective_base_url`（IDC 路由结果）。**不需要** app_id / app_secret
//!   —— iLink-App-Id 是全局常量，从 `wechat::appid::resolve_app_id` 取。
//! - `bot_token` 跟飞书 `app_secret` / 企微 `secret` 一样走 SecureStorage 加密。
//! - `effective_base_url` 是 LoginSession 跑完之后的实际 IDC 地址（如 SG 区
//!   是 `sg.ilink.weixin.qq.com`），后续所有业务 POST 都打这个；千万不要
//!   退回 DEFAULT_BASE_URL 否则路由 404。

use serde::{Deserialize, Serialize};

use crate::connector::im::types::{Platform, SecretStorageKind};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WechatStoredCredentials {
    /// 加密后的 bot_token；SecureStorage 不可用时回落明文（跟飞书 / 钉钉
    /// / 企微的 secret 处理保持一致）。
    pub bot_token_encrypted: String,
    pub bot_token_storage: SecretStorageKind,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WechatStoredBot {
    /// `ilink_bot_id` —— bot 唯一标识。channel router 拿它做 robot_code，
    /// sidebar 按它分组不同 wechat 账号。
    pub ilink_bot_id: String,
    /// `ilink_user_id` —— 登录的微信用户。诊断用，本身不参与回信路由。
    pub ilink_user_id: String,
    /// IDC 路由后的实际 API base URL（带 https:// 前缀）。所有业务 POST
    /// 都打这个，登录后不要再用 DEFAULT_BASE_URL。
    pub effective_base_url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WechatStoredMetadata {
    pub created_at: String,
    pub updated_at: String,
}

/// users/<scope>/channels/wechat/config.json schema. schema_version=1。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WechatStoredConfig {
    pub schema_version: u32,
    pub platform: Platform,
    pub configured: bool,
    pub enabled: bool,
    pub credentials: WechatStoredCredentials,
    pub bot: WechatStoredBot,
    pub metadata: WechatStoredMetadata,
}

/// 跟 `WECOM_AIBOT_SOURCE` / `FEISHU_DEVICE_CODE_SOURCE` 对齐 —— surfaced in
/// `ChannelConfigView.source`。微信走扫码（iLink scan-to-login）。
pub const WECHAT_ILINK_SCAN_SOURCE: &str = "WECHAT_ILINK_SCAN";

/// 每条会话的回信目标。本期最小：to_user_id（个微 1:1 私聊，不需要 chatid）
/// + context_token（每条消息回信要原样回填，由 worker 在收消息时刷新）。
#[derive(Debug, Clone)]
pub struct WechatSessionTarget {
    /// 对端用户 ID（`from_user_id`），sendMessage 要它来寻址。对应
    /// `ReplyTarget::external_conversation_key`。
    pub to_user_id: String,
    /// 最近一次入站消息的 `context_token`。回信时必带；缺失也能发，但回信
    /// 落在不同上下文窗的概率上升。**runtime 缓存更新 ≠ 持久化**，进程重启
    /// 后这块清零，第一次回信无 token，下一次就 OK。
    pub context_token: Option<String>,
}
