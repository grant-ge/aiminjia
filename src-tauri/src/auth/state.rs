//! Cloud authentication state — persisted as encrypted JSON.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// User information from the tenant portal.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UserInfo {
    pub id: i64,
    pub name: String,
    pub username: String,
    #[serde(default = "default_user_role")]
    pub role: String,
}

pub fn default_user_role() -> String {
    "member".to_string()
}

/// Tenant (organization) information.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TenantInfo {
    pub id: i64,
    pub name: String,
    pub balance: String,
    /// "personal" or "enterprise"; empty string when unknown.
    #[serde(default)]
    pub tenant_type: String,
    /// Custom product name (empty/None = default "AI小家").
    #[serde(default)]
    pub product_name: Option<String>,
    /// Custom logo URL (empty/None = default logo).
    #[serde(default)]
    pub logo_url: Option<String>,
    /// Custom accent color hex (empty/None = default #D4A843).
    #[serde(default)]
    pub accent_color: Option<String>,
    /// Custom primary/foreground color hex (empty/None = default #1D1D1F).
    #[serde(default)]
    pub primary_color: Option<String>,
    /// Custom sidebar background color for legacy tenant configs (empty/None = default #FAFAF8).
    #[serde(default)]
    pub bg_color: Option<String>,
    /// Custom sidebar background color (empty/None = fallback bg_color/default #FAFAF8).
    #[serde(default)]
    pub sidebar_bg_color: Option<String>,
    /// Custom font family identifier (empty/None = system default).
    #[serde(default)]
    pub font_family: Option<String>,
}

/// Cloud logical model info returned by AIjia Gateway V2.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CloudModelInfo {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub model_type: String,
}

/// Full authentication state for cloud mode.
///
/// Persisted as AES-256-GCM encrypted JSON in AppStorage (key: `cloud_auth`).
/// Automatically restored on app restart.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CloudAuth {
    pub access_token: String,
    pub access_expires_at: DateTime<Utc>,
    pub refresh_token: String,
    pub refresh_expires_at: DateTime<Utc>,
    pub session_key: String,
    pub session_key_expires_at: DateTime<Utc>,
    pub user: UserInfo,
    pub tenant: TenantInfo,
}

/// Response returned to the frontend after login.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CloudAuthInfo {
    pub logged_in: bool,
    pub user: Option<UserInfo>,
    pub tenant: Option<TenantInfo>,
    pub models: Vec<CloudModelInfo>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn cloud_auth_deserializes_legacy_user_without_role() {
        let raw = json!({
            "accessToken": "access",
            "accessExpiresAt": "2026-07-01T00:00:00Z",
            "refreshToken": "refresh",
            "refreshExpiresAt": "2026-07-02T00:00:00Z",
            "sessionKey": "session",
            "sessionKeyExpiresAt": "2026-07-02T00:00:00Z",
            "user": {
                "id": 26,
                "name": "Legacy User",
                "username": "legacy@example.com"
            },
            "tenant": {
                "id": 15,
                "name": "Tenant",
                "balance": "0"
            }
        });

        let auth: CloudAuth = serde_json::from_value(raw).expect("legacy auth stays readable");

        assert_eq!(auth.user.role, "member");
    }
}
