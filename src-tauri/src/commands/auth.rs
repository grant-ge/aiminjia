//! IPC commands for cloud authentication.

use std::sync::Arc;
use tauri::State;

use crate::auth::state::CloudAuthInfo;
use crate::auth::state::CloudModelInfo;
use crate::auth::AuthManager;
use crate::storage::{AiJiaHome, CurrentUserStorage, UserScope};

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

/// Login with username and password.
/// Returns user info, tenant info, and available models.
#[tauri::command]
pub async fn cloud_login(
    auth: State<'_, Arc<AuthManager>>,
    cus: State<'_, Arc<CurrentUserStorage>>,
    home: State<'_, Arc<AiJiaHome>>,
    file_mgr: State<'_, Arc<crate::storage::file_manager::FileManager>>,
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

    if let (Some(user), Some(tenant)) = (&result.user, &result.tenant) {
        let scope = crate::storage::UserScope::new(tenant.id, user.id);
        let user_dir = home.user_dir(&scope);
        if let Err(e) = crate::storage::migration_user_scope::migrate_legacy_to_user_scope_if_needed(
            home.root(),
            &user_dir,
            &scope.key(),
            &home.global_state_path(),
        ) {
            log::warn!("[cloud_login] migration warning: {}", e);
        }
        if let Err(e) = crate::storage::migration_user_scope::migrate_legacy_config_if_needed(
            home.root(),
            &user_dir,
            &home.global_dir(),
        ) {
            log::warn!("[cloud_login] config split warning: {}", e);
        }
        cus.activate_scope(scope.clone())
            .map_err(|e| format!("Failed to activate scope: {}", e))?;

        // Refresh FileManager with user-scoped workspacePath
        let workspace_path = cus
            .get()
            .and_then(|db| db.get_setting("workspacePath").ok().flatten())
            .unwrap_or_default();
        if !workspace_path.is_empty() {
            let p = std::path::PathBuf::from(&workspace_path);
            std::fs::create_dir_all(&p).ok();
            file_mgr.update_workspace_path(&p);
        } else {
            file_mgr.update_workspace_path(home.root());
        }

        let now = chrono::Utc::now().to_rfc3339();
        let scope_path = home.user_scope_json_path(&scope);

        // Preserve createdAt from existing scope.json if present; only update lastSeenAt.
        let created_at = std::fs::read_to_string(&scope_path)
            .ok()
            .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok())
            .and_then(|v| {
                v.get("createdAt")
                    .and_then(|x| x.as_str())
                    .map(String::from)
            })
            .unwrap_or_else(|| now.clone());

        let scope_json = serde_json::json!({
            "tenantId": tenant.id,
            "userId": user.id,
            "name": user.name,
            "username": user.username,
            "tenantName": tenant.name,
            "createdAt": created_at,
            "lastSeenAt": now,
        });
        let _ = std::fs::write(
            scope_path,
            serde_json::to_string_pretty(&scope_json).unwrap_or_default(),
        );

        let active = serde_json::json!({
            "scopeKey": scope.key(),
            "tenantId": tenant.id,
            "userId": user.id,
        });
        let _ = std::fs::write(
            home.active_account_path(),
            serde_json::to_string_pretty(&active).unwrap_or_default(),
        );
    }

    Ok(result)
}

/// Logout from cloud mode.
#[tauri::command]
pub async fn cloud_logout(
    auth: State<'_, Arc<AuthManager>>,
    cus: State<'_, Arc<CurrentUserStorage>>,
    home: State<'_, Arc<AiJiaHome>>,
    file_mgr: State<'_, Arc<crate::storage::file_manager::FileManager>>,
) -> Result<(), String> {
    auth.logout().await;
    cus.deactivate();
    // Reset FileManager to root default, preventing stale user workspace access
    file_mgr.update_workspace_path(home.root());
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

/// Send an SMS verification code for personal registration.
#[tauri::command]
pub async fn cloud_send_sms_code(
    auth: State<'_, Arc<AuthManager>>,
    phone: String,
) -> Result<(), String> {
    let phone = phone.trim();
    if phone.is_empty() {
        return Err("请输入手机号".to_string());
    }
    auth.send_sms_code(phone).await.map_err(format_auth_error)
}

/// Send an email verification code for personal registration.
#[tauri::command]
pub async fn cloud_send_email_code(
    auth: State<'_, Arc<AuthManager>>,
    email: String,
) -> Result<(), String> {
    let email = email.trim();
    if email.is_empty() {
        return Err("请输入邮箱".to_string());
    }
    auth.send_email_code(email).await.map_err(format_auth_error)
}

/// Register a personal account via phone or email + verification code.
/// On success the caller should follow up with `cloud_login` using the same
/// identifier + password — that path activates the user scope and creates
/// the session key.
#[tauri::command]
pub async fn cloud_register(
    auth: State<'_, Arc<AuthManager>>,
    method: String,
    phone: Option<String>,
    email: Option<String>,
    code: String,
    password: String,
    name: Option<String>,
) -> Result<(), String> {
    let method = method.trim();
    let phone = phone.unwrap_or_default();
    let phone = phone.trim();
    let email = email.unwrap_or_default();
    let email = email.trim();
    let code = code.trim();
    let name = name.unwrap_or_default();
    let name = name.trim();

    match method {
        "phone" => {
            if phone.is_empty() || code.is_empty() || password.is_empty() {
                return Err("请填写手机号、验证码和密码".to_string());
            }
        }
        "email" => {
            if email.is_empty() || code.is_empty() || password.is_empty() {
                return Err("请填写邮箱、验证码和密码".to_string());
            }
        }
        _ => return Err("注册方式无效".to_string()),
    }
    if password.len() < 8 {
        return Err("密码至少 8 个字符".to_string());
    }

    auth.register(method, phone, email, code, &password, name)
        .await
        .map_err(format_auth_error)
}

/// Cached tenant branding snapshot persisted to `~/.renlijia/users/{scope}/brand.json`.
/// Lets the login page pre-apply the previous tenant's brand after logout.
/// All fields are optional — missing means "use built-in default".
#[derive(Debug, Default, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BrandSnapshot {
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub product_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub logo_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub accent_color: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub primary_color: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub bg_color: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub sidebar_bg_color: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub font_family: Option<String>,
}

/// Read the last-active scope from `auth/active_account.json`. Returns
/// `None` when the file is missing, malformed, or has no scope info — all
/// of which are "not logged in here yet" signals, not errors.
fn read_active_scope(home: &AiJiaHome) -> Option<UserScope> {
    let raw = std::fs::read_to_string(home.active_account_path()).ok()?;
    let v: serde_json::Value = serde_json::from_str(&raw).ok()?;
    let tenant_id = v.get("tenantId").and_then(|x| x.as_i64())?;
    let user_id = v.get("userId").and_then(|x| x.as_i64())?;
    Some(UserScope::new(tenant_id, user_id))
}

/// Read the cached brand snapshot for the last-active account, if any.
/// Returns `None` when no account has ever logged in on this machine OR
/// when the cache file does not exist yet. Errors during read/parse are
/// silently downgraded to `None` because a corrupt cache should fall back
/// to defaults, not block the login page.
#[tauri::command]
pub fn get_last_brand(home: State<'_, Arc<AiJiaHome>>) -> Result<Option<BrandSnapshot>, String> {
    let Some(scope) = read_active_scope(&home) else {
        return Ok(None);
    };
    let path = home.user_brand_path(&scope);
    let raw = match std::fs::read_to_string(&path) {
        Ok(s) => s,
        Err(_) => return Ok(None),
    };
    match serde_json::from_str::<BrandSnapshot>(&raw) {
        Ok(snap) => Ok(Some(snap)),
        Err(e) => {
            log::warn!("[get_last_brand] parse failed at {:?}: {}", path, e);
            Ok(None)
        }
    }
}

/// Write the brand snapshot for the currently-active account. No-op when
/// `active_account.json` is missing (i.e. user has never logged in on this
/// machine) — there's no per-scope home to write into. Best-effort: I/O
/// failures are logged and swallowed so a failed write never blocks login.
#[tauri::command]
pub fn save_last_brand(
    home: State<'_, Arc<AiJiaHome>>,
    brand: BrandSnapshot,
) -> Result<(), String> {
    let Some(scope) = read_active_scope(&home) else {
        return Ok(());
    };
    let dir = home.user_dir(&scope);
    if let Err(e) = std::fs::create_dir_all(&dir) {
        log::warn!("[save_last_brand] mkdir {:?} failed: {}", dir, e);
        return Ok(());
    }
    let path = home.user_brand_path(&scope);
    let body = serde_json::to_string_pretty(&brand).unwrap_or_default();
    if let Err(e) = std::fs::write(&path, body) {
        log::warn!("[save_last_brand] write {:?} failed: {}", path, e);
    }
    Ok(())
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
