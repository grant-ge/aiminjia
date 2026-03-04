//! IPC commands for cloud authentication.

use std::sync::Arc;
use tauri::State;

use crate::auth::AuthManager;
use crate::auth::state::CloudAuthInfo;
use crate::auth::state::CloudModelInfo;

/// Login with username and password.
/// Returns user info, tenant info, and available models.
#[tauri::command]
pub async fn cloud_login(
    auth: State<'_, Arc<AuthManager>>,
    username: String,
    password: String,
) -> Result<CloudAuthInfo, String> {
    let username = username.trim();
    if username.is_empty() || password.is_empty() {
        return Err("请输入用户名和密码".to_string());
    }
    auth.login(username, &password).await.map_err(|e| e.to_string())
}

/// Logout from cloud mode.
#[tauri::command]
pub async fn cloud_logout(
    auth: State<'_, Arc<AuthManager>>,
) -> Result<(), String> {
    auth.logout().await;
    Ok(())
}

/// Get current cloud auth state (for app init / restore).
#[tauri::command]
pub async fn get_cloud_auth(
    auth: State<'_, Arc<AuthManager>>,
) -> Result<CloudAuthInfo, String> {
    Ok(auth.get_auth_info().await)
}

/// Fetch available cloud models.
#[tauri::command]
pub async fn get_cloud_models(
    auth: State<'_, Arc<AuthManager>>,
) -> Result<Vec<CloudModelInfo>, String> {
    auth.get_available_models().await.map_err(|e| e.to_string())
}
