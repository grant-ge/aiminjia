//! Adapter: turn an inbound IM message + downloaded attachments into a `PendingItem`.
//!
//! Per-platform helpers (`build_pending_item_from_dingtalk` /
//! `build_pending_item_from_feishu`) only differ by `PendingSource` tag —
//! the rest of the conversion (sender_nick policy, failure tail, attachment
//! mapping, id generation) is the same and lives in `build_pending_item_inner`.

use crate::runtime::chat::chat_turn_driver::ChatAttachmentRef;
use crate::runtime::pending::{PendingAttachment, PendingItem, PendingSource};

use super::super::types::ConversationType;

/// Build a `PendingItem` from a downloaded DingTalk message.
///
/// - `sender_nick` is preserved as-is for group chats (used as the `[sender]:`
///   prefix at drain time), set to `None` for private chats.
/// - Attachments are converted; `mime_type` and `file_size` are passed through.
pub fn build_pending_item_from_dingtalk(
    _msg_id: &str,
    conv_type: &ConversationType,
    sender_nick: &str,
    text: &str,
    attachments: Vec<ChatAttachmentRef>,
    download_failures: &[String],
) -> PendingItem {
    build_pending_item_inner(
        PendingSource::ImDingtalk,
        conv_type,
        sender_nick,
        text,
        attachments,
        download_failures,
    )
}

/// Build a `PendingItem` from a downloaded Feishu message.
///
/// Same shape as `build_pending_item_from_dingtalk` — only `source` differs.
/// `sender_nick` here is the render produced by `render_feishu_sender_nick`
/// in `manager.rs` (e.g. `飞书用户 ou_abcdef12`); preserved verbatim for
/// group chats, dropped for private chats.
pub fn build_pending_item_from_feishu(
    _msg_id: &str,
    conv_type: &ConversationType,
    sender_nick: &str,
    text: &str,
    attachments: Vec<ChatAttachmentRef>,
    download_failures: &[String],
) -> PendingItem {
    build_pending_item_inner(
        PendingSource::ImFeishu,
        conv_type,
        sender_nick,
        text,
        attachments,
        download_failures,
    )
}

/// Build a `PendingItem` from a Wecom message.
///
/// Same shape as `build_pending_item_from_feishu` — only `source` differs.
pub fn build_pending_item_from_wecom(
    _msg_id: &str,
    conv_type: &ConversationType,
    sender_nick: &str,
    text: &str,
    attachments: Vec<ChatAttachmentRef>,
    download_failures: &[String],
) -> PendingItem {
    build_pending_item_inner(
        PendingSource::ImWecom,
        conv_type,
        sender_nick,
        text,
        attachments,
        download_failures,
    )
}

/// Build a `PendingItem` from a Telegram message.
///
/// Same shape as `build_pending_item_from_wecom` — only `source` differs.
/// Telegram MVP has no attachment support, so `attachments` / `download_failures`
/// are always empty in the production path; the parameter signature is kept
/// uniform with the other platforms for future-proofing.
pub fn build_pending_item_from_telegram(
    _msg_id: &str,
    conv_type: &ConversationType,
    sender_nick: &str,
    text: &str,
    attachments: Vec<ChatAttachmentRef>,
    download_failures: &[String],
) -> PendingItem {
    build_pending_item_inner(
        PendingSource::ImTelegram,
        conv_type,
        sender_nick,
        text,
        attachments,
        download_failures,
    )
}

fn build_pending_item_inner(
    source: PendingSource,
    conv_type: &ConversationType,
    sender_nick: &str,
    text: &str,
    attachments: Vec<ChatAttachmentRef>,
    download_failures: &[String],
) -> PendingItem {
    let nick = match conv_type {
        ConversationType::Group => Some(sender_nick.to_string()),
        ConversationType::Private => None,
    };
    let body = if download_failures.is_empty() {
        text.to_string()
    } else {
        format!(
            "{}\n[注意：以下附件下载失败，未能加载：{}]",
            text,
            download_failures.join(", ")
        )
    };
    let pending_attachments: Vec<PendingAttachment> = attachments
        .into_iter()
        .map(|a| PendingAttachment {
            id: a.id,
            file_path: a.file_path,
            mime: a.mime_type,
            size_bytes: Some(a.file_size),
        })
        .collect();
    PendingItem {
        id: format!("pend-{}", uuid::Uuid::new_v4()),
        source,
        text: body,
        sender_nick: nick,
        attachments: pending_attachments,
        skill_command: None,
        received_at: chrono::Utc::now().to_rfc3339(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn att(id: &str) -> ChatAttachmentRef {
        ChatAttachmentRef {
            id: id.into(),
            file_name: format!("{id}.png"),
            file_path: format!("/tmp/{id}.png"),
            kind: "image".into(),
            file_size: 100,
            file_type: "png".into(),
            mime_type: Some("image/png".into()),
        }
    }

    #[test]
    fn group_chat_carries_sender_nick() {
        let item = build_pending_item_from_dingtalk(
            "m-1",
            &ConversationType::Group,
            "张三",
            "hello",
            vec![att("a")],
            &[],
        );
        assert_eq!(item.source, PendingSource::ImDingtalk);
        assert_eq!(item.sender_nick.as_deref(), Some("张三"));
        assert_eq!(item.text, "hello");
        assert_eq!(item.attachments.len(), 1);
        assert_eq!(item.attachments[0].id, "a");
        assert_eq!(item.attachments[0].mime.as_deref(), Some("image/png"));
    }

    #[test]
    fn private_chat_omits_sender_nick() {
        let item = build_pending_item_from_dingtalk(
            "m-2",
            &ConversationType::Private,
            "李四",
            "hi",
            vec![],
            &[],
        );
        assert!(item.sender_nick.is_none());
    }

    #[test]
    fn download_failures_appended_to_text() {
        let item = build_pending_item_from_dingtalk(
            "m-3",
            &ConversationType::Private,
            "x",
            "hello",
            vec![],
            &["a.docx".into()],
        );
        assert!(item.text.contains("hello"));
        assert!(item.text.contains("a.docx"));
        assert!(item.text.contains("下载失败"));
    }

    #[test]
    fn item_id_has_pend_prefix() {
        let item = build_pending_item_from_dingtalk(
            "m-4",
            &ConversationType::Private,
            "",
            "",
            vec![],
            &[],
        );
        assert!(item.id.starts_with("pend-"));
    }

    // ----- feishu variant ------------------------------------------------

    #[test]
    fn feishu_group_chat_carries_sender_nick() {
        let item = build_pending_item_from_feishu(
            "om-1",
            &ConversationType::Group,
            "飞书用户 ou_abcdef12",
            "hello from lark",
            vec![att("img_v2_001")],
            &[],
        );
        assert_eq!(item.source, PendingSource::ImFeishu);
        assert_eq!(item.sender_nick.as_deref(), Some("飞书用户 ou_abcdef12"));
        assert_eq!(item.text, "hello from lark");
        assert_eq!(item.attachments.len(), 1);
        assert_eq!(item.attachments[0].id, "img_v2_001");
        assert_eq!(item.attachments[0].mime.as_deref(), Some("image/png"));
    }

    #[test]
    fn feishu_private_chat_omits_sender_nick() {
        let item = build_pending_item_from_feishu(
            "om-2",
            &ConversationType::Private,
            "飞书用户 ou_short",
            "hi",
            vec![],
            &[],
        );
        assert_eq!(item.source, PendingSource::ImFeishu);
        assert!(item.sender_nick.is_none());
    }

    #[test]
    fn feishu_download_failures_appended_to_text() {
        let item = build_pending_item_from_feishu(
            "om-3",
            &ConversationType::Private,
            "x",
            "hello",
            vec![],
            &["a.pdf".into(), "b.docx".into()],
        );
        assert!(item.text.contains("hello"));
        assert!(item.text.contains("a.pdf"));
        assert!(item.text.contains("b.docx"));
        assert!(item.text.contains("下载失败"));
    }

    #[test]
    fn feishu_item_id_has_pend_prefix() {
        let item =
            build_pending_item_from_feishu("om-4", &ConversationType::Private, "", "", vec![], &[]);
        assert!(item.id.starts_with("pend-"));
    }
}
