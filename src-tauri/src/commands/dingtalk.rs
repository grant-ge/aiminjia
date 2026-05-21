//! IPC commands for DingTalk AI Table integration.

use std::sync::Arc;
use tauri::State;

use crate::connector::dingtalk::{DingtalkBridge, DingtalkStatusInfo};

/// Start DingTalk OAuth login (opens system browser).
#[tauri::command]
pub async fn dingtalk_login(
    bridge: State<'_, Arc<DingtalkBridge>>,
) -> Result<DingtalkStatusInfo, String> {
    bridge.login().await.map_err(|e| format!("{:#}", e))
}

/// Disconnect from DingTalk.
#[tauri::command]
pub async fn dingtalk_logout(bridge: State<'_, Arc<DingtalkBridge>>) -> Result<(), String> {
    bridge.logout().await.map_err(|e| format!("{:#}", e))
}

/// Get current DingTalk connection status (no network call).
#[tauri::command]
pub async fn dingtalk_status(
    bridge: State<'_, Arc<DingtalkBridge>>,
) -> Result<DingtalkStatusInfo, String> {
    Ok(bridge.status_info().await)
}

/// Refresh DingTalk auth status from dws (network call).
#[tauri::command]
pub async fn dingtalk_refresh_status(
    bridge: State<'_, Arc<DingtalkBridge>>,
) -> Result<DingtalkStatusInfo, String> {
    bridge
        .refresh_status()
        .await
        .map_err(|e| format!("{:#}", e))
}
