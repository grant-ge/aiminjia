use serde::{Deserialize, Serialize};

/// IM platform identifier.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Platform {
    Dingtalk,
    Feishu,
    Wechat,
    Wecom,
}

impl Platform {
    pub fn as_str(&self) -> &'static str {
        match self {
            Platform::Dingtalk => "dingtalk",
            Platform::Feishu => "feishu",
            Platform::Wechat => "wechat",
            Platform::Wecom => "wecom",
        }
    }

    pub fn from_str(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "dingtalk" => Some(Platform::Dingtalk),
            "feishu" => Some(Platform::Feishu),
            "wechat" => Some(Platform::Wechat),
            "wecom" => Some(Platform::Wecom),
            _ => None,
        }
    }

    pub fn all() -> [Self; 4] {
        [Self::Dingtalk, Self::Feishu, Self::Wechat, Self::Wecom]
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ChannelCapability {
    Available,
    ComingSoon,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ChannelConnectionState {
    Unconfigured,
    Disconnected,
    Connecting,
    Connected,
    Reconnecting,
    ConfigError,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum RobotCodeSource {
    Registration,
    AppKeyFallback,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChannelConfigView {
    pub platform: Platform,
    pub app_key: String,
    pub app_secret_masked: String,
    pub robot_code: String,
    pub robot_code_source: RobotCodeSource,
    pub source: String,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChannelPlatformState {
    pub platform: Platform,
    pub capability: ChannelCapability,
    pub configured: bool,
    pub enabled: bool,
    pub connection: ChannelConnectionState,
    pub config: Option<ChannelConfigView>,
    pub last_connected_at: Option<String>,
    pub last_error: Option<String>,
}

impl ChannelPlatformState {
    pub fn coming_soon(platform: Platform) -> Self {
        Self {
            platform,
            capability: ChannelCapability::ComingSoon,
            configured: false,
            enabled: false,
            connection: ChannelConnectionState::Unconfigured,
            config: None,
            last_connected_at: None,
            last_error: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChannelPlatformStatePayload {
    pub state: ChannelPlatformState,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum SecretStorageKind {
    SecureStorage,
    PlaintextFallback,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DingtalkStoredCredentials {
    pub app_key: String,
    /// Encrypted AppSecret. Falls back to plaintext only when SecureStorage is unavailable.
    pub app_secret_encrypted: String,
    pub app_secret_storage: SecretStorageKind,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DingtalkStoredBot {
    pub robot_code: String,
    pub robot_code_source: RobotCodeSource,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DingtalkStoredRegistration {
    pub source: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DingtalkStoredMetadata {
    pub created_at: String,
    pub updated_at: String,
}

/// DingTalk channel config stored at users/<scope>/channels/dingtalk/config.json.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DingtalkStoredConfig {
    pub schema_version: u32,
    pub platform: Platform,
    pub configured: bool,
    pub enabled: bool,
    pub credentials: DingtalkStoredCredentials,
    pub bot: DingtalkStoredBot,
    pub registration: DingtalkStoredRegistration,
    pub metadata: DingtalkStoredMetadata,
}

/// DingTalk OPEN_CLAW registration session begin result.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChannelRegistrationBeginResult {
    pub device_code: String,
    pub user_code: String,
    pub verification_uri_complete: String,
    pub verification_uri: String,
    pub interval_seconds: u64,
    pub expires_in_seconds: u64,
    pub source: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChannelRegistrationPollResult {
    pub state: ChannelRegistrationPollState,
    pub client_id: Option<String>,
    pub robot_code: Option<String>,
    pub config: Option<ChannelConfigView>,
    pub platform_state: Option<ChannelPlatformState>,
    pub fail_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum ChannelRegistrationPollState {
    Waiting,
    Success,
    Fail,
    Expired,
    Unknown,
}

/// Channel conversations are internal Lotus sessions backed by an external IM conversation.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChannelConversation {
    pub session_id: String,
    pub platform: Platform,
    pub conversation_type: ConversationType,
    pub external_id: String,
    pub display_name: String,
    pub unread_count: u32,
    /// 机器人维度，用来区分不同钉钉应用 / 不同机器人产生的对话。
    pub robot_code: String,
    /// 是否归属当前在线机器人；false 表示历史会话，UI 进折叠区，输入区禁用。
    pub is_active_robot: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ConversationType {
    Group,
    Private,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChannelAttachmentSpec {
    pub kind: AttachmentKind,
    pub download_code: String,
    pub file_name: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AttachmentKind {
    Picture,
    File,
}

/// One parsed message from DingTalk Stream.
#[derive(Debug, Clone)]
pub struct ChannelMessage {
    pub msg_id: String,
    pub conversation_type: ConversationType,
    pub conversation_key: String,
    pub sender_id: String,
    pub sender_nick: String,
    pub text: String,
    pub robot_code: String,
    pub reply_group_id: String,
    pub attachments: Vec<ChannelAttachmentSpec>,
    pub session_webhook: Option<String>,
}

/// channel:message event payload.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChannelMessagePayload {
    pub platform: String,
    pub session_id: String,
    pub sender_nick: String,
    pub text_preview: String,
}

#[cfg(test)]
mod tests {
    use serde_json::Value;

    use super::*;

    #[test]
    fn poll_result_serialization_never_exposes_client_secret_field() {
        let result = ChannelRegistrationPollResult {
            state: ChannelRegistrationPollState::Waiting,
            client_id: Some("app-key".into()),
            robot_code: Some("robot-code".into()),
            config: None,
            platform_state: None,
            fail_reason: None,
        };

        let value = serde_json::to_value(result).expect("serialize poll result");
        let object = value.as_object().expect("poll result object");

        assert!(!object.contains_key("clientSecret"));
        assert!(!matches!(value.get("clientSecret"), Some(Value::String(_))));
    }
}
