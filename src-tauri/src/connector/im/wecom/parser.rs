//! 把入站 aibot WS 帧映射到 trait 层中性 `ChannelMessage`。
//!
//! 字段对齐：
//! - `robot_code` ← bot_id（caller 传入）
//! - `reply_group_id` ← chatid（group）或 userid（single）
//! - `session_webhook` ← None（aibot 不用 webhook URL 概念）
//! - `ChannelAttachmentSpec.download_code` ← "wecom://{aeskey}@{url}" 形式（媒体下载时由 media.rs 还原）

use serde_json::Value;

use super::aibot_protocol::{InboundMessageBody, WsCmd, WsFrame};
use crate::connector::im::types::{
    AttachmentKind, ChannelAttachmentSpec, ChannelMessage, ConversationType,
};

#[derive(Debug)]
pub enum ParsedInbound {
    Message(ChannelMessage),
    /// 已知类型但本期不转发（voice / video / mixed 等）。
    Ignored,
}

pub fn parse_inbound(bot_id: &str, frame: &WsFrame<Value>) -> Option<ParsedInbound> {
    if frame.cmd != Some(WsCmd::MsgCallback) {
        return None;
    }
    let raw = frame.body.as_ref()?;
    let body: InboundMessageBody = serde_json::from_value(raw.clone()).ok()?;

    let (conversation_type, reply_group_id) = match body.chattype {
        super::aibot_protocol::ChatType::Single => {
            (ConversationType::Private, body.from.userid.clone())
        }
        super::aibot_protocol::ChatType::Group => (
            ConversationType::Group,
            body.chatid.clone().unwrap_or_default(),
        ),
    };

    let (text, attachments) = match body.msgtype.as_str() {
        "text" => {
            let t = body
                .payload
                .pointer("/text/content")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            (t, vec![])
        }
        "image" | "file" => {
            let kind = if body.msgtype == "image" {
                AttachmentKind::Picture
            } else {
                AttachmentKind::File
            };
            let key_path = if body.msgtype == "image" {
                "/image"
            } else {
                "/file"
            };
            let url = body
                .payload
                .pointer(&format!("{key_path}/url"))
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let aeskey = body
                .payload
                .pointer(&format!("{key_path}/aeskey"))
                .and_then(|v| v.as_str())
                .unwrap_or("");
            if url.is_empty() || aeskey.is_empty() {
                log::warn!(
                    "[wecom] {} msg {} missing url or aeskey, ignoring",
                    body.msgtype,
                    body.msgid
                );
                return Some(ParsedInbound::Ignored);
            }
            let file_name = body
                .payload
                .pointer(&format!("{key_path}/filename"))
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
                .unwrap_or_else(|| {
                    format!(
                        "{}-{}.{}",
                        body.msgid,
                        chrono::Utc::now().timestamp(),
                        if body.msgtype == "image" {
                            "jpg"
                        } else {
                            "bin"
                        }
                    )
                });
            let download_code = format!("wecom://{aeskey}@{url}");
            (
                String::new(),
                vec![ChannelAttachmentSpec {
                    kind,
                    download_code,
                    file_name,
                }],
            )
        }
        "voice" | "video" | "mixed" => return Some(ParsedInbound::Ignored),
        _ => return Some(ParsedInbound::Ignored),
    };

    Some(ParsedInbound::Message(ChannelMessage {
        msg_id: body.msgid,
        conversation_type,
        // conversation_key 使用 chatid（group）或 userid（single），跟 reply_group_id 对齐
        conversation_key: reply_group_id.clone(),
        sender_id: body.from.userid.clone(),
        sender_nick: body.from.userid, // aibot 不提供 nick，先用 userid
        text,
        robot_code: bot_id.to_string(),
        reply_group_id,
        attachments,
        session_webhook: None,
        created_at_ms: None,
    }))
}
