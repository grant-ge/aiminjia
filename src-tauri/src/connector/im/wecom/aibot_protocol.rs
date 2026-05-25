//! aibot WebSocket 协议帧的 Rust 类型 + serde 实现。
//!
//! 参考 `@wecom/aibot-node-sdk@1.0.7` 类型定义（`dist/index.d.ts`）。
//! 所有帧统一格式：`{ cmd?, headers: { req_id, .. }, body?, errcode?, errmsg? }`。
//!
//! - 发送：cmd + headers + body
//! - 服务端推送（消息 / 事件）：cmd + headers + body
//! - 响应 ack（认证 / 心跳 / 回复回执）：headers + errcode + errmsg（无 cmd / body）

use std::collections::BTreeMap;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

/// WebSocket 命令枚举，对应 SDK `WsCmd` 常量。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum WsCmd {
    #[serde(rename = "aibot_subscribe")]
    Subscribe,
    #[serde(rename = "ping")]
    Ping,
    #[serde(rename = "aibot_respond_msg")]
    Respond,
    #[serde(rename = "aibot_send_msg")]
    SendMsg,
    #[serde(rename = "aibot_msg_callback")]
    MsgCallback,
    #[serde(rename = "aibot_event_callback")]
    EventCallback,
    #[serde(rename = "aibot_upload_media_init")]
    UploadInit,
    #[serde(rename = "aibot_upload_media_chunk")]
    UploadChunk,
    #[serde(rename = "aibot_upload_media_finish")]
    UploadFinish,
}

/// 通用 WS 帧结构。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(bound(deserialize = "B: Deserialize<'de>"))]
pub struct WsFrame<B = serde_json::Value> {
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub cmd: Option<WsCmd>,
    pub headers: FrameHeaders,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub body: Option<B>,
    /// 响应帧才有。
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub errcode: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub errmsg: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct FrameHeaders {
    pub req_id: String,
    #[serde(flatten)]
    pub extra: BTreeMap<String, serde_json::Value>,
}

/// 入站：`aibot_msg_callback` body（用户消息）。
#[derive(Debug, Clone, Deserialize)]
pub struct InboundMessageBody {
    pub msgid: String,
    pub aibotid: String,
    #[serde(default)]
    pub chatid: Option<String>,
    pub chattype: ChatType,
    pub from: From,
    pub msgtype: String,
    #[serde(default)]
    pub create_time: Option<i64>,
    /// 留给 parser 按 msgtype 进一步解析（text/image/file/...）。
    #[serde(flatten)]
    pub payload: serde_json::Value,
}

/// 入站：`aibot_event_callback` body（事件）。
#[derive(Debug, Clone, Deserialize)]
pub struct EventCallbackBody {
    pub msgid: String,
    pub aibotid: String,
    #[serde(default)]
    pub chatid: Option<String>,
    #[serde(default)]
    pub chattype: Option<ChatType>,
    #[serde(default)]
    pub create_time: Option<i64>,
    pub from: From,
    pub msgtype: String, // 恒等于 "event"
    pub event: EventContent,
}

#[derive(Debug, Clone, Deserialize)]
pub struct EventContent {
    pub eventtype: EventType,
    #[serde(flatten)]
    pub extra: serde_json::Value,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
pub enum EventType {
    #[serde(rename = "enter_chat")]
    EnterChat,
    #[serde(rename = "template_card_event")]
    TemplateCardEvent,
    #[serde(rename = "feedback_event")]
    FeedbackEvent,
    #[serde(rename = "disconnected_event")]
    Disconnected,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ChatType {
    Single,
    Group,
}

#[derive(Debug, Clone, Deserialize)]
pub struct From {
    pub userid: String,
    #[serde(default)]
    pub corpid: Option<String>,
}

/// 出站：`aibot_subscribe` body（认证）。
#[derive(Debug, Clone, Serialize)]
pub struct SubscribeBody {
    pub secret: String,
    pub bot_id: String,
}

/// 出站：`aibot_respond_msg` body — markdown 形态。
#[derive(Debug, Clone, Serialize)]
pub struct RespondMarkdownBody {
    pub msgtype: &'static str,
    pub markdown: MarkdownContent,
}

impl RespondMarkdownBody {
    pub fn new(content: impl Into<String>) -> Self {
        Self {
            msgtype: "markdown",
            markdown: MarkdownContent {
                content: content.into(),
            },
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct MarkdownContent {
    pub content: String,
}

/// 出站：`aibot_send_msg` body（主动推送）。
#[derive(Debug, Clone, Serialize)]
pub struct SendMsgBody {
    pub chatid: String,
    #[serde(flatten)]
    pub payload: SendMsgPayload,
}

impl SendMsgBody {
    pub fn markdown(chatid: String, content: String) -> Self {
        Self {
            chatid,
            payload: SendMsgPayload::Markdown {
                msgtype: "markdown",
                markdown: MarkdownContent { content },
            },
        }
    }

    pub fn media(chatid: String, media_type: WeComMediaType, media_id: String) -> Self {
        Self {
            chatid,
            payload: SendMsgPayload::Media {
                media_type,
                media_id,
            },
        }
    }
}

#[derive(Debug, Clone)]
pub enum SendMsgPayload {
    Markdown {
        msgtype: &'static str,
        markdown: MarkdownContent,
    },
    Media {
        media_type: WeComMediaType,
        media_id: String,
    },
}

impl Serialize for SendMsgPayload {
    fn serialize<S: serde::Serializer>(&self, ser: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeMap;
        match self {
            SendMsgPayload::Markdown { msgtype, markdown } => {
                let mut m = ser.serialize_map(Some(2))?;
                m.serialize_entry("msgtype", msgtype)?;
                m.serialize_entry("markdown", markdown)?;
                m.end()
            }
            SendMsgPayload::Media {
                media_type,
                media_id,
            } => {
                let key = media_type.as_str();
                let mut m = ser.serialize_map(Some(2))?;
                m.serialize_entry("msgtype", key)?;
                m.serialize_entry(key, &serde_json::json!({ "media_id": media_id }))?;
                m.end()
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum WeComMediaType {
    File,
    Image,
    Voice,
    Video,
}

impl WeComMediaType {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::File => "file",
            Self::Image => "image",
            Self::Voice => "voice",
            Self::Video => "video",
        }
    }
}

/// 生成请求 ID：`{prefix}_{ms_timestamp}_{8-char-random}`。
/// 对应 SDK `generateReqId(prefix)`。
pub fn generate_req_id(prefix: &str) -> String {
    let ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let rand: String = (0..8)
        .map(|_| {
            const CS: &[u8] = b"abcdefghijklmnopqrstuvwxyz0123456789";
            CS[fastrand::usize(..CS.len())] as char
        })
        .collect();
    format!("{prefix}_{ms}_{rand}")
}
