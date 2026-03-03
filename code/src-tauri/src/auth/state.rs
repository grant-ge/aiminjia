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
}

/// Tenant (organization) information.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TenantInfo {
    pub id: i64,
    pub name: String,
    pub balance: String,
}

/// Cloud model info returned by /v1/models.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CloudModelInfo {
    pub id: String,
    pub name: String,
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
