# 钉钉 AI Card 流式回复实施计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 实现 AI 处理钉钉消息后，将回复通过 AI Card 流式投放回钉钉群/私聊，用户能实时看到 AI 打字效果。

**Architecture:** 在现有 `ChannelManager` 内新增 `DingtalkReplyManager`，它实现 `RuntimeEventSubscriber` trait 并订阅 `TauriChatCommandAdapter` 内部的 `RuntimeEventBus`；收到 `StreamDelta` 事件时调用钉钉 `PUT /v1.0/card/streaming`，收到 `StreamDone` 时调用 `finishAICard`。AI Card 在 AI 处理开始前创建并投放，`session_id → CardContext` 的映射表在内存中维护。

**Tech Stack:** Rust (reqwest, tokio), 钉钉 API v1.0 (card/instances, card/streaming, oauth2/accessToken)

---

## 文件清单

### 新建（Rust）
- `src-tauri/src/connector/channel/dingtalk_token.rs` — Access Token 获取和内存缓存（2h TTL，提前60s刷新）
- `src-tauri/src/connector/channel/dingtalk_card.rs` — AI Card API（create + deliver + stream + finish）
- `src-tauri/src/connector/channel/reply_manager.rs` — `DingtalkReplyManager`：实现 `RuntimeEventSubscriber`，维护 `session_id → CardContext` 映射

### 修改（Rust）
- `src-tauri/src/connector/channel/types.rs` — `ChannelMessage` 加 `session_webhook`、`app_key`、`app_secret` 字段
- `src-tauri/src/connector/channel/dingtalk_stream.rs` — 解析 `sessionWebhook`、`appKey`、`appSecret` 并填入 `ChannelMessage`
- `src-tauri/src/connector/channel/manager.rs` — 初始化 `DingtalkReplyManager`，AI 处理前先创建 card，订阅到 adapter 的 event bus
- `src-tauri/src/connector/channel/mod.rs` — 加 `pub mod dingtalk_token; pub mod dingtalk_card; pub mod reply_manager;`
- `src-tauri/src/transport/tauri_commands/chat.rs` — 加 `pub fn subscribe_event_listener` 方法暴露 event bus 订阅能力
- `src-tauri/src/runtime/session_runtime.rs` — 加 `pub fn subscribe_event_listener` 方法

---

## Task 1: Access Token 缓存模块

**Files:**
- Create: `src-tauri/src/connector/channel/dingtalk_token.rs`
- Modify: `src-tauri/src/connector/channel/mod.rs`

- [ ] **Step 1: 写失败测试**

在 `src-tauri/src/connector/channel/dingtalk_token.rs` 创建文件（先只包含测试）：

```rust
use std::sync::Arc;
use tokio::sync::Mutex;

const DINGTALK_API: &str = "https://api.dingtalk.com";

#[derive(Debug, Clone)]
pub struct TokenCache {
    inner: Arc<Mutex<CacheInner>>,
}

#[derive(Debug, Default)]
struct CacheInner {
    token: Option<String>,
    expires_at_ms: u64,
}

impl TokenCache {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(CacheInner::default())),
        }
    }

    /// 返回缓存的 token（如果还有 60s 以上有效期）
    pub async fn get_if_valid(&self) -> Option<String> {
        let now_ms = now_ms();
        let inner = self.inner.lock().await;
        if let Some(ref token) = inner.token {
            if inner.expires_at_ms > now_ms + 60_000 {
                return Some(token.clone());
            }
        }
        None
    }

    /// 更新缓存
    pub async fn set(&self, token: String, expires_in_secs: u64) {
        let mut inner = self.inner.lock().await;
        inner.token = Some(token);
        inner.expires_at_ms = now_ms() + expires_in_secs * 1000;
    }
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

/// 获取 Access Token，优先从缓存取；缓存过期时调用钉钉 API 刷新。
pub async fn get_access_token(
    cache: &TokenCache,
    app_key: &str,
    app_secret: &str,
) -> anyhow::Result<String> {
    if let Some(token) = cache.get_if_valid().await {
        return Ok(token);
    }

    let client = reqwest::Client::new();
    let resp = client
        .post(format!("{}/v1.0/oauth2/accessToken", DINGTALK_API))
        .json(&serde_json::json!({
            "appKey": app_key,
            "appSecret": app_secret,
        }))
        .send()
        .await
        .context("Failed to request DingTalk accessToken")?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        anyhow::bail!("DingTalk token request failed: {} {}", status, body);
    }

    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct TokenResp {
        access_token: String,
        expire_in: u64,
    }

    let data: TokenResp = resp.json().await.context("Failed to parse token response")?;
    cache.set(data.access_token.clone(), data.expire_in).await;
    Ok(data.access_token)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn cache_returns_none_when_empty() {
        let cache = TokenCache::new();
        assert!(cache.get_if_valid().await.is_none());
    }

    #[tokio::test]
    async fn cache_returns_token_when_valid() {
        let cache = TokenCache::new();
        cache.set("tok123".into(), 7200).await;
        assert_eq!(cache.get_if_valid().await.unwrap(), "tok123");
    }

    #[tokio::test]
    async fn cache_returns_none_when_near_expiry() {
        let cache = TokenCache::new();
        // 设置为 30s 后过期（< 60s 阈值）
        cache.set("tok_expiring".into(), 30).await;
        assert!(cache.get_if_valid().await.is_none());
    }
}
```

- [ ] **Step 2: 在 mod.rs 加模块声明**

在 `src-tauri/src/connector/channel/mod.rs` 开头加：
```rust
pub mod dingtalk_token;
```

- [ ] **Step 3: 运行测试确认通过**

```bash
cd /Users/oayzz/project/lotus/lotus-workbench/lotus-app/src-tauri && cargo test connector::channel::dingtalk_token::tests --lib -- --nocapture 2>&1 | tail -10
```

期望：3 个测试全部 ok

- [ ] **Step 4: 提交**

```bash
git add src-tauri/src/connector/channel/dingtalk_token.rs src-tauri/src/connector/channel/mod.rs
git commit -m "feat(channel): add DingTalk Access Token cache module"
```

---

## Task 2: AI Card API 模块

**Files:**
- Create: `src-tauri/src/connector/channel/dingtalk_card.rs`
- Modify: `src-tauri/src/connector/channel/mod.rs`

- [ ] **Step 1: 创建 dingtalk_card.rs**

```rust
//! 钉钉 AI Card API（create / deliver / stream / finish）

use anyhow::{Context, Result};
use serde_json::json;

use super::dingtalk_token::{get_access_token, TokenCache};

const DINGTALK_API: &str = "https://api.dingtalk.com";
const AI_CARD_TEMPLATE_ID: &str = "02fcf2f4-5e02-4a85-b672-46d1f715543e.schema";

/// 投放目标：群聊或私聊
#[derive(Debug, Clone)]
pub enum CardTarget {
    Group { open_conversation_id: String },
    Private { user_id: String },
}

/// 一个已创建并投放的 AI Card 实例
#[derive(Debug, Clone)]
pub struct CardInstance {
    pub card_instance_id: String,
    pub inputing_started: bool,
}

/// 创建 AI Card 并投放到目标会话。成功返回 CardInstance，失败返回 None（不中断主流程）。
pub async fn create_and_deliver_card(
    cache: &TokenCache,
    app_key: &str,
    app_secret: &str,
    robot_code: &str,
    target: &CardTarget,
) -> Option<CardInstance> {
    match try_create_and_deliver(cache, app_key, app_secret, robot_code, target).await {
        Ok(inst) => Some(inst),
        Err(e) => {
            log::warn!("[dingtalk-card] create/deliver failed: {:#}", e);
            None
        }
    }
}

async fn try_create_and_deliver(
    cache: &TokenCache,
    app_key: &str,
    app_secret: &str,
    robot_code: &str,
    target: &CardTarget,
) -> Result<CardInstance> {
    let token = get_access_token(cache, app_key, app_secret).await?;
    let client = reqwest::Client::new();

    let card_instance_id = format!(
        "card_{}_{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis(),
        &uuid::Uuid::new_v4().to_string()[..8]
    );

    // 1. 创建卡片实例
    let create_body = json!({
        "cardTemplateId": AI_CARD_TEMPLATE_ID,
        "outTrackId": card_instance_id,
        "cardData": {
            "cardParamMap": {
                "config": "{\"autoLayout\":true}"
            }
        },
        "callbackType": "STREAM",
        "imGroupOpenSpaceModel": { "supportForward": true },
        "imRobotOpenSpaceModel": { "supportForward": true }
    });

    let resp = client
        .post(format!("{}/v1.0/card/instances", DINGTALK_API))
        .header("x-acs-dingtalk-access-token", &token)
        .json(&create_body)
        .send()
        .await
        .context("Failed to create AI card")?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        anyhow::bail!("AI card create failed: {} {}", status, body);
    }

    // 2. 投放卡片
    let deliver_body = build_deliver_body(&card_instance_id, target, robot_code);

    let resp = client
        .post(format!("{}/v1.0/card/instances/deliver", DINGTALK_API))
        .header("x-acs-dingtalk-access-token", &token)
        .json(&deliver_body)
        .send()
        .await
        .context("Failed to deliver AI card")?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        anyhow::bail!("AI card deliver failed: {} {}", status, body);
    }

    log::info!("[dingtalk-card] card created and delivered: {}", card_instance_id);
    Ok(CardInstance { card_instance_id, inputing_started: false })
}

fn build_deliver_body(card_instance_id: &str, target: &CardTarget, robot_code: &str) -> serde_json::Value {
    match target {
        CardTarget::Group { open_conversation_id } => json!({
            "outTrackId": card_instance_id,
            "userIdType": 1,
            "openSpaceId": format!("dtv1.card//IM_GROUP.{}", open_conversation_id),
            "imGroupOpenDeliverModel": { "robotCode": robot_code }
        }),
        CardTarget::Private { user_id } => json!({
            "outTrackId": card_instance_id,
            "userIdType": 1,
            "openSpaceId": format!("dtv1.card//IM_ROBOT.{}", user_id),
            "imRobotOpenDeliverModel": {
                "spaceType": "IM_ROBOT",
                "robotCode": robot_code,
                "extension": { "dynamicSummary": "true" }
            }
        }),
    }
}

/// 流式更新 AI Card 内容（调用 PUT /v1.0/card/streaming）。
/// 第一次调用时先将卡片切换到 INPUTING 状态。
pub async fn stream_card(
    cache: &TokenCache,
    app_key: &str,
    app_secret: &str,
    card: &mut CardInstance,
    content: &str,
    is_finalize: bool,
) -> Result<()> {
    let token = get_access_token(cache, app_key, app_secret).await?;
    let client = reqwest::Client::new();

    // 首次调用：切换到 INPUTING 状态
    if !card.inputing_started {
        let status_body = json!({
            "outTrackId": card.card_instance_id,
            "cardData": {
                "cardParamMap": {
                    "flowStatus": "2",
                    "msgContent": content,
                    "staticMsgContent": "",
                    "sys_full_json_obj": "{\"order\":[\"msgContent\"]}",
                    "config": "{\"autoLayout\":true}"
                }
            }
        });
        let resp = client
            .put(format!("{}/v1.0/card/instances", DINGTALK_API))
            .header("x-acs-dingtalk-access-token", &token)
            .json(&status_body)
            .send()
            .await
            .context("Failed to set INPUTING status")?;
        if !resp.status().is_success() {
            log::warn!("[dingtalk-card] INPUTING PUT returned {}", resp.status());
        }
        card.inputing_started = true;
    }

    let guid = format!(
        "{}_{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis(),
        &uuid::Uuid::new_v4().to_string()[..8]
    );

    let stream_body = json!({
        "outTrackId": card.card_instance_id,
        "guid": guid,
        "key": "msgContent",
        "content": content,
        "isFull": true,
        "isFinalize": is_finalize,
        "isError": false
    });

    let resp = client
        .put(format!("{}/v1.0/card/streaming", DINGTALK_API))
        .header("x-acs-dingtalk-access-token", &token)
        .json(&stream_body)
        .send()
        .await
        .context("Failed to stream card content")?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        log::warn!("[dingtalk-card] streaming PUT {} {}", status, body);
    }
    Ok(())
}

/// 完成 AI Card（flowStatus=3，FINISHED）。
pub async fn finish_card(
    cache: &TokenCache,
    app_key: &str,
    app_secret: &str,
    card: &mut CardInstance,
    content: &str,
) -> Result<()> {
    // 先发一次 isFinalize=true 的 streaming
    stream_card(cache, app_key, app_secret, card, content, true).await?;

    let token = get_access_token(cache, app_key, app_secret).await?;
    let client = reqwest::Client::new();

    let finish_body = json!({
        "outTrackId": card.card_instance_id,
        "cardData": {
            "cardParamMap": {
                "flowStatus": "3",
                "msgContent": content,
                "staticMsgContent": "",
                "sys_full_json_obj": "{\"order\":[\"msgContent\"]}",
                "config": "{\"autoLayout\":true}"
            }
        },
        "cardUpdateOptions": { "updateCardDataByKey": true }
    });

    let resp = client
        .put(format!("{}/v1.0/card/instances", DINGTALK_API))
        .header("x-acs-dingtalk-access-token", &token)
        .json(&finish_body)
        .send()
        .await
        .context("Failed to finish AI card")?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        log::warn!("[dingtalk-card] FINISHED PUT {} {}", status, body);
    }

    log::info!("[dingtalk-card] card finished: {}", card.card_instance_id);
    Ok(())
}

/// 将卡片标记为失败（flowStatus=5）。
pub async fn fail_card(
    cache: &TokenCache,
    app_key: &str,
    app_secret: &str,
    card: &CardInstance,
) -> Result<()> {
    let token = get_access_token(cache, app_key, app_secret).await?;
    let client = reqwest::Client::new();

    let body = json!({
        "outTrackId": card.card_instance_id,
        "cardData": {
            "cardParamMap": {
                "flowStatus": "5",
                "msgContent": "处理失败，请稍后重试",
                "staticMsgContent": "",
                "sys_full_json_obj": "{\"order\":[\"msgContent\"]}",
                "config": "{\"autoLayout\":true}"
            }
        },
        "cardUpdateOptions": { "updateCardDataByKey": true }
    });

    let _ = client
        .put(format!("{}/v1.0/card/instances", DINGTALK_API))
        .header("x-acs-dingtalk-access-token", &token)
        .json(&body)
        .send()
        .await;
    Ok(())
}
```

- [ ] **Step 2: 在 mod.rs 加模块声明**

在 `src-tauri/src/connector/channel/mod.rs` 开头（`dingtalk_token` 声明之后）加：
```rust
pub mod dingtalk_card;
```

- [ ] **Step 3: 编译确认**

```bash
cd /Users/oayzz/project/lotus/lotus-workbench/lotus-app/src-tauri && cargo check 2>&1 | grep "^error" | head -10
```

期望：无 error

- [ ] **Step 4: 提交**

```bash
git add src-tauri/src/connector/channel/dingtalk_card.rs src-tauri/src/connector/channel/mod.rs
git commit -m "feat(channel): add DingTalk AI Card API module"
```

---

## Task 3: 暴露 event bus 订阅能力

**Files:**
- Modify: `src-tauri/src/runtime/session_runtime.rs`
- Modify: `src-tauri/src/transport/tauri_commands/chat.rs`

- [ ] **Step 1: 写失败测试**

在 `src-tauri/src/runtime/session_runtime.rs` 的 `#[cfg(test)]` 块末尾加：

```rust
#[test]
fn subscribe_event_listener_adds_subscriber() {
    use std::sync::Arc;
    use tokio::sync::Mutex;
    use crate::runtime::events::{RuntimeEvent, RuntimeEventKind};
    use crate::runtime::ids::{RunId, SessionId};

    struct CounterSubscriber {
        count: Arc<Mutex<usize>>,
    }

    #[async_trait::async_trait]
    impl crate::runtime::event_bus::RuntimeEventSubscriber for CounterSubscriber {
        async fn on_event(&self, _event: &RuntimeEvent) -> anyhow::Result<()> {
            *self.count.lock().await += 1;
            Ok(())
        }
    }

    let count = Arc::new(Mutex::new(0usize));
    let runtime = SessionRuntime::new(QueryEngine::new(), RuntimeEventBus::new());
    runtime.subscribe_event_listener(Arc::new(CounterSubscriber { count: count.clone() }));

    // emit an event through the bus to check the subscriber was added
    tokio::runtime::Runtime::new().unwrap().block_on(async {
        let event = RuntimeEvent {
            session_id: SessionId::new("s"),
            run_id: RunId::new("r"),
            agent_id: None,
            kind: RuntimeEventKind::StreamDone,
        };
        runtime.event_bus.emit(event).await.unwrap();
        assert_eq!(*count.lock().await, 1);
    });
}
```

- [ ] **Step 2: 运行确认失败**

```bash
cd /Users/oayzz/project/lotus/lotus-workbench/lotus-app/src-tauri && cargo test subscribe_event_listener_adds_subscriber --lib -- --nocapture 2>&1 | tail -5
```

期望：编译错误 `no method named subscribe_event_listener`

- [ ] **Step 3: 在 session_runtime.rs 加 subscribe_event_listener 方法**

在 `impl SessionRuntime {` 块内，`pub fn new` 之后加：

```rust
/// 向内部 event_bus 注册一个外部订阅者。
/// 用于让 channel 层的 DingtalkReplyManager 监听 AI 处理事件。
pub fn subscribe_event_listener(&self, subscriber: Arc<dyn crate::runtime::event_bus::RuntimeEventSubscriber>) {
    self.event_bus.subscribe(subscriber);
}
```

- [ ] **Step 4: 在 chat.rs 的 TauriChatCommandAdapter 加同名方法**

在 `pub async fn send_message` 方法之前加：

```rust
/// 向内部 runtime event bus 注册外部订阅者。
pub fn subscribe_event_listener(&self, subscriber: Arc<dyn crate::runtime::event_bus::RuntimeEventSubscriber>) {
    self.runtime.subscribe_event_listener(subscriber);
}
```

需要在 chat.rs 顶部确认已有 `use std::sync::Arc;`（应该已有）。

- [ ] **Step 5: 运行测试确认通过**

```bash
cd /Users/oayzz/project/lotus/lotus-workbench/lotus-app/src-tauri && cargo test subscribe_event_listener_adds_subscriber --lib -- --nocapture 2>&1 | tail -5
```

期望：`test subscribe_event_listener_adds_subscriber ... ok`

- [ ] **Step 6: 提交**

```bash
git add src-tauri/src/runtime/session_runtime.rs src-tauri/src/transport/tauri_commands/chat.rs
git commit -m "feat(channel): expose subscribe_event_listener on SessionRuntime and TauriChatCommandAdapter"
```

---

## Task 4: DingtalkReplyManager（RuntimeEventSubscriber）

**Files:**
- Create: `src-tauri/src/connector/channel/reply_manager.rs`
- Modify: `src-tauri/src/connector/channel/mod.rs`

- [ ] **Step 1: 创建 reply_manager.rs**

```rust
//! DingtalkReplyManager — 订阅 RuntimeEventBus，将 AI 回复流式投放到钉钉 AI Card

use std::collections::HashMap;
use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;
use tokio::sync::Mutex;

use crate::runtime::event_bus::RuntimeEventSubscriber;
use crate::runtime::events::{RuntimeEvent, RuntimeEventKind};

use super::dingtalk_card::{self, CardInstance, CardTarget};
use super::dingtalk_token::TokenCache;

/// 一个正在进行的回复上下文，关联到一个 session（即一次 run）
#[derive(Debug)]
struct ReplyContext {
    card: CardInstance,
    accumulated_text: String,
    app_key: String,
    app_secret: String,
    /// 会话对应的 run_id（用于精确匹配事件，避免串台）
    run_id: String,
}

pub struct DingtalkReplyManager {
    /// session_id → ReplyContext（每条消息开始前注册，StreamDone 后移除）
    contexts: Arc<Mutex<HashMap<String, ReplyContext>>>,
    token_cache: TokenCache,
}

impl DingtalkReplyManager {
    pub fn new() -> Self {
        Self {
            contexts: Arc::new(Mutex::new(HashMap::new())),
            token_cache: TokenCache::new(),
        }
    }

    /// 在 AI 处理开始前调用，创建 AI Card 并注册回复上下文。
    /// 如果 card 创建失败，不注册 context（消息处理继续，但钉钉不会收到回复）。
    pub async fn register(
        &self,
        session_id: String,
        run_id: String,
        app_key: String,
        app_secret: String,
        robot_code: String,
        target: CardTarget,
    ) {
        let card = dingtalk_card::create_and_deliver_card(
            &self.token_cache,
            &app_key,
            &app_secret,
            &robot_code,
            &target,
        )
        .await;

        if let Some(card) = card {
            let mut contexts = self.contexts.lock().await;
            contexts.insert(
                session_id.clone(),
                ReplyContext {
                    card,
                    accumulated_text: String::new(),
                    app_key,
                    app_secret,
                    run_id,
                },
            );
            log::info!("[reply-manager] registered context for session {}", session_id);
        } else {
            log::warn!("[reply-manager] card creation failed for session {}, no reply will be sent to DingTalk", session_id);
        }
    }
}

#[async_trait]
impl RuntimeEventSubscriber for DingtalkReplyManager {
    async fn on_event(&self, event: &RuntimeEvent) -> Result<()> {
        let session_id = event.session_id.as_str().to_string();
        let run_id = event.run_id.as_str().to_string();

        match &event.kind {
            RuntimeEventKind::StreamDelta { content } => {
                let mut contexts = self.contexts.lock().await;
                if let Some(ctx) = contexts.get_mut(&session_id) {
                    // run_id 不匹配时跳过（同一 session 的不同轮次）
                    if ctx.run_id != run_id {
                        return Ok(());
                    }
                    ctx.accumulated_text.push_str(content);
                    let text = ctx.accumulated_text.clone();
                    let app_key = ctx.app_key.clone();
                    let app_secret = ctx.app_secret.clone();
                    let cache = self.token_cache.clone();
                    let card = &mut ctx.card;
                    if let Err(e) = dingtalk_card::stream_card(
                        &cache,
                        &app_key,
                        &app_secret,
                        card,
                        &text,
                        false,
                    )
                    .await
                    {
                        log::warn!("[reply-manager] stream_card failed: {:#}", e);
                    }
                }
            }
            RuntimeEventKind::StreamDone => {
                let mut contexts = self.contexts.lock().await;
                if let Some(ctx) = contexts.get_mut(&session_id) {
                    if ctx.run_id != run_id {
                        return Ok(());
                    }
                    let text = ctx.accumulated_text.clone();
                    let app_key = ctx.app_key.clone();
                    let app_secret = ctx.app_secret.clone();
                    let cache = self.token_cache.clone();
                    let card = &mut ctx.card;
                    if let Err(e) = dingtalk_card::finish_card(
                        &cache,
                        &app_key,
                        &app_secret,
                        card,
                        &text,
                    )
                    .await
                    {
                        log::warn!("[reply-manager] finish_card failed: {:#}", e);
                    }
                    // 完成后移除 context
                    contexts.remove(&session_id);
                    log::info!("[reply-manager] finished reply for session {}", session_id);
                }
            }
            RuntimeEventKind::StreamError { error, .. } => {
                let mut contexts = self.contexts.lock().await;
                if let Some(ctx) = contexts.remove(&session_id) {
                    if ctx.run_id != run_id {
                        return Ok(());
                    }
                    let cache = self.token_cache.clone();
                    if let Err(e) = dingtalk_card::fail_card(
                        &cache,
                        &ctx.app_key,
                        &ctx.app_secret,
                        &ctx.card,
                    )
                    .await
                    {
                        log::warn!("[reply-manager] fail_card error: {:#}", e);
                    }
                    log::warn!("[reply-manager] stream error for session {}: {}", session_id, error);
                }
            }
            _ => {}
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::events::RuntimeEventKind;
    use crate::runtime::ids::{RunId, SessionId};

    fn make_event(session_id: &str, run_id: &str, kind: RuntimeEventKind) -> RuntimeEvent {
        RuntimeEvent {
            session_id: SessionId::new(session_id),
            run_id: RunId::new(run_id),
            agent_id: None,
            kind,
        }
    }

    #[tokio::test]
    async fn ignores_events_without_registered_context() {
        let mgr = DingtalkReplyManager::new();
        let event = make_event("no-such-session", "run1", RuntimeEventKind::StreamDone);
        // 没有 context 时 on_event 应该静默返回 Ok
        assert!(mgr.on_event(&event).await.is_ok());
    }

    #[tokio::test]
    async fn accumulates_delta_text() {
        let mgr = DingtalkReplyManager::new();
        // 手动插入一个假 context（不调用真实 API）
        {
            let mut ctx = mgr.contexts.lock().await;
            ctx.insert(
                "sess1".into(),
                ReplyContext {
                    card: CardInstance {
                        card_instance_id: "card1".into(),
                        inputing_started: false,
                    },
                    accumulated_text: String::new(),
                    app_key: "key".into(),
                    app_secret: "secret".into(),
                    run_id: "run1".into(),
                },
            );
        }

        // 发一个 StreamDelta，不会真正调 API（网络不通会返回 Err，我们忽略）
        let delta_event = make_event("sess1", "run1", RuntimeEventKind::StreamDelta {
            content: "hello ".into(),
        });
        let _ = mgr.on_event(&delta_event).await; // 忽略网络错误

        // 检查 accumulated_text 增加了
        let ctx = mgr.contexts.lock().await;
        assert_eq!(ctx["sess1"].accumulated_text, "hello ");
    }

    #[tokio::test]
    async fn skips_event_when_run_id_mismatch() {
        let mgr = DingtalkReplyManager::new();
        {
            let mut ctx = mgr.contexts.lock().await;
            ctx.insert(
                "sess2".into(),
                ReplyContext {
                    card: CardInstance { card_instance_id: "card2".into(), inputing_started: false },
                    accumulated_text: String::new(),
                    app_key: "key".into(),
                    app_secret: "secret".into(),
                    run_id: "run-A".into(), // 注册时的 run_id
                },
            );
        }
        // 不同 run_id 的事件，不应该更新
        let delta_event = make_event("sess2", "run-B", RuntimeEventKind::StreamDelta {
            content: "should not appear".into(),
        });
        let _ = mgr.on_event(&delta_event).await;

        let ctx = mgr.contexts.lock().await;
        assert_eq!(ctx["sess2"].accumulated_text, ""); // 没有变
    }
}
```

- [ ] **Step 2: 在 mod.rs 加模块声明**

在 `src-tauri/src/connector/channel/mod.rs` 开头加：
```rust
pub mod reply_manager;
pub use reply_manager::DingtalkReplyManager;
```

- [ ] **Step 3: 运行测试**

```bash
cd /Users/oayzz/project/lotus/lotus-workbench/lotus-app/src-tauri && cargo test connector::channel::reply_manager::tests --lib -- --nocapture 2>&1 | tail -10
```

期望：3 个测试全部 ok

- [ ] **Step 4: 提交**

```bash
git add src-tauri/src/connector/channel/reply_manager.rs src-tauri/src/connector/channel/mod.rs
git commit -m "feat(channel): add DingtalkReplyManager as RuntimeEventSubscriber"
```

---

## Task 5: ChannelMessage 加回复字段 + stream 解析

**Files:**
- Modify: `src-tauri/src/connector/channel/types.rs`
- Modify: `src-tauri/src/connector/channel/dingtalk_stream.rs`

- [ ] **Step 1: 修改 types.rs**

在 `ChannelMessage` struct 末尾加三个字段（`reply_group_id` 之后）：

```rust
    /// 机器人 AppKey（用于 Token 刷新）
    pub app_key: String,
    /// 机器人 AppSecret（明文，已从配置解密，仅在内存中流转）
    pub app_secret: String,
```

（`robot_code` 已存在，`reply_group_id` 已存在，只需加这两个字段）

- [ ] **Step 2: 修改 dingtalk_stream.rs 解析 sessionWebhook**

在 `DingtalkImData` struct 中加字段（在 `msg_id` 之后）：

```rust
    #[serde(rename = "sessionWebhook")]
    session_webhook: Option<String>,
```

在 `parse_im_message` 方法中，`Some(ChannelMessage { ... })` 的构造里加两个字段。

先读文件确认当前 `ChannelMessage` 构造的位置（parse_im_message 末尾），然后在 `reply_group_id` 之后加：

```rust
            app_key: self.app_key.clone(),
            app_secret: self.app_secret.clone(),
```

`DingtalkStreamClient` 已经有 `app_key` 和 `app_secret` 字段，直接用 `self.app_key` 和 `self.app_secret`。

- [ ] **Step 3: 编译确认**

```bash
cd /Users/oayzz/project/lotus/lotus-workbench/lotus-app/src-tauri && cargo check 2>&1 | grep "^error" | head -20
```

修复所有编译错误（主要是 `ChannelMessage` 构造缺字段的位置，搜索所有 `ChannelMessage {` 的地方补上 `app_key: String::new(), app_secret: String::new()` 即可）。

- [ ] **Step 4: 运行 dingtalk_stream 测试确认仍通过**

```bash
cd /Users/oayzz/project/lotus/lotus-workbench/lotus-app/src-tauri && cargo test connector::channel::dingtalk_stream::tests --lib -- --nocapture 2>&1 | tail -10
```

期望：4 个测试全部 ok（需要在测试的 `make_client` 或 `parse_*` 测试用例里补 `app_key`/`app_secret` 字段）

- [ ] **Step 5: 提交**

```bash
git add src-tauri/src/connector/channel/types.rs src-tauri/src/connector/channel/dingtalk_stream.rs
git commit -m "feat(channel): add app_key/app_secret to ChannelMessage for reply auth"
```

---

## Task 6: ChannelManager 集成回复链路

**Files:**
- Modify: `src-tauri/src/connector/channel/manager.rs`
- Modify: `src-tauri/src/lib.rs`

- [ ] **Step 1: 读 manager.rs 当前内容**

先读文件，确认字段和消息 loop 的具体位置。

- [ ] **Step 2: 修改 manager.rs**

**A. 加字段**：在 `ChannelManager` struct 里加：
```rust
    reply_manager: Arc<DingtalkReplyManager>,
```

需要 import：
```rust
use super::reply_manager::DingtalkReplyManager;
use super::dingtalk_card::CardTarget;
```

**B. 修改 `new()`**：参数和构造不变，在 `Self { ... }` 里加：
```rust
            reply_manager: Arc::new(DingtalkReplyManager::new()),
```

**C. 修改 `connect_dingtalk()`**：

在 `stream_client.start(update_status);` 之后，加订阅：
```rust
        // 把 reply_manager 订阅到 chat_adapter 的 event bus（只订阅一次）
        let reply_arc = Arc::clone(&self.reply_manager) as Arc<dyn crate::runtime::event_bus::RuntimeEventSubscriber>;
        self.chat_adapter.subscribe_event_listener(reply_arc);
```

**D. 修改消息 loop**：

在 `let session_id = match router.get_or_create_session(...)` 成功后（拿到 session_id 后），在 `let content = ...` 之前加：

```rust
                // 创建 AI Card 并注册回复上下文（在 AI 处理开始前）
                let card_target = match &conv_type {
                    ConversationType::Group => CardTarget::Group {
                        open_conversation_id: conv_key.clone(),
                    },
                    ConversationType::Private => CardTarget::Private {
                        user_id: msg.sender_id.clone(),
                    },
                };
                reply_manager_ref.register(
                    session_id.clone(),
                    // run_id 在 ChatTurnRequest 内部生成，我们用 session_id 作为关联键
                    // （reply_manager 的 run_id 匹配会在 Task 后续处理，此处先用空串占位）
                    String::new(),
                    msg.app_key.clone(),
                    msg.app_secret.clone(),
                    msg.robot_code.clone(),
                    card_target,
                ).await;
```

**等等**——run_id 的问题：`ChatTurnRequest` 会在 `new()` 里生成一个新的 run_id，而 `DingtalkReplyManager` 需要 run_id 来匹配事件。解决方案：**创建 `ChatTurnRequest` 先，把 run_id 提前拿到，再注册 reply context**：

将消息 loop 里 `let request = ChatTurnRequest::new(session_id.clone(), content, vec![]);` 移到 card 注册之前：

```rust
                let content = match &conv_type {
                    ConversationType::Group => format!("[{}]: {}", sender_nick, text),
                    ConversationType::Private => text.clone(),
                };

                let request = ChatTurnRequest::new(session_id.clone(), content, vec![]);
                let run_id = request.run_id.as_str().to_string();

                // 创建 AI Card 并注册回复上下文
                let card_target = match &conv_type {
                    ConversationType::Group => CardTarget::Group {
                        open_conversation_id: conv_key.clone(),
                    },
                    ConversationType::Private => CardTarget::Private {
                        user_id: msg.sender_id.clone(),
                    },
                };
                reply_manager_ref.register(
                    session_id.clone(),
                    run_id,
                    msg.app_key.clone(),
                    msg.app_secret.clone(),
                    msg.robot_code.clone(),
                    card_target,
                ).await;

                if let Err(e) = adapter.send_message(
                    session_id.clone(),
                    request.content.clone(),
                    vec![],
                    None,
                    None,
                    None,
                ).await {
                    log::error!("[channel] send_message failed: {}", e);
                }
```

需要 import `ChatTurnRequest`：
```rust
use crate::runtime::ChatTurnRequest;
```

把 `reply_manager_ref` 在 spawn 前克隆进去：
```rust
        let reply_manager_ref = Arc::clone(&self.reply_manager);
```

- [ ] **Step 3: 修改 lib.rs**

`ChannelManager::new` 的签名加了 `reply_manager` 字段但不需要外部传入，new() 内部创建，所以 **lib.rs 不需要修改**（`ChannelManager::new` 的参数签名不变）。

确认 lib.rs 里的 `ChannelManager::new(...)` 调用无需变动：
```bash
grep -n "ChannelManager::new" /Users/oayzz/project/lotus/lotus-workbench/lotus-app/src-tauri/src/lib.rs
```

- [ ] **Step 4: 编译确认**

```bash
cd /Users/oayzz/project/lotus/lotus-workbench/lotus-app/src-tauri && cargo check 2>&1 | grep "^error" | head -20
```

逐个修复（import 路径、字段名等）。

- [ ] **Step 5: 提交**

```bash
git add src-tauri/src/connector/channel/manager.rs
git commit -m "feat(channel): wire DingtalkReplyManager into message processing loop"
```

---

## Task 7: 修复其余 review 问题

**Files:**
- Modify: `src-tauri/src/commands/channel.rs`
- Modify: `src-tauri/src/connector/channel/manager.rs`

### 7A: 修复未登录时 command panic（channel commands 安全处理缺失 state）

- [ ] **Step 1: 修改 commands/channel.rs**

把所有三个 command 改为安全处理缺失的 managed state。读文件确认当前内容，然后：

```rust
use std::sync::Arc;
use tauri::{AppHandle, Manager, State};

use crate::connector::channel::{ChannelConversation, ChannelManager, ChannelStatus};

#[tauri::command]
pub async fn channel_save_config(
    app: AppHandle,
    app_key: String,
    app_secret: String,
    robot_code: String,
) -> Result<(), String> {
    let manager = app
        .try_state::<Arc<ChannelManager>>()
        .ok_or_else(|| "频道功能未初始化，请先登录".to_string())?;
    manager
        .save_config_and_connect(app_key, app_secret, robot_code)
        .await
        .map_err(|e| format!("{:#}", e))
}

#[tauri::command]
pub async fn channel_get_status(app: AppHandle) -> Result<ChannelStatus, String> {
    match app.try_state::<Arc<ChannelManager>>() {
        Some(m) => Ok(m.get_status().await),
        None => Ok(ChannelStatus::Unconfigured),
    }
}

#[tauri::command]
pub async fn channel_get_conversations(app: AppHandle) -> Result<Vec<ChannelConversation>, String> {
    match app.try_state::<Arc<ChannelManager>>() {
        Some(m) => Ok(m.get_conversations().await),
        None => Ok(vec![]),
    }
}
```

### 7B: 限制 seen_msg_ids 上限

- [ ] **Step 2: 修改 manager.rs 的消息 loop**

在幂等去重的代码块里加上限检查（在 `ids.insert(...)` 之后）：

```rust
                {
                    let mut ids = seen_ids.write().await;
                    if !msg.msg_id.is_empty() && !ids.insert(msg.msg_id.clone()) {
                        continue;
                    }
                    // 防止无限增长：超过 5000 条时清空（丢掉旧去重记录）
                    if ids.len() > 5000 {
                        ids.clear();
                        log::debug!("[channel] seen_msg_ids cleared (exceeded 5000)");
                    }
                }
```

### 7C: 修复 group display_name

- [ ] **Step 3: 修改 manager.rs 的 conversations 更新逻辑**

把群聊的 `display_name` 改为用 conv_key 前缀，不用 sender_nick：

```rust
                {
                    let mut convs_lock = convs.write().await;
                    if !convs_lock.iter().any(|c| c.session_id == session_id) {
                        let display_name = match &conv_type {
                            ConversationType::Group => format!("钉钉群 {}", &conv_key[..conv_key.len().min(8)]),
                            ConversationType::Private => sender_nick.clone(),
                        };
                        convs_lock.push(ChannelConversation {
                            session_id: session_id.clone(),
                            platform: Platform::Dingtalk,
                            conversation_type: conv_type.clone(),
                            external_id: conv_key.clone(),
                            display_name,
                            unread_count: 0,
                        });
                    }
                }
```

- [ ] **Step 4: 编译确认**

```bash
cd /Users/oayzz/project/lotus/lotus-workbench/lotus-app/src-tauri && cargo check 2>&1 | grep "^error" | head -10
```

- [ ] **Step 5: 提交**

```bash
git add src-tauri/src/commands/channel.rs src-tauri/src/connector/channel/manager.rs
git commit -m "fix(channel): safe channel commands, cap seen_msg_ids, fix group display_name"
```

---

## Task 8: 最终验证

- [ ] **Step 1: 完整编译**

```bash
cd /Users/oayzz/project/lotus/lotus-workbench/lotus-app/src-tauri && cargo build 2>&1 | grep "^error" | head -10
```

期望：无 error

- [ ] **Step 2: 运行所有 channel 测试**

```bash
cd /Users/oayzz/project/lotus/lotus-workbench/lotus-app/src-tauri && cargo test connector::channel --lib -- --nocapture 2>&1 | tail -20
```

期望：所有测试通过

- [ ] **Step 3: 运行架构约束回归测试**

```bash
cd /Users/oayzz/project/lotus/lotus-workbench/lotus-app/src-tauri && cargo test review_ --tests --no-fail-fast 2>&1 | grep -E "FAILED|ok\." | tail -20
```

期望：除 `review_send_message_clears_gateway_busy_after_runtime_returns`（既有问题）外全部通过

- [ ] **Step 4: 前端编译**

```bash
cd /Users/oayzz/project/lotus/lotus-workbench/lotus-app && pnpm build 2>&1 | grep "error TS" | head -10
```

期望：无 TS error

- [ ] **Step 5: 最终提交（如有未提交改动）**

```bash
git add -A
git commit -m "feat(channel): complete DingTalk AI Card streaming reply"
```
