//! 黄金路径测试：帧 serde 圆环（serialize → parse 还原相等）+ 真实样例向量。
//!
//! 样例向量来自 `@wecom/aibot-node-sdk@1.0.7` 类型定义 `dist/index.d.ts`
//! 注释中的协议示例。

use app_lib::connector::im::wecom::aibot_protocol::*;
use serde_json::json;

#[test]
fn subscribe_frame_serializes_to_expected_shape() {
    let frame = WsFrame::<SubscribeBody> {
        cmd: Some(WsCmd::Subscribe),
        headers: FrameHeaders {
            req_id: "abc-123".into(),
            extra: Default::default(),
        },
        body: Some(SubscribeBody {
            secret: "S".into(),
            bot_id: "B".into(),
        }),
        errcode: None,
        errmsg: None,
    };
    let v = serde_json::to_value(&frame).unwrap();
    assert_eq!(v["cmd"], "aibot_subscribe");
    assert_eq!(v["headers"]["req_id"], "abc-123");
    assert_eq!(v["body"]["secret"], "S");
    assert_eq!(v["body"]["bot_id"], "B");
    assert!(
        v.get("errcode").is_none(),
        "errcode must be skipped when None"
    );
}

#[test]
fn ping_frame_serializes_without_body() {
    let frame = WsFrame::<serde_json::Value> {
        cmd: Some(WsCmd::Ping),
        headers: FrameHeaders {
            req_id: "ping-1".into(),
            extra: Default::default(),
        },
        body: None,
        errcode: None,
        errmsg: None,
    };
    let v = serde_json::to_value(&frame).unwrap();
    assert_eq!(v["cmd"], "ping");
    assert!(v.get("body").is_none(), "body must be skipped when None");
}

#[test]
fn ack_frame_parses_without_cmd() {
    // 认证 / 心跳 ack：{ headers: { req_id }, errcode: 0, errmsg: "ok" }
    let raw = json!({
        "headers": { "req_id": "abc-123" },
        "errcode": 0,
        "errmsg": "ok"
    });
    let frame: WsFrame<serde_json::Value> = serde_json::from_value(raw).unwrap();
    assert!(frame.cmd.is_none());
    assert_eq!(frame.headers.req_id, "abc-123");
    assert_eq!(frame.errcode, Some(0));
    assert_eq!(frame.errmsg.as_deref(), Some("ok"));
}

#[test]
fn inbound_text_message_parses() {
    // 真实样例（构造）：用户在单聊发"hello"
    let raw = json!({
        "cmd": "aibot_msg_callback",
        "headers": { "req_id": "req-xyz" },
        "body": {
            "msgid": "MSGID_1",
            "aibotid": "BOTID",
            "chattype": "single",
            "from": { "userid": "U1" },
            "msgtype": "text",
            "create_time": 1700000000,
            "text": { "content": "hello" }
        }
    });
    let frame: WsFrame<InboundMessageBody> = serde_json::from_value(raw).unwrap();
    assert_eq!(frame.cmd, Some(WsCmd::MsgCallback));
    let b = frame.body.unwrap();
    assert_eq!(b.msgid, "MSGID_1");
    assert_eq!(b.aibotid, "BOTID");
    assert!(b.chatid.is_none(), "single chat has no chatid");
    assert!(matches!(b.chattype, ChatType::Single));
    assert_eq!(b.from.userid, "U1");
    assert_eq!(b.msgtype, "text");
    assert_eq!(b.payload["text"]["content"], "hello");
}

#[test]
fn inbound_image_message_keeps_aeskey_in_payload() {
    let raw = json!({
        "cmd": "aibot_msg_callback",
        "headers": { "req_id": "req-1" },
        "body": {
            "msgid": "M2",
            "aibotid": "B",
            "chatid": "GROUP_1",
            "chattype": "group",
            "from": { "userid": "U2" },
            "msgtype": "image",
            "image": {
                "url": "https://example.com/file",
                "aeskey": "AAAAAA"
            }
        }
    });
    let frame: WsFrame<InboundMessageBody> = serde_json::from_value(raw).unwrap();
    let b = frame.body.unwrap();
    assert_eq!(b.chatid.as_deref(), Some("GROUP_1"));
    assert!(matches!(b.chattype, ChatType::Group));
    assert_eq!(b.payload["image"]["url"], "https://example.com/file");
    assert_eq!(b.payload["image"]["aeskey"], "AAAAAA");
}

#[test]
fn event_callback_with_disconnected_event_parses() {
    let raw = json!({
        "cmd": "aibot_event_callback",
        "headers": { "req_id": "req-evt" },
        "body": {
            "msgid": "EVT1",
            "aibotid": "B",
            "create_time": 1700000001,
            "from": { "userid": "U1" },
            "msgtype": "event",
            "event": { "eventtype": "disconnected_event" }
        }
    });
    let frame: WsFrame<EventCallbackBody> = serde_json::from_value(raw).unwrap();
    assert_eq!(frame.cmd, Some(WsCmd::EventCallback));
    let b = frame.body.unwrap();
    assert!(matches!(b.event.eventtype, EventType::Disconnected));
}

#[test]
fn respond_markdown_body_serializes_with_fixed_msgtype() {
    let body = RespondMarkdownBody::new("# hi\n**bold**");
    let v = serde_json::to_value(&body).unwrap();
    assert_eq!(v["msgtype"], "markdown");
    assert_eq!(v["markdown"]["content"], "# hi\n**bold**");
}

#[test]
fn send_msg_body_markdown_includes_chatid() {
    let body = SendMsgBody::markdown("CHAT_1".into(), "hello".into());
    let v = serde_json::to_value(&body).unwrap();
    assert_eq!(v["chatid"], "CHAT_1");
    assert_eq!(v["msgtype"], "markdown");
    assert_eq!(v["markdown"]["content"], "hello");
}

#[test]
fn generate_req_id_format() {
    let id = generate_req_id("aibot_subscribe");
    let parts: Vec<&str> = id.split('_').collect();
    assert!(
        parts.len() >= 4,
        "format: {{prefix}}_{{ts}}_{{rand}}, got {id}"
    );

    let rand_part = parts.last().unwrap();
    assert_eq!(
        rand_part.len(),
        8,
        "random suffix must be 8 chars, got {rand_part:?}"
    );
    assert!(
        rand_part
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit()),
        "random suffix must be lowercase alphanumeric, got {rand_part:?}"
    );

    let ts_part = parts[parts.len() - 2];
    ts_part
        .parse::<u128>()
        .unwrap_or_else(|e| panic!("ts segment {ts_part:?} not u128: {e}"));

    let prefix_recovered = parts[..parts.len() - 2].join("_");
    assert_eq!(prefix_recovered, "aibot_subscribe");
}
