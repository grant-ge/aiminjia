//! Feishu-specific persisted types and runtime targets.

use serde::{Deserialize, Serialize};

use crate::connector::im::types::{Platform, SecretStorageKind};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FeishuStoredCredentials {
    pub app_id: String,
    pub app_secret_encrypted: String,
    pub app_secret_storage: SecretStorageKind,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FeishuStoredMetadata {
    pub created_at: String,
    pub updated_at: String,
}

/// users/<scope>/channels/feishu/config.json schema. schema_version=1 for PR2 onwards.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FeishuStoredConfig {
    pub schema_version: u32,
    pub platform: Platform,
    pub configured: bool,
    pub enabled: bool,
    pub credentials: FeishuStoredCredentials,
    pub metadata: FeishuStoredMetadata,
}

/// CardKit + reply credentials per session_id, populated by manager worker when
/// a message arrives and consumed by FeishuConnector::send.
#[derive(Debug, Clone)]
pub struct FeishuSessionTarget {
    /// CardKit receive_id_type ("chat_id" for group, "open_id" for private).
    pub receive_id_type: String,
    /// chat_id (group) or open_id (private).
    pub receive_id: String,
}
