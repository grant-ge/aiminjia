use app_lib::connector::im::wecom::aibot_protocol::*;
use app_lib::connector::im::wecom::parser::{parse_inbound, ParsedInbound};
use serde_json::json;

fn frame_with_body(body: serde_json::Value) -> WsFrame<serde_json::Value> {
    serde_json::from_value(json!({
        "cmd": "aibot_msg_callback",
        "headers": { "req_id": "REQ" },
        "body": body
    }))
    .unwrap()
}

#[test]
fn text_single_chat_maps_to_channel_message_with_robot_code_and_reply_group() {
    let frame = frame_with_body(json!({
        "msgid": "M1", "aibotid": "BOTID", "chattype": "single",
        "from": { "userid": "U1" }, "msgtype": "text",
        "text": { "content": "hello" }
    }));
    let parsed = parse_inbound("BOTID", &frame).expect("must parse");
    let msg = match parsed {
        ParsedInbound::Message(m) => m,
        _ => panic!(),
    };
    assert_eq!(msg.text, "hello");
    assert_eq!(msg.robot_code, "BOTID", "robot_code <- bot_id");
    assert_eq!(
        msg.reply_group_id, "U1",
        "single chat reply_group_id <- userid"
    );
    assert!(matches!(
        msg.conversation_type,
        app_lib::connector::im::types::ConversationType::Private
    ));
    assert_eq!(msg.sender_id, "U1");
    assert_eq!(msg.msg_id, "M1");
    assert!(
        msg.session_webhook.is_none(),
        "aibot has no session webhook concept"
    );
    assert!(msg.attachments.is_empty());
}

#[test]
fn text_group_chat_uses_chatid_for_reply_group() {
    let frame = frame_with_body(json!({
        "msgid": "M2", "aibotid": "BOTID", "chatid": "GROUP_1", "chattype": "group",
        "from": { "userid": "U2" }, "msgtype": "text",
        "text": { "content": "hi" }
    }));
    let msg = match parse_inbound("BOTID", &frame).unwrap() {
        ParsedInbound::Message(m) => m,
        _ => panic!(),
    };
    assert_eq!(
        msg.reply_group_id, "GROUP_1",
        "group chat reply_group_id <- chatid"
    );
    assert!(matches!(
        msg.conversation_type,
        app_lib::connector::im::types::ConversationType::Group
    ));
}

#[test]
fn image_message_emits_attachment_with_encoded_download_code() {
    let frame = frame_with_body(json!({
        "msgid": "M3", "aibotid": "BOTID", "chattype": "single",
        "from": { "userid": "U" }, "msgtype": "image",
        "image": { "url": "https://example.com/file?id=abc", "aeskey": "KEY1" }
    }));
    let msg = match parse_inbound("BOTID", &frame).unwrap() {
        ParsedInbound::Message(m) => m,
        _ => panic!(),
    };
    assert_eq!(msg.attachments.len(), 1);
    let att = &msg.attachments[0];
    use app_lib::connector::im::types::AttachmentKind;
    assert!(matches!(att.kind, AttachmentKind::Picture));
    // download_code 用 "wecom://{aeskey}@{url}" 形式承载，后续 media.rs 还原
    assert!(att.download_code.starts_with("wecom://KEY1@"));
    assert!(att
        .download_code
        .contains("https://example.com/file?id=abc"));
}

#[test]
fn file_message_emits_file_attachment() {
    let frame = frame_with_body(json!({
        "msgid": "M4", "aibotid": "BOTID", "chattype": "single",
        "from": { "userid": "U" }, "msgtype": "file",
        "file": { "url": "https://example.com/f", "aeskey": "K" }
    }));
    let msg = match parse_inbound("BOTID", &frame).unwrap() {
        ParsedInbound::Message(m) => m,
        _ => panic!(),
    };
    use app_lib::connector::im::types::AttachmentKind;
    assert!(matches!(msg.attachments[0].kind, AttachmentKind::File));
}

#[test]
fn voice_video_mixed_returns_ignored() {
    for mt in ["voice", "video", "mixed"] {
        let frame = frame_with_body(json!({
            "msgid": "M", "aibotid": "BOTID", "chattype": "single",
            "from": { "userid": "U" }, "msgtype": mt,
        }));
        let parsed = parse_inbound("BOTID", &frame);
        assert!(
            matches!(parsed, Some(ParsedInbound::Ignored)),
            "{mt} should be Ignored"
        );
    }
}

#[test]
fn event_callback_is_not_routed_through_parse_inbound() {
    // 事件帧不经过 parse_inbound（由 connector.rs 单独路由）
    let frame = serde_json::from_value::<WsFrame<serde_json::Value>>(json!({
        "cmd": "aibot_event_callback",
        "headers": { "req_id": "R" },
        "body": { "msgid": "E", "aibotid": "B", "from": { "userid": "U" }, "msgtype": "event",
                  "event": { "eventtype": "enter_chat" } }
    }))
    .unwrap();
    assert!(parse_inbound("BOTID", &frame).is_none());
}
