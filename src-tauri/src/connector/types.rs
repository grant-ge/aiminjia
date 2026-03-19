use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SaasAppInfo {
    pub id: u64,
    pub name: String,
    pub base_url: String,
    pub connect_mode: String,
    pub auth_required: Option<bool>,
    pub summary: Option<String>,
    pub icon_url: Option<String>,
    pub status: String,
    pub connected: bool,
    pub enabled: Option<bool>,
    pub has_credential: Option<bool>,
    pub capabilities: Vec<SaasCapabilityInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SaasCapabilityInfo {
    pub id: u64,
    pub name: String,
    pub description: String,
    pub http_method: String,
    pub path_template: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SaasCapabilitySample {
    pub id: u64,
    pub name: String,
    pub description: String,
    pub http_method: String,
    pub path_template: String,
    pub request_sample: Option<String>,
    pub response_sample: Option<String>,
    pub notes: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SaasCredential {
    pub access_token: Option<String>,
    pub api_credential: Option<String>,
    pub token_expires_at: Option<String>,
    pub oauth_user_info: Option<String>,
    pub status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectorConfig {
    pub apps: Vec<SaasAppInfo>,
    pub last_synced: Option<String>,
}
