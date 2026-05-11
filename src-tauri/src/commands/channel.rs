//! Tauri IPC commands for IM channel management.

use std::sync::Arc;
use tauri::{AppHandle, Manager};

use crate::connector::channel::{
    ChannelConversation, ChannelManager, ChannelPlatformState, ChannelRegistrationBeginResult,
    ChannelRegistrationPollResult, Platform,
};

fn parse_platform(platform: String) -> Result<Platform, String> {
    Platform::from_str(&platform).ok_or_else(|| format!("Unsupported channel platform: {platform}"))
}

fn manager(app: &AppHandle) -> Result<Arc<ChannelManager>, String> {
    app.try_state::<Arc<ChannelManager>>()
        .map(|state| state.inner().clone())
        .ok_or_else(|| "频道功能未初始化，请先登录".to_string())
}

#[tauri::command]
pub async fn channel_get_platforms(app: AppHandle) -> Result<Vec<ChannelPlatformState>, String> {
    manager(&app)?
        .get_platforms()
        .await
        .map_err(|e| format!("{:#}", e))
}

#[tauri::command]
pub async fn channel_get_platform(
    app: AppHandle,
    platform: String,
) -> Result<ChannelPlatformState, String> {
    manager(&app)?
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
        Platform::Dingtalk => manager(&app)?
            .begin_dingtalk_registration()
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
        Platform::Dingtalk => manager(&app)?
            .poll_dingtalk_registration(device_code)
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
    manager(&app)?
        .set_enabled(parse_platform(platform)?, enabled)
        .await
        .map_err(|e| format!("{:#}", e))
}

#[tauri::command]
pub async fn channel_remove_platform(
    app: AppHandle,
    platform: String,
) -> Result<ChannelPlatformState, String> {
    manager(&app)?
        .remove_platform(parse_platform(platform)?)
        .await
        .map_err(|e| format!("{:#}", e))
}

#[tauri::command]
pub async fn channel_reveal_secret(app: AppHandle, platform: String) -> Result<String, String> {
    manager(&app)?
        .reveal_secret(parse_platform(platform)?)
        .await
        .map_err(|e| format!("{:#}", e))
}

#[tauri::command]
pub async fn channel_get_conversations(
    app: AppHandle,
    platform: Option<String>,
) -> Result<Vec<ChannelConversation>, String> {
    if let Some(platform) = platform {
        let parsed = parse_platform(platform)?;
        if parsed != Platform::Dingtalk {
            return Ok(vec![]);
        }
    }
    match app.try_state::<Arc<ChannelManager>>() {
        Some(m) => Ok(m.get_conversations().await),
        None => Ok(vec![]),
    }
}
