//! `Event::Message` -> `Option<ChannelMessage>`. spec v3 §4.3 + §3.10 + §3.12.
//!
//! 不支持类型映射占位（让 AI 知道用户发了东西但内容不可用）；群事件 drop；
//! allow_from 列表过滤；quoted reply 前缀。
//!
//! PR7：IMAGE / DOCUMENT 真实下载到 tmp（downloader.is_some() 时），
//! 填 ChannelAttachmentSpec。downloader.is_none() 时保持 PR4 caption 行为不变。

use wa_rs::types::message::MessageInfo;
use wa_rs::wa_rs_proto::whatsapp as wa;

use super::config::WhatsAppChannelConfig;
use crate::connector::im::types::{
    AttachmentKind, ChannelAttachmentSpec, ChannelMessage, ConversationType,
};

/// 私聊判定：MessageSource.is_group=false。spec MVP 私聊 only。
pub fn is_private_chat(info: &MessageInfo) -> bool {
    !info.source.is_group
}

/// 转 ChannelMessage 的入口。失败/drop 返 None。caller（runtime.rs）拿到 Some
/// 才往 mpsc tx push。
///
/// allow_from：如 cfg.allow_from 是 Some(non-empty) 且发送方手机号不在列表 -> drop（None）。
/// allow_from = None 或 Some(空 vec) -> 不过滤。
/// is_from_me=true -> 永远 drop（不让 AI 回自己）。
///
/// PR7：downloader.is_some() 时对 IMAGE / DOCUMENT 真实下载：
/// - Ok  → 填 ChannelAttachmentSpec { kind, download_code=local_path, file_name }
/// - Err → 在 body 前 prefix `[附件下载失败]\n`，attachments 留空
///
/// downloader.is_none() 时保持 PR4 行为（caption 进 text，attachments 空）。
pub async fn normalize_async(
    msg: &wa::Message,
    info: &MessageInfo,
    cfg: Option<&WhatsAppChannelConfig>,
    downloader: Option<&super::download::WhatsAppMediaDownloader>,
) -> Option<ChannelMessage> {
    if info.source.is_from_me {
        return None;
    }
    if !is_private_chat(info) {
        return None;
    }
    if !is_allowed_sender(&info.source.sender, cfg) {
        log::debug!(
            "[whatsapp] sender {} not in allow_from, dropping",
            info.source.sender.user
        );
        return None;
    }

    // body：正常文本 / image caption / document caption / 不支持类型占位
    let body = extract_body_text(msg);
    let raw_text = match maybe_quoted_prefix(msg) {
        Some(prefix) => format!("{prefix}{body}"),
        None => body,
    };

    let msg_id_str = &info.id;
    let mut attachments: Vec<ChannelAttachmentSpec> = Vec::new();
    let text;

    if let Some(img) = msg.image_message.as_ref() {
        if let Some(dl) = downloader {
            match dl.download_image(img, msg_id_str).await {
                Ok(downloaded) => {
                    attachments.push(ChannelAttachmentSpec {
                        kind: AttachmentKind::Picture,
                        download_code: downloaded.path.to_string_lossy().to_string(),
                        file_name: downloaded.file_name,
                    });
                    text = raw_text;
                }
                Err(e) => {
                    log::warn!(
                        "[whatsapp] download_image failed for msg {}: {e:#}",
                        msg_id_str
                    );
                    text = format!("[附件下载失败]\n{raw_text}");
                }
            }
        } else {
            text = raw_text;
        }
    } else if let Some(doc) = msg.document_message.as_ref() {
        if let Some(dl) = downloader {
            match dl.download_document(doc, msg_id_str).await {
                Ok(downloaded) => {
                    attachments.push(ChannelAttachmentSpec {
                        kind: AttachmentKind::File,
                        download_code: downloaded.path.to_string_lossy().to_string(),
                        file_name: downloaded.file_name,
                    });
                    text = raw_text;
                }
                Err(e) => {
                    log::warn!(
                        "[whatsapp] download_document failed for msg {}: {e:#}",
                        msg_id_str
                    );
                    text = format!("[附件下载失败]\n{raw_text}");
                }
            }
        } else {
            text = raw_text;
        }
    } else {
        text = raw_text;
    }

    // 优先用 sender_alt（s.whatsapp.net 真号），否则 fallback chat。
    // 修 LID 模式 send 路由失败：朋友发来时 chat.server="lid"，必须用 sender_alt
    // 里的 @s.whatsapp.net JID 给 wa-rs send_message，否则发到 @lid 服务器拒绝。
    let routable_jid = info
        .source
        .sender_alt
        .as_ref()
        .filter(|j| j.server.contains("s.whatsapp.net"))
        .map(|j| format!("{}@{}", j.user, j.server))
        .unwrap_or_else(|| format!("{}@{}", info.source.chat.user, info.source.chat.server));
    let conv_key = routable_jid;
    let sender_id = format!("{}@{}", info.source.sender.user, info.source.sender.server);

    Some(ChannelMessage {
        msg_id: info.id.clone(),
        conversation_type: ConversationType::Private,
        conversation_key: conv_key,
        sender_id,
        sender_nick: info.push_name.clone(),
        text,
        robot_code: String::new(), // whatsapp 单账号无 robot_code 概念
        reply_group_id: String::new(),
        attachments,
        session_webhook: None,
        created_at_ms: Some(info.timestamp.timestamp_millis()),
    })
}

fn extract_body_text(msg: &wa::Message) -> String {
    // 1. 普通文本
    if let Some(s) = msg.conversation.as_ref() {
        if !s.is_empty() {
            return s.clone();
        }
    }
    if let Some(ext) = msg.extended_text_message.as_ref() {
        if let Some(t) = ext.text.as_ref() {
            if !t.is_empty() {
                return t.clone();
            }
        }
    }
    // 2. caption-bearing 类型
    if let Some(img) = msg.image_message.as_ref() {
        return img
            .caption
            .clone()
            .filter(|c| !c.is_empty())
            .unwrap_or_else(|| "[图片]".into());
    }
    if let Some(doc) = msg.document_message.as_ref() {
        let name = doc
            .file_name
            .clone()
            .filter(|n| !n.is_empty())
            .unwrap_or_else(|| "文件".into());
        let cap = doc.caption.clone().unwrap_or_default();
        if cap.is_empty() {
            return format!("[文件：{name}]");
        }
        return format!("[文件：{name}] {cap}");
    }
    if let Some(vid) = msg.video_message.as_ref() {
        let cap = vid.caption.clone().unwrap_or_default();
        if cap.is_empty() {
            return "[不支持的消息类型：视频]".into();
        }
        return format!("[不支持的消息类型：视频] {cap}");
    }
    // 3. 占位类型
    if msg.audio_message.is_some() {
        return "[不支持的消息类型：语音]".into();
    }
    if msg.sticker_message.is_some() {
        return "[不支持的消息类型：表情贴纸]".into();
    }
    if msg.location_message.is_some() || msg.live_location_message.is_some() {
        return "[不支持的消息类型：位置]".into();
    }
    if msg.contact_message.is_some() || msg.contacts_array_message.is_some() {
        return "[不支持的消息类型：联系人]".into();
    }
    // 4. 完全不认识
    "[不支持的消息类型]".into()
}

fn context_info_of(msg: &wa::Message) -> Option<&wa::ContextInfo> {
    if let Some(e) = msg.extended_text_message.as_ref() {
        if let Some(ci) = e.context_info.as_deref() {
            return Some(ci);
        }
    }
    if let Some(i) = msg.image_message.as_ref() {
        if let Some(ci) = i.context_info.as_deref() {
            return Some(ci);
        }
    }
    if let Some(d) = msg.document_message.as_ref() {
        if let Some(ci) = d.context_info.as_deref() {
            return Some(ci);
        }
    }
    if let Some(v) = msg.video_message.as_ref() {
        if let Some(ci) = v.context_info.as_deref() {
            return Some(ci);
        }
    }
    if let Some(a) = msg.audio_message.as_ref() {
        if let Some(ci) = a.context_info.as_deref() {
            return Some(ci);
        }
    }
    if let Some(s) = msg.sticker_message.as_ref() {
        if let Some(ci) = s.context_info.as_deref() {
            return Some(ci);
        }
    }
    None
}

fn maybe_quoted_prefix(msg: &wa::Message) -> Option<String> {
    let ctx = context_info_of(msg)?;
    let quoted = ctx.quoted_message.as_deref()?;
    let summary = summarize_quoted(quoted);
    if summary.is_empty() {
        return None;
    }
    Some(format!("[引用了消息：\"{summary}\"]\n"))
}

fn summarize_quoted(msg: &wa::Message) -> String {
    if let Some(s) = msg.conversation.as_ref() {
        if !s.is_empty() {
            return truncate_for_quote(s);
        }
    }
    if let Some(e) = msg.extended_text_message.as_ref() {
        if let Some(t) = e.text.as_ref() {
            if !t.is_empty() {
                return truncate_for_quote(t);
            }
        }
    }
    if let Some(img) = msg.image_message.as_ref() {
        let cap = img.caption.clone().unwrap_or_default();
        if cap.is_empty() {
            return "[图片]".into();
        }
        return format!("[图片] {}", truncate_for_quote(&cap));
    }
    if let Some(doc) = msg.document_message.as_ref() {
        let name = doc
            .file_name
            .clone()
            .filter(|n| !n.is_empty())
            .unwrap_or_else(|| "文件".into());
        return format!("[文件：{name}]");
    }
    if msg.audio_message.is_some() {
        return "[语音]".into();
    }
    if msg.video_message.is_some() {
        return "[视频]".into();
    }
    if msg.sticker_message.is_some() {
        return "[贴纸]".into();
    }
    "[消息]".into()
}

fn truncate_for_quote(s: &str) -> String {
    const MAX_CHARS: usize = 60;
    let trimmed = s.trim();
    let chars: Vec<char> = trimmed.chars().collect();
    if chars.len() <= MAX_CHARS {
        return trimmed.to_string();
    }
    let head: String = chars.into_iter().take(MAX_CHARS).collect();
    format!("{head}...")
}

fn is_allowed_sender(sender: &wa_rs::Jid, cfg: Option<&WhatsAppChannelConfig>) -> bool {
    let allow = match cfg.and_then(|c| c.allow_from.as_ref()) {
        Some(a) if !a.is_empty() => a,
        _ => return true, // None / 空 = 接收所有
    };
    let phone = normalize_phone(&sender.user);
    allow.iter().any(|s| normalize_phone(s) == phone)
}

fn normalize_phone(raw: &str) -> String {
    let cleaned: String = raw.chars().filter(|c| c.is_ascii_digit()).collect();
    format!("+{cleaned}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use wa_rs::Jid;

    fn make_info(is_from_me: bool, is_group: bool) -> MessageInfo {
        let mut info = MessageInfo::default();
        info.source.is_from_me = is_from_me;
        info.source.is_group = is_group;
        info.source.chat = Jid {
            user: "8613912345678".into(),
            server: "s.whatsapp.net".into(),
            ..Default::default()
        };
        info.source.sender = Jid {
            user: "8613912345678".into(),
            server: "s.whatsapp.net".into(),
            ..Default::default()
        };
        info.id = "MSG_TEST_1".into();
        info.push_name = "Alice".into();
        info
    }

    fn private_info() -> MessageInfo {
        make_info(false, false)
    }

    // 1. normalize_drops_self_message
    #[tokio::test]
    async fn normalize_drops_self_message() {
        let info = make_info(true, false);
        let msg = wa::Message::default();
        assert!(normalize_async(&msg, &info, None, None).await.is_none());
    }

    // 2. normalize_drops_group_message
    #[tokio::test]
    async fn normalize_drops_group_message() {
        let info = make_info(false, true);
        let msg = wa::Message::default();
        assert!(normalize_async(&msg, &info, None, None).await.is_none());
    }

    // 3. normalize_extracts_text_from_conversation
    #[tokio::test]
    async fn normalize_extracts_text_from_conversation() {
        let info = private_info();
        let mut msg = wa::Message::default();
        msg.conversation = Some("Hello, world!".into());
        let cm = normalize_async(&msg, &info, None, None)
            .await
            .expect("should produce Some");
        assert_eq!(cm.text, "Hello, world!");
        assert_eq!(cm.conversation_type, ConversationType::Private);
        assert_eq!(cm.msg_id, "MSG_TEST_1");
        assert_eq!(cm.sender_nick, "Alice");
    }

    // 4. normalize_extracts_text_from_extended_text
    #[tokio::test]
    async fn normalize_extracts_text_from_extended_text() {
        let info = private_info();
        let mut msg = wa::Message::default();
        let mut ext = wa::message::ExtendedTextMessage::default();
        ext.text = Some("Extended text here".into());
        msg.extended_text_message = Some(Box::new(ext));
        let cm = normalize_async(&msg, &info, None, None)
            .await
            .expect("should produce Some");
        assert_eq!(cm.text, "Extended text here");
    }

    // 5. normalize_image_caption_or_placeholder
    #[tokio::test]
    async fn normalize_image_caption_or_placeholder() {
        let info = private_info();

        // With caption
        let mut msg = wa::Message::default();
        let mut img = wa::message::ImageMessage::default();
        img.caption = Some("Look at this!".into());
        msg.image_message = Some(Box::new(img));
        let cm = normalize_async(&msg, &info, None, None)
            .await
            .expect("Some");
        assert_eq!(cm.text, "Look at this!");

        // Without caption → placeholder
        let mut msg2 = wa::Message::default();
        msg2.image_message = Some(Box::new(wa::message::ImageMessage::default()));
        let cm2 = normalize_async(&msg2, &info, None, None)
            .await
            .expect("Some");
        assert_eq!(cm2.text, "[图片]");
    }

    // 6. normalize_document_filename_and_caption
    #[tokio::test]
    async fn normalize_document_filename_and_caption() {
        let info = private_info();

        // file_name only, no caption
        let mut msg = wa::Message::default();
        let mut doc = wa::message::DocumentMessage::default();
        doc.file_name = Some("report.pdf".into());
        msg.document_message = Some(Box::new(doc));
        let cm = normalize_async(&msg, &info, None, None)
            .await
            .expect("Some");
        assert_eq!(cm.text, "[文件：report.pdf]");

        // file_name + caption
        let mut msg2 = wa::Message::default();
        let mut doc2 = wa::message::DocumentMessage::default();
        doc2.file_name = Some("report.pdf".into());
        doc2.caption = Some("Please review".into());
        msg2.document_message = Some(Box::new(doc2));
        let cm2 = normalize_async(&msg2, &info, None, None)
            .await
            .expect("Some");
        assert_eq!(cm2.text, "[文件：report.pdf] Please review");
    }

    // 7. normalize_voice_video_sticker_placeholders
    #[tokio::test]
    async fn normalize_voice_video_sticker_placeholders() {
        let info = private_info();

        let mut msg_audio = wa::Message::default();
        msg_audio.audio_message = Some(Box::new(wa::message::AudioMessage::default()));
        assert_eq!(
            normalize_async(&msg_audio, &info, None, None)
                .await
                .unwrap()
                .text,
            "[不支持的消息类型：语音]"
        );

        let mut msg_sticker = wa::Message::default();
        msg_sticker.sticker_message = Some(Box::new(wa::message::StickerMessage::default()));
        assert_eq!(
            normalize_async(&msg_sticker, &info, None, None)
                .await
                .unwrap()
                .text,
            "[不支持的消息类型：表情贴纸]"
        );

        let mut msg_loc = wa::Message::default();
        msg_loc.location_message = Some(Box::new(wa::message::LocationMessage::default()));
        assert_eq!(
            normalize_async(&msg_loc, &info, None, None)
                .await
                .unwrap()
                .text,
            "[不支持的消息类型：位置]"
        );
    }

    // 8. normalize_quoted_reply_prefix
    #[tokio::test]
    async fn normalize_quoted_reply_prefix() {
        let info = private_info();

        // Build a message with extended_text + context_info.quoted_message
        let mut quoted = wa::Message::default();
        quoted.conversation = Some("Original message".into());

        let mut ctx = wa::ContextInfo::default();
        ctx.quoted_message = Some(Box::new(quoted));

        let mut ext = wa::message::ExtendedTextMessage::default();
        ext.text = Some("This is my reply".into());
        ext.context_info = Some(Box::new(ctx));

        let mut msg = wa::Message::default();
        msg.extended_text_message = Some(Box::new(ext));

        let cm = normalize_async(&msg, &info, None, None)
            .await
            .expect("Some");
        assert_eq!(
            cm.text,
            "[引用了消息：\"Original message\"]\nThis is my reply"
        );
    }

    // 9. allow_from_filters_unlisted_sender
    #[tokio::test]
    async fn allow_from_filters_unlisted_sender() {
        let info = private_info(); // sender.user = "8613912345678"
        let msg = wa::Message {
            conversation: Some("hi".into()),
            ..Default::default()
        };
        let cfg = WhatsAppChannelConfig {
            schema_version: 1,
            jid: "bot@s.whatsapp.net".into(),
            push_name: "Bot".into(),
            paired_at: "2026-05-20T10:00:00Z".into(),
            allow_from: Some(vec!["+8613999999999".into()]), // different number
        };
        assert!(normalize_async(&msg, &info, Some(&cfg), None)
            .await
            .is_none());
    }

    // 10. allow_from_none_or_empty_passes_all
    #[tokio::test]
    async fn allow_from_none_or_empty_passes_all() {
        let info = private_info();
        let msg = wa::Message {
            conversation: Some("hi".into()),
            ..Default::default()
        };

        // None allow_from
        let cfg_none = WhatsAppChannelConfig {
            schema_version: 1,
            jid: "bot@s.whatsapp.net".into(),
            push_name: "Bot".into(),
            paired_at: "2026-05-20T10:00:00Z".into(),
            allow_from: None,
        };
        assert!(normalize_async(&msg, &info, Some(&cfg_none), None)
            .await
            .is_some());

        // Empty allow_from vec
        let cfg_empty = WhatsAppChannelConfig {
            allow_from: Some(vec![]),
            ..cfg_none
        };
        assert!(normalize_async(&msg, &info, Some(&cfg_empty), None)
            .await
            .is_some());
    }

    // 11. normalize_async_no_downloader_keeps_image_caption_in_text
    // Verifies that None downloader path does not affect IMAGE caption text
    // and leaves attachments empty.
    #[tokio::test]
    async fn normalize_async_no_downloader_keeps_image_caption_in_text() {
        let info = private_info();

        // Image with caption
        let mut msg = wa::Message::default();
        let mut img = wa::message::ImageMessage::default();
        img.caption = Some("look at this".into());
        msg.image_message = Some(Box::new(img));

        let cm = normalize_async(&msg, &info, None, None)
            .await
            .expect("should produce Some");
        assert_eq!(cm.text, "look at this");
        assert!(
            cm.attachments.is_empty(),
            "attachments must be empty when downloader is None"
        );

        // Image without caption → placeholder
        let mut msg2 = wa::Message::default();
        msg2.image_message = Some(Box::new(wa::message::ImageMessage::default()));
        let cm2 = normalize_async(&msg2, &info, None, None)
            .await
            .expect("Some");
        assert_eq!(cm2.text, "[图片]");
        assert!(cm2.attachments.is_empty());
    }
}
