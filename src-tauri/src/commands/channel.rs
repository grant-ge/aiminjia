//! Tauri IPC commands for IM channel management.

use std::sync::Arc;
use tauri::{AppHandle, Manager};

use crate::connector::im::{
    ChannelConversation, ChannelManager, ChannelPlatformState, ChannelRegistrationBeginResult,
    ChannelRegistrationPollResult, Platform,
};

fn parse_platform(platform: String) -> Result<Platform, String> {
    Platform::from_str(&platform).ok_or_else(|| format!("Unsupported channel platform: {platform}"))
}

async fn manager(app: &AppHandle) -> Result<Arc<ChannelManager>, String> {
    let slot = app
        .try_state::<Arc<crate::connector::im::ChannelManagerSlot>>()
        .ok_or_else(|| "频道功能未初始化，请先登录".to_string())?
        .inner()
        .clone();
    slot.current()
        .await
        .ok_or_else(|| "频道功能未初始化，请先登录".to_string())
}

#[tauri::command]
pub async fn channel_get_platforms(app: AppHandle) -> Result<Vec<ChannelPlatformState>, String> {
    manager(&app).await?
        .get_platforms()
        .await
        .map_err(|e| format!("{:#}", e))
}

#[tauri::command]
pub async fn channel_get_platform(
    app: AppHandle,
    platform: String,
) -> Result<ChannelPlatformState, String> {
    manager(&app).await?
        .get_platform(parse_platform(platform)?)
        .await
        .map_err(|e| format!("{:#}", e))
}

#[tauri::command]
pub async fn channel_begin_registration(
    app: AppHandle,
    platform: String,
) -> Result<ChannelRegistrationBeginResult, String> {
    match parse_platform(platform)? {
        Platform::Dingtalk => manager(&app).await?
            .begin_dingtalk_registration()
            .await
            .map_err(|e| format!("{:#}", e)),
        Platform::Feishu => manager(&app).await?
            .begin_feishu_registration()
            .await
            .map_err(|e| format!("{:#}", e)),
        Platform::Wechat => manager(&app).await?
            .begin_wechat_registration()
            .await
            .map_err(|e| format!("{:#}", e)),
        Platform::Whatsapp => manager(&app).await?
            .begin_whatsapp_registration()
            .await
            .map_err(|e| format!("{:#}", e)),
        other => Err(format!(
            "{} channel registration is not available yet",
            other.as_str()
        )),
    }
}

#[tauri::command]
pub async fn channel_poll_registration(
    app: AppHandle,
    platform: String,
    device_code: String,
) -> Result<ChannelRegistrationPollResult, String> {
    match parse_platform(platform)? {
        Platform::Dingtalk => manager(&app).await?
            .poll_dingtalk_registration(device_code)
            .await
            .map_err(|e| format!("{:#}", e)),
        Platform::Feishu => manager(&app).await?
            .poll_feishu_registration(device_code)
            .await
            .map_err(|e| format!("{:#}", e)),
        Platform::Wechat => manager(&app).await?
            .poll_wechat_registration(device_code)
            .await
            .map_err(|e| format!("{:#}", e)),
        Platform::Whatsapp => manager(&app).await?
            .poll_whatsapp_registration(device_code)
            .await
            .map_err(|e| format!("{:#}", e)),
        other => Err(format!(
            "{} channel registration is not available yet",
            other.as_str()
        )),
    }
}

#[tauri::command]
pub async fn channel_set_enabled(
    app: AppHandle,
    platform: String,
    enabled: bool,
) -> Result<ChannelPlatformState, String> {
    manager(&app).await?
        .set_enabled(parse_platform(platform)?, enabled)
        .await
        .map_err(|e| format!("{:#}", e))
}

#[tauri::command]
pub async fn channel_remove_platform(
    app: AppHandle,
    platform: String,
) -> Result<ChannelPlatformState, String> {
    manager(&app).await?
        .remove_platform(parse_platform(platform)?)
        .await
        .map_err(|e| format!("{:#}", e))
}

#[tauri::command]
pub async fn channel_reveal_secret(app: AppHandle, platform: String) -> Result<String, String> {
    manager(&app).await?
        .reveal_secret(parse_platform(platform)?)
        .await
        .map_err(|e| format!("{:#}", e))
}

#[tauri::command]
pub async fn channel_get_conversations(
    app: AppHandle,
    platform: Option<String>,
) -> Result<Vec<ChannelConversation>, String> {
    let parsed_filter = match platform {
        Some(p) => Some(parse_platform(p)?),
        None => None,
    };
    let conversations = match app.try_state::<Arc<crate::connector::im::ChannelManagerSlot>>() {
        Some(slot) => match slot.inner().clone().current().await {
            Some(cm) => cm.get_conversations().await,
            None => return Ok(vec![]),
        },
        None => return Ok(vec![]),
    };
    Ok(match parsed_filter {
        Some(p) => conversations
            .into_iter()
            .filter(|c| c.platform == p)
            .collect(),
        None => conversations,
    })
}

// ---- Wecom-specific commands (PR6b) ----------------------------------------

#[derive(serde::Serialize)]
pub struct WecomTestResult {
    pub ok: bool,
    pub error: Option<String>,
}

/// Save wecom credentials, connect, and return the current platform state.
#[tauri::command]
pub async fn channel_wecom_save(
    app: AppHandle,
    bot_id: String,
    secret: String,
    display_name: Option<String>,
) -> Result<ChannelPlatformState, String> {
    manager(&app).await?
        .save_wecom_and_connect(bot_id, secret, display_name)
        .await
        .map_err(|e| format!("{:#}", e))
}

/// Test a wecom connection with the given credentials without persisting them.
/// Connects a temp AibotClient, awaits the first lifecycle event with a 5-second
/// timeout, then cancels. Returns `ok: true` on Authenticated.
#[tauri::command]
pub async fn channel_wecom_test_connection(
    bot_id: String,
    secret: String,
) -> Result<WecomTestResult, String> {
    use crate::connector::im::wecom::aibot_client::{AibotClient, AibotClientConfig, AibotEvent};
    use std::time::Duration;
    use tokio::sync::mpsc;
    use tokio_util::sync::CancellationToken;

    let cfg = AibotClientConfig {
        bot_id: bot_id.clone(),
        secret: secret.clone(),
        ws_url: "wss://openws.work.weixin.qq.com".into(),
        heartbeat_interval: Duration::from_secs(60),
        reply_ack_timeout: Duration::from_secs(5),
        max_missed_pong: 3,
        max_reconnect_attempts: 0,
        max_auth_failure_attempts: 1,
        reconnect_base_delay: Duration::from_secs(1),
    };
    let aibot = Arc::new(AibotClient::new(cfg));
    let (evt_tx, mut evt_rx) = mpsc::channel::<AibotEvent>(8);
    let cancel = CancellationToken::new();
    let cancel_for_task = cancel.clone();

    tokio::spawn(async move {
        let _ = aibot.run(evt_tx, cancel_for_task).await;
    });

    let timeout = tokio::time::sleep(Duration::from_secs(5));
    tokio::pin!(timeout);

    loop {
        tokio::select! {
            _ = &mut timeout => {
                cancel.cancel();
                return Ok(WecomTestResult { ok: false, error: Some("连接超时".to_string()) });
            }
            evt = evt_rx.recv() => {
                match evt {
                    Some(AibotEvent::Authenticated) => {
                        cancel.cancel();
                        return Ok(WecomTestResult { ok: true, error: None });
                    }
                    Some(AibotEvent::AuthFailed(code, msg)) => {
                        cancel.cancel();
                        return Ok(WecomTestResult {
                            ok: false,
                            error: Some(format!("凭证错误 (code={code} msg={msg})")),
                        });
                    }
                    Some(AibotEvent::ConnectionDropped(reason)) => {
                        cancel.cancel();
                        return Ok(WecomTestResult { ok: false, error: Some(reason) });
                    }
                    Some(AibotEvent::KickedOut(reason)) => {
                        cancel.cancel();
                        return Ok(WecomTestResult { ok: false, error: Some(reason) });
                    }
                    Some(_) => {
                        // Reconnecting / Inbound — keep waiting.
                        continue;
                    }
                    None => {
                        // Channel closed.
                        return Ok(WecomTestResult {
                            ok: false,
                            error: Some("连接已关闭".to_string()),
                        });
                    }
                }
            }
        }
    }
}

/// Cancel the wecom stream if active, remove the config, return the state.
#[tauri::command]
pub async fn channel_wecom_remove(app: AppHandle) -> Result<ChannelPlatformState, String> {
    manager(&app).await?
        .remove_platform(Platform::Wecom)
        .await
        .map_err(|e| format!("{:#}", e))
}

/// Enable or disable the wecom channel; delegates to `manager.set_enabled`.
#[tauri::command]
pub async fn channel_wecom_set_enabled(
    app: AppHandle,
    enabled: bool,
) -> Result<ChannelPlatformState, String> {
    manager(&app).await?
        .set_enabled(Platform::Wecom, enabled)
        .await
        .map_err(|e| format!("{:#}", e))
}

// ---- Wecom QR scan registration (扫码一键创建) -----------------------------
//
// 协议参考企微团队官方 CLI `@wecom/wecom-openclaw-cli` 的 `dist/utils/qrcode.js`。
// 替代手动后台建机器人 + 抄 botId/secret 的流程：用户扫码 → 在企业微信里同意 →
// 前端拿到 botId/secret → 直接 channel_wecom_save 完成配置。

#[tauri::command]
pub async fn channel_wecom_begin_registration(
) -> Result<crate::connector::im::wecom::registration::WecomBeginResult, String> {
    crate::connector::im::wecom::registration::begin_registration()
        .await
        .map_err(|e| format!("{:#}", e))
}

#[tauri::command]
pub async fn channel_wecom_poll_registration(
    scode: String,
) -> Result<crate::connector::im::wecom::registration::WecomPollResult, String> {
    crate::connector::im::wecom::registration::poll_registration(&scode)
        .await
        .map_err(|e| format!("{:#}", e))
}

// ---- Telegram-specific commands (PR2) -------------------------------------

use crate::connector::im::telegram::registration as tg_reg;

#[tauri::command]
pub async fn channel_telegram_save(
    app: AppHandle,
    token: String,
) -> Result<ChannelPlatformState, String> {
    use crate::connector::im::telegram::api::TelegramApi;
    let api = TelegramApi::new(token.clone()).map_err(|e| format!("token 验证失败：{e}"))?;
    let info = api
        .get_me()
        .await
        .map_err(|e| format!("token 验证失败：{e}"))?;
    let username = info
        .username
        .ok_or_else(|| "bot 缺少 username，请在 BotFather 里设置".to_string())?;
    manager(&app).await?
        .save_telegram_and_connect(token, info.id.to_string(), username, info.first_name)
        .await
        .map_err(|e| format!("{:#}", e))
}

#[tauri::command]
pub async fn channel_telegram_remove(app: AppHandle) -> Result<ChannelPlatformState, String> {
    manager(&app).await?
        .remove_platform(Platform::Telegram)
        .await
        .map_err(|e| format!("{:#}", e))
}

#[tauri::command]
pub async fn channel_telegram_set_enabled(
    app: AppHandle,
    enabled: bool,
) -> Result<ChannelPlatformState, String> {
    manager(&app).await?
        .set_enabled(Platform::Telegram, enabled)
        .await
        .map_err(|e| format!("{:#}", e))
}

async fn telegram_connector(
    app: &AppHandle,
) -> Result<Arc<crate::connector::im::telegram::connector::TelegramConnector>, String> {
    manager(app).await?
        .telegram_connector()
        .await
        .ok_or_else(|| "Telegram connector 未启动".to_string())
}

#[tauri::command]
pub async fn channel_telegram_begin_pairing(
    app: AppHandle,
) -> Result<tg_reg::TelegramPairingBeginResult, String> {
    let c = telegram_connector(&app).await?;
    tg_reg::begin_pairing(&c)
        .await
        .map_err(|e| format!("{:#}", e))
}

#[tauri::command]
pub async fn channel_telegram_list_pending_pairings(
    app: AppHandle,
) -> Result<Vec<tg_reg::TelegramPendingPairing>, String> {
    let c = telegram_connector(&app).await?;
    tg_reg::list_pending(&c)
        .await
        .map_err(|e| format!("{:#}", e))
}

#[tauri::command]
pub async fn channel_telegram_approve_pairing(
    app: AppHandle,
    code: String,
) -> Result<tg_reg::TelegramPairedUser, String> {
    let c = telegram_connector(&app).await?;
    let cs = manager(&app).await?.config_store_arc();
    tg_reg::approve(&c, &cs, &code)
        .await
        .map_err(|e| format!("{:#}", e))
}

#[tauri::command]
pub async fn channel_telegram_reject_pairing(app: AppHandle, code: String) -> Result<(), String> {
    let c = telegram_connector(&app).await?;
    tg_reg::reject(&c, &code)
        .await
        .map_err(|e| format!("{:#}", e))
}

#[tauri::command]
pub async fn channel_telegram_revoke_user(
    app: AppHandle,
    user_id: i64,
) -> Result<ChannelPlatformState, String> {
    let c = telegram_connector(&app).await?;
    let cs = manager(&app).await?.config_store_arc();
    tg_reg::revoke_user(&c, &cs, user_id)
        .await
        .map_err(|e| format!("{:#}", e))?;
    manager(&app).await?
        .get_platform(Platform::Telegram)
        .await
        .map_err(|e| format!("{:#}", e))
}

#[tauri::command]
pub async fn channel_telegram_list_paired_users(
    app: AppHandle,
) -> Result<Vec<tg_reg::TelegramPairedUser>, String> {
    let cs = manager(&app).await?.config_store_arc();
    tg_reg::list_paired_users(&cs).map_err(|e| format!("{:#}", e))
}

// ---- WhatsApp-specific commands (PR8) -------------------------------------

/// 更新 WhatsApp allow_from 配置（spec v3 §3.10）。
/// `allow_from` 为空 vec 时清空 allowlist（接收所有入站）；
/// 非空时替换为新 allowlist。config.json 必须已存在（已配对），否则返回错误。
#[tauri::command]
pub async fn channel_whatsapp_update_allow_from(
    app: AppHandle,
    allow_from: Vec<String>,
) -> Result<(), String> {
    manager(&app).await?
        .update_whatsapp_allow_from(allow_from)
        .await
        .map_err(|e| format!("{:#}", e))
}

/// 读 WhatsApp 当前 allow_from 列表(供"管理允许列表"UI 预填用)。
/// 未配对返回 `None`,已配对但接收所有联系人返回 `Some([])`,有限制返回 `Some([...])`。
#[tauri::command]
pub async fn channel_whatsapp_get_allow_from(
    app: AppHandle,
) -> Result<Option<Vec<String>>, String> {
    manager(&app).await?
        .get_whatsapp_allow_from()
        .await
        .map_err(|e| format!("{:#}", e))
}
