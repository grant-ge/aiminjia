//! IPC commands for cloud authentication.

use std::sync::Arc;
use tauri::State;

use crate::auth::state::CloudAuthInfo;
use crate::auth::state::CloudModelInfo;
use crate::auth::AuthManager;

/// Map an internal anyhow error into a user-facing string.
///
/// If the underlying error is a `reqwest` network/connection failure (request
/// could not be sent, timed out, or connection failed), prepend a friendly
/// 中文 message. Otherwise format the full anyhow `Caused by` chain so the
/// real reason surfaces to the user (and to the support channel) instead of
/// being collapsed by `.to_string()`.
fn format_auth_error(e: anyhow::Error) -> String {
    if let Some(req_err) = e.downcast_ref::<reqwest::Error>() {
        if req_err.is_connect() || req_err.is_timeout() || req_err.is_request() {
            return format!("网络连接失败，请检查网络后重试\n\n详情：{:#}", e);
        }
    }
    format!("{:#}", e)
}

/// Branding info returned to frontend for instant (no-network) brand application.
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BrandingInfo {
    pub product_name: Option<String>,
    pub logo_url: Option<String>,
    pub accent_color: Option<String>,
    pub primary_color: Option<String>,
    pub bg_color: Option<String>,
    pub sidebar_bg_color: Option<String>,
    pub font_family: Option<String>,
}

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
    let result = auth
        .login(username, &password)
        .await
        .map_err(format_auth_error)?;
    log::info!(
        "[cloud_login] user={} models({})={:?}",
        username,
        result.models.len(),
        result
            .models
            .iter()
            .map(|m| format!("{}({})", m.id, m.model_type))
            .collect::<Vec<_>>()
    );
    Ok(result)
}

/// Logout from cloud mode.
#[tauri::command]
pub async fn cloud_logout(auth: State<'_, Arc<AuthManager>>) -> Result<(), String> {
    auth.logout().await;
    Ok(())
}

/// Get current cloud auth state (for app init / restore).
/// If logged in, proactively refreshes auth/profile from server so tenant branding
/// changes (product name/logo/colors) apply without requiring logout + re-login.
#[tauri::command]
pub async fn get_cloud_auth(auth: State<'_, Arc<AuthManager>>) -> Result<CloudAuthInfo, String> {
    Ok(auth.refresh_auth_info().await)
}

/// Fetch available cloud models.
#[tauri::command]
pub async fn get_cloud_models(
    auth: State<'_, Arc<AuthManager>>,
) -> Result<Vec<CloudModelInfo>, String> {
    let models = auth
        .get_available_models()
        .await
        .map_err(format_auth_error)?;
    log::info!(
        "[get_cloud_models] {} models returned: {:?}",
        models.len(),
        models
            .iter()
            .map(|m| format!("{}({})", m.id, m.model_type))
            .collect::<Vec<_>>()
    );
    Ok(models)
}

/// Change password on the cloud server.
/// After success, the user is automatically logged out.
#[tauri::command]
pub async fn cloud_change_password(
    auth: State<'_, Arc<AuthManager>>,
    old_password: String,
    new_password: String,
) -> Result<(), String> {
    if old_password.is_empty() || new_password.is_empty() {
        return Err("请输入旧密码和新密码".to_string());
    }
    if new_password.len() < 8 {
        return Err("新密码长度至少 8 个字符".to_string());
    }
    auth.change_password(&old_password, &new_password)
        .await
        .map_err(format_auth_error)
}

/// Get branding info from persisted auth state (no network call).
/// Returns instantly from in-memory state restored at app startup.
#[tauri::command]
pub async fn get_branding(auth: State<'_, Arc<AuthManager>>) -> Result<BrandingInfo, String> {
    let info = auth.get_auth_info().await;
    let branding = match info.tenant {
        Some(t) => BrandingInfo {
            product_name: t.product_name,
            logo_url: t.logo_url,
            accent_color: t.accent_color,
            primary_color: t.primary_color,
            bg_color: t.bg_color,
            sidebar_bg_color: t.sidebar_bg_color,
            font_family: t.font_family,
        },
        None => BrandingInfo {
            product_name: None,
            logo_url: None,
            accent_color: None,
            primary_color: None,
            bg_color: None,
            sidebar_bg_color: None,
            font_family: None,
        },
    };
    Ok(branding)
}
