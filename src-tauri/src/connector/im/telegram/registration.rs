//! 桌面端 pairing 命令实现：begin / list_pending / approve / reject / revoke 用户。
//!
//! 真正的 Bot API 调用走 connector.sender；store 操作走 ChannelConfigStore。

use std::sync::Arc;

use anyhow::Result;
use serde::{Deserialize, Serialize};

use super::connector::TelegramConnector;
use super::pairing::PendingPairing;
use super::types::AllowlistEntry;
use crate::connector::im::shared::config_store::ChannelConfigStore;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TelegramPairingBeginResult {
    pub code: String,
    pub deep_link: String,
    pub expires_in_seconds: u64,
    pub bot_username: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TelegramPendingPairing {
    pub code: String,
    pub user_id: i64,
    pub first_name: String,
    pub username: Option<String>,
    pub requested_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TelegramPairedUser {
    pub user_id: i64,
    pub first_name: String,
    pub username: Option<String>,
}

pub async fn begin_pairing(connector: &TelegramConnector) -> Result<TelegramPairingBeginResult> {
    let pairing = connector.pairing();
    let entry = pairing.begin().await?;
    let deep_link = format!(
        "https://t.me/{}?start={}",
        connector.bot_username(),
        entry.code
    );
    Ok(TelegramPairingBeginResult {
        code: entry.code,
        deep_link,
        expires_in_seconds: 300,
        bot_username: connector.bot_username().to_string(),
    })
}

pub async fn list_pending(connector: &TelegramConnector) -> Result<Vec<TelegramPendingPairing>> {
    let list = connector.pairing().list_pending().await;
    Ok(list
        .into_iter()
        .filter_map(|e: PendingPairing| {
            let p = e.pairer?;
            Some(TelegramPendingPairing {
                code: e.code,
                user_id: p.user_id,
                first_name: p.first_name,
                username: p.username,
                requested_at: p.attached_at.to_rfc3339(),
            })
        })
        .collect())
}

pub async fn approve(
    connector: &TelegramConnector,
    config_store: &Arc<ChannelConfigStore>,
    code: &str,
) -> Result<TelegramPairedUser> {
    let entry = connector
        .pairing()
        .take(code)
        .await
        .ok_or_else(|| anyhow::anyhow!("pairing code not found or expired"))?;
    let pairer = entry
        .pairer
        .ok_or_else(|| anyhow::anyhow!("pairing code has no pairer attached"))?;
    config_store.telegram_add_allowlist_entry(AllowlistEntry {
        user_id: pairer.user_id,
        first_name: pairer.first_name.clone(),
        username: pairer.username.clone(),
        paired_at: chrono::Utc::now().to_rfc3339(),
    })?;
    let _ = connector
        .sender()
        .send_plain(pairer.chat_id, "👋 你已连接 AIjia，可以开始对话。")
        .await;
    Ok(TelegramPairedUser {
        user_id: pairer.user_id,
        first_name: pairer.first_name,
        username: pairer.username,
    })
}

pub async fn reject(connector: &TelegramConnector, code: &str) -> Result<()> {
    connector.pairing().drop(code).await;
    Ok(())
}

pub async fn revoke_user(
    connector: &TelegramConnector,
    config_store: &Arc<ChannelConfigStore>,
    user_id: i64,
) -> Result<()> {
    config_store.telegram_remove_allowlist_user(user_id)?;
    // 顺手发个通知（best-effort）
    // 私聊 chat_id == user_id
    let _ = connector
        .sender()
        .send_plain(user_id, "你已被 AIjia 管理员移除连接。")
        .await;
    Ok(())
}

/// List currently-paired Telegram users from persisted config allowlist.
pub fn list_paired_users(
    config_store: &Arc<ChannelConfigStore>,
) -> Result<Vec<TelegramPairedUser>> {
    let Some(config) = config_store.read_telegram_config()? else {
        return Ok(Vec::new());
    };
    Ok(config
        .allowlist
        .into_iter()
        .map(|e| TelegramPairedUser {
            user_id: e.user_id,
            first_name: e.first_name,
            username: e.username,
        })
        .collect())
}
