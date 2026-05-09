# IM 频道（钉钉 Stream）实施计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 实现钉钉机器人 Stream 模式接入，用户在钉钉群/私聊 @机器人后，AIjia 本地 AI 处理并回复，同时 AIjia App 内有频道面板展示所有会话。

**Architecture:** Rust 层新增 `connector/channel/` 模块（`ChannelManager` 管理 WebSocket 长连接 + `ChannelSessionRouter` 路由群/私聊到不同 SessionId），通过现有 `SessionRuntime::run_chat_request()` 处理 AI 对话，回复走现有 `dws chat message send-by-bot`。前端新增 `features/channel/` 页面，路由/存储遵循现有模式。

**Tech Stack:** Rust (tokio + reqwest WebSocket upgrade)、React/TypeScript、Zustand、现有 `UserScopedPaths` + `SecureStorage` + `SessionRuntime`

---

## 文件清单

### 新建（Rust）
- `src-tauri/src/connector/channel/mod.rs` — 模块入口，re-export
- `src-tauri/src/connector/channel/types.rs` — `ChannelConfig`、`ChannelStatus`、`ChannelMessage`、`ChannelConversation` 等核心类型
- `src-tauri/src/connector/channel/router.rs` — `ChannelSessionRouter`：群/私聊 → SessionId 映射，持久化到 `channels/dingtalk_sessions.json`
- `src-tauri/src/connector/channel/dingtalk_stream.rs` — `DingtalkStreamClient`：钉钉 Stream WebSocket 长连接、断线重连、消息解析
- `src-tauri/src/connector/channel/manager.rs` — `ChannelManager`：生命周期管理、状态机、向 `SessionRuntime` 注入消息
- `src-tauri/src/commands/channel.rs` — Tauri IPC 命令：`channel_save_config`、`channel_connect`、`channel_disconnect`、`channel_get_status`、`channel_get_conversations`

### 修改（Rust）
- `src-tauri/src/connector/mod.rs` — 加 `pub mod channel;`
- `src-tauri/src/storage/user_scoped_paths.rs` — 加 `channels_dir()`、`channel_config_path()`、`channel_sessions_path()`
- `src-tauri/src/storage/aijia_home.rs` — `ensure_user_dirs()` 中加 `channels/` 目录创建
- `src-tauri/src/commands/mod.rs` — 加 `pub mod channel;`
- `src-tauri/src/lib.rs` — 注册 `ChannelManager` managed state + 注册 channel commands 到 invoke_handler

### 新建（前端）
- `src/features/channel/ChannelPage.tsx` — 频道面板主页：左侧平台/会话列表 + 右侧聊天视图
- `src/features/channel/ChannelConfig.tsx` — 钉钉配置表单（AppKey/AppSecret/RobotCode）
- `src/stores/channelStore.ts` — Zustand store：连接状态、频道会话列表、未读数

### 修改（前端）
- `src/lib/tauri.ts` — 加 `CHANNEL_EVENTS` 常量 + `channel_*` IPC 封装函数
- `src/stores/uiStore.ts` — Route 类型加 `{ kind: 'channel' }`
- `src/components/sidebar/SidebarNav.tsx` — 加「频道」导航项
- `src/App.tsx` — RouteSwitch 加 `channel` case

---

## Task 1: 存储路径扩展

**Files:**
- Modify: `src-tauri/src/storage/user_scoped_paths.rs`
- Modify: `src-tauri/src/storage/aijia_home.rs`

- [ ] **Step 1: 写失败测试**

在 `user_scoped_paths.rs` 的 `#[cfg(test)]` 块末尾加：

```rust
#[test]
fn channels_paths_under_user_scope() {
    let root = PathBuf::from("/tmp/test-renlijia");
    let paths = UserScopedPaths::new(&root, "t_1__u_2");
    let base = root.join("users/t_1__u_2");

    assert_eq!(paths.channels_dir(), base.join("channels"));
    assert_eq!(
        paths.channel_config_path("dingtalk"),
        base.join("channels/dingtalk_config.json")
    );
    assert_eq!(
        paths.channel_sessions_path("dingtalk"),
        base.join("channels/dingtalk_sessions.json")
    );
}
```

- [ ] **Step 2: 运行测试确认失败**

```bash
cd src-tauri && cargo test channels_paths_under_user_scope -- --nocapture
```

期望：`error[E0599]: no method named 'channels_dir'`

- [ ] **Step 3: 在 `user_scoped_paths.rs` 中 `downloads_dir()` 方法后加三个新方法**

```rust
pub fn channels_dir(&self) -> PathBuf {
    self.base.join("channels")
}
pub fn channel_config_path(&self, platform: &str) -> PathBuf {
    self.base.join("channels").join(format!("{}_config.json", platform))
}
pub fn channel_sessions_path(&self, platform: &str) -> PathBuf {
    self.base.join("channels").join(format!("{}_sessions.json", platform))
}
```

- [ ] **Step 4: 在 `aijia_home.rs` 的 `ensure_user_dirs()` 方法中加 channels 目录创建**

找到 `pub fn ensure_user_dirs(&self, scope: &UserScope) -> std::io::Result<()>` 方法，在现有 `create_dir_all` 调用之后加一行：

```rust
std::fs::create_dir_all(user_dir.join("channels"))?;
```

- [ ] **Step 5: 运行测试确认通过**

```bash
cd src-tauri && cargo test channels_paths_under_user_scope -- --nocapture
```

期望：`test channels_paths_under_user_scope ... ok`

- [ ] **Step 6: 提交**

```bash
git add src-tauri/src/storage/user_scoped_paths.rs src-tauri/src/storage/aijia_home.rs
git commit -m "feat(channel): add user-scoped channels dir to UserScopedPaths"
```

---

## Task 2: Channel 核心类型

**Files:**
- Create: `src-tauri/src/connector/channel/mod.rs`
- Create: `src-tauri/src/connector/channel/types.rs`
- Modify: `src-tauri/src/connector/mod.rs`

- [ ] **Step 1: 创建模块目录和 mod.rs**

```bash
mkdir -p src-tauri/src/connector/channel
```

`src-tauri/src/connector/channel/mod.rs` 内容：

```rust
pub mod types;
pub mod router;
pub mod dingtalk_stream;
pub mod manager;

pub use manager::ChannelManager;
pub use types::{ChannelConfig, ChannelStatus, ChannelConversation};
```

- [ ] **Step 2: 创建 types.rs**

`src-tauri/src/connector/channel/types.rs` 完整内容：

```rust
use serde::{Deserialize, Serialize};

/// 平台标识
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Platform {
    Dingtalk,
}

impl Platform {
    pub fn as_str(&self) -> &str {
        match self {
            Platform::Dingtalk => "dingtalk",
        }
    }
}

/// 钉钉频道配置（存入 dingtalk_config.json，AppSecret 加密后存储）
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DingtalkChannelConfig {
    pub app_key: String,
    /// AES-256-GCM 加密后的 AppSecret（格式：nonce_hex:ciphertext_hex）
    pub app_secret_encrypted: String,
    pub robot_code: String,
}

/// 频道连接状态
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", tag = "state")]
pub enum ChannelStatus {
    /// 未配置（无 AppKey 等信息）
    Unconfigured,
    /// 已配置但未连接
    Disconnected,
    /// 正在连接中
    Connecting,
    /// 已连接
    Connected,
    /// 重连等待中，delay_secs 为下次重连剩余秒数
    Reconnecting { delay_secs: u64 },
    /// 配置有误（如 401）
    ConfigError { message: String },
}

/// 频道会话（群聊或私聊）
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChannelConversation {
    /// 内部 session id（对应 conversations/ 目录下的对话）
    pub session_id: String,
    pub platform: Platform,
    pub conversation_type: ConversationType,
    /// 群聊：openConversationId；私聊：sender userId
    pub external_id: String,
    /// 显示名称（群名或用户昵称）
    pub display_name: String,
    pub unread_count: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ConversationType {
    Group,
    Private,
}

/// 从钉钉 Stream 解析出的一条消息
#[derive(Debug, Clone)]
pub struct ChannelMessage {
    pub msg_id: String,
    pub conversation_type: ConversationType,
    /// 群聊：openConversationId；私聊：sender userId
    pub conversation_key: String,
    pub sender_id: String,
    pub sender_nick: String,
    pub text: String,
    /// 回复时需要的 robot_code
    pub robot_code: String,
    /// 回复时需要的群 id（私聊时也设为 sender_id，send-by-bot 用 open_conversation_id）
    pub reply_group_id: String,
}

/// channel:status 事件 payload（推给前端）
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChannelStatusPayload {
    pub platform: String,
    pub status: ChannelStatus,
}

/// channel:message 事件 payload（新消息时推给前端，用于更新未读数）
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChannelMessagePayload {
    pub platform: String,
    pub session_id: String,
    pub sender_nick: String,
    pub text_preview: String,
}
```

- [ ] **Step 3: 在 connector/mod.rs 加 channel 模块**

在 `src-tauri/src/connector/mod.rs` 中加一行：

```rust
pub mod channel;
```

- [ ] **Step 4: 编译确认无误**

```bash
cd src-tauri && cargo check 2>&1 | grep -E "^error"
```

期望：无 error 输出

- [ ] **Step 5: 提交**

```bash
git add src-tauri/src/connector/channel/ src-tauri/src/connector/mod.rs
git commit -m "feat(channel): add channel types and module skeleton"
```

---

## Task 3: ChannelSessionRouter

**Files:**
- Create: `src-tauri/src/connector/channel/router.rs`

- [ ] **Step 1: 写失败测试**

创建 `src-tauri/src/connector/channel/router.rs`（先只包含测试）：

```rust
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use anyhow::Result;
use serde::{Deserialize, Serialize};

use super::types::ConversationType;

/// 持久化的映射表
#[derive(Debug, Default, Serialize, Deserialize)]
struct SessionsState {
    /// key: "group:{openConversationId}" 或 "private:{userId}"
    sessions: HashMap<String, String>,
}

pub struct ChannelSessionRouter {
    sessions_path: PathBuf,
    state: SessionsState,
}

impl ChannelSessionRouter {
    /// 从磁盘加载映射表，文件不存在时返回空状态
    pub fn load(sessions_path: &Path) -> Result<Self> {
        let state = if sessions_path.exists() {
            let content = std::fs::read_to_string(sessions_path)?;
            serde_json::from_str(&content).unwrap_or_default()
        } else {
            SessionsState::default()
        };
        Ok(Self {
            sessions_path: sessions_path.to_path_buf(),
            state,
        })
    }

    /// 查询或新建 session_id。新建时持久化到磁盘。
    pub fn get_or_create_session(
        &mut self,
        conversation_type: &ConversationType,
        external_id: &str,
        create_session: impl FnOnce() -> Result<String>,
    ) -> Result<String> {
        let key = Self::make_key(conversation_type, external_id);
        if let Some(session_id) = self.state.sessions.get(&key) {
            return Ok(session_id.clone());
        }
        let session_id = create_session()?;
        self.state.sessions.insert(key, session_id.clone());
        self.persist()?;
        Ok(session_id)
    }

    fn make_key(conversation_type: &ConversationType, external_id: &str) -> String {
        match conversation_type {
            ConversationType::Group => format!("group:{}", external_id),
            ConversationType::Private => format!("private:{}", external_id),
        }
    }

    fn persist(&self) -> Result<()> {
        if let Some(parent) = self.sessions_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let content = serde_json::to_string_pretty(&self.state)?;
        std::fs::write(&self.sessions_path, content)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn creates_new_session_for_group() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("channels/dingtalk_sessions.json");
        let mut router = ChannelSessionRouter::load(&path).unwrap();

        let session_id = router.get_or_create_session(
            &ConversationType::Group,
            "cid123",
            || Ok("sess-abc".to_string()),
        ).unwrap();

        assert_eq!(session_id, "sess-abc");
    }

    #[test]
    fn returns_existing_session_for_same_key() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("channels/dingtalk_sessions.json");
        let mut router = ChannelSessionRouter::load(&path).unwrap();

        router.get_or_create_session(
            &ConversationType::Group,
            "cid123",
            || Ok("sess-abc".to_string()),
        ).unwrap();

        // Second call should return same session, not call the closure
        let mut called = false;
        let session_id = router.get_or_create_session(
            &ConversationType::Group,
            "cid123",
            || { called = true; Ok("sess-xyz".to_string()) },
        ).unwrap();

        assert_eq!(session_id, "sess-abc");
        assert!(!called, "closure should not be called for existing session");
    }

    #[test]
    fn group_and_private_use_different_keys() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("channels/dingtalk_sessions.json");
        let mut router = ChannelSessionRouter::load(&path).unwrap();

        let group_sess = router.get_or_create_session(
            &ConversationType::Group,
            "id123",
            || Ok("sess-group".to_string()),
        ).unwrap();

        let private_sess = router.get_or_create_session(
            &ConversationType::Private,
            "id123",
            || Ok("sess-private".to_string()),
        ).unwrap();

        assert_ne!(group_sess, private_sess);
    }

    #[test]
    fn persists_and_reloads() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("channels/dingtalk_sessions.json");

        {
            let mut router = ChannelSessionRouter::load(&path).unwrap();
            router.get_or_create_session(
                &ConversationType::Private,
                "user42",
                || Ok("sess-persisted".to_string()),
            ).unwrap();
        }

        // Reload from disk
        let mut router2 = ChannelSessionRouter::load(&path).unwrap();
        let mut called = false;
        let session_id = router2.get_or_create_session(
            &ConversationType::Private,
            "user42",
            || { called = true; Ok("sess-new".to_string()) },
        ).unwrap();

        assert_eq!(session_id, "sess-persisted");
        assert!(!called, "should have loaded from disk, not created new");
    }
}
```

- [ ] **Step 2: 运行测试确认通过**

```bash
cd src-tauri && cargo test connector::channel::router::tests -- --nocapture
```

期望：4 个测试全部 `ok`

- [ ] **Step 3: 提交**

```bash
git add src-tauri/src/connector/channel/router.rs
git commit -m "feat(channel): add ChannelSessionRouter with persistence"
```

---

## Task 4: DingtalkStreamClient

**Files:**
- Create: `src-tauri/src/connector/channel/dingtalk_stream.rs`

钉钉 Stream 协议说明：
1. 调用 `POST https://api.dingtalk.com/v1.0/gateway/connections/open`，传 AppKey+AppSecret，获得 `endpoint` 和 `ticket`
2. 连接 `wss://{endpoint}?ticket={ticket}` 建立 WebSocket
3. 钉钉定期发 ping（type=SYSTEM, headers.topic=ping），需回复 pong
4. 消息事件的 JSON 结构：`{ type: "EVENT", headers: { topic: "dingTalk_IM_ROBOT", messageId }, data: "{...}" }`
5. data 字段是 JSON 字符串，内部有 `msgtype`、`text.content`、`senderNick`、`senderUserId`、`conversationId`、`conversationType`（1=单聊, 2=群聊）、`robotCode`

- [ ] **Step 1: 创建 dingtalk_stream.rs**

```rust
//! 钉钉 Stream 模式 WebSocket 客户端
//!
//! 协议参考：https://open.dingtalk.com/document/development/develop-stream-mode-push-server

use anyhow::{Context, Result};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{mpsc, Mutex};
use tokio::time::sleep;
use tokio_tungstenite::tungstenite::Message;
use futures_util::{SinkExt, StreamExt};

use super::types::{ChannelMessage, ConversationType};

const STREAM_OPEN_URL: &str = "https://api.dingtalk.com/v1.0/gateway/connections/open";
const MAX_RETRY_DELAY_SECS: u64 = 60;

#[derive(Deserialize)]
struct StreamOpenResponse {
    endpoint: String,
    ticket: String,
}

#[derive(Deserialize)]
struct StreamFrame {
    #[serde(rename = "type")]
    frame_type: String,
    headers: StreamHeaders,
    data: Option<String>,
}

#[derive(Deserialize)]
struct StreamHeaders {
    #[serde(rename = "messageId")]
    message_id: Option<String>,
    topic: Option<String>,
}

#[derive(Deserialize)]
struct DingtalkImData {
    #[serde(rename = "msgtype")]
    msg_type: Option<String>,
    text: Option<TextContent>,
    #[serde(rename = "senderNick")]
    sender_nick: Option<String>,
    #[serde(rename = "senderUserId")]
    sender_user_id: Option<String>,
    #[serde(rename = "conversationId")]
    conversation_id: Option<String>,
    #[serde(rename = "conversationType")]
    conversation_type: Option<String>,
    #[serde(rename = "robotCode")]
    robot_code: Option<String>,
    #[serde(rename = "msgId")]
    msg_id: Option<String>,
}

#[derive(Deserialize)]
struct TextContent {
    content: String,
}

#[derive(Clone)]
pub struct DingtalkStreamClient {
    app_key: String,
    app_secret: String,
    robot_code: String,
    message_tx: mpsc::Sender<ChannelMessage>,
    shutdown_tx: Arc<Mutex<Option<tokio::sync::oneshot::Sender<()>>>>,
}

impl DingtalkStreamClient {
    pub fn new(
        app_key: String,
        app_secret: String,
        robot_code: String,
        message_tx: mpsc::Sender<ChannelMessage>,
    ) -> Self {
        Self {
            app_key,
            app_secret,
            robot_code,
            message_tx,
            shutdown_tx: Arc::new(Mutex::new(None)),
        }
    }

    /// 启动 Stream 连接（后台 task）。断线后指数退避重连。
    /// 返回 status 事件接收端（Connected/Reconnecting/ConfigError）。
    pub fn start(
        &self,
        on_status: impl Fn(super::types::ChannelStatus) + Send + Sync + 'static,
    ) {
        let client = self.clone();
        let on_status = Arc::new(on_status);
        tokio::spawn(async move {
            client.run_with_retry(on_status).await;
        });
    }

    async fn run_with_retry(
        &self,
        on_status: Arc<impl Fn(super::types::ChannelStatus) + Send + Sync>,
    ) {
        let mut delay_secs: u64 = 1;
        loop {
            on_status(super::types::ChannelStatus::Connecting);

            match self.open_stream_connection().await {
                Ok((endpoint, ticket)) => {
                    delay_secs = 1; // reset on success
                    on_status(super::types::ChannelStatus::Connected);
                    log::info!("[dingtalk-stream] connected, endpoint={}", endpoint);

                    if let Err(e) = self.run_ws_loop(&endpoint, &ticket).await {
                        log::warn!("[dingtalk-stream] ws loop ended: {:#}", e);
                    }
                }
                Err(e) => {
                    let msg = e.to_string();
                    log::warn!("[dingtalk-stream] open failed: {:#}", e);
                    if msg.contains("401") || msg.contains("Unauthorized") {
                        on_status(super::types::ChannelStatus::ConfigError {
                            message: "AppKey 或 AppSecret 有误，请检查配置".into(),
                        });
                        return; // 不重连
                    }
                }
            }

            on_status(super::types::ChannelStatus::Reconnecting { delay_secs });
            sleep(Duration::from_secs(delay_secs)).await;
            delay_secs = (delay_secs * 2).min(MAX_RETRY_DELAY_SECS);
        }
    }

    async fn open_stream_connection(&self) -> Result<(String, String)> {
        let client = Client::new();
        let resp = client
            .post(STREAM_OPEN_URL)
            .header("Content-Type", "application/json")
            .json(&serde_json::json!({
                "clientId": self.app_key,
                "clientSecret": self.app_secret,
                "subscriptions": [
                    { "type": "EVENT", "topic": "dingTalk_IM_ROBOT" }
                ],
                "ua": "aijia/1.0",
                "localIp": "127.0.0.1"
            }))
            .send()
            .await
            .context("Failed to POST stream open")?;

        if resp.status() == reqwest::StatusCode::UNAUTHORIZED {
            anyhow::bail!("401 Unauthorized: invalid AppKey or AppSecret");
        }
        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!("Stream open failed: {} {}", status, body);
        }

        let data: StreamOpenResponse = resp.json().await.context("Failed to parse stream open response")?;
        Ok((data.endpoint, data.ticket))
    }

    async fn run_ws_loop(&self, endpoint: &str, ticket: &str) -> Result<()> {
        let url = format!("wss://{}?ticket={}", endpoint, ticket);
        let (ws_stream, _) = tokio_tungstenite::connect_async(&url)
            .await
            .context("WebSocket connect failed")?;

        let (mut write, mut read) = ws_stream.split();

        while let Some(msg) = read.next().await {
            let msg = msg.context("WebSocket read error")?;
            if let Message::Text(text) = msg {
                if let Ok(frame) = serde_json::from_str::<StreamFrame>(&text) {
                    // ping → pong
                    if frame.frame_type == "SYSTEM" {
                        if frame.headers.topic.as_deref() == Some("ping") {
                            let pong = serde_json::json!({
                                "code": 200,
                                "headers": { "messageId": frame.headers.message_id },
                                "message": "pong",
                                "data": ""
                            });
                            write.send(Message::Text(pong.to_string().into())).await.ok();
                        }
                        continue;
                    }

                    // IM 消息事件
                    if frame.frame_type == "EVENT" {
                        if let Some(data_str) = &frame.data {
                            if let Some(msg) = self.parse_im_message(data_str) {
                                // ack
                                let ack = serde_json::json!({
                                    "code": 200,
                                    "headers": { "messageId": frame.headers.message_id },
                                    "message": "OK",
                                    "data": ""
                                });
                                write.send(Message::Text(ack.to_string().into())).await.ok();
                                let _ = self.message_tx.send(msg).await;
                            }
                        }
                    }
                }
            } else if let Message::Close(_) = msg {
                anyhow::bail!("WebSocket closed by server");
            }
        }
        anyhow::bail!("WebSocket stream ended");
    }

    fn parse_im_message(&self, data_str: &str) -> Option<ChannelMessage> {
        let im: DingtalkImData = serde_json::from_str(data_str).ok()?;

        // 只处理 text 类型消息
        if im.msg_type.as_deref() != Some("text") {
            return None;
        }

        let text = im.text?.content;
        let sender_id = im.sender_user_id?;
        let sender_nick = im.sender_nick.unwrap_or_else(|| sender_id.clone());
        let msg_id = im.msg_id.unwrap_or_default();
        let robot_code = im.robot_code.unwrap_or_else(|| self.robot_code.clone());

        let (conversation_type, conversation_key, reply_group_id) =
            if im.conversation_type.as_deref() == Some("2") {
                // 群聊
                let conv_id = im.conversation_id?;
                (ConversationType::Group, conv_id.clone(), conv_id)
            } else {
                // 私聊
                (ConversationType::Private, sender_id.clone(), sender_id.clone())
            };

        Some(ChannelMessage {
            msg_id,
            conversation_type,
            conversation_key,
            sender_id,
            sender_nick,
            text,
            robot_code,
            reply_group_id,
        })
    }
}
```

- [ ] **Step 2: 在 Cargo.toml 加依赖**

检查当前依赖，tokio-tungstenite 和 futures-util 需要加入：

```bash
grep -n "tokio-tungstenite\|futures-util" src-tauri/Cargo.toml
```

若不存在，在 `[dependencies]` 部分加：

```toml
tokio-tungstenite = { version = "0.26", features = ["native-tls"] }
futures-util = "0.3"
```

- [ ] **Step 3: 编译确认无误**

```bash
cd src-tauri && cargo check 2>&1 | grep -E "^error"
```

期望：无 error

- [ ] **Step 4: 提交**

```bash
git add src-tauri/src/connector/channel/dingtalk_stream.rs src-tauri/Cargo.toml src-tauri/Cargo.lock
git commit -m "feat(channel): add DingtalkStreamClient with WebSocket + auto-reconnect"
```

---

## Task 5: ChannelManager

**Files:**
- Create: `src-tauri/src/connector/channel/manager.rs`

- [ ] **Step 1: 创建 manager.rs**

```rust
//! ChannelManager — 管理 IM 频道连接生命周期
//!
//! 职责：读取配置、启动 StreamClient、接收消息、路由到 SessionRuntime、发事件给前端

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Result;
use serde_json::Value;
use tauri::{AppHandle, Emitter};
use tokio::sync::{mpsc, RwLock};

use crate::runtime::{ChatTurnRequest, SessionRuntime};
use crate::storage::crypto::SecureStorage;

use super::dingtalk_stream::DingtalkStreamClient;
use super::router::ChannelSessionRouter;
use super::types::{
    ChannelConversation, ChannelMessage, ChannelMessagePayload, ChannelStatus,
    ChannelStatusPayload, ConversationType, DingtalkChannelConfig, Platform,
};

pub struct ChannelManager {
    app_handle: AppHandle,
    session_runtime: Arc<SessionRuntime>,
    secure_storage: Option<Arc<SecureStorage>>,
    channels_dir: PathBuf,
    sessions_path: PathBuf,
    status: RwLock<ChannelStatus>,
    seen_msg_ids: RwLock<HashSet<String>>,
    conversations: RwLock<Vec<ChannelConversation>>,
}

impl ChannelManager {
    pub fn new(
        app_handle: AppHandle,
        session_runtime: Arc<SessionRuntime>,
        secure_storage: Option<Arc<SecureStorage>>,
        channels_dir: PathBuf,
    ) -> Self {
        let sessions_path = channels_dir.join("dingtalk_sessions.json");
        Self {
            app_handle,
            session_runtime,
            secure_storage,
            channels_dir,
            sessions_path,
            status: RwLock::new(ChannelStatus::Unconfigured),
            seen_msg_ids: RwLock::new(HashSet::new()),
            conversations: RwLock::new(vec![]),
        }
    }

    /// 读取配置文件，若存在则自动连接
    pub async fn auto_connect_if_configured(&self) {
        let config_path = self.channels_dir.join("dingtalk_config.json");
        if !config_path.exists() {
            return;
        }
        match std::fs::read_to_string(&config_path)
            .ok()
            .and_then(|s| serde_json::from_str::<DingtalkChannelConfig>(&s).ok())
        {
            Some(config) => {
                if let Err(e) = self.connect_dingtalk(config).await {
                    log::warn!("[channel] auto_connect failed: {:#}", e);
                }
            }
            None => {
                log::warn!("[channel] dingtalk_config.json found but failed to parse");
            }
        }
    }

    /// 保存配置并建立连接
    pub async fn save_config_and_connect(
        &self,
        app_key: String,
        app_secret_plain: String,
        robot_code: String,
    ) -> Result<()> {
        let app_secret_encrypted = match &self.secure_storage {
            Some(ss) => ss.encrypt(&app_secret_plain)?,
            None => app_secret_plain.clone(), // fallback: plaintext
        };

        let config = DingtalkChannelConfig {
            app_key: app_key.clone(),
            app_secret_encrypted,
            robot_code: robot_code.clone(),
        };

        let config_path = self.channels_dir.join("dingtalk_config.json");
        std::fs::create_dir_all(&self.channels_dir)?;
        std::fs::write(&config_path, serde_json::to_string_pretty(&config)?)?;

        self.connect_dingtalk(config).await
    }

    async fn connect_dingtalk(&self, config: DingtalkChannelConfig) -> Result<()> {
        let app_secret_plain = match &self.secure_storage {
            Some(ss) => ss.decrypt(&config.app_secret_encrypted)?,
            None => config.app_secret_encrypted.clone(),
        };

        let (msg_tx, mut msg_rx) = mpsc::channel::<ChannelMessage>(64);

        let client = DingtalkStreamClient::new(
            config.app_key,
            app_secret_plain,
            config.robot_code,
            msg_tx,
        );

        // 状态回调
        let app = self.app_handle.clone();
        let status_lock = Arc::new(RwLock::new(()));
        let self_status = &self.status as *const RwLock<ChannelStatus> as usize;
        let app_clone = app.clone();

        client.start(move |status| {
            let payload = ChannelStatusPayload {
                platform: "dingtalk".into(),
                status: status.clone(),
            };
            let _ = app_clone.emit("channel:status", &payload);
        });

        // 消息处理 loop
        let runtime = self.session_runtime.clone();
        let sessions_path = self.sessions_path.clone();
        let seen_ids = Arc::clone(&self.seen_msg_ids) as Arc<RwLock<HashSet<String>>>;
        let convs = Arc::clone(&self.conversations) as Arc<RwLock<Vec<ChannelConversation>>>;
        let app_msg = self.app_handle.clone();
        let bridge_robot_code = "".to_string(); // robot_code already in ChannelMessage

        tokio::spawn(async move {
            let mut router = match ChannelSessionRouter::load(&sessions_path) {
                Ok(r) => r,
                Err(e) => {
                    log::error!("[channel] failed to load router: {:#}", e);
                    return;
                }
            };

            while let Some(msg) = msg_rx.recv().await {
                // 幂等去重
                {
                    let mut ids = seen_ids.write().await;
                    if !msg.msg_id.is_empty() && !ids.insert(msg.msg_id.clone()) {
                        continue;
                    }
                }

                let conv_type = msg.conversation_type.clone();
                let conv_key = msg.conversation_key.clone();
                let sender_nick = msg.sender_nick.clone();
                let text = msg.text.clone();
                let reply_group_id = msg.reply_group_id.clone();
                let robot_code = msg.robot_code.clone();

                // 路由到 session
                let rt = runtime.clone();
                let session_id = match router.get_or_create_session(
                    &conv_type,
                    &conv_key,
                    || {
                        let rt2 = rt.clone();
                        let title = match &conv_type {
                            ConversationType::Group => format!("钉钉群: {}", &conv_key[..8.min(conv_key.len())]),
                            ConversationType::Private => format!("钉钉私聊: {}", &sender_nick),
                        };
                        // 在 block_in_place 中同步创建对话
                        tokio::task::block_in_place(|| {
                            tokio::runtime::Handle::current().block_on(async {
                                rt2.create_conversation(&title).await
                            })
                        })
                    },
                ) {
                    Ok(id) => id,
                    Err(e) => {
                        log::error!("[channel] session routing failed: {:#}", e);
                        continue;
                    }
                };

                // 构造 content（群聊带发送者前缀）
                let content = match &conv_type {
                    ConversationType::Group => format!("[{}]: {}", sender_nick, text),
                    ConversationType::Private => text.clone(),
                };

                let request = ChatTurnRequest::new(session_id.clone(), content, vec![]);

                // 推给前端（新消息通知）
                let _ = app_msg.emit("channel:message", &ChannelMessagePayload {
                    platform: "dingtalk".into(),
                    session_id: session_id.clone(),
                    sender_nick: sender_nick.clone(),
                    text_preview: if text.len() > 30 { format!("{}...", &text[..30]) } else { text.clone() },
                });

                if let Err(e) = rt.run_chat_request(request).await {
                    log::error!("[channel] run_chat_request failed: {}", e);
                }
            }
        });

        Ok(())
    }

    pub async fn get_status(&self) -> ChannelStatus {
        self.status.read().await.clone()
    }

    pub async fn get_conversations(&self) -> Vec<ChannelConversation> {
        self.conversations.read().await.clone()
    }
}
```

- [ ] **Step 2: SessionRuntime 需要 create_conversation 方法，检查是否存在**

```bash
grep -n "create_conversation\|pub async fn create" src-tauri/src/runtime/session_runtime.rs | head -10
```

若不存在，后续步骤会在 task 7 通过 Tauri command 层的 `create_conversation` 来创建。此处先用 `uuid::Uuid::new_v4().to_string()` 作为 session_id（不持久化），在 Task 7 中替换为正确调用。

**注意**：如果 `create_conversation` 不存在，将 manager.rs 中的创建逻辑改为：

```rust
|| Ok(uuid::Uuid::new_v4().to_string())
```

- [ ] **Step 3: 编译确认无误**

```bash
cd src-tauri && cargo check 2>&1 | grep -E "^error" | head -20
```

逐个修复编译错误（主要是 import 路径和方法名）。

- [ ] **Step 4: 提交**

```bash
git add src-tauri/src/connector/channel/manager.rs
git commit -m "feat(channel): add ChannelManager with message routing to SessionRuntime"
```

---

## Task 6: Tauri Commands + 注册

**Files:**
- Create: `src-tauri/src/commands/channel.rs`
- Modify: `src-tauri/src/commands/mod.rs`
- Modify: `src-tauri/src/lib.rs`

- [ ] **Step 1: 创建 channel.rs**

```rust
//! Tauri IPC commands for IM channel management

use std::sync::Arc;
use tauri::State;

use crate::connector::channel::{ChannelConversation, ChannelManager, ChannelStatus};

#[tauri::command]
pub async fn channel_save_config(
    manager: State<'_, Arc<ChannelManager>>,
    app_key: String,
    app_secret: String,
    robot_code: String,
) -> Result<(), String> {
    manager
        .save_config_and_connect(app_key, app_secret, robot_code)
        .await
        .map_err(|e| format!("{:#}", e))
}

#[tauri::command]
pub async fn channel_get_status(
    manager: State<'_, Arc<ChannelManager>>,
) -> Result<ChannelStatus, String> {
    Ok(manager.get_status().await)
}

#[tauri::command]
pub async fn channel_get_conversations(
    manager: State<'_, Arc<ChannelManager>>,
) -> Result<Vec<ChannelConversation>, String> {
    Ok(manager.get_conversations().await)
}
```

- [ ] **Step 2: 在 commands/mod.rs 加一行**

```rust
pub mod channel;
```

- [ ] **Step 3: 在 lib.rs 中注册 ChannelManager 和 commands**

找到 `app.manage(current_user_storage.clone());` 附近，在用户 scope 激活后加 ChannelManager 初始化：

```rust
// 在 current_user_storage.activate_scope 成功后，获取 paths 并初始化 ChannelManager
if let Some(paths) = current_user_storage.resolve_paths() {
    let channel_manager = Arc::new(crate::connector::channel::ChannelManager::new(
        app.handle().clone(),
        session_runtime.clone(),  // 需确认 session_runtime 变量名
        secure_storage.clone(),
        paths.channels_dir(),
    ));
    let cm = channel_manager.clone();
    tauri::async_runtime::spawn(async move {
        cm.auto_connect_if_configured().await;
    });
    app.manage(channel_manager);
}
```

在 `invoke_handler` 的 `tauri::generate_handler![]` 末尾加：

```rust
commands::channel::channel_save_config,
commands::channel::channel_get_status,
commands::channel::channel_get_conversations,
```

- [ ] **Step 4: 编译确认**

```bash
cd src-tauri && cargo check 2>&1 | grep -E "^error" | head -20
```

- [ ] **Step 5: 提交**

```bash
git add src-tauri/src/commands/channel.rs src-tauri/src/commands/mod.rs src-tauri/src/lib.rs
git commit -m "feat(channel): register ChannelManager and Tauri commands"
```

---

## Task 7: 前端 IPC 封装 + Store

**Files:**
- Modify: `src/lib/tauri.ts`
- Create: `src/stores/channelStore.ts`

- [ ] **Step 1: 在 tauri.ts 中加 channel 事件常量和 IPC 函数**

在 `TAURI_EVENTS` 对象内加两个常量（在末尾 `} as const` 之前）：

```typescript
  CHANNEL_STATUS: 'channel:status',
  CHANNEL_MESSAGE: 'channel:message',
```

在文件末尾加 IPC 函数和类型：

```typescript
// ---------------------------------------------------------------------------
// Channel types
// ---------------------------------------------------------------------------

export type ChannelStatusState =
  | { state: 'unconfigured' }
  | { state: 'disconnected' }
  | { state: 'connecting' }
  | { state: 'connected' }
  | { state: 'reconnecting'; delaySecs: number }
  | { state: 'configError'; message: string }

export interface ChannelStatusPayload {
  platform: string
  status: ChannelStatusState
}

export interface ChannelMessagePayload {
  platform: string
  sessionId: string
  senderNick: string
  textPreview: string
}

export interface ChannelConversation {
  sessionId: string
  platform: string
  conversationType: 'group' | 'private'
  externalId: string
  displayName: string
  unreadCount: number
}

// ---------------------------------------------------------------------------
// Channel IPC
// ---------------------------------------------------------------------------

export function channelSaveConfig(
  appKey: string,
  appSecret: string,
  robotCode: string,
): Promise<void> {
  return invoke<void>('channel_save_config', { appKey, appSecret, robotCode })
}

export function channelGetStatus(): Promise<ChannelStatusState> {
  return invoke<ChannelStatusState>('channel_get_status')
}

export function channelGetConversations(): Promise<ChannelConversation[]> {
  return invoke<ChannelConversation[]>('channel_get_conversations')
}

export function onChannelStatus(
  handler: (payload: ChannelStatusPayload) => void,
): Promise<() => void> {
  return listen<ChannelStatusPayload>(TAURI_EVENTS.CHANNEL_STATUS, (e) => handler(e.payload))
}

export function onChannelMessage(
  handler: (payload: ChannelMessagePayload) => void,
): Promise<() => void> {
  return listen<ChannelMessagePayload>(TAURI_EVENTS.CHANNEL_MESSAGE, (e) => handler(e.payload))
}
```

- [ ] **Step 2: 创建 channelStore.ts**

```typescript
import { create } from 'zustand'
import {
  type ChannelConversation,
  type ChannelStatusState,
  channelGetConversations,
  channelGetStatus,
  onChannelMessage,
  onChannelStatus,
} from '@/lib/tauri'

interface ChannelState {
  dingtalkStatus: ChannelStatusState
  conversations: ChannelConversation[]
  activeSessionId: string | null

  setStatus: (platform: string, status: ChannelStatusState) => void
  setConversations: (convs: ChannelConversation[]) => void
  setActiveSession: (sessionId: string | null) => void
  incrementUnread: (sessionId: string) => void
  clearUnread: (sessionId: string) => void
  loadConversations: () => Promise<void>
}

export const useChannelStore = create<ChannelState>((set, get) => ({
  dingtalkStatus: { state: 'unconfigured' },
  conversations: [],
  activeSessionId: null,

  setStatus: (platform, status) => {
    if (platform === 'dingtalk') {
      set({ dingtalkStatus: status })
    }
  },

  setConversations: (convs) => set({ conversations: convs }),

  setActiveSession: (sessionId) => {
    set({ activeSessionId: sessionId })
    if (sessionId) get().clearUnread(sessionId)
  },

  incrementUnread: (sessionId) =>
    set((s) => ({
      conversations: s.conversations.map((c) =>
        c.sessionId === sessionId ? { ...c, unreadCount: c.unreadCount + 1 } : c,
      ),
    })),

  clearUnread: (sessionId) =>
    set((s) => ({
      conversations: s.conversations.map((c) =>
        c.sessionId === sessionId ? { ...c, unreadCount: 0 } : c,
      ),
    })),

  loadConversations: async () => {
    try {
      const convs = await channelGetConversations()
      set({ conversations: convs })
    } catch (e) {
      console.error('[channelStore] loadConversations failed', e)
    }
  },
}))

/** App 启动时调用一次，订阅后端事件 */
export async function initChannelListeners() {
  await onChannelStatus(({ platform, status }) => {
    useChannelStore.getState().setStatus(platform, status)
  })
  await onChannelMessage(({ sessionId }) => {
    const { activeSessionId } = useChannelStore.getState()
    if (sessionId !== activeSessionId) {
      useChannelStore.getState().incrementUnread(sessionId)
    }
  })
}
```

- [ ] **Step 3: TypeScript 编译检查**

```bash
pnpm build 2>&1 | grep -E "error TS" | head -20
```

期望：无 TS error

- [ ] **Step 4: 提交**

```bash
git add src/lib/tauri.ts src/stores/channelStore.ts
git commit -m "feat(channel): add channel IPC types and channelStore"
```

---

## Task 8: 前端路由和导航

**Files:**
- Modify: `src/stores/uiStore.ts`
- Modify: `src/components/sidebar/SidebarNav.tsx`
- Modify: `src/App.tsx`

- [ ] **Step 1: 在 uiStore.ts 加 channel route**

找到 `export type Route =` 定义，加一行：

```typescript
  | { kind: 'channel'; sessionId?: string }
```

在 `isRoute` 函数的 switch 中加：

```typescript
    case 'channel':
      return true
```

- [ ] **Step 2: 在 SidebarNav.tsx 加「频道」导航项**

`SidebarNavKey` 类型改为：

```typescript
export type SidebarNavKey = 'home' | 'skill-center' | 'schedules' | 'channel'
```

`NAV` 数组加一项（在 `schedules` 之后）：

```typescript
import { Blocks, Clock3, MessageSquare, SquarePen, type LucideIcon } from 'lucide-react'
// ...
  { key: 'channel', label: '频道', icon: MessageSquare },
```

- [ ] **Step 3: 在 App.tsx 的 RouteSwitch 加 channel case**

先在 import 区加：

```typescript
import { ChannelPage } from '@/features/channel/ChannelPage'
```

在 RouteSwitch 的 switch 中加（`chat` case 之后）：

```typescript
    case 'channel':
      return <ChannelPage sessionId={route.sessionId} />
```

- [ ] **Step 4: TypeScript 编译检查**

```bash
pnpm build 2>&1 | grep -E "error TS" | head -20
```

- [ ] **Step 5: 提交**

```bash
git add src/stores/uiStore.ts src/components/sidebar/SidebarNav.tsx src/App.tsx
git commit -m "feat(channel): add channel route and sidebar nav entry"
```

---

## Task 9: 频道面板 UI

**Files:**
- Create: `src/features/channel/ChannelPage.tsx`
- Create: `src/features/channel/ChannelConfig.tsx`

- [ ] **Step 1: 创建 ChannelConfig.tsx（配置表单）**

```tsx
import { useState } from 'react'
import { channelSaveConfig } from '@/lib/tauri'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'

interface ChannelConfigProps {
  onSaved?: () => void
}

export function ChannelConfig({ onSaved }: ChannelConfigProps) {
  const [appKey, setAppKey] = useState('')
  const [appSecret, setAppSecret] = useState('')
  const [robotCode, setRobotCode] = useState('')
  const [saving, setSaving] = useState(false)
  const [error, setError] = useState<string | null>(null)

  const handleSave = async () => {
    if (!appKey.trim() || !appSecret.trim() || !robotCode.trim()) {
      setError('请填写全部字段')
      return
    }
    setSaving(true)
    setError(null)
    try {
      await channelSaveConfig(appKey.trim(), appSecret.trim(), robotCode.trim())
      onSaved?.()
    } catch (e) {
      setError(e instanceof Error ? e.message : '保存失败，请重试')
    } finally {
      setSaving(false)
    }
  }

  return (
    <div className="flex flex-col gap-4 p-6 max-w-sm">
      <h2 className="text-base font-semibold">配置钉钉机器人</h2>
      <div className="flex flex-col gap-2">
        <label className="text-sm text-muted-foreground">AppKey</label>
        <Input
          value={appKey}
          onChange={(e) => setAppKey(e.target.value)}
          placeholder="dingXXXXXXXX"
        />
      </div>
      <div className="flex flex-col gap-2">
        <label className="text-sm text-muted-foreground">AppSecret</label>
        <Input
          type="password"
          value={appSecret}
          onChange={(e) => setAppSecret(e.target.value)}
          placeholder="输入后加密存储"
        />
      </div>
      <div className="flex flex-col gap-2">
        <label className="text-sm text-muted-foreground">RobotCode</label>
        <Input
          value={robotCode}
          onChange={(e) => setRobotCode(e.target.value)}
          placeholder="机器人 code"
        />
      </div>
      {error && <p className="text-sm text-red-500">{error}</p>}
      <Button onClick={handleSave} disabled={saving}>
        {saving ? '连接中...' : '保存并连接'}
      </Button>
    </div>
  )
}
```

- [ ] **Step 2: 创建 ChannelPage.tsx**

```tsx
import { useEffect } from 'react'
import { useChannelStore, initChannelListeners } from '@/stores/channelStore'
import { useUiStore } from '@/stores/uiStore'
import { useChatStore } from '@/stores/chatStore'
import { ChatArea } from '@/components/layout/ChatArea'
import { ChatBottomArea } from '@/components/chat-scene/ChatBottomArea'
import { ChannelConfig } from './ChannelConfig'
import type { ChannelStatusState } from '@/lib/tauri'

interface ChannelPageProps {
  sessionId?: string
}

function StatusDot({ status }: { status: ChannelStatusState }) {
  switch (status.state) {
    case 'connected':
      return <span className="h-2 w-2 rounded-full bg-green-500 inline-block" title="已连接" />
    case 'connecting':
    case 'reconnecting':
      return <span className="h-2 w-2 rounded-full bg-yellow-400 inline-block animate-pulse" title="连接中" />
    case 'configError':
      return <span className="h-2 w-2 rounded-full bg-red-500 inline-block" title={status.message} />
    default:
      return <span className="h-2 w-2 rounded-full bg-gray-400 inline-block" title="未连接" />
  }
}

export function ChannelPage({ sessionId }: ChannelPageProps) {
  const dingtalkStatus = useChannelStore((s) => s.dingtalkStatus)
  const conversations = useChannelStore((s) => s.conversations)
  const activeSessionId = useChannelStore((s) => s.activeSessionId)
  const setActiveSession = useChannelStore((s) => s.setActiveSession)
  const loadConversations = useChannelStore((s) => s.loadConversations)

  useEffect(() => {
    void initChannelListeners()
    void loadConversations()
  }, [loadConversations])

  useEffect(() => {
    if (sessionId) setActiveSession(sessionId)
  }, [sessionId, setActiveSession])

  const isUnconfigured = dingtalkStatus.state === 'unconfigured'
  const privateConvs = conversations.filter((c) => c.conversationType === 'private')
  const groupConvs = conversations.filter((c) => c.conversationType === 'group')

  return (
    <div className="flex h-full">
      {/* 左侧面板 */}
      <div className="w-56 flex-shrink-0 border-r flex flex-col bg-sidebar">
        <div className="flex items-center justify-between px-3 py-3 border-b">
          <span className="text-sm font-medium">频道</span>
        </div>

        {/* 钉钉分组 */}
        <div className="flex-1 overflow-y-auto py-2">
          <div className="px-3 py-1.5 flex items-center gap-2">
            <StatusDot status={dingtalkStatus} />
            <span className="text-sm font-medium">钉钉</span>
            {dingtalkStatus.state === 'configError' && (
              <span className="text-xs text-red-500">配置有误</span>
            )}
            {dingtalkStatus.state === 'reconnecting' && (
              <span className="text-xs text-muted-foreground">重连中...</span>
            )}
          </div>

          {isUnconfigured ? (
            <div className="px-3 py-1 text-xs text-muted-foreground">未配置，点击右侧设置</div>
          ) : (
            <>
              {privateConvs.length > 0 && (
                <div className="mt-1">
                  <div className="px-3 py-1 text-xs text-muted-foreground">私聊</div>
                  {privateConvs.map((c) => (
                    <button
                      key={c.sessionId}
                      type="button"
                      onClick={() => setActiveSession(c.sessionId)}
                      className={`w-full flex items-center justify-between px-4 py-1.5 text-sm hover:bg-sidebar-accent/40 ${
                        activeSessionId === c.sessionId ? 'bg-sidebar-accent' : ''
                      }`}
                    >
                      <span className="truncate">{c.displayName}</span>
                      {c.unreadCount > 0 && (
                        <span className="ml-1 text-xs bg-primary text-primary-foreground rounded-full px-1.5">
                          {c.unreadCount}
                        </span>
                      )}
                    </button>
                  ))}
                </div>
              )}

              {groupConvs.length > 0 && (
                <div className="mt-1">
                  <div className="px-3 py-1 text-xs text-muted-foreground">群聊</div>
                  {groupConvs.map((c) => (
                    <button
                      key={c.sessionId}
                      type="button"
                      onClick={() => setActiveSession(c.sessionId)}
                      className={`w-full flex items-center justify-between px-4 py-1.5 text-sm hover:bg-sidebar-accent/40 ${
                        activeSessionId === c.sessionId ? 'bg-sidebar-accent' : ''
                      }`}
                    >
                      <span className="truncate">{c.displayName}</span>
                      {c.unreadCount > 0 && (
                        <span className="ml-1 text-xs bg-primary text-primary-foreground rounded-full px-1.5">
                          {c.unreadCount}
                        </span>
                      )}
                    </button>
                  ))}
                </div>
              )}
            </>
          )}

          {/* 飞书占位 */}
          <div className="px-3 py-1.5 mt-2 flex items-center gap-2 opacity-40">
            <span className="h-2 w-2 rounded-full bg-gray-400 inline-block" />
            <span className="text-sm">飞书（未配置）</span>
          </div>
        </div>
      </div>

      {/* 右侧内容区 */}
      <div className="flex-1 flex flex-col overflow-hidden">
        {isUnconfigured ? (
          <div className="flex-1 flex items-center justify-center">
            <ChannelConfig onSaved={() => void loadConversations()} />
          </div>
        ) : activeSessionId ? (
          <>
            <div className="px-4 py-2 border-b text-sm text-muted-foreground bg-background">
              钉钉 · {conversations.find((c) => c.sessionId === activeSessionId)?.displayName ?? ''}
            </div>
            <div className="flex-1 flex flex-col overflow-hidden">
              <ChatArea />
              <ChatBottomArea />
            </div>
          </>
        ) : (
          <div className="flex-1 flex items-center justify-center text-muted-foreground text-sm">
            选择一个会话开始
          </div>
        )}
      </div>
    </div>
  )
}
```

- [ ] **Step 3: TypeScript 编译检查**

```bash
pnpm build 2>&1 | grep -E "error TS" | head -20
```

修复所有 TS 错误后继续。

- [ ] **Step 4: 提交**

```bash
git add src/features/channel/
git commit -m "feat(channel): add ChannelPage and ChannelConfig UI"
```

---

## Task 10: App 启动时初始化 + 端到端冒烟测试

**Files:**
- Modify: `src/App.tsx`

- [ ] **Step 1: 在 App.tsx 中调用 initChannelListeners**

找到 `useStreaming()` 或其他 hook 调用区，加：

```typescript
import { initChannelListeners } from '@/stores/channelStore'

// 在 AppShell 或顶层 useEffect 中：
useEffect(() => {
  void initChannelListeners()
}, [])
```

- [ ] **Step 2: 启动开发模式**

```bash
pnpm tauri:dev
```

- [ ] **Step 3: 冒烟测试检查项**

1. 左侧导航出现「频道」tab，点击后进入频道面板
2. 钉钉显示「未配置」状态（未配置 AppKey 时）
3. 右侧显示配置表单，三个输入框可以输入
4. 飞书显示灰色「未配置」占位
5. 无 JS 报错（打开 DevTools 检查 Console）

- [ ] **Step 4: 提交**

```bash
git add src/App.tsx
git commit -m "feat(channel): init channel listeners on app start"
```

---

## Task 11: Rust 集成编译验证

- [ ] **Step 1: 完整 Rust 编译**

```bash
cd src-tauri && cargo build 2>&1 | grep -E "^error" | head -20
```

修复所有编译错误。

- [ ] **Step 2: 运行所有 channel 相关单元测试**

```bash
cd src-tauri && cargo test connector::channel -- --nocapture
```

期望：所有测试通过。

- [ ] **Step 3: 运行架构约束回归测试（确保没破坏现有约束）**

```bash
cd src-tauri && cargo test review_ --tests --no-fail-fast 2>&1 | tail -20
```

期望：所有 `review_` 测试通过。

- [ ] **Step 4: 最终提交**

```bash
git add -A
git commit -m "feat(channel): complete DingTalk Stream IM channel integration"
```
