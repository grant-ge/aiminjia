//! Telegram Update → connector-neutral `ChannelMessage` / pairing intent.
//!
//! 解析维度：
//! - `text == "/start"`：缺 code，返回 PairingStart
//! - `text == "/start <code>"`：返回 PairingStart::WithCode(code)
//! - 私聊文本消息：text → `ChannelMessage.text`
//! - 私聊图片消息：photo 数组取最大尺寸 → `ChannelAttachmentSpec { Picture, file_id }`
//! - 私聊文件消息：document → `ChannelAttachmentSpec { File, file_id }`
//! - text 或 caption 任一存在即可作为消息正文；纯 attachment 时正文为空
//! - 群聊 / 频道 / 缺 from / 都没有(text+caption+photo+document)：Skip

use super::api::{TgMessage, TgUpdate};
use crate::connector::im::types::{
    AttachmentKind, ChannelAttachmentSpec, ChannelMessage, ConversationType,
};

#[derive(Debug, Clone)]
#[allow(clippy::large_enum_variant)] // transient parse output, lifetime is short-lived
pub enum ParsedInbound {
    /// 普通消息（已经过 from / chat / payload 校验）
    Message {
        message: ChannelMessage,
        user_id: i64,
        first_name: String,
        username: Option<String>,
        chat_id: i64,
        message_id: i64,
    },
    /// `/start <code>`（pairing 入口）
    PairingStart {
        code: Option<String>,
        user_id: i64,
        first_name: String,
        username: Option<String>,
        chat_id: i64,
    },
    /// 收到了我们不支持的消息类型（voice/video/sticker 等），需要回提示。
    Unsupported {
        chat_id: i64,
        user_id: i64,
        message_id: i64,
        kind: UnsupportedKind,
    },
    /// 群聊 / 频道 / 缺字段 / bot 给 bot —— 上层忽略
    Skip(SkipReason),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UnsupportedKind {
    Voice,
    Audio,
    Video,
    VideoNote,
    Sticker,
    Animation,
}

impl UnsupportedKind {
    pub fn hint_text(&self) -> &'static str {
        match self {
            Self::Voice => "🤖 我暂不支持处理语音消息",
            Self::Audio => "🤖 我暂不支持处理音频文件",
            Self::Video | Self::VideoNote => "🤖 我暂不支持处理视频",
            Self::Sticker => "🤖 我暂不支持识别贴纸",
            Self::Animation => "🤖 我暂不支持处理动图",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SkipReason {
    NotPrivateChat,
    SenderMissing,
    SenderIsBot,
    /// 消息没有任何可处理 payload（text / caption / photo / document 都为空）。
    NoPayload,
}

/// 入口。bot_id 是当前 connector 的 bot id（用来构造 robot_code）。
pub fn parse_update(update: &TgUpdate, bot_id: &str) -> ParsedInbound {
    let Some(msg) = &update.message else {
        return ParsedInbound::Skip(SkipReason::NoPayload);
    };
    parse_message(msg, update.update_id, bot_id)
}

fn parse_message(msg: &TgMessage, update_id: i64, bot_id: &str) -> ParsedInbound {
    if msg.chat.chat_type != "private" {
        return ParsedInbound::Skip(SkipReason::NotPrivateChat);
    }
    let Some(from) = msg.from.as_ref() else {
        return ParsedInbound::Skip(SkipReason::SenderMissing);
    };
    if from.is_bot {
        return ParsedInbound::Skip(SkipReason::SenderIsBot);
    }

    // /start [code] —— 必须有 text 且以 `/start` 开头才走 pairing 路径
    if let Some(text) = msg.text.as_deref() {
        if let Some(rest) = text.strip_prefix("/start") {
            let code = rest.trim();
            let code = if code.is_empty() {
                None
            } else {
                Some(code.to_string())
            };
            return ParsedInbound::PairingStart {
                code,
                user_id: from.id,
                first_name: from.first_name.clone(),
                username: from.username.clone(),
                chat_id: msg.chat.id,
            };
        }
    }

    // Unsupported 类型检测：voice/audio/video/video_note/sticker/animation
    // 我们刻意不进 LLM 链路，只回提示给用户，避免"消息石沉大海"的体验。
    let unsupported_kind = if msg.voice.is_some() {
        Some(UnsupportedKind::Voice)
    } else if msg.audio.is_some() {
        Some(UnsupportedKind::Audio)
    } else if msg.video.is_some() {
        Some(UnsupportedKind::Video)
    } else if msg.video_note.is_some() {
        Some(UnsupportedKind::VideoNote)
    } else if msg.sticker.is_some() {
        Some(UnsupportedKind::Sticker)
    } else if msg.animation.is_some() {
        Some(UnsupportedKind::Animation)
    } else {
        None
    };
    if let Some(kind) = unsupported_kind {
        let _ = update_id;
        return ParsedInbound::Unsupported {
            chat_id: msg.chat.id,
            user_id: from.id,
            message_id: msg.message_id,
            kind,
        };
    }

    // 收集附件：photo / document
    let mut attachments: Vec<ChannelAttachmentSpec> = Vec::new();
    if let Some(photo_sizes) = &msg.photo {
        // Telegram 把同一张图按多个尺寸返回，数组末尾是最大尺寸 / 原图。
        // 只下载最大那张（避免下重复的缩略图）。
        if let Some(largest) = photo_sizes.last() {
            attachments.push(ChannelAttachmentSpec {
                kind: AttachmentKind::Picture,
                // download_code 借用来存 file_id（与 wecom/feishu 同字段语义；
                // downloader 用 get_file → file_path 拿真实下载 URL）
                download_code: largest.file_id.clone(),
                // Telegram photo 没有原始文件名，按时间戳合成
                file_name: format!("photo-{}.jpg", msg.message_id),
            });
        }
    }
    if let Some(doc) = &msg.document {
        let file_name = doc
            .file_name
            .clone()
            .unwrap_or_else(|| format!("file-{}.bin", msg.message_id));
        attachments.push(ChannelAttachmentSpec {
            kind: AttachmentKind::File,
            download_code: doc.file_id.clone(),
            file_name,
        });
    }

    // 消息正文：优先 text；其次 caption（图片/文件附带的说明）；都没有则为空
    let body = msg
        .text
        .as_deref()
        .or(msg.caption.as_deref())
        .unwrap_or("")
        .to_string();

    if body.is_empty() && attachments.is_empty() {
        return ParsedInbound::Skip(SkipReason::NoPayload);
    }

    let channel_msg = ChannelMessage {
        msg_id: format!("tg-{}-{}", bot_id, update_id),
        conversation_type: ConversationType::Private,
        conversation_key: msg.chat.id.to_string(),
        sender_id: from.id.to_string(),
        sender_nick: from.first_name.clone(),
        text: body,
        robot_code: bot_id.to_string(),
        reply_group_id: String::new(),
        attachments,
        session_webhook: None,
        created_at_ms: msg.date.map(|s| s * 1000),
    };
    ParsedInbound::Message {
        message: channel_msg,
        user_id: from.id,
        first_name: from.first_name.clone(),
        username: from.username.clone(),
        chat_id: msg.chat.id,
        message_id: msg.message_id,
    }
}

#[cfg(test)]
mod tests {
    use super::super::api::{TgDocument, TgPhotoSize};
    use super::*;

    fn user(id: i64, name: &str, bot: bool) -> super::super::api::TgUser {
        super::super::api::TgUser {
            id,
            is_bot: bot,
            first_name: name.into(),
            username: None,
        }
    }

    fn chat(id: i64, t: &str) -> super::super::api::TgChat {
        super::super::api::TgChat {
            id,
            chat_type: t.into(),
        }
    }

    fn empty_msg(
        id: i64,
        from: super::super::api::TgUser,
        chat: super::super::api::TgChat,
    ) -> TgMessage {
        TgMessage {
            message_id: id,
            from: Some(from),
            chat,
            text: None,
            date: None,
            photo: None,
            document: None,
            caption: None,
            voice: None,
            audio: None,
            video: None,
            video_note: None,
            sticker: None,
            animation: None,
        }
    }

    fn update_with_msg(id: i64, msg: TgMessage) -> TgUpdate {
        TgUpdate {
            update_id: id,
            message: Some(msg),
        }
    }

    #[test]
    fn private_text_message_parses_to_channel_message() {
        let mut msg = empty_msg(1, user(42, "Alice", false), chat(42, "private"));
        msg.text = Some("hello".into());
        msg.date = Some(1_700_000_000);
        match parse_update(&update_with_msg(100, msg), "BOT") {
            ParsedInbound::Message {
                message,
                user_id,
                chat_id,
                ..
            } => {
                assert_eq!(user_id, 42);
                assert_eq!(chat_id, 42);
                assert_eq!(message.text, "hello");
                assert_eq!(message.attachments.len(), 0);
                assert_eq!(message.msg_id, "tg-BOT-100");
                assert_eq!(message.created_at_ms, Some(1_700_000_000 * 1000));
            }
            other => panic!("expected Message, got {:?}", other),
        }
    }

    #[test]
    fn photo_message_picks_largest_size() {
        let mut msg = empty_msg(20, user(42, "Alice", false), chat(42, "private"));
        msg.photo = Some(vec![
            TgPhotoSize {
                file_id: "thumb".into(),
                file_unique_id: None,
                file_size: Some(737),
                width: Some(57),
                height: Some(90),
            },
            TgPhotoSize {
                file_id: "biggest".into(),
                file_unique_id: None,
                file_size: Some(94142),
                width: Some(812),
                height: Some(1280),
            },
        ]);
        match parse_update(&update_with_msg(101, msg), "BOT") {
            ParsedInbound::Message { message, .. } => {
                assert_eq!(message.attachments.len(), 1);
                assert_eq!(message.attachments[0].kind, AttachmentKind::Picture);
                assert_eq!(message.attachments[0].download_code, "biggest");
                assert_eq!(message.attachments[0].file_name, "photo-20.jpg");
                assert_eq!(message.text, ""); // no caption
            }
            other => panic!("expected Message, got {:?}", other),
        }
    }

    #[test]
    fn photo_with_caption_uses_caption_as_text() {
        let mut msg = empty_msg(21, user(42, "Alice", false), chat(42, "private"));
        msg.photo = Some(vec![TgPhotoSize {
            file_id: "fid".into(),
            file_unique_id: None,
            file_size: None,
            width: None,
            height: None,
        }]);
        msg.caption = Some("看这张图".into());
        match parse_update(&update_with_msg(102, msg), "BOT") {
            ParsedInbound::Message { message, .. } => {
                assert_eq!(message.text, "看这张图");
                assert_eq!(message.attachments.len(), 1);
            }
            other => panic!("expected Message, got {:?}", other),
        }
    }

    #[test]
    fn document_uses_file_name_when_present() {
        let mut msg = empty_msg(22, user(42, "Alice", false), chat(42, "private"));
        msg.document = Some(TgDocument {
            file_id: "doc-fid".into(),
            file_unique_id: None,
            file_name: Some("report.pdf".into()),
            mime_type: Some("application/pdf".into()),
            file_size: Some(12345),
        });
        match parse_update(&update_with_msg(103, msg), "BOT") {
            ParsedInbound::Message { message, .. } => {
                assert_eq!(message.attachments.len(), 1);
                assert_eq!(message.attachments[0].kind, AttachmentKind::File);
                assert_eq!(message.attachments[0].download_code, "doc-fid");
                assert_eq!(message.attachments[0].file_name, "report.pdf");
            }
            other => panic!("expected Message, got {:?}", other),
        }
    }

    #[test]
    fn document_falls_back_to_synthetic_name_when_missing() {
        let mut msg = empty_msg(23, user(42, "Alice", false), chat(42, "private"));
        msg.document = Some(TgDocument {
            file_id: "doc-fid".into(),
            file_unique_id: None,
            file_name: None,
            mime_type: None,
            file_size: None,
        });
        match parse_update(&update_with_msg(104, msg), "BOT") {
            ParsedInbound::Message { message, .. } => {
                assert_eq!(message.attachments[0].file_name, "file-23.bin");
            }
            other => panic!("expected Message, got {:?}", other),
        }
    }

    #[test]
    fn start_with_code_parses_to_pairing_with_code() {
        let mut msg = empty_msg(1, user(42, "Alice", false), chat(42, "private"));
        msg.text = Some("/start ABC123".into());
        match parse_update(&update_with_msg(100, msg), "BOT") {
            ParsedInbound::PairingStart {
                code,
                user_id,
                chat_id,
                ..
            } => {
                assert_eq!(code.as_deref(), Some("ABC123"));
                assert_eq!(user_id, 42);
                assert_eq!(chat_id, 42);
            }
            other => panic!("expected PairingStart, got {:?}", other),
        }
    }

    #[test]
    fn bare_start_parses_to_pairing_without_code() {
        let mut msg = empty_msg(1, user(42, "Alice", false), chat(42, "private"));
        msg.text = Some("/start".into());
        match parse_update(&update_with_msg(100, msg), "BOT") {
            ParsedInbound::PairingStart { code, .. } => assert!(code.is_none()),
            other => panic!("expected PairingStart, got {:?}", other),
        }
    }

    #[test]
    fn group_message_skipped() {
        let mut msg = empty_msg(1, user(42, "Alice", false), chat(-100, "group"));
        msg.text = Some("hi".into());
        assert!(matches!(
            parse_update(&update_with_msg(100, msg), "BOT"),
            ParsedInbound::Skip(SkipReason::NotPrivateChat)
        ));
    }

    #[test]
    fn bot_sender_skipped() {
        let mut msg = empty_msg(1, user(42, "Botty", true), chat(42, "private"));
        msg.text = Some("hi".into());
        assert!(matches!(
            parse_update(&update_with_msg(100, msg), "BOT"),
            ParsedInbound::Skip(SkipReason::SenderIsBot)
        ));
    }

    #[test]
    fn no_payload_skipped() {
        let msg = empty_msg(1, user(42, "Alice", false), chat(42, "private"));
        assert!(matches!(
            parse_update(&update_with_msg(100, msg), "BOT"),
            ParsedInbound::Skip(SkipReason::NoPayload)
        ));
    }

    mod unsupported_tests {
        use super::super::super::api::{
            TgAnimation, TgAudio, TgSticker, TgVideo, TgVideoNote, TgVoice,
        };
        use super::*;

        #[test]
        fn voice_message_returns_unsupported_voice() {
            let mut msg = empty_msg(30, user(42, "Alice", false), chat(42, "private"));
            msg.voice = Some(TgVoice {
                duration: Some(5),
                file_size: Some(2048),
            });
            match parse_update(&update_with_msg(200, msg), "BOT") {
                ParsedInbound::Unsupported {
                    kind,
                    chat_id,
                    user_id,
                    message_id,
                } => {
                    assert_eq!(kind, UnsupportedKind::Voice);
                    assert_eq!(chat_id, 42);
                    assert_eq!(user_id, 42);
                    assert_eq!(message_id, 30);
                }
                other => panic!("expected Unsupported(Voice), got {:?}", other),
            }
        }

        #[test]
        fn audio_returns_unsupported_audio() {
            let mut msg = empty_msg(31, user(42, "Alice", false), chat(42, "private"));
            msg.audio = Some(TgAudio {
                duration: Some(180),
                file_size: Some(1024),
            });
            assert!(matches!(
                parse_update(&update_with_msg(201, msg), "BOT"),
                ParsedInbound::Unsupported {
                    kind: UnsupportedKind::Audio,
                    ..
                }
            ));
        }

        #[test]
        fn video_returns_unsupported_video() {
            let mut msg = empty_msg(32, user(42, "Alice", false), chat(42, "private"));
            msg.video = Some(TgVideo {
                duration: Some(10),
                file_size: None,
            });
            assert!(matches!(
                parse_update(&update_with_msg(202, msg), "BOT"),
                ParsedInbound::Unsupported {
                    kind: UnsupportedKind::Video,
                    ..
                }
            ));
        }

        #[test]
        fn video_note_returns_unsupported_video_note() {
            let mut msg = empty_msg(33, user(42, "Alice", false), chat(42, "private"));
            msg.video_note = Some(TgVideoNote {
                duration: Some(3),
                file_size: None,
            });
            assert!(matches!(
                parse_update(&update_with_msg(203, msg), "BOT"),
                ParsedInbound::Unsupported {
                    kind: UnsupportedKind::VideoNote,
                    ..
                }
            ));
        }

        #[test]
        fn sticker_returns_unsupported_sticker() {
            let mut msg = empty_msg(34, user(42, "Alice", false), chat(42, "private"));
            msg.sticker = Some(TgSticker {
                emoji: Some("😀".into()),
                set_name: None,
            });
            assert!(matches!(
                parse_update(&update_with_msg(204, msg), "BOT"),
                ParsedInbound::Unsupported {
                    kind: UnsupportedKind::Sticker,
                    ..
                }
            ));
        }

        #[test]
        fn animation_returns_unsupported_animation() {
            let mut msg = empty_msg(35, user(42, "Alice", false), chat(42, "private"));
            msg.animation = Some(TgAnimation {
                duration: Some(2),
                file_size: None,
            });
            assert!(matches!(
                parse_update(&update_with_msg(205, msg), "BOT"),
                ParsedInbound::Unsupported {
                    kind: UnsupportedKind::Animation,
                    ..
                }
            ));
        }

        #[test]
        fn hint_text_returns_non_empty_string_for_each_kind() {
            for kind in [
                UnsupportedKind::Voice,
                UnsupportedKind::Audio,
                UnsupportedKind::Video,
                UnsupportedKind::VideoNote,
                UnsupportedKind::Sticker,
                UnsupportedKind::Animation,
            ] {
                assert!(!kind.hint_text().is_empty());
            }
        }
    }
}
