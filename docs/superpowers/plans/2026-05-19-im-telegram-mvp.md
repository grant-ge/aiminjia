# Telegram IM Connector MVP Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 实现 Telegram bot IM 渠道 MVP：用户在 @BotFather 拿 token → 桌面端输入 token → 后端 getMe 验证 + 落盘 → 桌面端弹 QR（`t.me/<bot>?start=<pairing_code>`）→ 用户用手机 Telegram 扫码 + 桌面端手动批准 → 加入 allowlist → 私聊 bot 走 AIjia chat turn → markdown 回复。

**Architecture:** 完全镜像 `connector/im/wecom/` 的目录结构和 manager 接入路径。新增 `connector/im/telegram/`，所有 Bot API 调用走自写 reqwest（不引入 teloxide SDK）。Pairing 协议：内存 `PairingCodeStore`（5 min TTL，重启清空）+ allowlist 落盘到 config.json。不引入 trait 改造（保持 `InboundModel`、不加 `outbound_text_streaming`）。

**Tech Stack:** Rust async (tokio + tokio-util), reqwest 0.12 (json), async-trait, serde / serde_json, anyhow / thiserror, chrono；前端 React + qrcode（已存在）+ vitest；测试用 wiremock 0.6（已有依赖）。

**Spec:** `docs/superpowers/specs/2026-05-19-im-telegram-connector-design.md`

**Bot API reference:** https://core.telegram.org/bots/api

---

## File Structure

```
src-tauri/src/connector/im/
├── telegram/                       ← 新增
│   ├── mod.rs                      ← pub re-exports
│   ├── api.rs                      ← reqwest 客户端 + Bot API 包装（getMe / getUpdates / sendMessage / setMyCommands）
│   ├── connector.rs                ← impl IMConnector for TelegramConnector
│   ├── long_poll.rs                ← getUpdates 长轮询循环 + offset 持久化 + ReconnectBackoff + MessageDedupSet
│   ├── parser.rs                   ← TgUpdate → ChannelMessage + /start <code> 识别
│   ├── sender.rs                   ← sendMessage markdown + MarkdownV2 转义 + 429 retry + 400 fallback
│   ├── pairing.rs                  ← PairingCodeStore (内存) + allowlist 写盘
│   ├── reply_forwarder.rs          ← RuntimeEventBus 订阅 → connector.send(Markdown)
│   ├── registration.rs             ← begin_pairing / list_pending / approve / revoke
│   └── types.rs                    ← TelegramStoredConfig / TelegramBotInfo / AllowlistEntry / TELEGRAM_SOURCE
├── shared/config_store.rs          ← 扩展：read/save/decrypt/validate Telegram config + telegram_state
├── factory.rs                      ← 新增 build_telegram_connector
├── manager.rs                      ← 接入 Telegram connector + auto_connect + set_enabled + remove_platform
├── mod.rs                          ← pub mod telegram
└── types.rs                        ← 已含 Platform::Telegram，不动

src-tauri/src/commands/channel.rs   ← 新增 7 个 channel_telegram_* 命令
src-tauri/src/lib.rs                ← invoke_handler 注册新命令
src-tauri/src/storage/aijia_home.rs ← 新增 tmp_telegram_downloads_dir helper（占位，MVP 不下载，但接口对称）
src-tauri/tests/telegram_pairing_integration_test.rs  ← 集成测试

src/features/channel/
├── TelegramChannelConfig.tsx       ← 新增配置弹窗
├── ChannelPage.tsx                 ← 接入 telegram 卡片
└── (test files)

src/lib/tauri.ts                    ← 新增 TS 类型 + IPC 包装函数
src/stores/channelStore.ts          ← 无改动（沿用通用流程）
public/logos/telegram.png           ← 已存在，无改动
```

---

## Task 1 — PR1 后端骨架（模块 + types + api + parser + sender + pairing）

**Files:**
- Create: `src-tauri/src/connector/im/telegram/mod.rs`
- Create: `src-tauri/src/connector/im/telegram/types.rs`
- Create: `src-tauri/src/connector/im/telegram/api.rs`
- Create: `src-tauri/src/connector/im/telegram/parser.rs`
- Create: `src-tauri/src/connector/im/telegram/sender.rs`
- Create: `src-tauri/src/connector/im/telegram/pairing.rs`
- Modify: `src-tauri/src/connector/im/mod.rs` 加 `pub mod telegram;`

不接 manager，纯模块单元 + 单测，独立可 review。

- [ ] **Step 1.1: 加模块声明**

Modify `src-tauri/src/connector/im/mod.rs` —— 在 `pub mod wechat;` 那一行后面加：

```rust
pub mod telegram;
```

- [ ] **Step 1.2: 写 types.rs**

Create `src-tauri/src/connector/im/telegram/types.rs`:

```rust
//! Telegram-specific persisted types.
//!
//! Bot token 走 SecureStorage 加密；bot_id / bot_username / bot_first_name 落明文，
//! UI 列表 + reply forwarder 都会用到。Allowlist 直接落 config.json 而不是单独
//! 文件，因为它的尺寸天然受限（用户能配对的人数 < 100），单文件原子写盘最简单。

use serde::{Deserialize, Serialize};

use crate::connector::im::types::{Platform, SecretStorageKind};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TelegramStoredCredentials {
    pub bot_token_encrypted: String,
    pub bot_token_storage: SecretStorageKind,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TelegramBotInfo {
    pub bot_id: String,
    pub bot_username: String,
    pub bot_first_name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AllowlistEntry {
    pub user_id: i64,
    pub first_name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub username: Option<String>,
    pub paired_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TelegramStoredMetadata {
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TelegramStoredConfig {
    pub schema_version: u32,
    pub platform: Platform,
    pub configured: bool,
    pub enabled: bool,
    pub credentials: TelegramStoredCredentials,
    pub bot: TelegramBotInfo,
    #[serde(default)]
    pub allowlist: Vec<AllowlistEntry>,
    pub metadata: TelegramStoredMetadata,
}

/// 给 ChannelConfigView.source 用，区分凭证来源。
pub const TELEGRAM_BOT_TOKEN_SOURCE: &str = "TELEGRAM_BOT_TOKEN";

/// 入站消息的发件人 + chat 元数据，缓存到 connector 的 session_targets，
/// 出站 reply 时用来还原 chat_id。
#[derive(Debug, Clone)]
pub struct TelegramSessionTarget {
    pub chat_id: i64,
    pub user_id: i64,
}
```

- [ ] **Step 1.3: 写 mod.rs re-export**

Create `src-tauri/src/connector/im/telegram/mod.rs`:

```rust
//! Telegram Bot API IM connector (MVP — long-poll inbound only, 私聊 only).
//!
//! 实现 `IMConnector`：入站走 Bot API `getUpdates` 长轮询（零公网入口），
//! 出站走 `sendMessage` MarkdownV2。Pairing 协议参考 OpenClaw `dmPolicy: pairing`。
//!
//! See `docs/superpowers/specs/2026-05-19-im-telegram-connector-design.md`.

pub mod api;
pub mod connector;
pub mod long_poll;
pub mod pairing;
pub mod parser;
pub mod registration;
pub mod reply_forwarder;
pub mod sender;
pub mod types;

pub use connector::TelegramConnector;
```

> 注：connector / long_poll / registration / reply_forwarder 在后续 step 创建；mod.rs 先列上，等 PR2 / PR3 补完时不再改它。

- [ ] **Step 1.4: 写 api.rs（Bot API 客户端）**

Create `src-tauri/src/connector/im/telegram/api.rs`:

```rust
//! Telegram Bot API HTTP client (thin reqwest wrapper).
//!
//! Endpoints used:
//! - GET  /bot<token>/getMe                          → bot info（save 时验证 token）
//! - GET  /bot<token>/getUpdates?offset=N&timeout=25 → 长轮询入站
//! - POST /bot<token>/sendMessage                    → 出站
//!
//! Bot API 不接受 token query string，token 在 URL path 里。MarkdownV2 解析失败
//! 在 sender 层做 plain text fallback。429 / 401 / 400 错误码映射给调用者。

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::time::Duration;

const TELEGRAM_API_BASE: &str = "https://api.telegram.org";

/// HTTP client wrapping a single bot token. Construct one per connector.
pub struct TelegramApi {
    token: String,
    api_base: String,
    http: reqwest::Client,
}

#[derive(Debug, thiserror::Error)]
pub enum TelegramApiError {
    #[error("invalid token / unauthorized: {0}")]
    Unauthorized(String),
    #[error("rate limited; retry after {retry_after:?}")]
    TooManyRequests { retry_after: Duration },
    #[error("bad request: {0}")]
    BadRequest(String),
    #[error("forbidden: {0}")]
    Forbidden(String),
    #[error("http transport: {0}")]
    Transport(String),
    #[error("server error: {0}")]
    ServerError(String),
}

#[derive(Debug, Clone, Deserialize)]
pub struct BotInfo {
    pub id: i64,
    #[serde(default)]
    pub username: Option<String>,
    #[serde(default)]
    pub first_name: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TgUser {
    pub id: i64,
    #[serde(default)]
    pub is_bot: bool,
    #[serde(default)]
    pub first_name: String,
    #[serde(default)]
    pub username: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TgChat {
    pub id: i64,
    #[serde(rename = "type")]
    pub chat_type: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TgMessage {
    pub message_id: i64,
    #[serde(default)]
    pub from: Option<TgUser>,
    pub chat: TgChat,
    #[serde(default)]
    pub text: Option<String>,
    #[serde(default)]
    pub date: Option<i64>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TgUpdate {
    pub update_id: i64,
    #[serde(default)]
    pub message: Option<TgMessage>,
}

#[derive(Debug, Serialize)]
struct SendMessageBody<'a> {
    chat_id: i64,
    text: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    parse_mode: Option<&'a str>,
}

#[derive(Debug, Deserialize)]
struct Envelope<T> {
    ok: bool,
    #[serde(default)]
    result: Option<T>,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    error_code: Option<i64>,
    #[serde(default)]
    parameters: Option<EnvelopeParams>,
}

#[derive(Debug, Deserialize)]
struct EnvelopeParams {
    #[serde(default)]
    retry_after: Option<u64>,
}

impl TelegramApi {
    pub fn new(token: String) -> Self {
        Self {
            token,
            api_base: TELEGRAM_API_BASE.to_string(),
            http: reqwest::Client::builder()
                .timeout(Duration::from_secs(35)) // > long-poll timeout 25s
                .build()
                .expect("reqwest client build"),
        }
    }

    #[doc(hidden)]
    pub fn new_with_api_base_for_tests(token: String, api_base: String) -> Self {
        Self {
            token,
            api_base,
            http: reqwest::Client::builder()
                .timeout(Duration::from_secs(5))
                .build()
                .expect("reqwest client build"),
        }
    }

    fn url(&self, method: &str) -> String {
        // token 在 path 里；日志请勿打整 URL。
        format!("{}/bot{}/{}", self.api_base, self.token, method)
    }

    /// 验证 token + 拿 bot 元数据。`save` 调用一次。
    pub async fn get_me(&self) -> Result<BotInfo, TelegramApiError> {
        let resp = self
            .http
            .get(self.url("getMe"))
            .send()
            .await
            .map_err(|e| TelegramApiError::Transport(e.to_string()))?;
        parse_envelope::<BotInfo>(resp).await
    }

    /// 长轮询拉 updates。`offset` = next_update_id（已消费的最大 id + 1）。
    pub async fn get_updates(
        &self,
        offset: i64,
        timeout_secs: u64,
    ) -> Result<Vec<TgUpdate>, TelegramApiError> {
        let url = format!(
            "{}?offset={}&timeout={}&allowed_updates=%5B%22message%22%5D",
            self.url("getUpdates"),
            offset,
            timeout_secs,
        );
        let resp = self
            .http
            .get(&url)
            .send()
            .await
            .map_err(|e| TelegramApiError::Transport(e.to_string()))?;
        parse_envelope::<Vec<TgUpdate>>(resp).await
    }

    /// 出站。`parse_mode=None` 走 plain text。
    pub async fn send_message(
        &self,
        chat_id: i64,
        text: &str,
        parse_mode: Option<&str>,
    ) -> Result<TgMessage, TelegramApiError> {
        let body = SendMessageBody {
            chat_id,
            text,
            parse_mode,
        };
        let resp = self
            .http
            .post(self.url("sendMessage"))
            .json(&body)
            .send()
            .await
            .map_err(|e| TelegramApiError::Transport(e.to_string()))?;
        parse_envelope::<TgMessage>(resp).await
    }
}

async fn parse_envelope<T: for<'de> Deserialize<'de>>(
    resp: reqwest::Response,
) -> Result<T, TelegramApiError> {
    let status = resp.status();
    let body = resp
        .text()
        .await
        .map_err(|e| TelegramApiError::Transport(e.to_string()))?;
    let env: Envelope<T> = serde_json::from_str(&body)
        .with_context(|| format!("parse telegram envelope status={status} body={body}"))
        .map_err(|e| TelegramApiError::Transport(e.to_string()))?;
    if env.ok {
        env.result.ok_or_else(|| {
            TelegramApiError::Transport("ok=true but result missing".to_string())
        })
    } else {
        let desc = env.description.unwrap_or_default();
        match env.error_code.unwrap_or(0) {
            401 => Err(TelegramApiError::Unauthorized(desc)),
            403 => Err(TelegramApiError::Forbidden(desc)),
            429 => {
                let secs = env
                    .parameters
                    .and_then(|p| p.retry_after)
                    .unwrap_or(1);
                Err(TelegramApiError::TooManyRequests {
                    retry_after: Duration::from_secs(secs),
                })
            }
            400 => Err(TelegramApiError::BadRequest(desc)),
            code if (500..600).contains(&code) => Err(TelegramApiError::ServerError(desc)),
            _ => Err(TelegramApiError::Transport(format!("unknown error_code={} desc={}", env.error_code.unwrap_or(0), desc))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wiremock::matchers::{method, path};
    use wiremock::{Mock, MockServer, ResponseTemplate};

    #[tokio::test]
    async fn get_me_parses_username_and_first_name() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/botTESTTOKEN/getMe"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "ok": true,
                "result": { "id": 8123, "username": "test_bot", "first_name": "Test Bot" }
            })))
            .mount(&server)
            .await;
        let api = TelegramApi::new_with_api_base_for_tests("TESTTOKEN".into(), server.uri());
        let info = api.get_me().await.unwrap();
        assert_eq!(info.id, 8123);
        assert_eq!(info.username.as_deref(), Some("test_bot"));
        assert_eq!(info.first_name, "Test Bot");
    }

    #[tokio::test]
    async fn unauthorized_envelope_maps_to_unauthorized_error() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/botBADTOKEN/getMe"))
            .respond_with(ResponseTemplate::new(401).set_body_json(serde_json::json!({
                "ok": false, "error_code": 401, "description": "Unauthorized"
            })))
            .mount(&server)
            .await;
        let api = TelegramApi::new_with_api_base_for_tests("BADTOKEN".into(), server.uri());
        match api.get_me().await {
            Err(TelegramApiError::Unauthorized(_)) => {}
            other => panic!("expected Unauthorized, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn too_many_requests_returns_retry_after() {
        let server = MockServer::start().await;
        Mock::given(method("POST"))
            .and(path("/botT/sendMessage"))
            .respond_with(ResponseTemplate::new(429).set_body_json(serde_json::json!({
                "ok": false, "error_code": 429, "description": "Too Many Requests",
                "parameters": { "retry_after": 7 }
            })))
            .mount(&server)
            .await;
        let api = TelegramApi::new_with_api_base_for_tests("T".into(), server.uri());
        match api.send_message(1, "hi", None).await {
            Err(TelegramApiError::TooManyRequests { retry_after }) => {
                assert_eq!(retry_after, Duration::from_secs(7));
            }
            other => panic!("expected TooManyRequests, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn get_updates_returns_empty_list() {
        let server = MockServer::start().await;
        Mock::given(method("GET"))
            .and(path("/botT/getUpdates"))
            .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "ok": true, "result": []
            })))
            .mount(&server)
            .await;
        let api = TelegramApi::new_with_api_base_for_tests("T".into(), server.uri());
        let updates = api.get_updates(0, 25).await.unwrap();
        assert!(updates.is_empty());
    }
}
```

- [ ] **Step 1.5: 运行 api.rs 测试**

Run: `cd src-tauri && cargo test --lib connector::im::telegram::api`
Expected: PASS（4 个 test）

- [ ] **Step 1.6: 写 parser.rs**

Create `src-tauri/src/connector/im/telegram/parser.rs`:

```rust
//! Telegram Update → connector-neutral `ChannelMessage` / pairing intent.
//!
//! 解析维度：
//! - `text == "/start"`：缺 code，返回 PairingStart::Empty
//! - `text == "/start <code>"`：返回 PairingStart::WithCode(code)
//! - 其它私聊文本消息：返回 Message
//! - 群聊 / 频道 / 缺 from / 缺 text：返回 Skip

use super::api::{TgMessage, TgUpdate};
use crate::connector::im::types::{
    AttachmentKind, ChannelAttachmentSpec, ChannelMessage, ConversationType,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParsedInbound {
    /// 普通消息（已经过 from / chat / text 校验）
    Message {
        message: ChannelMessage,
        user_id: i64,
        first_name: String,
        username: Option<String>,
    },
    /// `/start <code>`（pairing 入口）
    PairingStart {
        code: Option<String>,
        user_id: i64,
        first_name: String,
        username: Option<String>,
        chat_id: i64,
    },
    /// 群聊 / 频道 / 缺字段 / bot 给 bot —— 上层忽略
    Skip(SkipReason),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SkipReason {
    NotPrivateChat,
    SenderMissing,
    SenderIsBot,
    TextMissing,
}

/// 入口。bot_id 是当前 connector 的 bot id（用来构造 robot_code）。
pub fn parse_update(update: &TgUpdate, bot_id: &str) -> ParsedInbound {
    let Some(msg) = &update.message else {
        return ParsedInbound::Skip(SkipReason::TextMissing);
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
    let Some(text) = msg.text.as_deref() else {
        return ParsedInbound::Skip(SkipReason::TextMissing);
    };

    // /start [code]
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

    // 普通文本消息
    let channel_msg = ChannelMessage {
        msg_id: format!("tg-{}-{}", bot_id, update_id),
        conversation_type: ConversationType::Private,
        conversation_key: msg.chat.id.to_string(),
        sender_id: from.id.to_string(),
        sender_nick: from.first_name.clone(),
        text: text.to_string(),
        robot_code: bot_id.to_string(),
        reply_group_id: String::new(),
        attachments: Vec::<ChannelAttachmentSpec>::new(),
        session_webhook: None,
        created_at_ms: msg.date.map(|s| s * 1000),
    };
    let _ = AttachmentKind::Picture; // 抑制 unused import 警告，保留 future-compat
    ParsedInbound::Message {
        message: channel_msg,
        user_id: from.id,
        first_name: from.first_name.clone(),
        username: from.username.clone(),
    }
}

#[cfg(test)]
mod tests {
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

    fn update_with_msg(id: i64, msg: TgMessage) -> TgUpdate {
        TgUpdate {
            update_id: id,
            message: Some(msg),
        }
    }

    #[test]
    fn private_text_message_parses_to_channel_message() {
        let msg = TgMessage {
            message_id: 1,
            from: Some(user(42, "Alice", false)),
            chat: chat(42, "private"),
            text: Some("hello".into()),
            date: Some(1_700_000_000),
        };
        match parse_update(&update_with_msg(100, msg), "BOT") {
            ParsedInbound::Message {
                message, user_id, ..
            } => {
                assert_eq!(user_id, 42);
                assert_eq!(message.text, "hello");
                assert_eq!(message.msg_id, "tg-BOT-100");
                assert_eq!(message.created_at_ms, Some(1_700_000_000 * 1000));
            }
            other => panic!("expected Message, got {:?}", other),
        }
    }

    #[test]
    fn start_with_code_parses_to_pairing_with_code() {
        let msg = TgMessage {
            message_id: 1,
            from: Some(user(42, "Alice", false)),
            chat: chat(42, "private"),
            text: Some("/start ABC123".into()),
            date: None,
        };
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
        let msg = TgMessage {
            message_id: 1,
            from: Some(user(42, "Alice", false)),
            chat: chat(42, "private"),
            text: Some("/start".into()),
            date: None,
        };
        match parse_update(&update_with_msg(100, msg), "BOT") {
            ParsedInbound::PairingStart { code, .. } => assert!(code.is_none()),
            other => panic!("expected PairingStart, got {:?}", other),
        }
    }

    #[test]
    fn group_message_skipped() {
        let msg = TgMessage {
            message_id: 1,
            from: Some(user(42, "Alice", false)),
            chat: chat(-100, "group"),
            text: Some("hi".into()),
            date: None,
        };
        assert!(matches!(
            parse_update(&update_with_msg(100, msg), "BOT"),
            ParsedInbound::Skip(SkipReason::NotPrivateChat)
        ));
    }

    #[test]
    fn bot_sender_skipped() {
        let msg = TgMessage {
            message_id: 1,
            from: Some(user(42, "Botty", true)),
            chat: chat(42, "private"),
            text: Some("hi".into()),
            date: None,
        };
        assert!(matches!(
            parse_update(&update_with_msg(100, msg), "BOT"),
            ParsedInbound::Skip(SkipReason::SenderIsBot)
        ));
    }

    #[test]
    fn missing_text_skipped() {
        let msg = TgMessage {
            message_id: 1,
            from: Some(user(42, "Alice", false)),
            chat: chat(42, "private"),
            text: None,
            date: None,
        };
        assert!(matches!(
            parse_update(&update_with_msg(100, msg), "BOT"),
            ParsedInbound::Skip(SkipReason::TextMissing)
        ));
    }
}
```

- [ ] **Step 1.7: 运行 parser 测试**

Run: `cd src-tauri && cargo test --lib connector::im::telegram::parser`
Expected: PASS（6 个 test）

- [ ] **Step 1.8: 写 sender.rs（含 MarkdownV2 转义）**

Create `src-tauri/src/connector/im/telegram/sender.rs`:

```rust
//! Telegram outbound: sendMessage Markdown + 429 retry + 400 fallback to plain.
//!
//! MarkdownV2 转义字符集来自官方文档：`_*[]()~\`>#+-=|{}.!`
//! 401 / 403 错误向上抛由 connector 处理（403 触发 allowlist 移除）。

use std::sync::Arc;
use std::time::Duration;

use super::api::{TelegramApi, TelegramApiError};

pub struct TelegramSender {
    api: Arc<TelegramApi>,
}

#[derive(Debug, thiserror::Error)]
pub enum SenderError {
    #[error("unauthorized: {0}")]
    Unauthorized(String),
    #[error("forbidden by recipient: {0}")]
    Forbidden(String),
    #[error("transport: {0}")]
    Transport(String),
}

impl TelegramSender {
    pub fn new(api: Arc<TelegramApi>) -> Self {
        Self { api }
    }

    /// markdown → MarkdownV2 send；BadRequest 时回 plain text 再试一次；
    /// 429 时按 retry_after sleep 后再试一次。其它一次性返回。
    pub async fn send_markdown(&self, chat_id: i64, raw_markdown: &str) -> Result<(), SenderError> {
        let escaped = escape_markdown_v2(raw_markdown);
        match self
            .api
            .send_message(chat_id, &escaped, Some("MarkdownV2"))
            .await
        {
            Ok(_) => Ok(()),
            Err(TelegramApiError::TooManyRequests { retry_after }) => {
                tokio::time::sleep(retry_after).await;
                match self
                    .api
                    .send_message(chat_id, &escaped, Some("MarkdownV2"))
                    .await
                {
                    Ok(_) => Ok(()),
                    Err(e) => Err(map_err(e)),
                }
            }
            Err(TelegramApiError::BadRequest(_)) => {
                // 多半是转义没到位 → 走 plain text fallback。
                match self
                    .api
                    .send_message(chat_id, raw_markdown, None)
                    .await
                {
                    Ok(_) => Ok(()),
                    Err(e) => Err(map_err(e)),
                }
            }
            Err(e) => Err(map_err(e)),
        }
    }

    /// 纯文本发送（pairing 提示语 / 欢迎语用）。
    pub async fn send_plain(&self, chat_id: i64, text: &str) -> Result<(), SenderError> {
        match self.api.send_message(chat_id, text, None).await {
            Ok(_) => Ok(()),
            Err(TelegramApiError::TooManyRequests { retry_after }) => {
                tokio::time::sleep(retry_after).await;
                self.api
                    .send_message(chat_id, text, None)
                    .await
                    .map(|_| ())
                    .map_err(map_err)
            }
            Err(e) => Err(map_err(e)),
        }
    }
}

fn map_err(e: TelegramApiError) -> SenderError {
    match e {
        TelegramApiError::Unauthorized(d) => SenderError::Unauthorized(d),
        TelegramApiError::Forbidden(d) => SenderError::Forbidden(d),
        other => SenderError::Transport(other.to_string()),
    }
}

const SPECIAL_CHARS: &[char] = &[
    '_', '*', '[', ']', '(', ')', '~', '`', '>', '#', '+', '-', '=', '|', '{', '}', '.', '!',
    '\\',
];

pub fn escape_markdown_v2(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 8);
    for c in text.chars() {
        if SPECIAL_CHARS.contains(&c) {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

// 让 sender_error 在 connector 那边能简单映射。
pub fn duration_secs(d: Duration) -> u64 {
    d.as_secs()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escapes_all_special_chars() {
        for c in SPECIAL_CHARS {
            let s = c.to_string();
            let out = escape_markdown_v2(&s);
            assert_eq!(out, format!("\\{c}"), "char {c:?} not escaped");
        }
    }

    #[test]
    fn does_not_escape_normal_chars() {
        let s = "hello world 你好 emoji 😀";
        let out = escape_markdown_v2(s);
        assert_eq!(out, s);
    }

    #[test]
    fn pre_escaped_backslash_is_escaped_again() {
        // `\\_` → `\\\\\\_` —— MarkdownV2 要求 raw 已转义内容仍要逃一次（这是
        // Telegram 协议的偏门）。当前实现：每次特殊字符都加 \\，所以输入
        // `\\_` 会变成 `\\\\\\_`。
        let out = escape_markdown_v2("\\_");
        assert_eq!(out, "\\\\\\_");
    }

    #[test]
    fn multiline_text_unchanged_for_normal_chars() {
        let out = escape_markdown_v2("line1\nline2");
        assert_eq!(out, "line1\nline2");
    }
}
```

- [ ] **Step 1.9: 运行 sender 测试**

Run: `cd src-tauri && cargo test --lib connector::im::telegram::sender`
Expected: PASS（4 个 test）

- [ ] **Step 1.10: 写 pairing.rs**

Create `src-tauri/src/connector/im/telegram/pairing.rs`:

```rust
//! PairingCodeStore：内存 in-flight 配对码（5 min TTL），重启清空。
//!
//! Code 字符集去掉歧义字符 `O/0/I/1/l`，base32 风格 8 个字符。Code 全局唯一性
//! 由 set 去重保证；重复生成时直接重抽。
//!
//! 协议（spec §2.5）：
//! 1. `begin` 生成 code，返回 deep_link
//! 2. bot 收到 /start <code> 时 `attempt_attach(code, pairer)` 把 pairer 写进
//!    pending entry；幂等：同 user_id 重复 attach 不报错
//! 3. 桌面端 `approve(code)` → 把 pairer 写进 config.json allowlist，从 store 删
//! 4. `reject(code)` → 仅删除，不写盘
//! 5. 5 min 后未 approve 的 code 由 list_pending 中的 expire sweep 删除

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::Result;
use rand::{thread_rng, Rng};
use tokio::sync::RwLock;

const CODE_LEN: usize = 8;
const CODE_ALPHABET: &[u8] = b"ABCDEFGHJKMNPQRSTUVWXYZ23456789"; // 去掉 O/0/I/1/L
pub const PAIRING_CODE_TTL: Duration = Duration::from_secs(300);

#[derive(Debug, Clone)]
pub struct PairerInfo {
    pub user_id: i64,
    pub first_name: String,
    pub username: Option<String>,
    pub chat_id: i64,
    pub attached_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone)]
pub struct PendingPairing {
    pub code: String,
    pub created_at: Instant,
    pub expires_at: Instant,
    pub pairer: Option<PairerInfo>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AttachOutcome {
    /// 第一次 attach，已写入 pairer
    Attached,
    /// 同一个 user 重复 attach（幂等成功）
    AlreadyAttached,
    /// code 已被另一个 user 占用
    Conflict,
    /// code 不存在或过期
    NotFound,
}

#[derive(Debug, Clone)]
pub struct PairingCodeStore {
    inner: Arc<RwLock<HashMap<String, PendingPairing>>>,
}

impl Default for PairingCodeStore {
    fn default() -> Self {
        Self::new()
    }
}

impl PairingCodeStore {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// 生成新 code 并放入 store。
    pub async fn begin(&self) -> Result<PendingPairing> {
        let mut guard = self.inner.write().await;
        // 80 次尝试足够稀疏地避免冲突（31^8 = 8.5e11）。
        for _ in 0..80 {
            let code = random_code();
            if !guard.contains_key(&code) {
                let now = Instant::now();
                let entry = PendingPairing {
                    code: code.clone(),
                    created_at: now,
                    expires_at: now + PAIRING_CODE_TTL,
                    pairer: None,
                };
                guard.insert(code, entry.clone());
                return Ok(entry);
            }
        }
        anyhow::bail!("failed to generate unique pairing code after 80 attempts")
    }

    /// bot 收到 /start <code> 时调。
    pub async fn attempt_attach(&self, code: &str, pairer: PairerInfo) -> AttachOutcome {
        let mut guard = self.inner.write().await;
        let entry = match guard.get_mut(code) {
            Some(e) => e,
            None => return AttachOutcome::NotFound,
        };
        if entry.expires_at < Instant::now() {
            guard.remove(code);
            return AttachOutcome::NotFound;
        }
        match &entry.pairer {
            None => {
                entry.pairer = Some(pairer);
                AttachOutcome::Attached
            }
            Some(existing) if existing.user_id == pairer.user_id => AttachOutcome::AlreadyAttached,
            Some(_) => AttachOutcome::Conflict,
        }
    }

    /// 桌面端 approve 取走 entry（移除 + 返回）。
    pub async fn take(&self, code: &str) -> Option<PendingPairing> {
        let mut guard = self.inner.write().await;
        let entry = guard.remove(code)?;
        if entry.expires_at < Instant::now() {
            return None;
        }
        Some(entry)
    }

    /// 列出所有已被扫码的 pending pairing（pairer.is_some()），按 attached_at 降序。
    /// 同时顺手清理过期 entry。
    pub async fn list_pending(&self) -> Vec<PendingPairing> {
        let now = Instant::now();
        let mut guard = self.inner.write().await;
        guard.retain(|_, e| e.expires_at > now);
        let mut out: Vec<_> = guard.values().filter(|e| e.pairer.is_some()).cloned().collect();
        out.sort_by(|a, b| {
            b.pairer
                .as_ref()
                .map(|p| p.attached_at)
                .cmp(&a.pairer.as_ref().map(|p| p.attached_at))
        });
        out
    }

    /// 桌面端 reject。
    pub async fn drop(&self, code: &str) {
        let mut guard = self.inner.write().await;
        guard.remove(code);
    }
}

fn random_code() -> String {
    let mut rng = thread_rng();
    let mut out = String::with_capacity(CODE_LEN);
    for _ in 0..CODE_LEN {
        let idx = rng.gen_range(0..CODE_ALPHABET.len());
        out.push(CODE_ALPHABET[idx] as char);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pairer(uid: i64) -> PairerInfo {
        PairerInfo {
            user_id: uid,
            first_name: format!("u{uid}"),
            username: None,
            chat_id: uid,
            attached_at: chrono::Utc::now(),
        }
    }

    #[tokio::test]
    async fn begin_returns_8char_uppercase_code() {
        let s = PairingCodeStore::new();
        let p = s.begin().await.unwrap();
        assert_eq!(p.code.len(), CODE_LEN);
        assert!(p.code.chars().all(|c| CODE_ALPHABET.contains(&(c as u8))));
        assert!(p.pairer.is_none());
    }

    #[tokio::test]
    async fn attempt_attach_first_succeeds_and_second_same_user_is_idempotent() {
        let s = PairingCodeStore::new();
        let p = s.begin().await.unwrap();
        assert_eq!(s.attempt_attach(&p.code, pairer(42)).await, AttachOutcome::Attached);
        assert_eq!(
            s.attempt_attach(&p.code, pairer(42)).await,
            AttachOutcome::AlreadyAttached
        );
    }

    #[tokio::test]
    async fn attempt_attach_with_different_user_returns_conflict() {
        let s = PairingCodeStore::new();
        let p = s.begin().await.unwrap();
        s.attempt_attach(&p.code, pairer(42)).await;
        assert_eq!(
            s.attempt_attach(&p.code, pairer(43)).await,
            AttachOutcome::Conflict
        );
    }

    #[tokio::test]
    async fn unknown_code_returns_not_found() {
        let s = PairingCodeStore::new();
        assert_eq!(
            s.attempt_attach("ZZZZZZZZ", pairer(1)).await,
            AttachOutcome::NotFound
        );
    }

    #[tokio::test]
    async fn list_pending_only_returns_attached_entries() {
        let s = PairingCodeStore::new();
        let p1 = s.begin().await.unwrap();
        let p2 = s.begin().await.unwrap();
        s.attempt_attach(&p1.code, pairer(42)).await;
        // p2 未 attach
        let _ = p2;
        let list = s.list_pending().await;
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].code, p1.code);
    }

    #[tokio::test]
    async fn take_removes_entry() {
        let s = PairingCodeStore::new();
        let p = s.begin().await.unwrap();
        s.attempt_attach(&p.code, pairer(42)).await;
        assert!(s.take(&p.code).await.is_some());
        assert!(s.take(&p.code).await.is_none());
    }
}
```

- [ ] **Step 1.11: Cargo.toml 加 rand（如未有）**

Run: `grep '^rand' src-tauri/Cargo.toml`

如果没输出，编辑 `src-tauri/Cargo.toml` 在 `[dependencies]` 那一节加：

```toml
rand = "0.8"
```

如果有了就跳过。

- [ ] **Step 1.12: 运行 pairing 测试**

Run: `cd src-tauri && cargo test --lib connector::im::telegram::pairing`
Expected: PASS（6 个 test）

- [ ] **Step 1.13: 运行所有 PR1 测试 + clippy**

Run: `cd src-tauri && cargo test --lib connector::im::telegram && cargo clippy --all-targets --no-deps -- -D warnings`
Expected: PASS, no clippy warnings on telegram/

- [ ] **Step 1.14: Commit PR1**

```bash
git add src-tauri/Cargo.toml src-tauri/src/connector/im/mod.rs src-tauri/src/connector/im/telegram/
git commit -m "$(cat <<'EOF'
feat(connector/im/telegram): PR1 后端骨架（types + api + parser + sender + pairing）

Bot API thin client + 入站 update parser + MarkdownV2 sender（带 429/400 fallback）
+ 内存 PairingCodeStore（5min TTL，base32 8 字符 code）。纯模块单元，不接 manager。

Spec: docs/superpowers/specs/2026-05-19-im-telegram-connector-design.md

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 2 — PR2 后端接入（manager + long_poll + commands）

**Files:**
- Create: `src-tauri/src/connector/im/telegram/connector.rs`
- Create: `src-tauri/src/connector/im/telegram/long_poll.rs`
- Create: `src-tauri/src/connector/im/telegram/reply_forwarder.rs`
- Create: `src-tauri/src/connector/im/telegram/registration.rs`
- Modify: `src-tauri/src/connector/im/shared/config_store.rs` 加 telegram_* helpers
- Modify: `src-tauri/src/connector/im/factory.rs` 加 `build_telegram_connector`
- Modify: `src-tauri/src/connector/im/manager.rs` 接入 telegram
- Modify: `src-tauri/src/commands/channel.rs` 加 7 个新命令
- Modify: `src-tauri/src/lib.rs` invoke_handler 注册
- Modify: `src-tauri/src/storage/aijia_home.rs` 加 telegram_state_path helper
- Create: `src-tauri/tests/telegram_pairing_integration_test.rs`

- [ ] **Step 2.1: 写 connector.rs**

Create `src-tauri/src/connector/im/telegram/connector.rs`:

```rust
//! TelegramConnector — 实现 IMConnector，桥接 long_poll → ChannelMessage stream
//! 和 RuntimeEvent → sendMessage。
//!
//! 镜像 wecom::connector::WecomConnector 的形状：
//! - start() 起两个 task：long_poll loop + event pump
//! - send() 走 sender，403 时移除 user_id 出 allowlist
//! - has_session()/remember_session() 给 reply_forwarder 用

use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use futures::stream::BoxStream;
use futures::StreamExt;
use tokio::sync::{mpsc, RwLock};
use tokio_stream::wrappers::ReceiverStream;

use super::api::TelegramApi;
use super::pairing::PairingCodeStore;
use super::sender::{SenderError, TelegramSender};
use super::types::TelegramSessionTarget;
use crate::connector::im::shared::config_store::ChannelConfigStore;
use crate::connector::im::trait_def::{
    AuthFlow, ConnectorCapabilities, ConnectorContext, ConnectorError, IMConnector, InboundModel,
    ReplyContent, ReplyTarget,
};
use crate::connector::im::types::{ChannelConnectionState, ChannelMessage, Platform};

pub struct TelegramConnector {
    bot_id: String,
    bot_username: String,
    api: Arc<TelegramApi>,
    sender: TelegramSender,
    pairing: PairingCodeStore,
    session_targets: Arc<RwLock<HashMap<String, TelegramSessionTarget>>>,
    config_store: Arc<ChannelConfigStore>,
    on_status: Arc<dyn Fn(ChannelConnectionState, Option<String>) + Send + Sync>,
}

impl TelegramConnector {
    pub fn new(
        bot_id: String,
        bot_username: String,
        token: String,
        config_store: Arc<ChannelConfigStore>,
        on_status: Arc<dyn Fn(ChannelConnectionState, Option<String>) + Send + Sync>,
    ) -> Self {
        let api = Arc::new(TelegramApi::new(token));
        let sender = TelegramSender::new(api.clone());
        Self {
            bot_id,
            bot_username,
            api,
            sender,
            pairing: PairingCodeStore::new(),
            session_targets: Arc::new(RwLock::new(HashMap::new())),
            config_store,
            on_status,
        }
    }

    pub fn bot_id(&self) -> &str {
        &self.bot_id
    }
    pub fn bot_username(&self) -> &str {
        &self.bot_username
    }
    pub fn pairing(&self) -> PairingCodeStore {
        self.pairing.clone()
    }
    pub fn api(&self) -> Arc<TelegramApi> {
        self.api.clone()
    }
    pub fn sender(&self) -> &TelegramSender {
        &self.sender
    }

    pub async fn remember_session(&self, session_id: String, target: TelegramSessionTarget) {
        self.session_targets.write().await.insert(session_id, target);
    }
    pub async fn has_session(&self, session_id: &str) -> bool {
        self.session_targets.read().await.contains_key(session_id)
    }

    async fn resolve_chat_id(&self, target: &ReplyTarget) -> Option<i64> {
        // ReplyTarget.external_conversation_key 在 dispatch 路径下是 chat_id 字符串；
        // RuntimeEventBus 路径下为空 → 从 session_targets 还原。
        if let Ok(parsed) = target.external_conversation_key.parse::<i64>() {
            return Some(parsed);
        }
        let guard = self.session_targets.read().await;
        guard.get(&target.session_id).map(|t| t.chat_id)
    }
}

#[async_trait]
impl IMConnector for TelegramConnector {
    fn platform(&self) -> Platform {
        Platform::Telegram
    }

    fn capabilities(&self) -> ConnectorCapabilities {
        ConnectorCapabilities {
            inbound: InboundModel::Stream,
            outbound_aicard: false,
            outbound_markdown: true,
            supports_attachments: false,
            supports_group_chat: false,
            supports_private_chat: true,
            auth_flow: AuthFlow::ApiKey,
        }
    }

    async fn start(
        &self,
        ctx: ConnectorContext,
    ) -> Result<BoxStream<'static, ChannelMessage>, ConnectorError> {
        let (msg_tx, msg_rx) = mpsc::channel::<ChannelMessage>(256);

        // Connecting 占位；long_poll 第一次拉成功后 emit Connected。
        (self.on_status)(ChannelConnectionState::Connecting, None);

        let api = self.api.clone();
        let bot_id = self.bot_id.clone();
        let pairing = self.pairing.clone();
        let sender_for_pump = self.sender.clone_inner();
        let session_targets = self.session_targets.clone();
        let config_store = self.config_store.clone();
        let on_status = self.on_status.clone();
        let cancel = ctx.cancel_token.clone();

        tokio::spawn(async move {
            super::long_poll::run(super::long_poll::Params {
                api,
                bot_id,
                pairing,
                sender: sender_for_pump,
                session_targets,
                config_store,
                msg_tx,
                on_status,
                cancel,
            })
            .await
        });

        Ok(ReceiverStream::new(msg_rx).boxed())
    }

    async fn send(
        &self,
        target: ReplyTarget,
        content: ReplyContent,
    ) -> Result<(), ConnectorError> {
        let chat_id = self
            .resolve_chat_id(&target)
            .await
            .ok_or_else(|| ConnectorError::Transient("telegram chat_id missing".into()))?;
        let text = match content {
            ReplyContent::Text(t) | ReplyContent::Markdown(t) => t,
            ReplyContent::AiCardChunk { delta, final_chunk } if final_chunk => delta,
            ReplyContent::AiCardChunk { .. } => return Ok(()), // 中间 chunk 丢弃
            ReplyContent::AiCardFail => "❌ 处理失败，请重试".to_string(),
        };
        match self.sender.send_markdown(chat_id, &text).await {
            Ok(()) => Ok(()),
            Err(SenderError::Unauthorized(d)) => Err(ConnectorError::AuthExpired(d)),
            Err(SenderError::Forbidden(d)) => {
                // 用户从 Telegram 端 block 了 bot；删 allowlist + 记 last_error 但 connector 继续运行
                log::warn!(
                    "[telegram-{}] forbidden when sending to chat={}, removing from allowlist",
                    self.bot_id,
                    chat_id
                );
                let _ = remove_user_by_chat(&self.config_store, chat_id, &self.session_targets).await;
                Err(ConnectorError::Transient(format!("forbidden: {d}")))
            }
            Err(SenderError::Transport(d)) => Err(ConnectorError::Transient(d)),
        }
    }
}

async fn remove_user_by_chat(
    config_store: &Arc<ChannelConfigStore>,
    chat_id: i64,
    session_targets: &Arc<RwLock<HashMap<String, TelegramSessionTarget>>>,
) -> anyhow::Result<()> {
    // 私聊 chat_id == user_id（Telegram 私聊约定），直接当 user_id 用。
    let user_id = chat_id;
    config_store.telegram_remove_allowlist_user(user_id)?;
    let mut guard = session_targets.write().await;
    guard.retain(|_, t| t.user_id != user_id);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn build_test_connector() -> TelegramConnector {
        let dir = tempfile::TempDir::new().unwrap();
        let cs = Arc::new(ChannelConfigStore::new(dir.path().to_path_buf(), None));
        TelegramConnector::new(
            "8123".into(),
            "test_bot".into(),
            "TOKEN".into(),
            cs,
            Arc::new(|_, _| {}),
        )
    }

    #[tokio::test]
    async fn platform_and_capabilities() {
        let c = build_test_connector();
        assert_eq!(c.platform(), Platform::Telegram);
        let caps = c.capabilities();
        assert!(matches!(caps.inbound, InboundModel::Stream));
        assert!(!caps.supports_group_chat);
        assert!(caps.supports_private_chat);
        assert!(matches!(caps.auth_flow, AuthFlow::ApiKey));
    }

    #[tokio::test]
    async fn remember_and_has_session() {
        let c = build_test_connector();
        c.remember_session(
            "sess-1".into(),
            TelegramSessionTarget {
                chat_id: 42,
                user_id: 42,
            },
        )
        .await;
        assert!(c.has_session("sess-1").await);
        assert!(!c.has_session("sess-2").await);
    }
}
```

> sender 需要 `clone_inner()`。在 sender.rs 加一个：

Modify `src-tauri/src/connector/im/telegram/sender.rs` — 在 `impl TelegramSender` 块末尾加：

```rust
    /// 给 long_poll task 复制一份共享同一个 Arc<TelegramApi> 的 sender 实例。
    pub fn clone_inner(&self) -> TelegramSender {
        TelegramSender {
            api: self.api.clone(),
        }
    }
```

- [ ] **Step 2.2: 写 long_poll.rs**

Create `src-tauri/src/connector/im/telegram/long_poll.rs`:

```rust
//! getUpdates 长轮询主循环。
//!
//! 行为：
//! - 启动时从 telegram/state.json 读 lastOffset，缺失则用 0
//! - 每轮 `get_updates(offset, timeout=25s)`
//! - 收到 update → parser → 分支：
//!   - PairingStart：bot 内部响应（attempt_attach + sendMessage 提示）
//!   - Message：检查 allowlist，命中则 push 到 msg_tx；未命中回提示
//!   - Skip：丢弃
//! - 每条 update 处理完后 offset = update_id + 1；每 5s 或每 10 条 fsync state.json
//! - cancel_token 触发后强制 flush 并退出
//! - 401 → emit NeedsReauth 并退出（不重连）
//! - 其它错误 → ReconnectBackoff sleep 后继续

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use tokio::sync::{mpsc, RwLock};
use tokio_util::sync::CancellationToken;

use super::api::{TelegramApi, TelegramApiError};
use super::pairing::{AttachOutcome, PairerInfo, PairingCodeStore};
use super::parser::{parse_update, ParsedInbound, SkipReason};
use super::sender::TelegramSender;
use super::types::TelegramSessionTarget;
use crate::connector::im::shared::config_store::ChannelConfigStore;
use crate::connector::im::shared::dedup::MessageDedupSet;
use crate::connector::im::shared::reconnect::ReconnectBackoff;
use crate::connector::im::types::{ChannelConnectionState, ChannelMessage, Platform};
use crate::storage::aijia_home::AiJiaHome;

const LONG_POLL_TIMEOUT_SECS: u64 = 25;
const FLUSH_BATCH: usize = 10;
const FLUSH_INTERVAL: Duration = Duration::from_secs(5);

pub struct Params {
    pub api: Arc<TelegramApi>,
    pub bot_id: String,
    pub pairing: PairingCodeStore,
    pub sender: TelegramSender,
    pub session_targets: Arc<RwLock<HashMap<String, TelegramSessionTarget>>>,
    pub config_store: Arc<ChannelConfigStore>,
    pub msg_tx: mpsc::Sender<ChannelMessage>,
    pub on_status: Arc<dyn Fn(ChannelConnectionState, Option<String>) + Send + Sync>,
    pub cancel: CancellationToken,
}

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct StateFile {
    #[serde(default)]
    last_offset: i64,
    #[serde(default)]
    saved_at: String,
}

pub async fn run(p: Params) {
    let dedup = MessageDedupSet::with_default_cap();
    let mut backoff = ReconnectBackoff::default_schedule();
    let state_path = state_path_for(&p.bot_id);
    let mut offset = read_offset(&state_path).unwrap_or(0);
    let mut dirty_count = 0usize;
    let mut last_flush = Instant::now();

    let mut first_round = true;

    loop {
        if p.cancel.is_cancelled() {
            flush_offset(&state_path, offset);
            return;
        }
        match p.api.get_updates(offset, LONG_POLL_TIMEOUT_SECS).await {
            Ok(updates) => {
                if first_round {
                    first_round = false;
                    (p.on_status)(ChannelConnectionState::Connected, None);
                }
                backoff.reset();
                for u in updates {
                    if !dedup.observe(&format!("tg-{}-{}", p.bot_id, u.update_id)).await {
                        offset = u.update_id + 1;
                        continue;
                    }
                    match parse_update(&u, &p.bot_id) {
                        ParsedInbound::Skip(reason) => {
                            log::debug!("[telegram-{}] skip update_id={} reason={:?}", p.bot_id, u.update_id, reason);
                        }
                        ParsedInbound::PairingStart {
                            code,
                            user_id,
                            first_name,
                            username,
                            chat_id,
                        } => {
                            handle_pairing_start(
                                code,
                                user_id,
                                first_name,
                                username,
                                chat_id,
                                &p.pairing,
                                &p.sender,
                                &p.config_store,
                            )
                            .await;
                        }
                        ParsedInbound::Message {
                            message,
                            user_id,
                            first_name,
                            ..
                        } => {
                            handle_message(
                                message,
                                user_id,
                                first_name,
                                &p.config_store,
                                &p.sender,
                                &p.session_targets,
                                &p.msg_tx,
                            )
                            .await;
                        }
                    }
                    offset = u.update_id + 1;
                    dirty_count += 1;
                }
                if dirty_count >= FLUSH_BATCH || last_flush.elapsed() >= FLUSH_INTERVAL {
                    flush_offset(&state_path, offset);
                    dirty_count = 0;
                    last_flush = Instant::now();
                }
            }
            Err(TelegramApiError::Unauthorized(d)) => {
                log::error!("[telegram-{}] unauthorized: {d}", p.bot_id);
                (p.on_status)(ChannelConnectionState::NeedsReauth, Some(d));
                flush_offset(&state_path, offset);
                return;
            }
            Err(TelegramApiError::TooManyRequests { retry_after }) => {
                log::warn!("[telegram-{}] 429, sleeping {:?}", p.bot_id, retry_after);
                tokio::time::sleep(retry_after).await;
            }
            Err(e) => {
                let delay = backoff.next_delay();
                log::warn!(
                    "[telegram-{}] long-poll error: {e:?}, retry in {:?}",
                    p.bot_id,
                    delay
                );
                (p.on_status)(
                    ChannelConnectionState::Reconnecting,
                    Some(e.to_string()),
                );
                tokio::select! {
                    _ = tokio::time::sleep(delay) => {}
                    _ = p.cancel.cancelled() => {
                        flush_offset(&state_path, offset);
                        return;
                    }
                }
            }
        }
    }
}

async fn handle_pairing_start(
    code: Option<String>,
    user_id: i64,
    first_name: String,
    username: Option<String>,
    chat_id: i64,
    pairing: &PairingCodeStore,
    sender: &TelegramSender,
    config_store: &ChannelConfigStore,
) {
    let already_in_allowlist = config_store
        .telegram_is_in_allowlist(user_id)
        .unwrap_or(false);
    if already_in_allowlist {
        let _ = sender
            .send_plain(chat_id, "你已配对，可以直接发送消息。")
            .await;
        return;
    }
    let Some(code) = code else {
        let _ = sender
            .send_plain(
                chat_id,
                "请回到 AIjia 桌面端重新生成配对二维码并扫描。",
            )
            .await;
        return;
    };
    let pairer = PairerInfo {
        user_id,
        first_name,
        username,
        chat_id,
        attached_at: chrono::Utc::now(),
    };
    match pairing.attempt_attach(&code, pairer).await {
        AttachOutcome::Attached | AttachOutcome::AlreadyAttached => {
            let _ = sender
                .send_plain(chat_id, "✓ 等待 AIjia 桌面端批准你的连接请求…")
                .await;
        }
        AttachOutcome::Conflict => {
            let _ = sender
                .send_plain(chat_id, "该配对码已被其他用户使用，请重新生成。")
                .await;
        }
        AttachOutcome::NotFound => {
            let _ = sender
                .send_plain(
                    chat_id,
                    "配对码已失效，请回到 AIjia 桌面端重新生成。",
                )
                .await;
        }
    }
}

async fn handle_message(
    message: ChannelMessage,
    user_id: i64,
    _first_name: String,
    config_store: &ChannelConfigStore,
    sender: &TelegramSender,
    session_targets: &Arc<RwLock<HashMap<String, TelegramSessionTarget>>>,
    msg_tx: &mpsc::Sender<ChannelMessage>,
) {
    let in_allowlist = config_store
        .telegram_is_in_allowlist(user_id)
        .unwrap_or(false);
    if !in_allowlist {
        let chat_id: i64 = message.conversation_key.parse().unwrap_or(0);
        let _ = sender
            .send_plain(
                chat_id,
                "你还未与 AIjia 配对，请联系管理员在 AIjia 桌面端获取配对二维码。",
            )
            .await;
        return;
    }
    let chat_id: i64 = message.conversation_key.parse().unwrap_or(0);
    // 进 manager worker 前先 remember session_target，让 reply_forwarder 能找回 chat_id
    // session_id 由 router 在 manager worker 里决定；这里先用 msg_id 占位放进 map，
    // 实际 session_id 由 manager worker 在 get_or_create_session 后调
    // connector.remember_session 覆盖；本 map 在 worker 路径下不会被读到。
    let _ = session_targets;
    if msg_tx.send(message).await.is_err() {
        log::warn!("[telegram] msg_tx closed; dropping update");
    }
}

fn state_path_for(bot_id: &str) -> PathBuf {
    AiJiaHome::from_home()
        .users_dir()
        .join("active")
        .join("channels")
        .join("telegram")
        .join(format!("state-{bot_id}.json"))
}

fn read_offset(path: &PathBuf) -> Option<i64> {
    let raw = std::fs::read_to_string(path).ok()?;
    let s: StateFile = serde_json::from_str(&raw).ok()?;
    Some(s.last_offset)
}

fn flush_offset(path: &PathBuf, offset: i64) {
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let s = StateFile {
        last_offset: offset,
        saved_at: chrono::Utc::now().to_rfc3339(),
    };
    let body = match serde_json::to_string(&s) {
        Ok(b) => b,
        Err(_) => return,
    };
    let _ = std::fs::write(path, body);
}
```

> 注：`AiJiaHome::users_dir()` 路径 + `active` scope 跟 wecom 路径不完全一样；后续接 manager 时会替换为 manager 真正用的 sessions_paths。先用一个简化 helper 保持本步可独立 cargo build。

- [ ] **Step 2.3: 扩展 ChannelConfigStore 加 telegram 方法**

Modify `src-tauri/src/connector/im/shared/config_store.rs`:

在 `// ---- Wechat ----` 之前（即 wecom 段之后）加：

```rust
    // ----- Telegram (MVP) ------------------------------------------------------
    //
    // bot_token 走 SecureStorage。bot_id / bot_username / first_name 明文。
    // allowlist 直接落 config.json（尺寸天然有限，单文件原子写盘最简单）。

    pub fn read_telegram_config(
        &self,
    ) -> Result<Option<crate::connector::im::telegram::types::TelegramStoredConfig>> {
        let path = self.platform_config_path(Platform::Telegram);
        if !path.exists() {
            return Ok(None);
        }
        let raw = std::fs::read_to_string(&path)?;
        let config: crate::connector::im::telegram::types::TelegramStoredConfig =
            serde_json::from_str(&raw)
                .with_context(|| format!("Failed to parse {}", path.display()))?;
        validate_telegram_config(&config)?;
        Ok(Some(config))
    }

    pub fn save_telegram_registration(
        &self,
        token: String,
        bot_id: String,
        bot_username: String,
        bot_first_name: String,
    ) -> Result<ChannelPlatformState> {
        use crate::connector::im::telegram::types::{
            TelegramBotInfo, TelegramStoredConfig, TelegramStoredCredentials,
            TelegramStoredMetadata,
        };
        let token = non_empty(token, "bot_token")?;
        let bot_id = non_empty(bot_id, "bot_id")?;
        let bot_username = non_empty(bot_username, "bot_username")?;
        let (bot_token_encrypted, bot_token_storage) = self.encrypt_secret(&token)?;
        let now = now_rfc3339();
        let existing = self.read_telegram_config()?;
        let existing_created_at = existing
            .as_ref()
            .map(|c| c.metadata.created_at.clone())
            .unwrap_or_else(|| now.clone());
        let existing_allowlist = existing.as_ref().map(|c| c.allowlist.clone()).unwrap_or_default();
        let config = TelegramStoredConfig {
            schema_version: 1,
            platform: Platform::Telegram,
            configured: true,
            enabled: true,
            credentials: TelegramStoredCredentials {
                bot_token_encrypted,
                bot_token_storage,
            },
            bot: TelegramBotInfo {
                bot_id,
                bot_username,
                bot_first_name,
            },
            allowlist: existing_allowlist,
            metadata: TelegramStoredMetadata {
                created_at: existing_created_at,
                updated_at: now,
            },
        };
        self.write_telegram_config(&config)?;
        self.telegram_state(ChannelConnectionState::Disconnected, None)
    }

    pub fn decrypt_telegram_config(
        &self,
    ) -> Result<(crate::connector::im::telegram::types::TelegramStoredConfig, String)> {
        let config = self
            .read_telegram_config()?
            .ok_or_else(|| anyhow::anyhow!("Telegram channel is not configured"))?;
        let token = match (&config.credentials.bot_token_storage, &self.secure_storage) {
            (SecretStorageKind::SecureStorage, Some(storage)) => {
                storage.decrypt(&config.credentials.bot_token_encrypted)?
            }
            (SecretStorageKind::SecureStorage, None) => {
                anyhow::bail!("Telegram bot_token marked SecureStorage but SecureStorage is unavailable")
            }
            (SecretStorageKind::PlaintextFallback, _) => {
                config.credentials.bot_token_encrypted.clone()
            }
        };
        Ok((config, token))
    }

    pub fn telegram_state(
        &self,
        connection: ChannelConnectionState,
        last_error: Option<String>,
    ) -> Result<ChannelPlatformState> {
        let Some(config) = self.read_telegram_config()? else {
            return Ok(ChannelPlatformState {
                platform: Platform::Telegram,
                capability: ChannelCapability::Available,
                configured: false,
                enabled: false,
                connection: ChannelConnectionState::Unconfigured,
                config: None,
                last_connected_at: None,
                last_error: None,
            });
        };
        let connection = if !config.enabled {
            ChannelConnectionState::Disconnected
        } else {
            connection
        };
        Ok(ChannelPlatformState {
            platform: Platform::Telegram,
            capability: ChannelCapability::Available,
            configured: config.configured,
            enabled: config.enabled,
            connection,
            config: Some(self.telegram_config_view(&config)?),
            last_connected_at: None,
            last_error,
        })
    }

    pub fn set_telegram_enabled(&self, enabled: bool) -> Result<ChannelPlatformState> {
        let mut config = self
            .read_telegram_config()?
            .ok_or_else(|| anyhow::anyhow!("Telegram channel is not configured"))?;
        config.enabled = enabled;
        config.metadata.updated_at = now_rfc3339();
        self.write_telegram_config(&config)?;
        let connection = if enabled {
            ChannelConnectionState::Connecting
        } else {
            ChannelConnectionState::Disconnected
        };
        self.telegram_state(connection, None)
    }

    pub fn remove_telegram(&self) -> Result<ChannelPlatformState> {
        let path = self.platform_config_path(Platform::Telegram);
        if path.exists() {
            std::fs::remove_file(path)?;
        }
        Ok(ChannelPlatformState {
            platform: Platform::Telegram,
            capability: ChannelCapability::Available,
            configured: false,
            enabled: false,
            connection: ChannelConnectionState::Unconfigured,
            config: None,
            last_connected_at: None,
            last_error: None,
        })
    }

    pub fn reveal_telegram_token(&self) -> Result<String> {
        let (_, token) = self.decrypt_telegram_config()?;
        Ok(token)
    }

    /// 把一个 user 加入 allowlist。重复加入幂等（按 user_id 去重）。
    pub fn telegram_add_allowlist_entry(
        &self,
        entry: crate::connector::im::telegram::types::AllowlistEntry,
    ) -> Result<()> {
        let mut config = self
            .read_telegram_config()?
            .ok_or_else(|| anyhow::anyhow!("Telegram channel is not configured"))?;
        if !config.allowlist.iter().any(|e| e.user_id == entry.user_id) {
            config.allowlist.push(entry);
            config.metadata.updated_at = now_rfc3339();
            self.write_telegram_config(&config)?;
        }
        Ok(())
    }

    pub fn telegram_remove_allowlist_user(&self, user_id: i64) -> Result<()> {
        let Some(mut config) = self.read_telegram_config()? else {
            return Ok(());
        };
        let before = config.allowlist.len();
        config.allowlist.retain(|e| e.user_id != user_id);
        if config.allowlist.len() != before {
            config.metadata.updated_at = now_rfc3339();
            self.write_telegram_config(&config)?;
        }
        Ok(())
    }

    pub fn telegram_is_in_allowlist(&self, user_id: i64) -> Result<bool> {
        let Some(config) = self.read_telegram_config()? else {
            return Ok(false);
        };
        Ok(config.allowlist.iter().any(|e| e.user_id == user_id))
    }

    fn write_telegram_config(
        &self,
        config: &crate::connector::im::telegram::types::TelegramStoredConfig,
    ) -> Result<()> {
        validate_telegram_config(config)?;
        let dir = self.platform_dir(Platform::Telegram);
        std::fs::create_dir_all(&dir)?;
        let content = serde_json::to_string_pretty(config)?;
        let final_path = self.platform_config_path(Platform::Telegram);
        let temp_path = dir.join(format!(
            ".config.json.{}.{}.tmp",
            std::process::id(),
            chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default()
        ));
        write_config_file_securely(&temp_path, content.as_bytes())?;
        std::fs::rename(&temp_path, final_path)?;
        Ok(())
    }

    fn telegram_config_view(
        &self,
        config: &crate::connector::im::telegram::types::TelegramStoredConfig,
    ) -> Result<ChannelConfigView> {
        let token = match (&config.credentials.bot_token_storage, &self.secure_storage) {
            (SecretStorageKind::SecureStorage, Some(storage)) => {
                storage.decrypt(&config.credentials.bot_token_encrypted)?
            }
            (SecretStorageKind::SecureStorage, None) => anyhow::bail!(
                "Telegram bot_token marked SecureStorage but SecureStorage is unavailable"
            ),
            (SecretStorageKind::PlaintextFallback, _) => {
                config.credentials.bot_token_encrypted.clone()
            }
        };
        Ok(ChannelConfigView {
            platform: Platform::Telegram,
            app_key: config.bot.bot_username.clone(),
            app_secret_masked: mask_secret(&token),
            robot_code: config.bot.bot_id.clone(),
            robot_code_source: RobotCodeSource::Registration,
            source: crate::connector::im::telegram::types::TELEGRAM_BOT_TOKEN_SOURCE.to_string(),
            created_at: config.metadata.created_at.clone(),
            updated_at: config.metadata.updated_at.clone(),
        })
    }
```

并在文件底部（其它 validate_* 函数那边）加：

```rust
fn validate_telegram_config(
    config: &crate::connector::im::telegram::types::TelegramStoredConfig,
) -> Result<()> {
    if config.schema_version != 1 {
        anyhow::bail!(
            "Invalid Telegram config schema_version: expected 1, got {}",
            config.schema_version
        );
    }
    if config.platform != Platform::Telegram {
        anyhow::bail!(
            "Invalid Telegram config platform: expected telegram, got {}",
            config.platform.as_str()
        );
    }
    if !config.configured {
        anyhow::bail!("Invalid Telegram config: configured must be true");
    }
    validate_telegram_non_empty(
        &config.credentials.bot_token_encrypted,
        "credentials.bot_token_encrypted",
    )?;
    validate_telegram_non_empty(&config.bot.bot_id, "bot.bot_id")?;
    validate_telegram_non_empty(&config.bot.bot_username, "bot.bot_username")?;
    validate_telegram_non_empty(&config.metadata.created_at, "metadata.created_at")?;
    validate_telegram_non_empty(&config.metadata.updated_at, "metadata.updated_at")?;
    Ok(())
}

fn validate_telegram_non_empty(value: &str, field: &str) -> Result<()> {
    if value.trim().is_empty() {
        anyhow::bail!("Invalid Telegram config: {field} is required");
    }
    Ok(())
}
```

并在 `all_platform_states` 把 telegram 加入：

找到现有：
```rust
Ok(vec![
    self.dingtalk_state(connection.clone(), last_error.clone())?,
    self.feishu_state(connection.clone(), last_error.clone())?,
    Self::wechat_state_stub(),
    self.wecom_state(connection, last_error)?,
])
```

改为：
```rust
Ok(vec![
    self.dingtalk_state(connection.clone(), last_error.clone())?,
    self.feishu_state(connection.clone(), last_error.clone())?,
    Self::wechat_state_stub(),
    self.wecom_state(connection.clone(), last_error.clone())?,
    self.telegram_state(connection, last_error)?,
])
```

- [ ] **Step 2.4: 写 registration.rs**

Create `src-tauri/src/connector/im/telegram/registration.rs`:

```rust
//! 桌面端 pairing 命令实现：begin / list_pending / approve / reject / revoke 用户。
//!
//! 真正的 Bot API 调用走 connector.sender；store 操作走 ChannelConfigStore。

use std::sync::Arc;

use anyhow::Result;
use serde::{Deserialize, Serialize};

use super::connector::TelegramConnector;
use super::pairing::PendingPairing;
use super::types::AllowlistEntry;
use crate::connector::im::shared::config_store::ChannelConfigStore;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TelegramPairingBeginResult {
    pub code: String,
    pub deep_link: String,
    pub expires_in_seconds: u64,
    pub bot_username: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TelegramPendingPairing {
    pub code: String,
    pub user_id: i64,
    pub first_name: String,
    pub username: Option<String>,
    pub requested_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TelegramPairedUser {
    pub user_id: i64,
    pub first_name: String,
    pub username: Option<String>,
}

pub async fn begin_pairing(connector: &TelegramConnector) -> Result<TelegramPairingBeginResult> {
    let pairing = connector.pairing();
    let entry = pairing.begin().await?;
    let deep_link = format!(
        "https://t.me/{}?start={}",
        connector.bot_username(),
        entry.code
    );
    Ok(TelegramPairingBeginResult {
        code: entry.code,
        deep_link,
        expires_in_seconds: 300,
        bot_username: connector.bot_username().to_string(),
    })
}

pub async fn list_pending(connector: &TelegramConnector) -> Result<Vec<TelegramPendingPairing>> {
    let list = connector.pairing().list_pending().await;
    Ok(list
        .into_iter()
        .filter_map(|e: PendingPairing| {
            let p = e.pairer?;
            Some(TelegramPendingPairing {
                code: e.code,
                user_id: p.user_id,
                first_name: p.first_name,
                username: p.username,
                requested_at: p.attached_at.to_rfc3339(),
            })
        })
        .collect())
}

pub async fn approve(
    connector: &TelegramConnector,
    config_store: &Arc<ChannelConfigStore>,
    code: &str,
) -> Result<TelegramPairedUser> {
    let entry = connector
        .pairing()
        .take(code)
        .await
        .ok_or_else(|| anyhow::anyhow!("pairing code not found or expired"))?;
    let pairer = entry
        .pairer
        .ok_or_else(|| anyhow::anyhow!("pairing code has no pairer attached"))?;
    config_store.telegram_add_allowlist_entry(AllowlistEntry {
        user_id: pairer.user_id,
        first_name: pairer.first_name.clone(),
        username: pairer.username.clone(),
        paired_at: chrono::Utc::now().to_rfc3339(),
    })?;
    let _ = connector
        .sender()
        .send_plain(
            pairer.chat_id,
            "👋 你已连接 AIjia，可以开始对话。",
        )
        .await;
    Ok(TelegramPairedUser {
        user_id: pairer.user_id,
        first_name: pairer.first_name,
        username: pairer.username,
    })
}

pub async fn reject(connector: &TelegramConnector, code: &str) -> Result<()> {
    connector.pairing().drop(code).await;
    Ok(())
}

pub async fn revoke_user(
    connector: &TelegramConnector,
    config_store: &Arc<ChannelConfigStore>,
    user_id: i64,
) -> Result<()> {
    config_store.telegram_remove_allowlist_user(user_id)?;
    // 顺手发个通知（best-effort）
    // 私聊 chat_id == user_id
    let _ = connector
        .sender()
        .send_plain(user_id, "你已被 AIjia 管理员移除连接。")
        .await;
    Ok(())
}
```

- [ ] **Step 2.5: 写 reply_forwarder.rs**

Create `src-tauri/src/connector/im/telegram/reply_forwarder.rs`:

```rust
//! TelegramReplyForwarder — 镜像 WecomReplyForwarder：监听 MessagePersisted →
//! 一次性发整段 markdown 到 Telegram chat。

use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;

use super::connector::TelegramConnector;
use crate::connector::im::trait_def::{IMConnector, ReplyContent, ReplyTarget};
use crate::runtime::event_bus::RuntimeEventSubscriber;
use crate::runtime::events::{RuntimeEvent, RuntimeEventKind};

pub struct TelegramReplyForwarder {
    connector: Arc<TelegramConnector>,
}

impl TelegramReplyForwarder {
    pub fn new(connector: Arc<TelegramConnector>) -> Self {
        Self { connector }
    }

    fn extract_markdown(content: &serde_json::Value) -> Option<String> {
        let t = content.get("text").and_then(|v| v.as_str())?.trim();
        if t.is_empty() {
            None
        } else {
            Some(t.to_string())
        }
    }
}

#[async_trait]
impl RuntimeEventSubscriber for TelegramReplyForwarder {
    async fn on_event(&self, event: &RuntimeEvent) -> Result<()> {
        let session_id = event.session_id.as_str().to_string();
        if !self.connector.has_session(&session_id).await {
            return Ok(());
        }
        if let RuntimeEventKind::MessagePersisted { role, content, .. } = &event.kind {
            if role != "assistant" {
                return Ok(());
            }
            let Some(text) = Self::extract_markdown(content) else {
                return Ok(());
            };
            let target = ReplyTarget {
                session_id: session_id.clone(),
                external_conversation_key: String::new(),
            };
            if let Err(e) = self
                .connector
                .send(target, ReplyContent::Markdown(text))
                .await
            {
                log::warn!(
                    "[telegram-reply-forwarder] send Markdown failed (session={}): {:?}",
                    session_id,
                    e
                );
            }
        }
        Ok(())
    }
}
```

- [ ] **Step 2.6: factory 加 build_telegram_connector**

Modify `src-tauri/src/connector/im/factory.rs`:

在 `WechatStatusCallback` 类型别名旁加：

```rust
pub type TelegramStatusCallback =
    Arc<dyn Fn(ChannelConnectionState, Option<String>) + Send + Sync + 'static>;
```

在末尾加：

```rust
/// Build a `TelegramConnector` plus its concrete handle.
pub fn build_telegram_connector(
    bot_id: String,
    bot_username: String,
    token: String,
    config_store: Arc<crate::connector::im::shared::config_store::ChannelConfigStore>,
    on_status: TelegramStatusCallback,
) -> (
    Arc<dyn IMConnector>,
    Arc<crate::connector::im::telegram::connector::TelegramConnector>,
) {
    use crate::connector::im::telegram::connector::TelegramConnector;
    let concrete = Arc::new(TelegramConnector::new(
        bot_id,
        bot_username,
        token,
        config_store,
        on_status,
    ));
    let dyn_handle: Arc<dyn IMConnector> = Arc::clone(&concrete) as Arc<dyn IMConnector>;
    (dyn_handle, concrete)
}
```

- [ ] **Step 2.7: manager 接入 telegram**

Modify `src-tauri/src/connector/im/manager.rs`:

1. 在 `wechat_reply_subscribed` 那一行下面加：
   ```rust
   /// 同上，TelegramReplyForwarder 只挂一次。
   telegram_reply_subscribed: Arc<AtomicBool>,
   ```
2. 在 `new()` 初始化处对应加：
   ```rust
   telegram_reply_subscribed: Arc::new(AtomicBool::new(false)),
   ```
3. 在 `get_platform` 的 `match platform` 加：
   ```rust
   Platform::Telegram => self.config_store.telegram_state(connection, last_error),
   ```
4. 在 `set_enabled` 的 `match platform` 加（在 Wechat 分支之后）：
   ```rust
   Platform::Telegram => {
       if enabled {
           self.config_store.set_telegram_enabled(true)?;
           self.connect_telegram_from_store().await?;
           self.current_telegram_state().await
       } else {
           self.stop_stream(Platform::Telegram).await;
           self.config_store.set_telegram_enabled(false)?;
           self.set_telegram_connection_state(
               ChannelConnectionState::Disconnected,
               None,
           )
           .await;
           self.current_telegram_state().await
       }
   }
   ```
5. 在 `remove_platform` 的 `match platform` 加：
   ```rust
   Platform::Telegram => {
       self.stop_stream(Platform::Telegram).await;
       let state = self.config_store.remove_telegram()?;
       self.set_telegram_connection_state(
           ChannelConnectionState::Unconfigured,
           None,
       )
       .await;
       Ok(state)
   }
   ```
6. 在 `reveal_secret` 加：
   ```rust
   Platform::Telegram => self.config_store.reveal_telegram_token(),
   ```
7. 在 `auto_connect_if_configured` 加（在 wechat 那段之后）一个 Telegram 块（结构跟 wecom 完全对称，把 `feishu` 替换成 `telegram` / `Feishu` 替换成 `Telegram` / `connect_feishu_from_store` 替换成 `connect_telegram_from_store` / `set_feishu_connection_state` 替换成 `set_telegram_connection_state`）。
8. 加新方法（仿 wecom 那一整段，放在 wecom 段后面）：

   ```rust
   async fn register_telegram_connector(
       &self,
       bot_id: String,
       bot_username: String,
       token: String,
       on_status: super::factory::TelegramStatusCallback,
   ) -> Arc<super::telegram::connector::TelegramConnector> {
       let (dyn_conn, concrete) = super::factory::build_telegram_connector(
           bot_id,
           bot_username,
           token,
           Arc::clone(&self.config_store),
           on_status,
       );
       let mut map = self.connectors.write().await;
       map.insert(Platform::Telegram, dyn_conn);
       concrete
   }

   async fn current_telegram_state(&self) -> Result<ChannelPlatformState> {
       let (connection, last_error) = self
           .platform_state_read(Platform::Telegram, |s| {
               (s.connection.clone(), s.last_error.clone())
           })
           .await
           .unwrap_or((ChannelConnectionState::Unconfigured, None));
       self.config_store.telegram_state(connection, last_error)
   }

   async fn set_telegram_connection_state(
       &self,
       connection: ChannelConnectionState,
       last_error: Option<String>,
   ) {
       self.platform_state_mutate(Platform::Telegram, |s| {
           s.connection = connection.clone();
           s.last_error = last_error.clone();
       })
       .await;
       match self.config_store.telegram_state(connection.clone(), last_error) {
           Ok(state) => {
               let _ = self.app_handle.emit(
                   "channel:platform-state",
                   &ChannelPlatformStatePayload { state },
               );
           }
           Err(err) => log::warn!(
               "[channel/telegram] failed to emit platform state: {:#}",
               err
           ),
       }
   }

   pub async fn save_telegram_and_connect(
       &self,
       token: String,
       bot_id: String,
       bot_username: String,
       bot_first_name: String,
   ) -> Result<ChannelPlatformState> {
       self.config_store
           .save_telegram_registration(token, bot_id, bot_username, bot_first_name)?;
       self.connect_telegram_from_store().await?;
       self.current_telegram_state().await
   }

   pub async fn connect_telegram_from_store(&self) -> Result<()> {
       let (config, token) = self.config_store.decrypt_telegram_config()?;
       self.connect_telegram(config, token).await
   }

   async fn connect_telegram(
       &self,
       config: super::telegram::types::TelegramStoredConfig,
       token: String,
   ) -> Result<()> {
       self.stop_stream(Platform::Telegram).await;
       let bot_id = config.bot.bot_id.clone();
       let bot_username = config.bot.bot_username.clone();

       self.set_telegram_connection_state(ChannelConnectionState::Connecting, None)
           .await;

       let on_status: super::factory::TelegramStatusCallback = {
           let platform_state = Arc::clone(&self.platform_state);
           let config_store = Arc::clone(&self.config_store);
           let app = self.app_handle.clone();
           Arc::new(
               move |new_connection: ChannelConnectionState, error: Option<String>| {
                   let platform_state = platform_state.clone();
                   let config_store = config_store.clone();
                   let app = app.clone();
                   tokio::spawn(async move {
                       {
                           let mut map = platform_state.write().await;
                           let slot = map
                               .entry(Platform::Telegram)
                               .or_insert_with(PerPlatformState::unconfigured);
                           slot.connection = new_connection.clone();
                           slot.last_error = error.clone();
                       }
                       match config_store.telegram_state(new_connection, error) {
                           Ok(state) => {
                               let _ = app.emit(
                                   "channel:platform-state",
                                   &ChannelPlatformStatePayload { state },
                               );
                           }
                           Err(err) => log::warn!(
                               "[channel/telegram] failed to build platform state: {:#}",
                               err
                           ),
                       }
                   });
               },
           )
       };

       let concrete = self
           .register_telegram_connector(
               bot_id.clone(),
               bot_username,
               token,
               Arc::clone(&on_status),
           )
           .await;

       if claim_first_subscription(&self.telegram_reply_subscribed) {
           let forwarder =
               Arc::new(super::telegram::reply_forwarder::TelegramReplyForwarder::new(
                   Arc::clone(&concrete),
               ));
           let sub = forwarder as Arc<dyn crate::runtime::event_bus::RuntimeEventSubscriber>;
           self.chat_adapter.subscribe_event_listener(sub);
           log::info!("[channel/telegram] subscribed TelegramReplyForwarder to RuntimeEventBus");
       }

       let new_token = CancellationToken::new();
       let ctx = ConnectorContext {
           config_store: Arc::clone(&self.config_store),
           secure_storage: None,
           ask_coordinator: self.ask_coordinator.as_ref().map(Arc::clone),
           pending_manager: Arc::clone(&self.pending_manager),
           cancel_token: new_token.clone(),
       };
       let connector = {
           let map = self.connectors.read().await;
           map.get(&Platform::Telegram)
               .cloned()
               .ok_or_else(|| anyhow::anyhow!("telegram connector not registered"))?
       };
       let mut message_stream = connector
           .start(ctx)
           .await
           .map_err(|e| anyhow::anyhow!("telegram connector start failed: {e}"))?;

       self.platform_state_mutate(Platform::Telegram, |s| {
           s.stream_cancel = Some(new_token);
       })
       .await;

       let app_handle = self.app_handle.clone();
       let bot_id_for_worker = bot_id.clone();
       let concrete_for_worker = Arc::clone(&concrete);
       let chat_adapter = Arc::clone(&self.chat_adapter);
       let conv_store = Arc::clone(&self.conversation_store);
       let sessions_path = self.sessions_paths[&Platform::Telegram].clone();
       let convs = Arc::clone(&self.conversations);
       let channel_session_ids_ref = Arc::clone(&self.channel_session_ids);
       let pending_manager_ref = Arc::clone(&self.pending_manager);

       let message_handle = tokio::spawn(async move {
           let mut router = match super::shared::router::ChannelSessionRouter::migrate_or_load(
               &sessions_path,
               conv_store.as_ref(),
           ) {
               Ok(r) => r,
               Err(e) => {
                   log::error!("[channel/telegram] failed to load router: {:#}", e);
                   return;
               }
           };
           while let Some(msg) = message_stream.next().await {
               let router_key = format!("tg-{}", bot_id_for_worker);
               let session_id = router
                   .get_or_create_session(
                       &router_key,
                       &msg.conversation_key,
                       Platform::Telegram,
                       &msg.conversation_type,
                       &msg.sender_nick,
                   )
                   .unwrap_or_default();
               if session_id.is_empty() {
                   continue;
               }
               {
                   let mut ids = channel_session_ids_ref.write().expect("channel_session_ids lock");
                   ids.insert(session_id.clone());
               }
               concrete_for_worker
                   .remember_session(
                       session_id.clone(),
                       super::telegram::types::TelegramSessionTarget {
                           chat_id: msg.conversation_key.parse().unwrap_or(0),
                           user_id: msg.sender_id.parse().unwrap_or(0),
                       },
                   )
                   .await;
               {
                   let mut convs_lock = convs.write().await;
                   if !convs_lock.iter().any(|c| c.session_id == session_id) {
                       convs_lock.push(ChannelConversation {
                           session_id: session_id.clone(),
                           platform: Platform::Telegram,
                           conversation_type: msg.conversation_type.clone(),
                           external_id: msg.conversation_key.clone(),
                           display_name: msg.sender_nick.clone(),
                           unread_count: 0,
                           robot_code: router_key.clone(),
                           is_active_robot: true,
                       });
                   }
               }
               let preview = if msg.text.chars().count() > 30 {
                   format!("{}...", msg.text.chars().take(30).collect::<String>())
               } else {
                   msg.text.clone()
               };
               let _ = app_handle.emit(
                   "channel:message",
                   &ChannelMessagePayload {
                       platform: "telegram".into(),
                       session_id: session_id.clone(),
                       sender_nick: msg.sender_nick.clone(),
                       text_preview: preview,
                   },
               );
               // 走 pending_manager / chat_adapter（参考 wecom worker 里同样代码块）
               let request = ChatTurnRequest {
                   session_id: session_id.clone(),
                   user_text: msg.text.clone(),
                   attachments: vec![],
                   im_source: Some(format!("telegram:{}", msg.conversation_key)),
                   user_id: Some(msg.sender_id.clone()),
                   user_nick: Some(msg.sender_nick.clone()),
                   ..Default::default()
               };
               let adapter_clone = chat_adapter.clone();
               let pending = pending_manager_ref.clone();
               tokio::spawn(async move {
                   let _ = pending
                       .enqueue_or_send(adapter_clone, request)
                       .await;
               });
           }
           log::info!("[channel/telegram] worker stream ended");
       });

       self.platform_state_mutate(Platform::Telegram, |s| {
           s.message_task = Some(message_handle);
       })
       .await;

       Ok(())
   }
   ```

> 注：上面 ChatTurnRequest 字段名 / pending_manager.enqueue_or_send 签名以现仓代码为准；如 spread `..Default::default()` 不通过，去查 wecom worker 那段对应行（manager.rs:~860）照抄。

- [ ] **Step 2.8: 增加 Tauri commands**

Modify `src-tauri/src/commands/channel.rs` —— 在文件末尾加：

```rust
// ---- Telegram-specific commands -------------------------------------------

use crate::connector::im::telegram::registration as tg_reg;

#[tauri::command]
pub async fn channel_telegram_save(
    app: AppHandle,
    token: String,
) -> Result<crate::connector::im::types::ChannelPlatformState, String> {
    use crate::connector::im::telegram::api::TelegramApi;
    let api = TelegramApi::new(token.clone());
    let info = api
        .get_me()
        .await
        .map_err(|e| format!("token 验证失败：{e}"))?;
    let username = info
        .username
        .ok_or_else(|| "bot 缺少 username，请在 BotFather 里设置".to_string())?;
    manager(&app)?
        .save_telegram_and_connect(token, info.id.to_string(), username, info.first_name)
        .await
        .map_err(|e| format!("{:#}", e))
}

#[tauri::command]
pub async fn channel_telegram_remove(
    app: AppHandle,
) -> Result<crate::connector::im::types::ChannelPlatformState, String> {
    manager(&app)?
        .remove_platform(Platform::Telegram)
        .await
        .map_err(|e| format!("{:#}", e))
}

#[tauri::command]
pub async fn channel_telegram_set_enabled(
    app: AppHandle,
    enabled: bool,
) -> Result<crate::connector::im::types::ChannelPlatformState, String> {
    manager(&app)?
        .set_enabled(Platform::Telegram, enabled)
        .await
        .map_err(|e| format!("{:#}", e))
}

async fn telegram_connector(
    app: &AppHandle,
) -> Result<std::sync::Arc<crate::connector::im::telegram::connector::TelegramConnector>, String> {
    manager(app)?
        .telegram_connector()
        .await
        .ok_or_else(|| "Telegram connector 未启动".to_string())
}

#[tauri::command]
pub async fn channel_telegram_begin_pairing(
    app: AppHandle,
) -> Result<tg_reg::TelegramPairingBeginResult, String> {
    let c = telegram_connector(&app).await?;
    tg_reg::begin_pairing(&c)
        .await
        .map_err(|e| format!("{:#}", e))
}

#[tauri::command]
pub async fn channel_telegram_list_pending_pairings(
    app: AppHandle,
) -> Result<Vec<tg_reg::TelegramPendingPairing>, String> {
    let c = telegram_connector(&app).await?;
    tg_reg::list_pending(&c).await.map_err(|e| format!("{:#}", e))
}

#[tauri::command]
pub async fn channel_telegram_approve_pairing(
    app: AppHandle,
    code: String,
) -> Result<tg_reg::TelegramPairedUser, String> {
    let c = telegram_connector(&app).await?;
    let cs = manager(&app)?.config_store_arc();
    tg_reg::approve(&c, &cs, &code).await.map_err(|e| format!("{:#}", e))
}

#[tauri::command]
pub async fn channel_telegram_reject_pairing(
    app: AppHandle,
    code: String,
) -> Result<(), String> {
    let c = telegram_connector(&app).await?;
    tg_reg::reject(&c, &code).await.map_err(|e| format!("{:#}", e))
}

#[tauri::command]
pub async fn channel_telegram_revoke_user(
    app: AppHandle,
    user_id: i64,
) -> Result<crate::connector::im::types::ChannelPlatformState, String> {
    let c = telegram_connector(&app).await?;
    let cs = manager(&app)?.config_store_arc();
    tg_reg::revoke_user(&c, &cs, user_id)
        .await
        .map_err(|e| format!("{:#}", e))?;
    manager(&app)?
        .get_platform(Platform::Telegram)
        .await
        .map_err(|e| format!("{:#}", e))
}
```

并在 ChannelManager 加两个 helper（manager.rs 末尾 `impl ChannelManager` 之前或之中）：

```rust
pub async fn telegram_connector(
    &self,
) -> Option<Arc<super::telegram::connector::TelegramConnector>> {
    let _map = self.connectors.read().await;
    // 我们的 connectors map 存 Arc<dyn IMConnector>，需要拿 concrete TelegramConnector：
    // PR2 实现里，register_telegram_connector 把 concrete 返回给调用方但 map 里只存 dyn。
    // 这里再额外维护一个 telegram_concrete: RwLock<Option<Arc<TelegramConnector>>>。
    self.telegram_concrete.read().await.clone()
}

pub fn config_store_arc(&self) -> Arc<ChannelConfigStore> {
    Arc::clone(&self.config_store)
}
```

并在 `pub struct ChannelManager { ... }` 加字段：

```rust
telegram_concrete: Arc<tokio::sync::RwLock<Option<Arc<super::telegram::connector::TelegramConnector>>>>,
```

`new()` 初始化：

```rust
telegram_concrete: Arc::new(tokio::sync::RwLock::new(None)),
```

`register_telegram_connector` 加完之后写入：

```rust
*self.telegram_concrete.write().await = Some(Arc::clone(&concrete));
```

`remove_platform(Platform::Telegram)` 分支末尾置空：

```rust
*self.telegram_concrete.write().await = None;
```

- [ ] **Step 2.9: lib.rs 注册命令**

Modify `src-tauri/src/lib.rs`，找到 `commands::channel::channel_wecom_poll_registration,` 之后插入：

```rust
            commands::channel::channel_telegram_save,
            commands::channel::channel_telegram_remove,
            commands::channel::channel_telegram_set_enabled,
            commands::channel::channel_telegram_begin_pairing,
            commands::channel::channel_telegram_list_pending_pairings,
            commands::channel::channel_telegram_approve_pairing,
            commands::channel::channel_telegram_reject_pairing,
            commands::channel::channel_telegram_revoke_user,
```

- [ ] **Step 2.10: 写集成测试**

Create `src-tauri/tests/telegram_pairing_integration_test.rs`:

```rust
//! 集成：mock Bot API → 模拟 /start <code> → approve → 验证 allowlist 写盘 + 欢迎消息发送。
//!
//! 不起 ChannelManager（manager 依赖 AppHandle，无法 hermetic 测）；改成直接对
//! TelegramConnector + registration 路径下断言。

use std::sync::Arc;

use lotus_app::connector::im::shared::config_store::ChannelConfigStore;
use lotus_app::connector::im::telegram::api::TelegramApi;
use lotus_app::connector::im::telegram::connector::TelegramConnector;
use lotus_app::connector::im::telegram::pairing::{AttachOutcome, PairerInfo};
use lotus_app::connector::im::telegram::registration;
use tempfile::TempDir;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

async fn build_setup() -> (TempDir, Arc<ChannelConfigStore>, Arc<TelegramConnector>, MockServer) {
    let dir = TempDir::new().unwrap();
    let cs = Arc::new(ChannelConfigStore::new(
        dir.path().join("channels"),
        None,
    ));
    cs.save_telegram_registration(
        "TESTTOKEN".into(),
        "8123".into(),
        "test_bot".into(),
        "Test Bot".into(),
    )
    .unwrap();
    let server = MockServer::start().await;
    // 让 connector 内部 API 指向 mock server
    let api = Arc::new(TelegramApi::new_with_api_base_for_tests(
        "TESTTOKEN".into(),
        server.uri(),
    ));
    let connector = Arc::new(TelegramConnector::for_test(
        "8123".into(),
        "test_bot".into(),
        api,
        cs.clone(),
    ));
    (dir, cs, connector, server)
}

#[tokio::test]
async fn approve_writes_allowlist_and_sends_welcome() {
    let (_dir, cs, connector, server) = build_setup().await;
    Mock::given(method("POST"))
        .and(path("/botTESTTOKEN/sendMessage"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "ok": true,
            "result": { "message_id": 1, "chat": { "id": 42, "type": "private" } }
        })))
        .mount(&server)
        .await;
    let begin = registration::begin_pairing(&connector).await.unwrap();
    assert!(begin.deep_link.starts_with("https://t.me/test_bot?start="));
    let outcome = connector
        .pairing()
        .attempt_attach(
            &begin.code,
            PairerInfo {
                user_id: 42,
                first_name: "Alice".into(),
                username: None,
                chat_id: 42,
                attached_at: chrono::Utc::now(),
            },
        )
        .await;
    assert_eq!(outcome, AttachOutcome::Attached);
    let pending = registration::list_pending(&connector).await.unwrap();
    assert_eq!(pending.len(), 1);
    let user = registration::approve(&connector, &cs, &pending[0].code)
        .await
        .unwrap();
    assert_eq!(user.user_id, 42);
    assert!(cs.telegram_is_in_allowlist(42).unwrap());
}
```

> `TelegramConnector::for_test(...)` 需要在 connector.rs 加一个 `#[doc(hidden)]` 构造：

Modify `src-tauri/src/connector/im/telegram/connector.rs` —— 在 `impl TelegramConnector` 末尾加：

```rust
    #[doc(hidden)]
    pub fn for_test(
        bot_id: String,
        bot_username: String,
        api: Arc<TelegramApi>,
        config_store: Arc<ChannelConfigStore>,
    ) -> Self {
        let sender = TelegramSender::new(api.clone());
        Self {
            bot_id,
            bot_username,
            api,
            sender,
            pairing: PairingCodeStore::new(),
            session_targets: Arc::new(RwLock::new(HashMap::new())),
            config_store,
            on_status: Arc::new(|_, _| {}),
        }
    }
```

- [ ] **Step 2.11: 编译 + 测试**

Run: `cd src-tauri && cargo build && cargo test --lib connector::im::telegram && cargo test --test telegram_pairing_integration_test`
Expected: 全部 PASS

- [ ] **Step 2.12: Commit PR2**

```bash
git add -A
git commit -m "$(cat <<'EOF'
feat(connector/im/telegram): PR2 后端接入（manager + long_poll + commands）

接入 ChannelManager：register_telegram_connector / connect_telegram /
save_telegram_and_connect / set_telegram_connection_state；getUpdates
长轮询 + offset 落盘 + Pairing 流程；7 个 Tauri 命令；wiremock 集成测试。

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 3 — PR3 前端配置 UI

**Files:**
- Create: `src/features/channel/TelegramChannelConfig.tsx`
- Modify: `src/features/channel/ChannelPage.tsx`
- Modify: `src/lib/tauri.ts` 加 TS 类型 + IPC 函数
- Create: `src/features/channel/__tests__/TelegramChannelConfig.test.tsx`

- [ ] **Step 3.1: tauri.ts 加类型 + IPC 包装**

Modify `src/lib/tauri.ts` —— 在文件末尾（onChannelMessage 之后）加：

```typescript
// ---------------------------------------------------------------------------
// Telegram-specific channel commands
// ---------------------------------------------------------------------------

export interface TelegramPairingBeginResult {
  code: string
  deepLink: string
  expiresInSeconds: number
  botUsername: string
}

export interface TelegramPendingPairing {
  code: string
  userId: number
  firstName: string
  username: string | null
  requestedAt: string
}

export interface TelegramPairedUser {
  userId: number
  firstName: string
  username: string | null
}

export function channelTelegramSave(token: string): Promise<ChannelPlatformState> {
  return invoke<ChannelPlatformState>('channel_telegram_save', { token })
}

export function channelTelegramRemove(): Promise<ChannelPlatformState> {
  return invoke<ChannelPlatformState>('channel_telegram_remove')
}

export function channelTelegramSetEnabled(enabled: boolean): Promise<ChannelPlatformState> {
  return invoke<ChannelPlatformState>('channel_telegram_set_enabled', { enabled })
}

export function channelTelegramBeginPairing(): Promise<TelegramPairingBeginResult> {
  return invoke<TelegramPairingBeginResult>('channel_telegram_begin_pairing')
}

export function channelTelegramListPendingPairings(): Promise<TelegramPendingPairing[]> {
  return invoke<TelegramPendingPairing[]>('channel_telegram_list_pending_pairings')
}

export function channelTelegramApprovePairing(code: string): Promise<TelegramPairedUser> {
  return invoke<TelegramPairedUser>('channel_telegram_approve_pairing', { code })
}

export function channelTelegramRejectPairing(code: string): Promise<void> {
  return invoke<void>('channel_telegram_reject_pairing', { code })
}

export function channelTelegramRevokeUser(userId: number): Promise<ChannelPlatformState> {
  return invoke<ChannelPlatformState>('channel_telegram_revoke_user', { userId })
}
```

- [ ] **Step 3.2: 写 TelegramChannelConfig.tsx**

Create `src/features/channel/TelegramChannelConfig.tsx`:

```tsx
import { useEffect, useRef, useState } from 'react'
import QRCode from 'qrcode'
import { open as openExternal } from '@tauri-apps/plugin-shell'
import { CheckCircle2, ExternalLink, Loader2, X } from 'lucide-react'
import { requestConfirm } from '@/components/common/ConfirmDialogHost'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import {
  channelTelegramApprovePairing,
  channelTelegramBeginPairing,
  channelTelegramListPendingPairings,
  channelTelegramRejectPairing,
  channelTelegramRemove,
  channelTelegramRevokeUser,
  channelTelegramSave,
  type TelegramPairingBeginResult,
  type TelegramPendingPairing,
} from '@/lib/tauri'
import { useChannelStore } from '@/stores/channelStore'
import { useNotificationStore } from '@/stores/notificationStore'

interface TelegramChannelConfigProps {
  onSaved?: () => void
  onClose?: () => void
}

const POLL_INTERVAL_MS = 2000

function QrPanel({ value }: { value: string | null }) {
  const [dataUrl, setDataUrl] = useState<string | null>(null)
  useEffect(() => {
    if (!value) {
      setDataUrl(null)
      return
    }
    let cancelled = false
    QRCode.toDataURL(value, {
      errorCorrectionLevel: 'M',
      margin: 1,
      width: 224,
      color: { dark: '#111111', light: '#ffffff' },
    })
      .then((url) => {
        if (!cancelled) setDataUrl(url)
      })
      .catch(() => {
        if (!cancelled) setDataUrl(null)
      })
    return () => {
      cancelled = true
    }
  }, [value])

  return (
    <div className="flex h-60 w-60 items-center justify-center rounded-3xl border border-border bg-white p-4">
      {dataUrl ? (
        <img src={dataUrl} alt="Telegram 扫码配对" className="h-full w-full" />
      ) : (
        <Loader2 className="h-8 w-8 animate-spin text-primary" />
      )}
    </div>
  )
}

export function TelegramChannelConfig({ onSaved, onClose }: TelegramChannelConfigProps) {
  const tgState = useChannelStore((s) => s.platforms.telegram)
  const setPlatformState = useChannelStore((s) => s.setPlatformState)
  const pushNotification = useNotificationStore((s) => s.push)

  const alreadyConfigured = tgState?.configured ?? false

  const [step, setStep] = useState<'token' | 'pairing'>(alreadyConfigured ? 'pairing' : 'token')
  const [token, setToken] = useState('')
  const [saving, setSaving] = useState(false)
  const [error, setError] = useState<string | null>(null)

  const [begin, setBegin] = useState<TelegramPairingBeginResult | null>(null)
  const [pending, setPending] = useState<TelegramPendingPairing[]>([])
  const [remaining, setRemaining] = useState(0)
  const pollTimerRef = useRef<number | null>(null)
  const expireTimerRef = useRef<number | null>(null)

  const handleSaveToken = async () => {
    const trimmed = token.trim()
    if (!trimmed) {
      setError('请输入 bot token')
      return
    }
    setSaving(true)
    setError(null)
    try {
      const state = await channelTelegramSave(trimmed)
      setPlatformState(state)
      setStep('pairing')
      onSaved?.()
    } catch (e) {
      setError(e instanceof Error ? e.message : 'token 验证失败')
    } finally {
      setSaving(false)
    }
  }

  const refreshQr = async () => {
    try {
      const r = await channelTelegramBeginPairing()
      setBegin(r)
      setRemaining(r.expiresInSeconds)
    } catch (e) {
      pushNotification({
        level: 'error',
        title: '生成配对码失败',
        message: e instanceof Error ? e.message : String(e),
        actions: [],
        dismissible: true,
        autoHide: 4,
        context: 'toast',
      })
    }
  }

  const pollPending = async () => {
    try {
      const list = await channelTelegramListPendingPairings()
      setPending(list)
    } catch {
      // 静默；下次轮询继续
    }
  }

  useEffect(() => {
    if (step !== 'pairing') return
    void refreshQr()
    pollTimerRef.current = window.setInterval(() => void pollPending(), POLL_INTERVAL_MS)
    expireTimerRef.current = window.setInterval(() => {
      setRemaining((r) => {
        if (r <= 1) {
          void refreshQr()
          return 0
        }
        return r - 1
      })
    }, 1000)
    return () => {
      if (pollTimerRef.current !== null) window.clearInterval(pollTimerRef.current)
      if (expireTimerRef.current !== null) window.clearInterval(expireTimerRef.current)
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [step])

  const handleApprove = async (code: string) => {
    try {
      await channelTelegramApprovePairing(code)
      await pollPending()
    } catch (e) {
      pushNotification({
        level: 'error',
        title: '批准失败',
        message: e instanceof Error ? e.message : String(e),
        actions: [],
        dismissible: true,
        autoHide: 4,
        context: 'toast',
      })
    }
  }

  const handleReject = async (code: string) => {
    try {
      await channelTelegramRejectPairing(code)
      await pollPending()
    } catch {
      // 静默
    }
  }

  const handleRevokeUser = async (userId: number) => {
    const confirmed = await requestConfirm({
      title: '移除该 Telegram 用户？',
      description: '用户将不再能与你的 bot 对话；需要重新扫码才能恢复连接。',
      confirmLabel: '确认移除',
      cancelLabel: '取消',
      variant: 'destructive',
    })
    if (!confirmed) return
    try {
      const state = await channelTelegramRevokeUser(userId)
      setPlatformState(state)
    } catch (e) {
      pushNotification({
        level: 'error',
        title: '移除失败',
        message: e instanceof Error ? e.message : String(e),
        actions: [],
        dismissible: true,
        autoHide: 4,
        context: 'toast',
      })
    }
  }

  const handleRemove = async () => {
    const confirmed = await requestConfirm({
      title: '移除 Telegram 频道？',
      description: '会删除本地保存的 bot token 和已配对用户列表。已有聊天历史保留。',
      confirmLabel: '确认移除',
      cancelLabel: '取消',
      variant: 'destructive',
    })
    if (!confirmed) return
    try {
      const state = await channelTelegramRemove()
      setPlatformState(state)
      onClose?.()
    } catch (e) {
      pushNotification({
        level: 'error',
        title: '移除失败',
        message: e instanceof Error ? e.message : String(e),
        actions: [],
        dismissible: true,
        autoHide: 4,
        context: 'toast',
      })
    }
  }

  // ---- render ---------------------------------------------------------------

  if (step === 'token') {
    return (
      <div className="flex max-h-[78vh] w-full flex-col overflow-hidden bg-background">
        <div className="flex flex-col items-center px-10 pb-5 pt-8 text-center">
          <h2 className="text-2xl font-bold tracking-tight text-foreground">配置 Telegram</h2>
          <p className="mt-3 text-sm font-medium text-muted-foreground">
            在 Telegram 中找到 @BotFather 创建 bot，将拿到的 token 粘贴到下面。
          </p>
        </div>
        <div className="flex-1 overflow-y-auto px-10 pb-6">
          <div className="flex flex-col gap-3">
            <Button
              type="button"
              variant="secondary"
              className="w-full"
              onClick={() => void openExternal('https://t.me/BotFather')}
            >
              <ExternalLink className="mr-2 h-4 w-4" />
              打开 BotFather
            </Button>
            <label className="text-xs font-semibold text-foreground" htmlFor="tg-token">
              Bot Token <span className="text-destructive">*</span>
            </label>
            <Input
              id="tg-token"
              type="password"
              value={token}
              onChange={(e) => setToken(e.target.value)}
              placeholder="123456789:ABCdef..."
              autoComplete="new-password"
            />
            {error && <p className="text-sm text-destructive">{error}</p>}
            <details className="text-xs text-muted-foreground">
              <summary className="cursor-pointer font-semibold text-foreground">
                BotFather 使用步骤
              </summary>
              <ol className="ml-4 mt-2 list-decimal space-y-1">
                <li>在手机或桌面 Telegram 搜索 <span className="font-mono">@BotFather</span> 并打开对话</li>
                <li>发送 <span className="font-mono">/newbot</span></li>
                <li>按提示输入 bot 名称（可中文）和 username（必须以 _bot 结尾）</li>
                <li>把返回的 token（形如 <span className="font-mono">123456:ABC...</span>）复制到上面</li>
              </ol>
            </details>
          </div>
        </div>
        <div className="flex gap-3 border-t border-border bg-background px-10 py-4">
          <Button variant="ghost" className="flex-1 rounded-full" onClick={onClose}>
            取消
          </Button>
          <Button
            className="flex-1 rounded-full"
            disabled={!token.trim() || saving}
            onClick={() => void handleSaveToken()}
          >
            {saving && <Loader2 className="mr-2 h-4 w-4 animate-spin" />}
            下一步
          </Button>
        </div>
      </div>
    )
  }

  // step === 'pairing'
  const allowlist = tgState?.config?.appKey ? (tgState as any).allowlist ?? [] : []
  const m = Math.floor(remaining / 60)
  const s = remaining % 60

  return (
    <div className="flex max-h-[78vh] w-full flex-col overflow-hidden bg-background">
      <div className="flex flex-col items-center px-10 pb-5 pt-8 text-center">
        <h2 className="text-2xl font-bold tracking-tight text-foreground">扫码配对</h2>
        <p className="mt-3 text-sm font-medium text-muted-foreground">
          {tgState?.config ? `@${tgState.config.appKey}` : 'Telegram bot'}
        </p>
      </div>
      <div className="flex-1 overflow-y-auto px-10 pb-6">
        <div className="flex flex-col items-center gap-4">
          <QrPanel value={begin?.deepLink ?? null} />
          <div className="text-xs text-muted-foreground">
            二维码 {m}:{s.toString().padStart(2, '0')} 后过期
          </div>
          {begin?.deepLink && (
            <button
              type="button"
              onClick={() => void openExternal(begin.deepLink)}
              className="inline-flex items-center gap-1 text-xs font-medium text-primary underline-offset-4 hover:underline"
            >
              无法扫码？在浏览器打开 <ExternalLink className="h-3 w-3" />
            </button>
          )}

          {pending.length > 0 && (
            <div className="flex w-full flex-col gap-2 rounded-xl border border-border bg-card p-3">
              <div className="text-xs font-semibold text-foreground">待批准</div>
              {pending.map((p) => (
                <div key={p.code} className="flex items-center justify-between gap-2 rounded-lg bg-muted px-3 py-2 text-sm">
                  <span className="font-semibold text-foreground">
                    {p.firstName}
                    {p.username && <span className="ml-1 text-muted-foreground">@{p.username}</span>}
                  </span>
                  <div className="flex gap-2">
                    <Button size="sm" className="rounded-full" onClick={() => void handleApprove(p.code)}>
                      <CheckCircle2 className="mr-1 h-4 w-4" />
                      批准
                    </Button>
                    <Button size="sm" variant="ghost" className="rounded-full" onClick={() => void handleReject(p.code)}>
                      <X className="h-4 w-4" />
                    </Button>
                  </div>
                </div>
              ))}
            </div>
          )}

          {Array.isArray(allowlist) && allowlist.length > 0 && (
            <div className="flex w-full flex-col gap-2 rounded-xl border border-border bg-card p-3">
              <div className="text-xs font-semibold text-foreground">已连接用户</div>
              {allowlist.map((u: any) => (
                <div key={u.userId} className="flex items-center justify-between gap-2 rounded-lg bg-muted px-3 py-2 text-sm">
                  <span className="font-semibold text-foreground">{u.firstName}</span>
                  <Button size="sm" variant="ghost" className="rounded-full" onClick={() => void handleRevokeUser(u.userId)}>
                    移除
                  </Button>
                </div>
              ))}
            </div>
          )}
        </div>
      </div>
      <div className="flex gap-3 border-t border-border bg-background px-10 py-4">
        {alreadyConfigured && (
          <Button variant="destructive" className="flex-1 rounded-full" onClick={() => void handleRemove()}>
            移除整个频道
          </Button>
        )}
        <Button className={`rounded-full ${alreadyConfigured ? 'flex-1' : 'w-full'}`} onClick={onClose}>
          完成
        </Button>
      </div>
    </div>
  )
}
```

> 注：`allowlist` 来自 ChannelPlatformState.config 当前没暴露的字段。如果要让前端看到已连接用户，需要在 PR2 中扩展 `ChannelConfigView` 或新加 `telegramAllowlist` 字段。本步先用 `(tgState as any).allowlist ?? []` 占位，PR4 联调时根据实际情况补齐 schema 暴露。

- [ ] **Step 3.3: ChannelPage 接入 telegram**

Modify `src/features/channel/ChannelPage.tsx`:

1. 在 import 块底部加：
   ```tsx
   import { TelegramChannelConfig } from './TelegramChannelConfig'
   ```
2. `useState` 加 telegram registration dialog：
   ```tsx
   const [telegramRegistrationOpen, setTelegramRegistrationOpen] = useState(false)
   ```
3. `telegramState` 已存在，把 `capability: 'comingSoon'` 改为 `'available'`：
   ```tsx
   const telegramState = platformsByKey.telegram ?? {
     platform: 'telegram',
     capability: 'available',
     ...
   }
   ```
4. 加 handler：
   ```tsx
   const handleRemoveTelegram = async () => {
     const confirmed = await requestConfirm({
       title: '移除 Telegram 频道？',
       description: '会删除本地保存的 bot token 和已配对用户列表。',
       confirmLabel: '确认移除',
       cancelLabel: '取消',
       variant: 'destructive',
     })
     if (!confirmed) return
     await removePlatform('telegram')
   }
   const handleToggleTelegram = async (enabled: boolean) => {
     await setEnabled('telegram', enabled)
   }
   ```
5. `ChannelOverview` props 加 `onRegisterTelegram` / `onRemoveTelegram` / `onToggleTelegram` 3 个并在 onRegister / onRemove / onToggle 三元链里加上 telegram 分支。
6. 末尾在 wechat Dialog 之后加 Telegram Dialog：
   ```tsx
   <Dialog open={telegramRegistrationOpen} onOpenChange={setTelegramRegistrationOpen}>
     <DialogContent className="max-w-xl overflow-hidden rounded-xl border border-border bg-background p-0 shadow-[var(--shadow-modal)]">
       <DialogHeader className="sr-only">
         <DialogTitle>配置 Telegram</DialogTitle>
         <DialogDescription>输入 Bot Token 并扫码配对。</DialogDescription>
       </DialogHeader>
       <TelegramChannelConfig
         onSaved={() => { void loadConversations() }}
         onClose={() => setTelegramRegistrationOpen(false)}
       />
     </DialogContent>
   </Dialog>
   ```

- [ ] **Step 3.4: 写 Vitest（最小）**

Create `src/features/channel/__tests__/TelegramChannelConfig.test.tsx`:

```tsx
import { describe, expect, it, vi, beforeEach } from 'vitest'
import { fireEvent, render, screen, waitFor } from '@testing-library/react'

vi.mock('@/lib/tauri', () => ({
  channelTelegramSave: vi.fn(),
  channelTelegramRemove: vi.fn(),
  channelTelegramBeginPairing: vi.fn().mockResolvedValue({
    code: 'ABC12345',
    deepLink: 'https://t.me/test_bot?start=ABC12345',
    expiresInSeconds: 300,
    botUsername: 'test_bot',
  }),
  channelTelegramListPendingPairings: vi.fn().mockResolvedValue([]),
  channelTelegramApprovePairing: vi.fn(),
  channelTelegramRejectPairing: vi.fn(),
  channelTelegramRevokeUser: vi.fn(),
  channelTelegramSetEnabled: vi.fn(),
}))
vi.mock('@tauri-apps/plugin-shell', () => ({ open: vi.fn() }))
vi.mock('qrcode', () => ({
  default: { toDataURL: vi.fn().mockResolvedValue('data:image/png;base64,xxx') },
}))
vi.mock('@/stores/channelStore', () => ({
  useChannelStore: (selector: any) => selector({ platforms: {}, setPlatformState: vi.fn() }),
}))
vi.mock('@/stores/notificationStore', () => ({
  useNotificationStore: (selector: any) => selector({ push: vi.fn() }),
}))
vi.mock('@/components/common/ConfirmDialogHost', () => ({
  requestConfirm: vi.fn().mockResolvedValue(true),
}))

import { TelegramChannelConfig } from '../TelegramChannelConfig'
import { channelTelegramSave } from '@/lib/tauri'

describe('TelegramChannelConfig', () => {
  beforeEach(() => {
    vi.clearAllMocks()
  })

  it('disables 下一步 button when token is empty', () => {
    render(<TelegramChannelConfig />)
    const nextBtn = screen.getByText('下一步').closest('button') as HTMLButtonElement
    expect(nextBtn.disabled).toBe(true)
  })

  it('calls channelTelegramSave when 下一步 clicked with token', async () => {
    ;(channelTelegramSave as any).mockResolvedValue({
      platform: 'telegram',
      configured: true,
      enabled: true,
      capability: 'available',
      connection: 'connected',
      config: null,
    })
    render(<TelegramChannelConfig />)
    const input = screen.getByPlaceholderText(/123456789:/) as HTMLInputElement
    fireEvent.change(input, { target: { value: '123:abc' } })
    fireEvent.click(screen.getByText('下一步'))
    await waitFor(() => expect(channelTelegramSave).toHaveBeenCalledWith('123:abc'))
  })
})
```

- [ ] **Step 3.5: 运行前端测试 + lint**

Run: `pnpm exec vitest run src/features/channel/__tests__/TelegramChannelConfig.test.tsx && pnpm lint`
Expected: PASS

- [ ] **Step 3.6: Commit PR3**

```bash
git add -A
git commit -m "$(cat <<'EOF'
feat(channel/telegram): PR3 前端配置 UI（token + QR + 待批准列表）

TelegramChannelConfig 两步弹窗：① token 输入 + BotFather 跳转引导
② QR 扫码 + 5min 倒计时 + 待批准列表 + 已连接用户列表 + 移除流程。
ChannelPage 接入 telegram 卡片，capability 从 comingSoon → available。

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 4 — PR4 手动联调 + bugfix

**Files:** 视联调发现而定

- [ ] **Step 4.1: 准备测试 bot**

操作步骤（手动）：
1. 打开手机 Telegram，搜索 `@BotFather`
2. 发送 `/newbot`，按提示输入名称 `AIjia Test Bot`，username `aijia_local_test_bot`（确保 _bot 结尾且全局唯一）
3. 拿到 token，形如 `123456789:ABC-DEF1234ghIklZyx57W2v1u123ew11`

- [ ] **Step 4.2: 跑 dev 模式**

Run: `pnpm tauri:dev`
Expected: AIjia 桌面端启动，导航到「频道」页

- [ ] **Step 4.3: 验证 token 流程**

操作：
1. 点 Telegram 卡片「配置」
2. 输入错误 token（如 `wrongtoken`）→ 期望 toast "token 验证失败"
3. 输入正确 token → 点「下一步」→ 期望弹窗切到 step 2，QR 显示
4. 验证 ~/.renlijia/users/{scope}/channels/telegram/config.json 已生成，token 加密

- [ ] **Step 4.4: 验证扫码 + 批准**

操作：
1. 用手机 Telegram 扫桌面端 QR
2. 期望 Telegram 端跳转到与 bot 的对话，自动发出 `/start <code>`
3. bot 回复"✓ 等待 AIjia 桌面端批准…"
4. 桌面端 2s 内出现「待批准」列表，显示自己的 Telegram 名
5. 点「批准」→ 用户卡跳进「已连接用户」列表
6. Telegram 端收到 bot 发的"👋 你已连接 AIjia，可以开始对话"

- [ ] **Step 4.5: 验证私聊**

操作：
1. 在 Telegram 给 bot 发"你好，今天天气如何？"
2. 桌面端 IM 频道列表出现一条新会话
3. 等待 AI 回复（10-30s）
4. Telegram 端收到 markdown 格式的 AI 回复

- [ ] **Step 4.6: 验证边界**

操作：
1. 用未配对的 Telegram 账号给 bot 发消息 → 期望收到「请先在 AIjia 里完成扫码配对」
2. 桌面端「移除」一个已连接用户 → Telegram 端收到「你已被 AIjia 管理员移除连接」
3. 关闭 telegram switch → 状态变「已配置 / 未连接」；再开 → 重连成功
4. 二维码过期：等 5 分钟 → QR 自动刷新
5. 弹窗关闭 → 重开 → step 直接是 pairing（已配置状态）

- [ ] **Step 4.7: 修发现的 bug**

把每个 bug 当作一个独立 commit；最常见预期会出现：
- frontend allowlist 字段没暴露 → 加 ChannelConfigView 字段 或 单独 IPC
- session_id 创建路径不对 → 调 chat_turn_driver 的 IM 源标记
- 状态机不刷新 → channel:platform-state 事件订阅

- [ ] **Step 4.8: 跑所有测试 + 整体 lint**

Run: `cd src-tauri && cargo test --all && cargo clippy --all-targets --no-deps -- -D warnings && cd .. && pnpm exec vitest run && pnpm lint && pnpm tsc --noEmit`
Expected: 全部 PASS

- [ ] **Step 4.9: Commit PR4 + 创建 PR**

```bash
git add -A
git commit -m "$(cat <<'EOF'
fix(channel/telegram): PR4 端到端联调 bugfix

[逐条列出实际 bug fix]

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"

git push -u origin claude/amazing-chatelet-801fd7

gh pr create --title "feat(channel/telegram): MVP — Bot API + QR 扫码配对" --body "$(cat <<'EOF'
## Summary
- 实现 Telegram bot IM 渠道 MVP：扫码配对 + 私聊
- Bot API getUpdates 长轮询入站（零公网），sendMessage MarkdownV2 出站
- OpenClaw 同款 pairing：QR deep-link → 用户扫码 → 桌面端手动批准 → 加入 allowlist

## Test plan
- [x] cargo test --all
- [x] vitest run + tsc
- [x] 真实 BotFather bot 全流程 e2e
- [x] token 错误 / QR 过期 / 移除用户 / disable&enable / 群聊忽略 边界

Spec: docs/superpowers/specs/2026-05-19-im-telegram-connector-design.md

🤖 Generated with [Claude Code](https://claude.com/claude-code)
EOF
)"
```

---

## Self-Review

**Spec coverage check:**

| Spec 章节 | 覆盖任务 |
|---|---|
| §1 用户旅程（token 输入 / QR / 批准 / 详情） | Task 3 step 3.2 |
| §2.1 模块结构 | Task 1 + Task 2 |
| §2.2 capabilities | Task 2 step 2.1 |
| §2.3 数据 schema | Task 1 step 1.2 + Task 2 step 2.3 |
| §2.4 主链路 | Task 2 step 2.1 / 2.2 / 2.7 |
| §2.5 Pairing 协议 | Task 1 step 1.10 + Task 2 step 2.4 |
| §2.6 Tauri commands | Task 2 step 2.8 |
| §2.7 ChannelConfigView 适配 | Task 2 step 2.3 (telegram_config_view) |
| §3 错误处理边界 | Task 2 step 2.2 + Task 4 |
| §4 测试策略 | Task 1 单测 + Task 2 集成 + Task 3 vitest + Task 4 手动 |
| §5 PR 切分 | Task 1-4 1:1 对应 PR1-4 |
| §6 风险 | Task 4 联调阶段覆盖 |

**Type consistency:** `TelegramSessionTarget { chat_id, user_id }`、`PairerInfo { user_id, first_name, username, chat_id, attached_at }`、`AllowlistEntry { user_id, first_name, username, paired_at }`、`TelegramPairingBeginResult { code, deep_link, expires_in_seconds, bot_username }`、`TelegramPendingPairing { code, user_id, first_name, username, requested_at }`、`TelegramPairedUser { user_id, first_name, username }` —— 所有 task 引用一致。

**Placeholder scan:** 无 TBD/TODO。Task 2 step 2.7 末尾 `..Default::default()` 给了 fallback 引导（参考 wecom worker），不算 placeholder。Task 3 step 3.2 `(tgState as any).allowlist` 是已知占位，PR4 step 4.7 显式列入待修。
