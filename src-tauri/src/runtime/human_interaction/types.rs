use crate::runtime::ids::{RunId, SessionId, ToolCallId};

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ImPlatform {
    Dingtalk,
    Feishu,
    Wecom,
    Wechat,
    Telegram,
    Whatsapp,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TurnOrigin {
    App,
    Im {
        platform: ImPlatform,
        external_conversation_key: String,
        sender_id: Option<String>,
        sender_label: Option<String>,
        account_id: Option<String>,
        thread_id: Option<String>,
    },
}

impl Default for TurnOrigin {
    fn default() -> Self {
        Self::App
    }
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImReplyTarget {
    pub session_id: String,
    pub external_conversation_key: String,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum OutputBinding {
    AppOnly,
    Im {
        platform: ImPlatform,
        target: ImReplyTarget,
        allow_streaming_reply: bool,
    },
}

impl Default for OutputBinding {
    fn default() -> Self {
        Self::AppOnly
    }
}

impl OutputBinding {
    pub fn im(
        platform: ImPlatform,
        session_id: impl Into<String>,
        external_conversation_key: impl Into<String>,
        allow_streaming_reply: bool,
    ) -> Self {
        Self::Im {
            platform,
            target: ImReplyTarget {
                session_id: session_id.into(),
                external_conversation_key: external_conversation_key.into(),
            },
            allow_streaming_reply,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct HumanInteractionId(String);

impl HumanInteractionId {
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum HumanInteractionKind {
    PermissionAsk,
    AskUserQuestion,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum HumanInteractionStatus {
    Pending,
    Resolved,
    Cancelled,
    Abandoned,
    Expired,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HumanInteractionRef {
    pub id: HumanInteractionId,
    pub session_id: SessionId,
    pub run_id: RunId,
    pub tool_call_id: ToolCallId,
    pub kind: HumanInteractionKind,
    pub turn_origin: TurnOrigin,
    pub output_binding: OutputBinding,
    pub status: HumanInteractionStatus,
}

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InboundUserMessage {
    pub session_id: SessionId,
    pub turn_origin: TurnOrigin,
    pub output_binding: OutputBinding,
    pub content: String,
    pub received_at_ms: i64,
}

impl InboundUserMessage {
    pub fn app_text(session_id: impl Into<String>, content: impl Into<String>) -> Self {
        Self {
            session_id: SessionId::new(session_id.into()),
            turn_origin: TurnOrigin::App,
            output_binding: OutputBinding::AppOnly,
            content: content.into(),
            received_at_ms: chrono::Utc::now().timestamp_millis(),
        }
    }

    pub fn im_text(
        session_id: impl Into<String>,
        platform: ImPlatform,
        external_conversation_key: impl Into<String>,
        content: impl Into<String>,
    ) -> Self {
        let session_id = session_id.into();
        let external_conversation_key = external_conversation_key.into();
        Self {
            session_id: SessionId::new(session_id.clone()),
            turn_origin: TurnOrigin::Im {
                platform,
                external_conversation_key: external_conversation_key.clone(),
                sender_id: None,
                sender_label: None,
                account_id: None,
                thread_id: None,
            },
            output_binding: OutputBinding::im(
                platform,
                session_id,
                external_conversation_key,
                true,
            ),
            content: content.into(),
            received_at_ms: chrono::Utc::now().timestamp_millis(),
        }
    }
}
