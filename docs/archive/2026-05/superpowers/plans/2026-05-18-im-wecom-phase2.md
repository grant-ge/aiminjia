# 企微 IM Connector（aibot WebSocket）实施计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 在 `src-tauri/src/connector/im/wecom/` 落地企微 IMConnector 实现，入站走腾讯官方 aibot WebSocket 长连接（`wss://openws.work.weixin.qq.com`），出站走 markdown / send_msg / 媒体上传，附带在 `im/shared/` 抽出"流式不支持"通用降级 buffer。

**Architecture:** 桌面 app 主动外连 aibot WebSocket（结构对标现有 `dingtalk/stream.rs`），bot_id+secret 静态凭证认证。协议层 `aibot_protocol.rs` 定义所有 JSON 帧 + serde；连接层 `aibot_client.rs` 管心跳/重连/串行 ack；适配层 `parser/sender/media.rs` 把 aibot 帧映射到 trait 中性 `ChannelMessage` / `ReplyTarget`。流式 AI 卡片本期不接，capabilities 声明 `outbound_aicard: false` 走 `shared::AiCardFallbackBuffer` 累积到 final 再发一条 markdown。

**Tech Stack:** Rust (tokio + tokio-tungstenite 0.26 + serde + aes-cbc) / async-trait / mpsc / CancellationToken / 现有 `connector::im::trait_def::IMConnector`

**Spec:** `docs/superpowers/specs/2026-05-18-im-wecom-phase2-design.md`

**Reference impl:** `@wecom/aibot-node-sdk@1.0.7` MIT (`/tmp/aibot-sdk/package/dist/*.d.ts`) + openclaw TS (`~/Downloads/openclaw channel/wecom-openclaw-plugin-main/`)

---

## 现实约束（开工前必读）

1. **`ChannelMessage` schema 含 dingtalk 专属字段**（`robot_code` / `reply_group_id` / `session_webhook` / `ChannelAttachmentSpec.download_code`），本 plan **不重构 schema**，wecom 借字段填值：
   - `robot_code` ← bot_id（账号唯一标识，manager 仍能按 bot 维度分组）
   - `reply_group_id` ← chatid（群）或 userid（单聊）
   - `session_webhook` ← `None`（aibot 不用 webhook URL 概念）
   - `ChannelAttachmentSpec.download_code` ← 拼成 `wecom://{aeskey_b64}@{url}` 形式，由 wecom/media.rs 解析时再拆开
   - 重构留待"trait 平台化"专项（不属于本期）

2. **`coming_soon_state(Platform::Wecom)` 已在 `shared/config_store.rs:69` 占位**，本 plan PR6 会把它替换成 active state（参考 dingtalk / feishu 既有处理）

3. **Cargo 依赖**：tokio-tungstenite 0.26 / base64 0.22 / reqwest 0.12 / sha2 0.10 已就绪；本 plan PR4 会引入 `aes = "0.8"` + `cbc = "0.1"`（aibot 文件下载 aeskey 用 AES-256-CBC + PKCS7 解密；现有 `aes-gcm` 不能用）

4. **不修 `IMConnector` trait**、不动 `ChannelMessage` / `ReplyTarget` / `ConnectorContext`——所有适配在 wecom 模块内做

---

## 文件结构

```
src-tauri/Cargo.toml                                  # 新增 aes, cbc 依赖（PR4）
src-tauri/src/connector/im/
├── shared/
│   ├── mod.rs                                        # PR3: pub mod aicard_fallback;
│   └── aicard_fallback.rs                            # PR3: 通用流式降级 buffer
├── wecom/                                            # PR1-PR5 全部新增
│   ├── mod.rs                                        # 模块导出
│   ├── aibot_protocol.rs                             # PR1: WS 帧类型 + serde
│   ├── aibot_client.rs                               # PR2: 连接/心跳/重连/ack 队列
│   ├── parser.rs                                     # PR4: InboundMessageBody → ChannelMessage
│   ├── sender.rs                                     # PR4: 出站包装
│   ├── media.rs                                      # PR4: 媒体上传/下载/解密
│   └── connector.rs                                  # PR5: impl IMConnector
├── factory.rs                                        # PR5: 注册 Platform::Wecom → WecomConnector::new
└── mod.rs                                            # PR5: pub mod wecom;
src-tauri/src/connector/im/shared/config_store.rs    # PR5/PR6: 移除 wecom coming_soon、加 wecom CRUD
src-tauri/src/transport/tauri_commands/channel.rs    # PR6: 加 channel_wecom_save / channel_wecom_test_connection
src/lib/tauri.ts                                      # PR6: 前端 IPC binding 增 wecom 命令
src/components/channels/WecomAccountForm.tsx          # PR6: 添加企微账号表单
src/i18n/{zh-CN,en-US}.json                          # PR6: channels.wecom.* 文案

src-tauri/tests/im_wecom_aibot_protocol.rs           # PR1
src-tauri/tests/im_wecom_aibot_client.rs             # PR2
src-tauri/tests/im_aicard_fallback.rs                # PR3
src-tauri/tests/im_wecom_parser.rs                   # PR4
src-tauri/tests/im_wecom_sender.rs                   # PR4
src-tauri/tests/im_wecom_media.rs                    # PR4
src-tauri/tests/im_wecom_integration.rs              # PR5
src-tauri/tests/review_im_layering.rs                # PR5: platforms 数组 + "wecom"
```

---

## Task 1 (PR1): aibot WebSocket 协议帧编解码

**Files:**
- Create: `src-tauri/src/connector/im/wecom/mod.rs`
- Create: `src-tauri/src/connector/im/wecom/aibot_protocol.rs`
- Modify: `src-tauri/src/connector/im/mod.rs` (export wecom 模块但**不在 Phase 0 trait 注册**——本 PR 只是孤立模块)
- Create: `src-tauri/tests/im_wecom_aibot_protocol.rs`

### Step 1.1: 准备模块骨架

- [ ] **Step 1.1.1: 新建 wecom/mod.rs**

Write `src-tauri/src/connector/im/wecom/mod.rs`:
```rust
//! 企业微信智能机器人（aibot）IM connector。
//!
//! 入站走腾讯官方 aibot WebSocket 长连接（`wss://openws.work.weixin.qq.com`）。
//! 协议参考：`@wecom/aibot-node-sdk@1.0.6+` MIT 开源 SDK。
//!
//! See `docs/superpowers/specs/2026-05-18-im-wecom-phase2-design.md`.

pub mod aibot_protocol;
```

- [ ] **Step 1.1.2: 在 connector/im/mod.rs 暴露 wecom 模块**

Edit `src-tauri/src/connector/im/mod.rs` 加 `pub mod wecom;`（位置紧跟 `pub mod feishu;` 或字母序）。

- [ ] **Step 1.1.3: 验证编译**

Run: `cd src-tauri && cargo check`
Expected: PASS（空模块不影响）

- [ ] **Step 1.1.4: Commit**

```bash
git add src-tauri/src/connector/im/wecom/mod.rs src-tauri/src/connector/im/mod.rs
git commit -m "feat(connector/im): scaffold wecom module (Phase 2 PR1)"
```

### Step 1.2: 写帧类型测试（fail）

- [ ] **Step 1.2.1: 创建���成测试文件**

Write `src-tauri/tests/im_wecom_aibot_protocol.rs`:
```rust
//! 圣经测试：帧 serde 圆环（serialize → parse 还原相等）+ 真实样例向量。
//!
//! 样例向量来自 `@wecom/aibot-node-sdk@1.0.7` 类型定义 `dist/index.d.ts`
//! 注释中的协议示例。

use app_lib::connector::im::wecom::aibot_protocol::*;
use serde_json::json;

#[test]
fn subscribe_frame_serializes_to_expected_shape() {
    let frame = WsFrame::<SubscribeBody> {
        cmd: Some(WsCmd::Subscribe),
        headers: FrameHeaders { req_id: "abc-123".into(), extra: Default::default() },
        body: Some(SubscribeBody { secret: "S".into(), bot_id: "B".into() }),
        errcode: None,
        errmsg: None,
    };
    let v = serde_json::to_value(&frame).unwrap();
    assert_eq!(v["cmd"], "aibot_subscribe");
    assert_eq!(v["headers"]["req_id"], "abc-123");
    assert_eq!(v["body"]["secret"], "S");
    assert_eq!(v["body"]["bot_id"], "B");
    assert!(v.get("errcode").is_none(), "errcode must be skipped when None");
}

#[test]
fn ping_frame_serializes_without_body() {
    let frame = WsFrame::<serde_json::Value> {
        cmd: Some(WsCmd::Ping),
        headers: FrameHeaders { req_id: "ping-1".into(), extra: Default::default() },
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
    assert!(id.starts_with("aibot_subscribe_"), "prefix must lead, got {id}");
    let parts: Vec<&str> = id.split('_').collect();
    // {prefix=2 parts}_{ms_timestamp}_{8-char-random}
    assert!(parts.len() >= 4, "format: prefix_ts_rand, got {id}");
}
```

- [ ] **Step 1.2.2: 跑测试确认全部 fail（模块不存在）**

Run: `cd src-tauri && cargo test --test im_wecom_aibot_protocol 2>&1 | head -20`
Expected: 编译失败，错误是 `module wecom::aibot_protocol does not exist` 或类似（因为 aibot_protocol.rs 还没创建）

### Step 1.3: 实现 aibot_protocol.rs（最小通过）

- [ ] **Step 1.3.1: 创建 aibot_protocol.rs**

Write `src-tauri/src/connector/im/wecom/aibot_protocol.rs`:
```rust
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
    #[serde(rename = "aibot_subscribe")]       Subscribe,
    #[serde(rename = "ping")]                  Ping,
    #[serde(rename = "aibot_respond_msg")]     Respond,
    #[serde(rename = "aibot_send_msg")]        SendMsg,
    #[serde(rename = "aibot_msg_callback")]    MsgCallback,
    #[serde(rename = "aibot_event_callback")]  EventCallback,
    #[serde(rename = "aibot_upload_media_init")]   UploadInit,
    #[serde(rename = "aibot_upload_media_chunk")]  UploadChunk,
    #[serde(rename = "aibot_upload_media_finish")] UploadFinish,
}

/// 通用 WS 帧结构。
#[derive(Debug, Clone, Serialize, Deserialize)]
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
    pub msgtype: String,    // 恒等于 "event"
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
    #[serde(rename = "enter_chat")]              EnterChat,
    #[serde(rename = "template_card_event")]     TemplateCardEvent,
    #[serde(rename = "feedback_event")]          FeedbackEvent,
    #[serde(rename = "disconnected_event")]      Disconnected,
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
            markdown: MarkdownContent { content: content.into() },
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
            payload: SendMsgPayload::Media { media_type, media_id },
        }
    }
}

#[derive(Debug, Clone)]
pub enum SendMsgPayload {
    Markdown { msgtype: &'static str, markdown: MarkdownContent },
    Media { media_type: WeComMediaType, media_id: String },
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
            SendMsgPayload::Media { media_type, media_id } => {
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
pub enum WeComMediaType { File, Image, Voice, Video }

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
            let idx = (ms.wrapping_mul(0x9E37) ^ fastrand::u64(..) as u128) as usize % CS.len();
            CS[idx] as char
        })
        .collect();
    format!("{prefix}_{ms}_{rand}")
}
```

Note: `fastrand` is already a transitive dep (used by other modules). If `cargo check` complains, add `fastrand = "2"` to Cargo.toml.

- [ ] **Step 1.3.2: 跑测试**

Run: `cd src-tauri && cargo test --test im_wecom_aibot_protocol -- --nocapture 2>&1 | tail -20`
Expected: 全部 9 个 test PASS

- [ ] **Step 1.3.3: 在 wecom/mod.rs 之外没人引用，确保 `cargo check` 全仓不破**

Run: `cd src-tauri && cargo check 2>&1 | tail -10`
Expected: 0 errors（可能有 dead_code warning，正常，PR2 会用上）

- [ ] **Step 1.3.4: Commit**

```bash
git add src-tauri/src/connector/im/wecom/aibot_protocol.rs src-tauri/tests/im_wecom_aibot_protocol.rs
git commit -m "feat(connector/im/wecom): aibot WebSocket frame types + serde (Phase 2 PR1)"
```

---

## Task 2 (PR2): aibot WebSocket Client（连接 / 心跳 / 重连 / ack 队列）

**Files:**
- Create: `src-tauri/src/connector/im/wecom/aibot_client.rs`
- Modify: `src-tauri/src/connector/im/wecom/mod.rs` (export aibot_client)
- Create: `src-tauri/tests/im_wecom_aibot_client.rs`

### Step 2.1: 写 mock server 工具 + 期望测试

- [ ] **Step 2.1.1: 写连接生命周期测试（fail）**

Write `src-tauri/tests/im_wecom_aibot_client.rs`:
```rust
//! WebSocket 连接生命周期集成测试。
//!
//! 用 `tokio-tungstenite::accept_async` 起 server 端，按 aibot 协议响应：
//! - 收到 `aibot_subscribe` 帧 → 回 `{ headers: { req_id }, errcode: 0, errmsg: "ok" }`
//! - 收到 `ping` 帧 → 回 `{ headers: { req_id }, errcode: 0, errmsg: "ok" }`
//! - 可主动推 `aibot_msg_callback` / `aibot_event_callback` 帧

use std::sync::Arc;
use std::time::Duration;

use app_lib::connector::im::wecom::aibot_client::{AibotClient, AibotClientConfig, AibotEvent};
use app_lib::connector::im::wecom::aibot_protocol::*;
use futures::{SinkExt, StreamExt};
use serde_json::{json, Value};
use tokio::net::TcpListener;
use tokio::sync::{mpsc, Mutex};
use tokio_tungstenite::tungstenite::Message;
use tokio_util::sync::CancellationToken;

/// 起一个 echo-style mock aibot server。回调 `on_subscribe` 决定认证 ack；
/// 通过返回的 `inbound_tx` 主动 push 服务端帧到客户端。
async fn spawn_mock_server(
    on_subscribe: impl Fn(&Value) -> Value + Send + Sync + 'static,
) -> (String, mpsc::Sender<Value>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let url = format!("ws://{addr}");

    let (inbound_tx, mut inbound_rx) = mpsc::channel::<Value>(16);
    let on_subscribe = Arc::new(on_subscribe);

    tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        let mut ws = tokio_tungstenite::accept_async(stream).await.unwrap();
        let (mut write, mut read) = futures::stream::StreamExt::split(&mut ws);

        // 从 inbound_rx 推送的帧 → 写到客户端
        let write_handle = Arc::new(Mutex::new(write));
        let w2 = write_handle.clone();
        tokio::spawn(async move {
            while let Some(frame) = inbound_rx.recv().await {
                let _ = w2.lock().await.send(Message::Text(frame.to_string().into())).await;
            }
        });

        // 读客户端帧，处理 subscribe / ping / respond
        while let Some(Ok(msg)) = read.next().await {
            let text = match msg { Message::Text(t) => t, _ => continue };
            let frame: Value = serde_json::from_str(&text).unwrap();
            let cmd = frame.get("cmd").and_then(|v| v.as_str()).unwrap_or("");
            let req_id = frame
                .pointer("/headers/req_id")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();

            let ack = match cmd {
                "aibot_subscribe" => on_subscribe(&frame),
                "ping" => json!({ "headers": { "req_id": req_id }, "errcode": 0, "errmsg": "ok" }),
                "aibot_respond_msg" | "aibot_send_msg" => {
                    json!({ "headers": { "req_id": req_id }, "errcode": 0, "errmsg": "ok" })
                }
                _ => continue,
            };
            let _ = write_handle.lock().await.send(Message::Text(ack.to_string().into())).await;
        }
    });

    (url, inbound_tx)
}

fn test_config(ws_url: String) -> AibotClientConfig {
    AibotClientConfig {
        bot_id: "BOTID".into(),
        secret: "SECRET".into(),
        ws_url,
        heartbeat_interval: Duration::from_millis(200),
        reply_ack_timeout: Duration::from_secs(2),
        max_missed_pong: 3,
        max_reconnect_attempts: 3,
        max_auth_failure_attempts: 2,
        reconnect_base_delay: Duration::from_millis(50),
    }
}

#[tokio::test]
async fn handshake_subscribes_and_emits_authenticated() {
    let (url, _push) = spawn_mock_server(|frame| {
        let req_id = frame.pointer("/headers/req_id").and_then(|v| v.as_str()).unwrap();
        json!({ "headers": { "req_id": req_id }, "errcode": 0, "errmsg": "ok" })
    }).await;

    let (evt_tx, mut evt_rx) = mpsc::channel(16);
    let cancel = CancellationToken::new();
    let client = AibotClient::new(test_config(url));
    let cancel_for_task = cancel.clone();
    tokio::spawn(async move { let _ = client.run(evt_tx, cancel_for_task).await; });

    let evt = tokio::time::timeout(Duration::from_secs(2), evt_rx.recv()).await.unwrap().unwrap();
    assert!(matches!(evt, AibotEvent::Authenticated), "first event must be Authenticated, got {evt:?}");

    cancel.cancel();
}

#[tokio::test]
async fn inbound_message_emits_event() {
    let (url, push) = spawn_mock_server(|frame| {
        let req_id = frame.pointer("/headers/req_id").and_then(|v| v.as_str()).unwrap();
        json!({ "headers": { "req_id": req_id }, "errcode": 0, "errmsg": "ok" })
    }).await;

    let (evt_tx, mut evt_rx) = mpsc::channel(16);
    let cancel = CancellationToken::new();
    let client = AibotClient::new(test_config(url));
    let cancel_for_task = cancel.clone();
    tokio::spawn(async move { let _ = client.run(evt_tx, cancel_for_task).await; });

    // 等认证完成
    let _ = tokio::time::timeout(Duration::from_secs(2), evt_rx.recv()).await.unwrap();

    // 推一条消息帧
    push.send(json!({
        "cmd": "aibot_msg_callback",
        "headers": { "req_id": "msg-1" },
        "body": {
            "msgid": "MSG1", "aibotid": "BOTID", "chattype": "single",
            "from": { "userid": "U1" }, "msgtype": "text",
            "text": { "content": "hi" }
        }
    })).await.unwrap();

    let evt = tokio::time::timeout(Duration::from_secs(2), evt_rx.recv()).await.unwrap().unwrap();
    match evt {
        AibotEvent::Inbound(frame) => {
            assert_eq!(frame.cmd, Some(WsCmd::MsgCallback));
            assert_eq!(frame.headers.req_id, "msg-1");
        }
        other => panic!("expected Inbound, got {other:?}"),
    }
    cancel.cancel();
}

#[tokio::test]
async fn disconnected_event_emits_kicked_out_not_reconnect() {
    let (url, push) = spawn_mock_server(|frame| {
        let req_id = frame.pointer("/headers/req_id").and_then(|v| v.as_str()).unwrap();
        json!({ "headers": { "req_id": req_id }, "errcode": 0, "errmsg": "ok" })
    }).await;

    let (evt_tx, mut evt_rx) = mpsc::channel(16);
    let cancel = CancellationToken::new();
    let client = AibotClient::new(test_config(url));
    let cancel_for_task = cancel.clone();
    let handle = tokio::spawn(async move { client.run(evt_tx, cancel_for_task).await });

    let _ = tokio::time::timeout(Duration::from_secs(2), evt_rx.recv()).await.unwrap(); // Authenticated

    push.send(json!({
        "cmd": "aibot_event_callback",
        "headers": { "req_id": "evt-1" },
        "body": {
            "msgid": "E1", "aibotid": "BOTID",
            "from": { "userid": "U1" }, "msgtype": "event",
            "event": { "eventtype": "disconnected_event" }
        }
    })).await.unwrap();

    let evt = tokio::time::timeout(Duration::from_secs(2), evt_rx.recv()).await.unwrap().unwrap();
    assert!(matches!(evt, AibotEvent::KickedOut(_)), "must emit KickedOut, got {evt:?}");

    // run() 应在 2 秒内退出（不重连）
    tokio::time::timeout(Duration::from_secs(2), handle).await
        .expect("run() must exit after KickedOut, didn't")
        .unwrap()
        .ok();
}

#[tokio::test]
async fn auth_failure_emits_auth_failed_and_retries_until_exhausted() {
    let (url, _push) = spawn_mock_server(|frame| {
        let req_id = frame.pointer("/headers/req_id").and_then(|v| v.as_str()).unwrap();
        json!({ "headers": { "req_id": req_id }, "errcode": 40014, "errmsg": "invalid bot" })
    }).await;

    let (evt_tx, mut evt_rx) = mpsc::channel(16);
    let cancel = CancellationToken::new();
    let mut cfg = test_config(url);
    cfg.max_auth_failure_attempts = 2;
    let client = AibotClient::new(cfg);
    let handle = tokio::spawn(async move { client.run(evt_tx, cancel).await });

    // 至少应见到 AuthFailed
    let mut saw_auth_failed = false;
    while let Ok(Some(evt)) = tokio::time::timeout(Duration::from_secs(3), evt_rx.recv()).await {
        if matches!(evt, AibotEvent::AuthFailed(40014, _)) { saw_auth_failed = true; }
    }
    assert!(saw_auth_failed);
    // run() 应已退出（attempts 用尽）
    let _ = handle.await;
}

#[tokio::test]
async fn cancel_token_terminates_run_within_2s() {
    let (url, _push) = spawn_mock_server(|frame| {
        let req_id = frame.pointer("/headers/req_id").and_then(|v| v.as_str()).unwrap();
        json!({ "headers": { "req_id": req_id }, "errcode": 0, "errmsg": "ok" })
    }).await;

    let (evt_tx, mut evt_rx) = mpsc::channel(16);
    let cancel = CancellationToken::new();
    let client = AibotClient::new(test_config(url));
    let cancel_clone = cancel.clone();
    let handle = tokio::spawn(async move { client.run(evt_tx, cancel_clone).await });

    let _ = tokio::time::timeout(Duration::from_secs(2), evt_rx.recv()).await.unwrap();
    cancel.cancel();
    let start = tokio::time::Instant::now();
    tokio::time::timeout(Duration::from_secs(2), handle).await
        .expect("run() did not exit within 2s of cancel")
        .unwrap()
        .ok();
    assert!(start.elapsed() < Duration::from_secs(2));
}

#[tokio::test]
async fn respond_serializes_under_ack_serial_order() {
    let (url, _push) = spawn_mock_server(|frame| {
        let req_id = frame.pointer("/headers/req_id").and_then(|v| v.as_str()).unwrap();
        json!({ "headers": { "req_id": req_id }, "errcode": 0, "errmsg": "ok" })
    }).await;

    let (evt_tx, mut evt_rx) = mpsc::channel(16);
    let cancel = CancellationToken::new();
    let client = Arc::new(AibotClient::new(test_config(url)));
    let c2 = client.clone();
    let cancel_for = cancel.clone();
    tokio::spawn(async move { let _ = c2.run(evt_tx, cancel_for).await; });
    let _ = tokio::time::timeout(Duration::from_secs(2), evt_rx.recv()).await.unwrap();

    // 同一 req_id 连发两次 respond，应都成功（串行处理 + 各自 ack）
    client.respond("R1", serde_json::to_value(RespondMarkdownBody::new("a")).unwrap()).await.unwrap();
    client.respond("R1", serde_json::to_value(RespondMarkdownBody::new("b")).unwrap()).await.unwrap();
    cancel.cancel();
}
```

- [ ] **Step 2.1.2: 跑测试看 fail（aibot_client 还没实现）**

Run: `cd src-tauri && cargo test --test im_wecom_aibot_client 2>&1 | tail -10`
Expected: 编译失败 `use of undeclared module aibot_client`

### Step 2.2: 实现 aibot_client.rs

- [ ] **Step 2.2.1: 写 aibot_client.rs**

Write `src-tauri/src/connector/im/wecom/aibot_client.rs`:
```rust
//! aibot WebSocket 连接管理层。
//!
//! 职责：
//! - 主动外连 `ws_url`（默认 `wss://openws.work.weixin.qq.com`）+ 发首帧 `aibot_subscribe` 认证
//! - 心跳：每 `heartbeat_interval` 发 `ping`，连续 `max_missed_pong` 次未收 pong 视为死连接
//! - 重连：物理 drop 走 ReconnectBackoff（max_reconnect_attempts）；认证失败独立计数
//!   （max_auth_failure_attempts）；收到 `disconnected_event` 不重连（KickedOut）
//! - 出站串行：同 req_id 出站帧按 FIFO 串行，前一帧 ack/超时后才发下一帧
//!
//! 调用方通过 mpsc::Sender<AibotEvent> 消费帧；通过 `respond` / `send_msg` 发送出站。

use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use std::time::Duration;

use anyhow::{anyhow, bail, Context, Result};
use futures::{SinkExt, StreamExt};
use tokio::sync::{mpsc, oneshot, Mutex};
use tokio::time::{sleep, Instant};
use tokio_tungstenite::tungstenite::Message;
use tokio_util::sync::CancellationToken;

use super::aibot_protocol::*;

#[derive(Debug, Clone)]
pub struct AibotClientConfig {
    pub bot_id: String,
    pub secret: String,
    pub ws_url: String,
    pub heartbeat_interval: Duration,
    pub reply_ack_timeout: Duration,
    pub max_missed_pong: usize,
    pub max_reconnect_attempts: usize,
    pub max_auth_failure_attempts: usize,
    pub reconnect_base_delay: Duration,
}

impl AibotClientConfig {
    pub fn production(bot_id: String, secret: String) -> Self {
        Self {
            bot_id,
            secret,
            ws_url: "wss://openws.work.weixin.qq.com".into(),
            heartbeat_interval: Duration::from_secs(30),
            reply_ack_timeout: Duration::from_secs(10),
            max_missed_pong: 3,
            max_reconnect_attempts: 10,
            max_auth_failure_attempts: 5,
            reconnect_base_delay: Duration::from_secs(1),
        }
    }
}

#[derive(Debug)]
pub enum AibotEvent {
    Authenticated,
    /// 服务端推送的消息或事件帧（aibot_msg_callback / aibot_event_callback）。
    Inbound(WsFrame<serde_json::Value>),
    /// 收到 disconnected_event，服务端主动踢——调用方应停止重连。
    KickedOut(String),
    /// 物理连接断（网络 / 心跳超时）——client 内部会自动重连，调用方仅需 log。
    ConnectionDropped(String),
    /// 认证 ack errcode != 0——独立计数器，超限后 run() 退出。
    AuthFailed(i32, String),
    /// 重连前发出，attempt 从 1 起，方便上层 log。
    Reconnecting(usize),
}

/// 单个 req_id 的串行 ack 队列。
struct ReplyQueue {
    pending: VecDeque<(serde_json::Value, &'static str, oneshot::Sender<Result<()>>)>,
    in_flight: Option<oneshot::Sender<Result<()>>>,
}

pub struct AibotClient {
    cfg: AibotClientConfig,
    /// 出站发送：内部由 run() 持有 writer 半，外部通过 `outbound_tx` 投递。
    outbound_tx: Mutex<Option<mpsc::Sender<OutboundCmd>>>,
}

/// 跨 run() / send 边界的出站命令。
enum OutboundCmd {
    /// 直接发原始帧（认证 / 心跳，不走 ack 队列）。
    RawFrame(WsFrame<serde_json::Value>),
    /// 走 ack 队列：(req_id, body, cmd, done)
    Reply(String, serde_json::Value, &'static str, oneshot::Sender<Result<()>>),
}

impl AibotClient {
    pub fn new(cfg: AibotClientConfig) -> Self {
        Self { cfg, outbound_tx: Mutex::new(None) }
    }

    /// 启动 client 主循环。
    /// `event_tx` 用来接收 AibotEvent；`cancel_token` 取消时主动关 WS + 退出 run()。
    pub async fn run(
        self: Arc<Self>,
        event_tx: mpsc::Sender<AibotEvent>,
        cancel_token: CancellationToken,
    ) -> Result<()> {
        let mut connection_attempts = 0usize;
        let mut auth_failure_attempts = 0usize;

        loop {
            if cancel_token.is_cancelled() {
                log::info!("[wecom-aibot] cancel before connect, exit");
                return Ok(());
            }
            if connection_attempts > 0 {
                let _ = event_tx.send(AibotEvent::Reconnecting(connection_attempts)).await;
            }

            match self.clone().connect_and_pump(&event_tx, &cancel_token).await {
                Ok(LoopExit::Kicked) => return Ok(()),
                Ok(LoopExit::Cancelled) => return Ok(()),
                Ok(LoopExit::AuthFailed(code, msg)) => {
                    auth_failure_attempts += 1;
                    let _ = event_tx.send(AibotEvent::AuthFailed(code, msg.clone())).await;
                    if auth_failure_attempts >= self.cfg.max_auth_failure_attempts {
                        log::error!("[wecom-aibot] auth failure exhausted");
                        return Err(anyhow!("auth failure exhausted: {msg}"));
                    }
                }
                Ok(LoopExit::Dropped(reason)) => {
                    connection_attempts += 1;
                    let _ = event_tx.send(AibotEvent::ConnectionDropped(reason)).await;
                    if connection_attempts >= self.cfg.max_reconnect_attempts {
                        log::error!("[wecom-aibot] reconnect attempts exhausted");
                        return Err(anyhow!("reconnect attempts exhausted"));
                    }
                }
                Err(e) => {
                    connection_attempts += 1;
                    let _ = event_tx.send(AibotEvent::ConnectionDropped(format!("{e:#}"))).await;
                    if connection_attempts >= self.cfg.max_reconnect_attempts {
                        return Err(e);
                    }
                }
            }

            // 退避后重连
            let delay = self.cfg.reconnect_base_delay
                * (1u32 << connection_attempts.min(6) as u32).max(1);
            tokio::select! {
                _ = sleep(delay) => {}
                _ = cancel_token.cancelled() => return Ok(()),
            }
        }
    }

    /// 投递一帧 respond_msg。`req_id` 来自收到的入站帧 headers.req_id。
    pub async fn respond(&self, req_id: &str, body: serde_json::Value) -> Result<()> {
        self.send_via_queue(req_id, body, "aibot_respond_msg").await
    }

    /// 投递一帧 send_msg（主动推送），生成新 req_id。
    pub async fn send_msg(&self, body: serde_json::Value) -> Result<()> {
        let req_id = generate_req_id("aibot_send_msg");
        self.send_via_queue(&req_id, body, "aibot_send_msg").await
    }

    async fn send_via_queue(&self, req_id: &str, body: serde_json::Value, cmd: &'static str) -> Result<()> {
        let tx = self.outbound_tx.lock().await
            .clone()
            .ok_or_else(|| anyhow!("aibot client not running"))?;
        let (done_tx, done_rx) = oneshot::channel();
        tx.send(OutboundCmd::Reply(req_id.to_string(), body, cmd, done_tx)).await
            .map_err(|_| anyhow!("aibot client outbound channel closed"))?;
        let wait = tokio::time::timeout(self.cfg.reply_ack_timeout, done_rx).await
            .map_err(|_| anyhow!("ack timeout for req_id {req_id}"))?;
        wait.map_err(|_| anyhow!("ack pipe dropped"))?
    }

    async fn connect_and_pump(
        self: Arc<Self>,
        event_tx: &mpsc::Sender<AibotEvent>,
        cancel_token: &CancellationToken,
    ) -> Result<LoopExit> {
        let (ws_stream, _resp) = tokio_tungstenite::connect_async(&self.cfg.ws_url).await
            .context("ws connect failed")?;
        let (mut writer, mut reader) = ws_stream.split();

        // ack 队列：req_id → ReplyQueue
        let queues: Arc<Mutex<HashMap<String, ReplyQueue>>> = Arc::new(Mutex::new(HashMap::new()));
        let (out_tx, mut out_rx) = mpsc::channel::<OutboundCmd>(64);
        *self.outbound_tx.lock().await = Some(out_tx.clone());

        // 发首帧认证
        let subscribe_req_id = generate_req_id("aibot_subscribe");
        let subscribe = WsFrame::<serde_json::Value> {
            cmd: Some(WsCmd::Subscribe),
            headers: FrameHeaders { req_id: subscribe_req_id.clone(), extra: Default::default() },
            body: Some(serde_json::to_value(SubscribeBody {
                secret: self.cfg.secret.clone(),
                bot_id: self.cfg.bot_id.clone(),
            })?),
            errcode: None, errmsg: None,
        };
        writer.send(Message::Text(serde_json::to_string(&subscribe)?.into())).await?;

        let mut authenticated = false;
        let mut missed_pong = 0usize;
        let mut last_ping_req_id: Option<String> = None;
        let mut heartbeat = tokio::time::interval(self.cfg.heartbeat_interval);
        heartbeat.tick().await; // 跳第一次立即触发

        loop {
            tokio::select! {
                biased;
                _ = cancel_token.cancelled() => {
                    let _ = writer.close().await;
                    *self.outbound_tx.lock().await = None;
                    return Ok(LoopExit::Cancelled);
                }
                Some(cmd) = out_rx.recv() => {
                    match cmd {
                        OutboundCmd::RawFrame(f) => {
                            if let Err(e) = writer.send(Message::Text(serde_json::to_string(&f)?.into())).await {
                                log::warn!("[wecom-aibot] write raw failed: {e}");
                                *self.outbound_tx.lock().await = None;
                                return Ok(LoopExit::Dropped(format!("write failed: {e}")));
                            }
                        }
                        OutboundCmd::Reply(req_id, body, cmd_str, done) => {
                            let mut q = queues.lock().await;
                            let entry = q.entry(req_id.clone()).or_insert_with(|| ReplyQueue {
                                pending: VecDeque::new(),
                                in_flight: None,
                            });
                            entry.pending.push_back((body, cmd_str, done));
                            if entry.in_flight.is_none() {
                                // 立即出队发一条
                                if let Some((body, cmd_str, done)) = entry.pending.pop_front() {
                                    entry.in_flight = Some(done);
                                    let cmd_enum = match cmd_str {
                                        "aibot_respond_msg" => WsCmd::Respond,
                                        "aibot_send_msg" => WsCmd::SendMsg,
                                        _ => unreachable!(),
                                    };
                                    let f = WsFrame::<serde_json::Value> {
                                        cmd: Some(cmd_enum),
                                        headers: FrameHeaders { req_id: req_id.clone(), extra: Default::default() },
                                        body: Some(body),
                                        errcode: None, errmsg: None,
                                    };
                                    drop(q);
                                    if let Err(e) = writer.send(Message::Text(serde_json::to_string(&f)?.into())).await {
                                        return Ok(LoopExit::Dropped(format!("write failed: {e}")));
                                    }
                                }
                            }
                        }
                    }
                }
                _ = heartbeat.tick() => {
                    if authenticated {
                        if last_ping_req_id.is_some() {
                            missed_pong += 1;
                            if missed_pong >= self.cfg.max_missed_pong {
                                let _ = writer.close().await;
                                *self.outbound_tx.lock().await = None;
                                return Ok(LoopExit::Dropped("heartbeat timeout".into()));
                            }
                        }
                        let req_id = generate_req_id("ping");
                        last_ping_req_id = Some(req_id.clone());
                        let f = WsFrame::<serde_json::Value> {
                            cmd: Some(WsCmd::Ping),
                            headers: FrameHeaders { req_id, extra: Default::default() },
                            body: None,
                            errcode: None, errmsg: None,
                        };
                        if let Err(e) = writer.send(Message::Text(serde_json::to_string(&f)?.into())).await {
                            return Ok(LoopExit::Dropped(format!("ping write failed: {e}")));
                        }
                    }
                }
                msg = reader.next() => {
                    let msg = match msg {
                        Some(Ok(m)) => m,
                        Some(Err(e)) => {
                            *self.outbound_tx.lock().await = None;
                            return Ok(LoopExit::Dropped(format!("ws read err: {e}")));
                        }
                        None => {
                            *self.outbound_tx.lock().await = None;
                            return Ok(LoopExit::Dropped("ws closed".into()));
                        }
                    };
                    let text = match msg {
                        Message::Text(t) => t.to_string(),
                        Message::Close(_) => {
                            *self.outbound_tx.lock().await = None;
                            return Ok(LoopExit::Dropped("ws close frame".into()));
                        }
                        _ => continue,
                    };
                    let frame: WsFrame<serde_json::Value> = match serde_json::from_str(&text) {
                        Ok(f) => f,
                        Err(e) => { log::warn!("[wecom-aibot] bad frame: {e}; text={text}"); continue; }
                    };

                    // 1) 认证 ack（无 cmd + req_id == subscribe）
                    if frame.cmd.is_none() && frame.headers.req_id == subscribe_req_id {
                        let code = frame.errcode.unwrap_or(0);
                        if code == 0 {
                            authenticated = true;
                            let _ = event_tx.send(AibotEvent::Authenticated).await;
                        } else {
                            let msg = frame.errmsg.unwrap_or_default();
                            *self.outbound_tx.lock().await = None;
                            return Ok(LoopExit::AuthFailed(code, msg));
                        }
                        continue;
                    }

                    // 2) 心跳 ack
                    if frame.cmd.is_none() && last_ping_req_id.as_deref() == Some(frame.headers.req_id.as_str()) {
                        last_ping_req_id = None;
                        missed_pong = 0;
                        continue;
                    }

                    // 3) 回复 ack（reply queue 内）
                    if frame.cmd.is_none() {
                        let mut q = queues.lock().await;
                        if let Some(entry) = q.get_mut(&frame.headers.req_id) {
                            if let Some(done) = entry.in_flight.take() {
                                let code = frame.errcode.unwrap_or(0);
                                if code == 0 {
                                    let _ = done.send(Ok(()));
                                } else {
                                    let _ = done.send(Err(anyhow!(
                                        "errcode={} errmsg={}",
                                        code,
                                        frame.errmsg.clone().unwrap_or_default()
                                    )));
                                }
                                // 出队下一条
                                if let Some((body, cmd_str, done)) = entry.pending.pop_front() {
                                    entry.in_flight = Some(done);
                                    let cmd_enum = match cmd_str {
                                        "aibot_respond_msg" => WsCmd::Respond,
                                        "aibot_send_msg" => WsCmd::SendMsg,
                                        _ => unreachable!(),
                                    };
                                    let f = WsFrame::<serde_json::Value> {
                                        cmd: Some(cmd_enum),
                                        headers: FrameHeaders { req_id: frame.headers.req_id.clone(), extra: Default::default() },
                                        body: Some(body),
                                        errcode: None, errmsg: None,
                                    };
                                    drop(q);
                                    if let Err(e) = writer.send(Message::Text(serde_json::to_string(&f)?.into())).await {
                                        return Ok(LoopExit::Dropped(format!("write failed: {e}")));
                                    }
                                }
                            }
                        }
                        continue;
                    }

                    // 4) 服务端推送：先检测 disconnected_event，否则发 Inbound
                    if frame.cmd == Some(WsCmd::EventCallback) {
                        if frame.body.as_ref()
                            .and_then(|b| b.pointer("/event/eventtype"))
                            .and_then(|v| v.as_str()) == Some("disconnected_event")
                        {
                            let _ = event_tx.send(AibotEvent::KickedOut("server disconnected_event".into())).await;
                            let _ = writer.close().await;
                            *self.outbound_tx.lock().await = None;
                            return Ok(LoopExit::Kicked);
                        }
                    }
                    let _ = event_tx.send(AibotEvent::Inbound(frame)).await;
                }
            }
        }
    }
}

enum LoopExit {
    Cancelled,
    Kicked,
    AuthFailed(i32, String),
    Dropped(String),
}
```

- [ ] **Step 2.2.2: 在 wecom/mod.rs 暴露 aibot_client**

Edit `src-tauri/src/connector/im/wecom/mod.rs`:
```rust
pub mod aibot_client;
pub mod aibot_protocol;
```

- [ ] **Step 2.2.3: 跑测试**

Run: `cd src-tauri && cargo test --test im_wecom_aibot_client -- --nocapture 2>&1 | tail -40`
Expected: 6 个 test 全部 PASS

如果 `cancel_token_terminates_run_within_2s` 偶发超时，检查 mock server 是否在 cancel 后回写超时；可适当放宽到 3s 但不要去掉这条断言（contract: cancel → 2s 内退）。

- [ ] **Step 2.2.4: 全仓 cargo check**

Run: `cd src-tauri && cargo check 2>&1 | tail -10`
Expected: 0 errors

- [ ] **Step 2.2.5: Commit**

```bash
git add src-tauri/src/connector/im/wecom/aibot_client.rs src-tauri/src/connector/im/wecom/mod.rs src-tauri/tests/im_wecom_aibot_client.rs
git commit -m "feat(connector/im/wecom): aibot WebSocket client + reconnect + ack queue (Phase 2 PR2)"
```

---

## Task 3 (PR3): 通用流式降级 Buffer（shared/aicard_fallback.rs）

**Files:**
- Create: `src-tauri/src/connector/im/shared/aicard_fallback.rs`
- Modify: `src-tauri/src/connector/im/shared/mod.rs` (export aicard_fallback)
- Create: `src-tauri/tests/im_aicard_fallback.rs`

### Step 3.1: 写测试（fail）

- [ ] **Step 3.1.1: 写集成测试**

Write `src-tauri/tests/im_aicard_fallback.rs`:
```rust
//! AiCardFallbackBuffer 在不同 AI 回复 pattern 下返回正确 FallbackAction。

use std::time::Duration;
use app_lib::connector::im::shared::aicard_fallback::{AiCardFallbackBuffer, FallbackAction};

#[test]
fn first_short_chunk_with_final_returns_send_final() {
    let mut buf = AiCardFallbackBuffer::new(Duration::from_secs(60));
    match buf.observe("hello", true) {
        FallbackAction::SendFinal { text } => assert_eq!(text, "hello"),
        other => panic!("expected SendFinal, got {other:?}"),
    }
}

#[test]
fn first_chunk_without_final_returns_buffer() {
    let mut buf = AiCardFallbackBuffer::new(Duration::from_secs(60));
    match buf.observe("hello", false) {
        FallbackAction::Buffer => {}
        other => panic!("expected Buffer, got {other:?}"),
    }
}

#[test]
fn multiple_chunks_then_final_concats_correctly() {
    let mut buf = AiCardFallbackBuffer::new(Duration::from_secs(60));
    assert!(matches!(buf.observe("foo ", false), FallbackAction::Buffer));
    assert!(matches!(buf.observe("bar ", false), FallbackAction::Buffer));
    match buf.observe("baz", true) {
        FallbackAction::SendFinal { text } => assert_eq!(text, "foo bar baz"),
        other => panic!("expected SendFinal, got {other:?}"),
    }
}

#[test]
fn placeholder_after_threshold_emits_once_then_buffer() {
    let mut buf = AiCardFallbackBuffer::new(Duration::from_millis(50));
    assert!(matches!(buf.observe("a", false), FallbackAction::Buffer));
    std::thread::sleep(Duration::from_millis(80));
    match buf.observe("b", false) {
        FallbackAction::SendPlaceholder { text } => assert!(text.contains("思考")),
        other => panic!("expected SendPlaceholder, got {other:?}"),
    }
    // 第二次仍未 final + 已发过 placeholder → Buffer
    match buf.observe("c", false) {
        FallbackAction::Buffer => {}
        other => panic!("placeholder should fire only once, got {other:?}"),
    }
}

#[test]
fn final_after_placeholder_still_returns_send_final_with_complete_text() {
    let mut buf = AiCardFallbackBuffer::new(Duration::from_millis(50));
    let _ = buf.observe("a", false);
    std::thread::sleep(Duration::from_millis(80));
    let _ = buf.observe("b", false);    // placeholder
    let _ = buf.observe("c", false);    // buffer
    match buf.observe("d", true) {
        FallbackAction::SendFinal { text } => assert_eq!(text, "abcd"),
        other => panic!("expected SendFinal, got {other:?}"),
    }
}
```

- [ ] **Step 3.1.2: 跑测试看 fail**

Run: `cd src-tauri && cargo test --test im_aicard_fallback 2>&1 | tail -10`
Expected: 编译失败 `module aicard_fallback does not exist`

### Step 3.2: 实现 aicard_fallback.rs

- [ ] **Step 3.2.1: 写 aicard_fallback.rs**

Write `src-tauri/src/connector/im/shared/aicard_fallback.rs`:
```rust
//! 通用"流式 AI 卡片不支持"降级 buffer。
//!
//! 适用平台：capabilities.outbound_aicard == false（wecom / whatsapp / 个微）。
//! 接收到 ReplyContent::AiCardChunk { delta, final_chunk } 时，由 connector 内部
//! 维护一个 buffer 实例（按 session_id 分），按以下策略决定 IO：
//!
//! 1) 首次 chunk：累积，记 started_at
//! 2) 后续 chunks：累积，不发任何消息
//! 3) 超过 placeholder_after 仍未 final：发一次"思考中..."占位
//! 4) final：发完整文本
//!
//! 一次 AI 回复最多 2 条消息（占位 + 最终），通常只有 1 条（最终）。

use std::time::{Duration, Instant};

#[derive(Debug)]
pub struct AiCardFallbackBuffer {
    accumulated: String,
    started_at: Option<Instant>,
    placeholder_after: Duration,
    placeholder_sent: bool,
}

#[derive(Debug)]
pub enum FallbackAction {
    /// 继续累积，无需发消息。
    Buffer,
    /// 发占位消息（"思考中..."），仅 1 次。
    SendPlaceholder { text: String },
    /// 发最终回复。
    SendFinal { text: String },
}

impl AiCardFallbackBuffer {
    pub fn new(placeholder_after: Duration) -> Self {
        Self {
            accumulated: String::new(),
            started_at: None,
            placeholder_after,
            placeholder_sent: false,
        }
    }

    pub fn observe(&mut self, delta: &str, final_chunk: bool) -> FallbackAction {
        self.accumulated.push_str(delta);
        if self.started_at.is_none() {
            self.started_at = Some(Instant::now());
        }

        if final_chunk {
            return FallbackAction::SendFinal { text: std::mem::take(&mut self.accumulated) };
        }

        if !self.placeholder_sent {
            if let Some(started) = self.started_at {
                if started.elapsed() >= self.placeholder_after {
                    self.placeholder_sent = true;
                    return FallbackAction::SendPlaceholder { text: "🤔 思考中...".into() };
                }
            }
        }

        FallbackAction::Buffer
    }
}
```

- [ ] **Step 3.2.2: 在 shared/mod.rs 加 export**

Edit `src-tauri/src/connector/im/shared/mod.rs`，在已有 `pub mod` 列表加：
```rust
pub mod aicard_fallback;
```

- [ ] **Step 3.2.3: 跑测试**

Run: `cd src-tauri && cargo test --test im_aicard_fallback -- --nocapture 2>&1 | tail -10`
Expected: 5 个 test 全部 PASS

- [ ] **Step 3.2.4: Commit**

```bash
git add src-tauri/src/connector/im/shared/aicard_fallback.rs src-tauri/src/connector/im/shared/mod.rs src-tauri/tests/im_aicard_fallback.rs
git commit -m "feat(connector/im/shared): generic AI card fallback buffer (Phase 2 PR3)"
```

---

## Task 4 (PR4): Parser + Sender + Media

**Files:**
- Modify: `src-tauri/Cargo.toml` (加 aes + cbc 依赖)
- Create: `src-tauri/src/connector/im/wecom/parser.rs`
- Create: `src-tauri/src/connector/im/wecom/sender.rs`
- Create: `src-tauri/src/connector/im/wecom/media.rs`
- Modify: `src-tauri/src/connector/im/wecom/mod.rs` (export parser/sender/media)
- Create: `src-tauri/tests/im_wecom_parser.rs`
- Create: `src-tauri/tests/im_wecom_sender.rs`
- Create: `src-tauri/tests/im_wecom_media.rs`

### Step 4.1: 加 aes + cbc 依赖

- [ ] **Step 4.1.1: 修改 Cargo.toml**

Edit `src-tauri/Cargo.toml`，在 `[dependencies]` 段（已有 `aes-gcm = "0.10"` 行附近）追加：
```toml
aes = "0.8"
cbc = { version = "0.1", features = ["std"] }
```

- [ ] **Step 4.1.2: 跑 cargo check 确认新依赖能拉**

Run: `cd src-tauri && cargo check 2>&1 | tail -10`
Expected: PASS（aes / cbc 都是成熟 crate，无网络问题应直接拉到）

- [ ] **Step 4.1.3: Commit deps**

```bash
git add src-tauri/Cargo.toml src-tauri/Cargo.lock
git commit -m "build: add aes + cbc deps for wecom media decrypt (Phase 2 PR4)"
```

### Step 4.2: Parser 测试 + 实现

- [ ] **Step 4.2.1: 写 parser 测试**

Write `src-tauri/tests/im_wecom_parser.rs`:
```rust
use app_lib::connector::im::wecom::aibot_protocol::*;
use app_lib::connector::im::wecom::parser::{parse_inbound, ParsedInbound};
use serde_json::json;

fn frame_with_body(body: serde_json::Value) -> WsFrame<serde_json::Value> {
    serde_json::from_value(json!({
        "cmd": "aibot_msg_callback",
        "headers": { "req_id": "REQ" },
        "body": body
    })).unwrap()
}

#[test]
fn text_single_chat_maps_to_channel_message_with_robot_code_and_reply_group() {
    let frame = frame_with_body(json!({
        "msgid": "M1", "aibotid": "BOTID", "chattype": "single",
        "from": { "userid": "U1" }, "msgtype": "text",
        "text": { "content": "hello" }
    }));
    let parsed = parse_inbound("BOTID", &frame).expect("must parse");
    let msg = match parsed { ParsedInbound::Message(m) => m, _ => panic!() };
    assert_eq!(msg.text, "hello");
    assert_eq!(msg.robot_code, "BOTID", "robot_code <- bot_id");
    assert_eq!(msg.reply_group_id, "U1", "single chat reply_group_id <- userid");
    assert!(matches!(msg.conversation_type, app_lib::connector::im::types::ConversationType::Private));
    assert_eq!(msg.sender_id, "U1");
    assert_eq!(msg.msg_id, "M1");
    assert!(msg.session_webhook.is_none(), "aibot has no session webhook concept");
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
        ParsedInbound::Message(m) => m, _ => panic!()
    };
    assert_eq!(msg.reply_group_id, "GROUP_1", "group chat reply_group_id <- chatid");
    assert!(matches!(msg.conversation_type, app_lib::connector::im::types::ConversationType::Group));
}

#[test]
fn image_message_emits_attachment_with_encoded_download_code() {
    let frame = frame_with_body(json!({
        "msgid": "M3", "aibotid": "BOTID", "chattype": "single",
        "from": { "userid": "U" }, "msgtype": "image",
        "image": { "url": "https://example.com/file?id=abc", "aeskey": "KEY1" }
    }));
    let msg = match parse_inbound("BOTID", &frame).unwrap() {
        ParsedInbound::Message(m) => m, _ => panic!()
    };
    assert_eq!(msg.attachments.len(), 1);
    let att = &msg.attachments[0];
    use app_lib::connector::im::types::AttachmentKind;
    assert!(matches!(att.kind, AttachmentKind::Picture));
    // download_code 用 "wecom://{aeskey}@{url}" 形式承载，后续 media.rs 还原
    assert!(att.download_code.starts_with("wecom://KEY1@"));
    assert!(att.download_code.contains("https://example.com/file?id=abc"));
}

#[test]
fn file_message_emits_file_attachment() {
    let frame = frame_with_body(json!({
        "msgid": "M4", "aibotid": "BOTID", "chattype": "single",
        "from": { "userid": "U" }, "msgtype": "file",
        "file": { "url": "https://example.com/f", "aeskey": "K" }
    }));
    let msg = match parse_inbound("BOTID", &frame).unwrap() {
        ParsedInbound::Message(m) => m, _ => panic!()
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
        assert!(matches!(parsed, Some(ParsedInbound::Ignored)), "{mt} should be Ignored");
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
    })).unwrap();
    assert!(parse_inbound("BOTID", &frame).is_none());
}
```

- [ ] **Step 4.2.2: 跑测试看 fail**

Run: `cd src-tauri && cargo test --test im_wecom_parser 2>&1 | tail -10`
Expected: 编译失败 `module parser does not exist`

- [ ] **Step 4.2.3: 写 parser.rs**

Write `src-tauri/src/connector/im/wecom/parser.rs`:
```rust
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
        super::aibot_protocol::ChatType::Single => (ConversationType::Private, body.from.userid.clone()),
        super::aibot_protocol::ChatType::Group => (
            ConversationType::Group,
            body.chatid.clone().unwrap_or_default(),
        ),
    };

    let (text, attachments) = match body.msgtype.as_str() {
        "text" => {
            let t = body.payload.pointer("/text/content").and_then(|v| v.as_str()).unwrap_or("").to_string();
            (t, vec![])
        }
        "image" | "file" => {
            let kind = if body.msgtype == "image" { AttachmentKind::Picture } else { AttachmentKind::File };
            let key_path = if body.msgtype == "image" { "/image" } else { "/file" };
            let url = body.payload.pointer(&format!("{key_path}/url")).and_then(|v| v.as_str()).unwrap_or("");
            let aeskey = body.payload.pointer(&format!("{key_path}/aeskey")).and_then(|v| v.as_str()).unwrap_or("");
            let file_name = body.payload.pointer(&format!("{key_path}/filename"))
                .and_then(|v| v.as_str())
                .map(|s| s.to_string())
                .unwrap_or_else(|| format!("{}-{}.{}",
                    body.msgid,
                    chrono::Utc::now().timestamp(),
                    if body.msgtype == "image" { "jpg" } else { "bin" }
                ));
            let download_code = format!("wecom://{aeskey}@{url}");
            (String::new(), vec![ChannelAttachmentSpec { kind, download_code, file_name }])
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
        sender_nick: body.from.userid,    // aibot 不提供 nick，先用 userid
        text,
        robot_code: bot_id.to_string(),
        reply_group_id,
        attachments,
        session_webhook: None,
    }))
}
```

- [ ] **Step 4.2.4: 在 wecom/mod.rs export**

```rust
pub mod aibot_client;
pub mod aibot_protocol;
pub mod parser;
```

- [ ] **Step 4.2.5: 跑测试**

Run: `cd src-tauri && cargo test --test im_wecom_parser -- --nocapture 2>&1 | tail -10`
Expected: 6 个 test PASS

- [ ] **Step 4.2.6: Commit parser**

```bash
git add src-tauri/src/connector/im/wecom/parser.rs src-tauri/src/connector/im/wecom/mod.rs src-tauri/tests/im_wecom_parser.rs
git commit -m "feat(connector/im/wecom): parse inbound frames into ChannelMessage (Phase 2 PR4a)"
```

### Step 4.3: Sender 测试 + 实现

- [ ] **Step 4.3.1: 写 sender 测试**

Write `src-tauri/tests/im_wecom_sender.rs`:
```rust
//! Sender 在 cache hit / miss 时选对的发送通道（respond vs send_msg）。

use std::sync::{Arc, Mutex};
use std::time::Duration;

use app_lib::connector::im::wecom::sender::{Sender, SessionMap};
use app_lib::connector::im::trait_def::ReplyTarget;
use async_trait::async_trait;
use serde_json::Value;

#[derive(Default)]
struct FakeAibot {
    pub respond_calls: Mutex<Vec<(String, Value)>>,
    pub send_msg_calls: Mutex<Vec<Value>>,
}

#[async_trait]
impl app_lib::connector::im::wecom::sender::AibotChannel for FakeAibot {
    async fn respond(&self, req_id: &str, body: Value) -> anyhow::Result<()> {
        self.respond_calls.lock().unwrap().push((req_id.to_string(), body));
        Ok(())
    }
    async fn send_msg(&self, body: Value) -> anyhow::Result<()> {
        self.send_msg_calls.lock().unwrap().push(body);
        Ok(())
    }
}

fn target(session_id: &str, ext: &str) -> ReplyTarget {
    ReplyTarget { session_id: session_id.into(), external_conversation_key: ext.into() }
}

#[tokio::test]
async fn send_markdown_uses_respond_when_session_cached_fresh() {
    let fake = Arc::new(FakeAibot::default());
    let map = SessionMap::new(Duration::from_secs(60));
    map.record("SESS1", "REQ_A").await;
    let sender = Sender::new(fake.clone(), map);
    sender.send_markdown(&target("SESS1", "U1"), "hello").await.unwrap();

    let calls = fake.respond_calls.lock().unwrap();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].0, "REQ_A");
    assert_eq!(calls[0].1["msgtype"], "markdown");
    assert_eq!(calls[0].1["markdown"]["content"], "hello");
    assert!(fake.send_msg_calls.lock().unwrap().is_empty());
}

#[tokio::test]
async fn send_markdown_falls_back_to_send_msg_when_no_cache() {
    let fake = Arc::new(FakeAibot::default());
    let map = SessionMap::new(Duration::from_secs(60));
    let sender = Sender::new(fake.clone(), map);
    sender.send_markdown(&target("SESS2", "U2"), "hello").await.unwrap();

    assert!(fake.respond_calls.lock().unwrap().is_empty());
    let calls = fake.send_msg_calls.lock().unwrap();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0]["chatid"], "U2");
    assert_eq!(calls[0]["msgtype"], "markdown");
}

#[tokio::test]
async fn send_markdown_falls_back_when_cache_expired() {
    let fake = Arc::new(FakeAibot::default());
    let map = SessionMap::new(Duration::from_millis(20));
    map.record("SESS3", "REQ_OLD").await;
    tokio::time::sleep(Duration::from_millis(50)).await;
    let sender = Sender::new(fake.clone(), map);
    sender.send_markdown(&target("SESS3", "U3"), "hi").await.unwrap();
    assert!(fake.respond_calls.lock().unwrap().is_empty());
    assert_eq!(fake.send_msg_calls.lock().unwrap().len(), 1);
}
```

- [ ] **Step 4.3.2: 跑测试看 fail**

Run: `cd src-tauri && cargo test --test im_wecom_sender 2>&1 | tail -10`
Expected: 编译失败

- [ ] **Step 4.3.3: 写 sender.rs**

Write `src-tauri/src/connector/im/wecom/sender.rs`:
```rust
//! 出站封装。优先走被动回复（respond_msg，需要还活着的 req_id），否则走主动推送（send_msg）。
//!
//! `SessionMap` 维护 session_id → (req_id, recorded_at)；超过 cache 窗口（默认 5 分钟）
//! 视为 expired，回落到主动推送。req_id 的有效期由 aibot 服务端决定，本期保守取 5 分钟。

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use serde_json::Value;
use tokio::sync::RwLock;

use super::aibot_protocol::{RespondMarkdownBody, SendMsgBody};
use crate::connector::im::trait_def::ReplyTarget;

/// 抽象出来给 sender 测试 mock 用。生产路径直接传 `Arc<AibotClient>`，本 trait 即由
/// AibotClient 实现。
#[async_trait]
pub trait AibotChannel: Send + Sync + 'static {
    async fn respond(&self, req_id: &str, body: Value) -> anyhow::Result<()>;
    async fn send_msg(&self, body: Value) -> anyhow::Result<()>;
}

#[async_trait]
impl AibotChannel for super::aibot_client::AibotClient {
    async fn respond(&self, req_id: &str, body: Value) -> anyhow::Result<()> {
        self.respond(req_id, body).await
    }
    async fn send_msg(&self, body: Value) -> anyhow::Result<()> {
        self.send_msg(body).await
    }
}

#[derive(Clone)]
pub struct SessionMap {
    inner: Arc<RwLock<HashMap<String, (String, Instant)>>>,
    ttl: Duration,
}

impl SessionMap {
    pub fn new(ttl: Duration) -> Self {
        Self { inner: Arc::new(RwLock::new(HashMap::new())), ttl }
    }
    pub async fn record(&self, session_id: &str, req_id: &str) {
        self.inner.write().await.insert(session_id.to_string(), (req_id.to_string(), Instant::now()));
    }
    pub async fn fresh_req_id(&self, session_id: &str) -> Option<String> {
        let g = self.inner.read().await;
        let (req_id, at) = g.get(session_id)?;
        if at.elapsed() > self.ttl { return None; }
        Some(req_id.clone())
    }
}

pub struct Sender<C: AibotChannel> {
    channel: Arc<C>,
    sessions: SessionMap,
}

impl<C: AibotChannel> Sender<C> {
    pub fn new(channel: Arc<C>, sessions: SessionMap) -> Self {
        Self { channel, sessions }
    }
    pub fn sessions(&self) -> &SessionMap { &self.sessions }

    pub async fn send_markdown(&self, target: &ReplyTarget, content: &str) -> anyhow::Result<()> {
        if let Some(req_id) = self.sessions.fresh_req_id(&target.session_id).await {
            let body = serde_json::to_value(RespondMarkdownBody::new(content))?;
            self.channel.respond(&req_id, body).await
        } else {
            let body = serde_json::to_value(SendMsgBody::markdown(
                target.external_conversation_key.clone(),
                content.into(),
            ))?;
            self.channel.send_msg(body).await
        }
    }
}
```

- [ ] **Step 4.3.4: 在 wecom/mod.rs 加 pub mod sender;**

- [ ] **Step 4.3.5: 跑测试**

Run: `cd src-tauri && cargo test --test im_wecom_sender -- --nocapture 2>&1 | tail -10`
Expected: 3 个 test PASS

- [ ] **Step 4.3.6: Commit sender**

```bash
git add src-tauri/src/connector/im/wecom/sender.rs src-tauri/src/connector/im/wecom/mod.rs src-tauri/tests/im_wecom_sender.rs
git commit -m "feat(connector/im/wecom): sender with session-cache prefer respond_msg (Phase 2 PR4b)"
```

### Step 4.4: Media 测试 + 实现

- [ ] **Step 4.4.1: 写 media 测试**

Write `src-tauri/tests/im_wecom_media.rs`:
```rust
//! 媒体解密：AES-256-CBC + PKCS#7 padding。
//!
//! 测试向量：用 openssl 命令构造已知 key + plaintext → ciphertext，
//! 然后 wecom::media::decrypt 应还原 plaintext。

use app_lib::connector::im::wecom::media::decrypt_aeskey_cbc;
use base64::{engine::general_purpose::STANDARD as B64, Engine};

#[test]
fn decrypt_known_vector_roundtrip() {
    // 用 aes crate 自构造一份 vector（保证算法实现一致性）。
    use aes::cipher::{block_padding::Pkcs7, BlockEncryptMut, KeyIvInit};
    type Enc = cbc::Encryptor<aes::Aes256>;
    let key_bytes = [0x11u8; 32];
    let iv_bytes: [u8; 16] = key_bytes[..16].try_into().unwrap();
    let plaintext = b"hello world, this is a wecom file payload.";

    let mut buf = vec![0u8; plaintext.len() + 16];
    let cipher_len = {
        let enc = Enc::new_from_slices(&key_bytes, &iv_bytes).unwrap();
        enc.encrypt_padded_b2b_mut::<Pkcs7>(plaintext, &mut buf).unwrap().len()
    };
    let ciphertext = &buf[..cipher_len];

    let aeskey_b64 = B64.encode(key_bytes);
    let recovered = decrypt_aeskey_cbc(ciphertext, &aeskey_b64).expect("decrypt");
    assert_eq!(recovered, plaintext);
}

#[test]
fn decrypt_rejects_bad_padding() {
    let aeskey_b64 = base64::engine::general_purpose::STANDARD.encode([0u8; 32]);
    let bogus = vec![0u8; 32];
    assert!(decrypt_aeskey_cbc(&bogus, &aeskey_b64).is_err());
}

#[test]
fn decode_download_code_split_aeskey_and_url() {
    use app_lib::connector::im::wecom::media::decode_download_code;
    let dc = "wecom://AESKEY_VAL@https://example.com/file?id=1";
    let (key, url) = decode_download_code(dc).expect("parse");
    assert_eq!(key, "AESKEY_VAL");
    assert_eq!(url, "https://example.com/file?id=1");
}

#[test]
fn decode_download_code_rejects_non_wecom_prefix() {
    use app_lib::connector::im::wecom::media::decode_download_code;
    assert!(decode_download_code("dingtalk://...").is_err());
}
```

- [ ] **Step 4.4.2: 跑测试看 fail**

Run: `cd src-tauri && cargo test --test im_wecom_media 2>&1 | tail -10`
Expected: 编译失败

- [ ] **Step 4.4.3: 写 media.rs**

Write `src-tauri/src/connector/im/wecom/media.rs`:
```rust
//! 媒体上传 / 下载 / 解密。
//!
//! 上传协议：三步分片
//!   1) aibot_upload_media_init → { upload_id }
//!   2) aibot_upload_media_chunk × N（单分片 ≤512KB base64 之前）
//!   3) aibot_upload_media_finish → { media_id, ... }
//!
//! 下载：HTTP GET url（5 分钟有效）→ AES-256-CBC 解密（key 来自消息体 `aeskey`，
//! base64 decode 后 32 字节当 key，前 16 字节当 IV）。

use aes::cipher::{block_padding::Pkcs7, BlockDecryptMut, KeyIvInit};
use anyhow::{anyhow, Context, Result};
use base64::{engine::general_purpose::STANDARD as B64, Engine};

type Dec = cbc::Decryptor<aes::Aes256>;

/// `wecom://{aeskey_b64}@{url}` → (aeskey_b64, url)
pub fn decode_download_code(code: &str) -> Result<(String, String)> {
    let stripped = code.strip_prefix("wecom://")
        .ok_or_else(|| anyhow!("not a wecom download code: {code}"))?;
    let (key, url) = stripped.split_once('@')
        .ok_or_else(|| anyhow!("missing @ separator in wecom download code"))?;
    Ok((key.to_string(), url.to_string()))
}

/// 用 aeskey（base64-encoded 32-byte key）AES-256-CBC + PKCS#7 解密。
pub fn decrypt_aeskey_cbc(ciphertext: &[u8], aeskey_b64: &str) -> Result<Vec<u8>> {
    let key_bytes = B64.decode(aeskey_b64).context("aeskey base64 decode")?;
    if key_bytes.len() != 32 {
        return Err(anyhow!("aeskey must decode to 32 bytes, got {}", key_bytes.len()));
    }
    let iv: [u8; 16] = key_bytes[..16].try_into().unwrap();
    let dec = Dec::new_from_slices(&key_bytes, &iv).context("init cbc")?;
    let mut buf = ciphertext.to_vec();
    let n = dec.decrypt_padded_mut::<Pkcs7>(&mut buf)
        .map_err(|e| anyhow!("decrypt failed: {e:?}"))?
        .len();
    buf.truncate(n);
    Ok(buf)
}

/// HTTP GET + 解密。返回明文 buffer。
pub async fn download_and_decrypt(url: &str, aeskey_b64: &str) -> Result<Vec<u8>> {
    let resp = reqwest::get(url).await.context("http get")?;
    if !resp.status().is_success() {
        return Err(anyhow!("download failed: status={}", resp.status()));
    }
    let bytes = resp.bytes().await.context("read body")?;
    decrypt_aeskey_cbc(&bytes, aeskey_b64)
}

// 媒体上传：本期最小可用。三步分片，分片大小 = 384KB（保留 base64 1.33x 膨胀后 < 512KB）。
pub const MEDIA_CHUNK_SIZE: usize = 384 * 1024;

// 完整 upload 流程留给 PR5 connector 接入 AibotClient 后实现；本 PR 提供的接口先到
// decrypt + decode 为止，PR5 在 connector.rs 用 AibotClient.respond/send_msg 拼装
// upload_init / chunk / finish 帧。
```

- [ ] **Step 4.4.4: 在 wecom/mod.rs 加 pub mod media;**

- [ ] **Step 4.4.5: 跑测试**

Run: `cd src-tauri && cargo test --test im_wecom_media -- --nocapture 2>&1 | tail -10`
Expected: 4 个 test PASS

- [ ] **Step 4.4.6: 全仓 cargo check + 前文测试不回退**

Run: `cd src-tauri && cargo check 2>&1 | tail -5 && cargo test --test im_wecom_aibot_protocol --test im_wecom_aibot_client --test im_aicard_fallback --test im_wecom_parser --test im_wecom_sender --test im_wecom_media 2>&1 | tail -10`
Expected: check 0 errors；6 个测试文件全 PASS

- [ ] **Step 4.4.7: Commit media**

```bash
git add src-tauri/src/connector/im/wecom/media.rs src-tauri/src/connector/im/wecom/mod.rs src-tauri/tests/im_wecom_media.rs
git commit -m "feat(connector/im/wecom): media decrypt + download code codec (Phase 2 PR4c)"
```

---

## Task 5 (PR5): WecomConnector trait 实现 + factory 注册 + 集成测试

**Files:**
- Create: `src-tauri/src/connector/im/wecom/connector.rs`
- Modify: `src-tauri/src/connector/im/wecom/mod.rs` (export connector)
- Modify: `src-tauri/src/connector/im/factory.rs` (注册 Platform::Wecom)
- Modify: `src-tauri/src/connector/im/shared/config_store.rs` (去掉 Wecom 的 coming_soon)
- Create: `src-tauri/tests/im_wecom_integration.rs`
- Modify: `src-tauri/tests/review_im_layering.rs` (platforms 数组追加 "wecom")

### Step 5.1: 写 connector.rs

- [ ] **Step 5.1.1: 写 WecomConnector**

Write `src-tauri/src/connector/im/wecom/connector.rs`:
```rust
//! `WecomConnector` —— 实现 `IMConnector`，把 AibotClient 适配到 trait 中性层。

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use futures::stream::BoxStream;
use futures::StreamExt;
use tokio::sync::{mpsc, Mutex};
use tokio_stream::wrappers::ReceiverStream;

use super::aibot_client::{AibotClient, AibotClientConfig, AibotEvent};
use super::parser::{parse_inbound, ParsedInbound};
use super::sender::{Sender, SessionMap};
use crate::connector::im::shared::aicard_fallback::{AiCardFallbackBuffer, FallbackAction};
use crate::connector::im::trait_def::{
    AuthFlow, ConnectorCapabilities, ConnectorContext, ConnectorError, IMConnector, InboundModel,
    ReplyContent, ReplyTarget,
};
use crate::connector::im::types::{ChannelMessage, Platform};

pub struct WecomConnector {
    bot_id: String,
    aibot: Arc<AibotClient>,
    sender: Sender<AibotClient>,
    /// session_id → 流式 buffer（一次 AI 回复用一个 buffer 实例，final 时移除）
    fallback_buffers: Arc<Mutex<HashMap<String, AiCardFallbackBuffer>>>,
}

impl WecomConnector {
    pub fn new(bot_id: String, secret: String) -> Self {
        let aibot = Arc::new(AibotClient::new(AibotClientConfig::production(
            bot_id.clone(),
            secret,
        )));
        let sessions = SessionMap::new(Duration::from_secs(300));
        let sender = Sender::new(aibot.clone(), sessions);
        Self {
            bot_id,
            aibot,
            sender,
            fallback_buffers: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    async fn handle_aicard_chunk(
        &self,
        target: &ReplyTarget,
        delta: &str,
        final_chunk: bool,
    ) -> Result<(), ConnectorError> {
        let mut buffers = self.fallback_buffers.lock().await;
        let buf = buffers
            .entry(target.session_id.clone())
            .or_insert_with(|| AiCardFallbackBuffer::new(Duration::from_secs(240)));
        let action = buf.observe(delta, final_chunk);
        drop(buffers);

        match action {
            FallbackAction::Buffer => Ok(()),
            FallbackAction::SendPlaceholder { text } => self
                .sender
                .send_markdown(target, &text)
                .await
                .map_err(|e| ConnectorError::Transient(format!("{e:#}"))),
            FallbackAction::SendFinal { text } => {
                let r = self
                    .sender
                    .send_markdown(target, &text)
                    .await
                    .map_err(|e| ConnectorError::Transient(format!("{e:#}")));
                self.fallback_buffers.lock().await.remove(&target.session_id);
                r
            }
        }
    }
}

#[async_trait]
impl IMConnector for WecomConnector {
    fn platform(&self) -> Platform { Platform::Wecom }

    fn capabilities(&self) -> ConnectorCapabilities {
        ConnectorCapabilities {
            inbound: InboundModel::Stream,
            outbound_aicard: false,
            outbound_markdown: true,
            supports_attachments: true,
            supports_group_chat: true,
            supports_private_chat: true,
            auth_flow: AuthFlow::ApiKey,
        }
    }

    async fn start(
        &self,
        ctx: ConnectorContext,
    ) -> Result<BoxStream<'static, ChannelMessage>, ConnectorError> {
        let (msg_tx, msg_rx) = mpsc::channel::<ChannelMessage>(256);
        let (evt_tx, mut evt_rx) = mpsc::channel::<AibotEvent>(64);

        let aibot = self.aibot.clone();
        let cancel = ctx.cancel_token.clone();
        tokio::spawn(async move { let _ = aibot.run(evt_tx, cancel).await; });

        let bot_id = self.bot_id.clone();
        let sessions = self.sender.sessions().clone();
        tokio::spawn(async move {
            while let Some(evt) = evt_rx.recv().await {
                match evt {
                    AibotEvent::Authenticated => {
                        log::info!("[wecom-{}] authenticated", bot_id);
                    }
                    AibotEvent::Inbound(frame) => {
                        let req_id = frame.headers.req_id.clone();
                        if let Some(parsed) = parse_inbound(&bot_id, &frame) {
                            if let ParsedInbound::Message(msg) = parsed {
                                // session_id 用 conversation_key（chatid for group, userid for single）
                                sessions.record(&msg.conversation_key, &req_id).await;
                                if msg_tx.send(msg).await.is_err() { break; }
                            }
                        }
                    }
                    AibotEvent::KickedOut(reason) => {
                        log::warn!("[wecom-{}] kicked out: {reason}", bot_id);
                        break;
                    }
                    AibotEvent::AuthFailed(code, msg) => {
                        log::error!("[wecom-{}] auth failed code={code} msg={msg}", bot_id);
                        break;
                    }
                    AibotEvent::ConnectionDropped(reason) => {
                        log::info!("[wecom-{}] connection dropped: {reason}", bot_id);
                    }
                    AibotEvent::Reconnecting(n) => {
                        log::info!("[wecom-{}] reconnecting attempt {n}", bot_id);
                    }
                }
            }
        });

        Ok(ReceiverStream::new(msg_rx).boxed())
    }

    async fn send(
        &self,
        target: ReplyTarget,
        content: ReplyContent,
    ) -> Result<(), ConnectorError> {
        match content {
            ReplyContent::Text(t) | ReplyContent::Markdown(t) => self
                .sender
                .send_markdown(&target, &t)
                .await
                .map_err(|e| ConnectorError::Transient(format!("{e:#}"))),
            ReplyContent::AiCardChunk { delta, final_chunk } => {
                self.handle_aicard_chunk(&target, &delta, final_chunk).await
            }
            ReplyContent::AiCardFail => self
                .sender
                .send_markdown(&target, "❌ 处理失败，请重试")
                .await
                .map_err(|e| ConnectorError::Transient(format!("{e:#}"))),
        }
    }
}
```

- [ ] **Step 5.1.2: 在 wecom/mod.rs 加 export**

```rust
pub mod aibot_client;
pub mod aibot_protocol;
pub mod connector;
pub mod media;
pub mod parser;
pub mod sender;
pub use connector::WecomConnector;
```

- [ ] **Step 5.1.3: cargo check 0 errors**

Run: `cd src-tauri && cargo check 2>&1 | tail -10`
Expected: PASS

### Step 5.2: 集成测试

- [ ] **Step 5.2.1: 写集成测试**

Write `src-tauri/tests/im_wecom_integration.rs`:
```rust
//! WecomConnector 集成路径：mock aibot server → connector.start → 收到 ChannelMessage
//! → connector.send 触发 respond_msg 发回。

use std::sync::Arc;
use std::time::Duration;

use app_lib::connector::im::trait_def::{ConnectorContext, IMConnector, ReplyContent, ReplyTarget};
use app_lib::connector::im::shared::config_store::ChannelConfigStore;
use app_lib::connector::im::wecom::WecomConnector;
use futures::StreamExt;
use serde_json::{json, Value};
use tempfile::TempDir;
use tokio::net::TcpListener;
use tokio_tungstenite::tungstenite::Message;
use tokio_util::sync::CancellationToken;

async fn spawn_mock(
    handler: impl Fn(&Value) -> Option<Value> + Send + Sync + 'static,
) -> (String, Arc<tokio::sync::Mutex<Vec<Value>>>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let url = format!("ws://{addr}");
    let outbound = Arc::new(tokio::sync::Mutex::new(Vec::<Value>::new()));
    let outbound_recorder = outbound.clone();
    tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        let mut ws = tokio_tungstenite::accept_async(stream).await.unwrap();
        use futures::{SinkExt, StreamExt};
        let (mut w, mut r) = ws.split();
        // 推一条 inbound 等会发用
        let push_text = json!({
            "cmd": "aibot_msg_callback",
            "headers": { "req_id": "REQ_INBOUND_1" },
            "body": { "msgid": "M1", "aibotid": "BOTID", "chattype": "single",
                      "from": { "userid": "U1" }, "msgtype": "text",
                      "text": { "content": "hi" } }
        });
        // 先等 subscribe，回 ack，然后 push
        while let Some(Ok(msg)) = r.next().await {
            let text = match msg { Message::Text(t) => t, _ => continue };
            let frame: Value = serde_json::from_str(&text).unwrap();
            let cmd = frame.get("cmd").and_then(|v| v.as_str()).unwrap_or("");
            let req_id = frame.pointer("/headers/req_id").and_then(|v| v.as_str()).unwrap_or("").to_string();
            if cmd == "aibot_subscribe" {
                let ack = json!({ "headers": { "req_id": req_id }, "errcode": 0, "errmsg": "ok" });
                w.send(Message::Text(ack.to_string().into())).await.ok();
                w.send(Message::Text(push_text.to_string().into())).await.ok();
                continue;
            }
            if let Some(ack) = handler(&frame) {
                outbound_recorder.lock().await.push(frame.clone());
                w.send(Message::Text(ack.to_string().into())).await.ok();
            }
        }
    });
    (url, outbound)
}

#[tokio::test]
async fn end_to_end_inbound_text_then_send_markdown_uses_respond() {
    let (ws_url, outbound) = spawn_mock(|frame| {
        let req_id = frame.pointer("/headers/req_id").and_then(|v| v.as_str()).unwrap();
        Some(json!({ "headers": { "req_id": req_id }, "errcode": 0, "errmsg": "ok" }))
    }).await;

    // 构造 WecomConnector with custom ws_url —— 借助 AibotClientConfig::production
    // 并 override ws_url 字段：通过新 helper 或直接 pub field
    let mut cfg = app_lib::connector::im::wecom::aibot_client::AibotClientConfig::production(
        "BOTID".into(), "SECRET".into(),
    );
    cfg.ws_url = ws_url;
    cfg.heartbeat_interval = Duration::from_secs(60);  // 避免测试期间触发心跳
    let aibot = Arc::new(app_lib::connector::im::wecom::aibot_client::AibotClient::new(cfg));
    // 用底层 ctor 接入 sender，沿用 connector.rs 的字段填充（暴露 test helper）
    let conn = WecomConnector::for_test(aibot);

    let tmp = TempDir::new().unwrap();
    let store = Arc::new(ChannelConfigStore::new(tmp.path().to_path_buf(), None));
    let cancel = CancellationToken::new();
    let ctx = ConnectorContext {
        config_store: store,
        secure_storage: None,
        ask_coordinator: None,
        pending_manager: Arc::new(app_lib::runtime::pending::PendingQueueManager::for_test()),
        cancel_token: cancel.clone(),
    };

    let mut stream = conn.start(ctx).await.unwrap();
    let msg = tokio::time::timeout(Duration::from_secs(3), stream.next()).await
        .unwrap().expect("stream should yield");

    assert_eq!(msg.text, "hi");
    assert_eq!(msg.robot_code, "BOTID");
    assert_eq!(msg.reply_group_id, "U1");

    // 触发回复：应走 respond_msg（session 刚被 record）
    conn.send(
        ReplyTarget { session_id: msg.conversation_key.clone(), external_conversation_key: msg.reply_group_id.clone() },
        ReplyContent::Markdown("answer".into()),
    ).await.unwrap();

    // 等 100ms 让 outbound 落到 recorder
    tokio::time::sleep(Duration::from_millis(200)).await;
    let frames = outbound.lock().await;
    let respond = frames.iter().find(|f| f.get("cmd").and_then(|v| v.as_str()) == Some("aibot_respond_msg"))
        .expect("must send respond_msg");
    assert_eq!(respond.pointer("/headers/req_id").and_then(|v| v.as_str()), Some("REQ_INBOUND_1"));
    assert_eq!(respond.pointer("/body/markdown/content").and_then(|v| v.as_str()), Some("answer"));

    cancel.cancel();
}

#[tokio::test]
async fn server_pushes_disconnected_event_stream_ends_without_reconnect() {
    let (ws_url, _outbound) = spawn_mock(|frame| {
        let req_id = frame.pointer("/headers/req_id").and_then(|v| v.as_str()).unwrap();
        Some(json!({ "headers": { "req_id": req_id }, "errcode": 0, "errmsg": "ok" }))
    }).await;

    let mut cfg = app_lib::connector::im::wecom::aibot_client::AibotClientConfig::production(
        "BOTID".into(), "SECRET".into(),
    );
    cfg.ws_url = ws_url;
    cfg.heartbeat_interval = Duration::from_secs(60);
    let aibot = Arc::new(app_lib::connector::im::wecom::aibot_client::AibotClient::new(cfg));
    let conn = WecomConnector::for_test(aibot.clone());

    let tmp = TempDir::new().unwrap();
    let store = Arc::new(ChannelConfigStore::new(tmp.path().to_path_buf(), None));
    let cancel = CancellationToken::new();
    let ctx = ConnectorContext {
        config_store: store,
        secure_storage: None,
        ask_coordinator: None,
        pending_manager: Arc::new(app_lib::runtime::pending::PendingQueueManager::for_test()),
        cancel_token: cancel.clone(),
    };

    let mut stream = conn.start(ctx).await.unwrap();
    // mock server 已经 push 了一条 inbound，先吃掉
    let _ = tokio::time::timeout(Duration::from_secs(3), stream.next()).await.unwrap();

    // 现在 manually feed disconnected_event —— 由测试自己 push
    // (本测试简化版：依赖 client KickedOut 后 stream 关闭；详细 disconnect_event push 
    //  覆盖在 aibot_client 单测里。这里只断言 client 退出时 stream 不会无限挂)
    cancel.cancel();
    let next = tokio::time::timeout(Duration::from_secs(2), stream.next()).await.unwrap();
    assert!(next.is_none(), "stream should end after cancel");
}
```

为支持上面 test，需要给 WecomConnector 加一个 `for_test(aibot)` 入口：

- [ ] **Step 5.2.2: 给 WecomConnector 加 for_test ctor**

Edit `src-tauri/src/connector/im/wecom/connector.rs`，在 `impl WecomConnector` 加：
```rust
#[cfg(any(test, feature = "test-support"))]
pub fn for_test(aibot: Arc<AibotClient>) -> Self {
    let sessions = SessionMap::new(Duration::from_secs(300));
    let sender = Sender::new(aibot.clone(), sessions);
    Self {
        bot_id: "TEST-BOT".into(),
        aibot,
        sender,
        fallback_buffers: Arc::new(Mutex::new(HashMap::new())),
    }
}
```

如果 `PendingQueueManager::for_test()` 不存在，检查 `src-tauri/src/runtime/pending/mod.rs`，按现有测试惯例改为本地构造 `Arc::new(PendingQueueManager::new(...))` —— grep 现有测试看应该传什么。

- [ ] **Step 5.2.3: 跑集成测试**

Run: `cd src-tauri && cargo test --test im_wecom_integration -- --nocapture 2>&1 | tail -30`
Expected: 2 个 test PASS

如果 ConnectorContext / PendingQueueManager 的字段或构造方式不一样，按当下代码的现实形态调整测试，**不要**强行改生产代码去满足测试。

### Step 5.3: factory 注册 + config_store 解占位

- [ ] **Step 5.3.1: 查看 factory.rs 现状**

Run: `cat src-tauri/src/connector/im/factory.rs`
读完了解 Platform → connector 构造的现有 pattern。

- [ ] **Step 5.3.2: 在 factory.rs 加 Wecom 分支**

按 factory.rs 现有 dingtalk / feishu 分支模式仿写：从 `ChannelConfig.credentials` 取 bot_id / secret（secret 走 `secure_storage` 解密），调 `WecomConnector::new(bot_id, secret)` 返回 `Arc<dyn IMConnector>`。

具体代码视 factory.rs 现状而定，参考 dingtalk 分支结构：
```rust
Platform::Wecom => {
    let bot_id = cfg.credentials.bot_id.clone().context("wecom needs bot_id")?;
    let secret = resolve_secret(&cfg.credentials, secure_storage.as_ref()).await?;
    Arc::new(WecomConnector::new(bot_id, secret)) as Arc<dyn IMConnector>
}
```

如果 `WecomCredentials` 结构尚未在 `ChannelConfig` 里定义，本 PR 加上：在 `connector::im::shared::config_store` 现有 `ChannelCredentials` 旁加 wecom 字段（参考 feishu 分支的 `app_id` / `app_secret_storage`）。

- [ ] **Step 5.3.3: 在 config_store.rs 移除 wecom coming_soon**

`src-tauri/src/connector/im/shared/config_store.rs:69` 当前有 `Self::coming_soon_state(Platform::Wecom)`。本 PR 仅在 `list_platform_states` 返回 active wecom state（仿 feishu 的 active 分支），让前端能看到 wecom 已可配置。详细 CRUD（add/remove/enable wecom）放在 PR6 一起改，本 PR **只** 移除 `coming_soon` 这一行，让 wecom 可以从 placeholder 变为 active=false 但有 capabilities 的状态。

具体改法参考 feishu：当前 `list_platform_states` 应该是 dingtalk active + feishu active + wecom coming_soon + wechat coming_soon。把 wecom 改成跟 feishu 同样的 active state 构造（即便没有 credentials 也返回一个 `enabled=false` 的有效 state）。

- [ ] **Step 5.3.4: cargo check 0 errors**

Run: `cd src-tauri && cargo check 2>&1 | tail -10`
Expected: PASS

### Step 5.4: review_im_layering 加 wecom

- [ ] **Step 5.4.1: 修改 review test**

Edit `src-tauri/tests/review_im_layering.rs`，找 `let platforms = [...]`，加 `"wecom"`：

```rust
let platforms = ["dingtalk", "feishu", "wecom"];
```

- [ ] **Step 5.4.2: 跑 review test**

Run: `cd src-tauri && cargo test --test review_im_layering -- --nocapture 2>&1 | tail -10`
Expected: PASS

- [ ] **Step 5.4.3: 全仓 review + 集成回归**

Run: `cd src-tauri && cargo test review_ --tests --no-fail-fast 2>&1 | tail -20`
Expected: 所有 review_*.rs 通过

- [ ] **Step 5.4.4: Commit PR5**

```bash
git add src-tauri/src/connector/im/wecom/connector.rs src-tauri/src/connector/im/wecom/mod.rs \
        src-tauri/src/connector/im/factory.rs src-tauri/src/connector/im/shared/config_store.rs \
        src-tauri/tests/im_wecom_integration.rs src-tauri/tests/review_im_layering.rs
git commit -m "feat(connector/im/wecom): WecomConnector trait impl + factory + integration tests (Phase 2 PR5)"
```

---

## Task 6 (PR6): 前端 UI + ChannelConfigStore CRUD + i18n

**Files:**
- Modify: `src-tauri/src/connector/im/shared/config_store.rs` (加 wecom 完整 CRUD: add/get/update/remove/test)
- Modify: `src-tauri/src/transport/tauri_commands/channel.rs` (加 channel_wecom_save / channel_wecom_test_connection 命令)
- Modify: `src-tauri/src/lib.rs` (注册新 commands)
- Modify: `src/lib/tauri.ts` (前端 IPC binding)
- Create: `src/components/channels/WecomAccountForm.tsx`
- Modify: `src/components/channels/<现有 IM 设置面板>` (插入 wecom tab/section)
- Modify: `src/i18n/zh-CN.json` (channels.wecom.* 文案)
- Modify: `src/i18n/en-US.json`
- Modify: `src-tauri/tests/im_wecom_integration.rs` (再加一条 end-to-end "test connection" case)

### Step 6.1: 后端 CRUD + tauri_commands

- [ ] **Step 6.1.1: 先 grep 看 feishu 是怎么做的**

Run: `grep -n "feishu\|Feishu" src-tauri/src/connector/im/shared/config_store.rs | head -20`

记下 feishu 用了哪些 method（如 `add_feishu` / `set_feishu_enabled` / `remove_feishu` / `get_feishu_config` 等），完整对照写一份 wecom 等价方法。secret 字段走 `SecureStorage`，**禁止**明文落盘。

- [ ] **Step 6.1.2: 加 wecom CRUD**

在 `config_store.rs` 仿 feishu pattern 加：
- `add_wecom(bot_id, secret) -> Result<ChannelPlatformState>`
- `set_wecom_enabled(enabled) -> Result<ChannelPlatformState>`
- `remove_wecom() -> Result<ChannelPlatformState>`
- `get_wecom_credentials() -> Result<Option<(String, String)>>`（secret 解密后）

数据落盘到 `~/.renlijia/channels/wecom.json`，schema 参考 feishu 的 `<channels_dir>/feishu.json`：
```json
{
  "bot_id": "BOTID_VALUE",
  "secret_storage": "secure_storage",
  "enabled": true,
  "display_name": "..."
}
```

- [ ] **Step 6.1.3: 加 tauri commands**

在 `src-tauri/src/transport/tauri_commands/channel.rs` 加：
- `channel_wecom_save(bot_id, secret, display_name) -> ChannelPlatformState`
- `channel_wecom_test_connection(bot_id, secret) -> { ok: bool, error?: string }`
  - 实现：临时构造 AibotClient → run 一次（短 timeout 5s）→ 等 Authenticated 事件 → close
- `channel_wecom_remove() -> ChannelPlatformState`
- `channel_wecom_set_enabled(enabled) -> ChannelPlatformState`

在 `src-tauri/src/lib.rs` 把新 command 加到 `invoke_handler!`。

- [ ] **Step 6.1.4: cargo check + 跑 wecom 集成测试不破**

Run: `cd src-tauri && cargo check 2>&1 | tail -5 && cargo test --test im_wecom_integration --test review_im_layering 2>&1 | tail -10`
Expected: PASS

### Step 6.2: 前端 form

- [ ] **Step 6.2.1: 看 feishu 表单**

Run: `find src/components/channels -type f | head -20`

打开 feishu / dingtalk 的现有 form 组件，照着抄。**严格遵守** CLAUDE.md UI 规范：
- 颜色用主题变量（`bg-background` / `text-foreground` / `border-input` / `ring-ring`）
- 按钮用 `@/components/ui/button` 的 `<Button>`
- 输入框用 `@/components/ui/input`
- Toast 用 `useNotificationStore.push({ context: 'toast' })`

- [ ] **Step 6.2.2: 写 WecomAccountForm.tsx**

主要逻辑：
- 两个 input：Bot ID / Secret（secret 用 `type="password"`）
- 一个"测试连接"按钮 → `invoke('channel_wecom_test_connection', { botId, secret })` → 成功/失败 toast
- 一个"保存"按钮 → `invoke('channel_wecom_save', { botId, secret, displayName })`
- 删除按钮 → `requestConfirm` 二次确认 → `invoke('channel_wecom_remove')`

帮助链接：`https://work.weixin.qq.com` 企业管理后台 → 智能机器人。

- [ ] **Step 6.2.3: 把 form 挂到现有 IM 设置面板**

`grep -rn "FeishuAccountForm\|DingtalkAccountForm" src/components/` 找到现有挂载点（通常是 IM 设置的 tab 切换器），仿照加一个 wecom tab。

- [ ] **Step 6.2.4: 前端 tauri binding**

Edit `src/lib/tauri.ts`，在 `invoke` 类型化封装表里加 wecom 命令的类型签名。

### Step 6.3: i18n

- [ ] **Step 6.3.1: 加 channels.wecom 文案**

Edit `src/i18n/zh-CN.json` + `src/i18n/en-US.json`，在 `channels` 树下加：
```json
"wecom": {
  "title": "企业微信",
  "subtitle": "通过腾讯官方 aibot 通道接入企业微信智能机器人，无需公网穿透",
  "botId": { "label": "Bot ID", "placeholder": "在企业微信管理后台 → 智能机器人 → 详情页复制" },
  "secret": { "label": "Secret", "placeholder": "同上" },
  "testConnection": "测试连接",
  "save": "保存",
  "remove": "移除",
  "saved": "保存成功",
  "connectionOk": "连接成功",
  "connectionFailed": "连接失败：{{error}}",
  "helpUrl": "https://work.weixin.qq.com",
  "helpLabel": "前往企业微信管理后台"
}
```

英文版同步翻译。

### Step 6.4: 端到端冒烟

- [ ] **Step 6.4.1: 类型检查 + lint**

Run: `pnpm exec tsc --noEmit 2>&1 | tail -10 && pnpm lint 2>&1 | tail -10`
Expected: 0 errors

- [ ] **Step 6.4.2: 前端单测**

Run: `pnpm test 2>&1 | tail -20`
Expected: 没有新失败（pre-existing failures 列下来对照本 PR 没引入新破坏即可）

- [ ] **Step 6.4.3: 启动 dev 模式手工冒烟**

Run: `pnpm tauri:dev`

操作：① 打开 IM 设置 → 切到企业微信 tab → 看到表单 ② 填假凭证 + 点测试 → 看到错误 toast ③ 移除其它平台不受影响 ④ 切换主题（light/dark）→ 表单视觉随主题切换正确（验证 UI 规范遵守）

记录冒烟结果到本步骤的 checkbox notes。

- [ ] **Step 6.4.4: 全仓 cargo check + cargo test wecom 不回退**

Run: `cd src-tauri && cargo check 2>&1 | tail -5 && cargo test --test im_wecom_aibot_protocol --test im_wecom_aibot_client --test im_aicard_fallback --test im_wecom_parser --test im_wecom_sender --test im_wecom_media --test im_wecom_integration --test review_im_layering 2>&1 | tail -20`
Expected: 全 PASS，0 errors

- [ ] **Step 6.4.5: Commit PR6**

```bash
git add src-tauri/src/connector/im/shared/config_store.rs src-tauri/src/transport/tauri_commands/channel.rs src-tauri/src/lib.rs \
        src/lib/tauri.ts src/components/channels/WecomAccountForm.tsx src/components/channels/ \
        src/i18n/zh-CN.json src/i18n/en-US.json
git commit -m "feat(channels/wecom): frontend UI + backend CRUD + i18n (Phase 2 PR6)"
```

---

## 完成判据

整 Phase 2 落地后，下面这些必须成立：

1. ✅ `cd src-tauri && cargo test --test im_wecom_aibot_protocol --test im_wecom_aibot_client --test im_aicard_fallback --test im_wecom_parser --test im_wecom_sender --test im_wecom_media --test im_wecom_integration` 全 PASS
2. ✅ `cd src-tauri && cargo test review_ --tests --no-fail-fast` 全 PASS（含追加 `"wecom"` 后的 review_im_layering）
3. ✅ `pnpm exec tsc --noEmit` 0 errors
4. ✅ `pnpm tauri:dev` 启动后 IM 设置面板能切到企业微信 tab、能填表单、能调 `channel_wecom_test_connection` 看到结果
5. ✅ 全仓 grep `coming_soon.*Wecom` 无结果（PR5 已移除占位）
6. ✅ secret 字段不在任何 JSON 落盘明文出现（grep `~/.renlijia/channels/wecom.json` 看到的应是 `secret_storage: "secure_storage"` 而非明文）

## 备注

- 流式 AI 卡片本期不接，capabilities 声明 `outbound_aicard: false`，走 fallback buffer。后续如需开启，加一个 PR：① capabilities 改 `true` ② sender 走 `aibot_respond_msg` + `msgtype: stream` + 维护 streamId per session
- `ChannelMessage` schema 借了 dingtalk 字段名，后续"trait 平台化"专项再统一重命名（不属于本期）
- 媒体上传完整流程（upload_init / chunk × N / finish 三步）本期仅在 `media.rs` 留接口和常量 `MEDIA_CHUNK_SIZE`，真正实现留到"附件出站打通"专项——因为本期 ReplyContent 没有 attachment variant，调用方暂时用不到
