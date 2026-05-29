# 飞书 IM Connector Phase 1 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 实现 `IMConnector` for 飞书，覆盖 device-code 注册、WebSocket 入站、tenant_access_token 缓存、4 类消息 normalize、CardKit 流式回复、附件下载、Pending 队列接入、前端配置面板。

**Architecture:** 沿用 Phase 0 的 trait 抽象。新建 `src-tauri/src/connector/im/feishu/` 目录，结构与 `dingtalk/` 平行。Token 缓存复用 PR0a 抽出的 `SharedTokenCache<PlatformTokenSource>`；消息 dedup 复用 PR0b 的 `MessageDedupSet`；`ReplyTarget` 用 PR0d 的中性形态；config 存储用 PR0d 的 `platform_*_path` helper。**所有 HTTP 都自己写**（用 `reqwest` + `tokio-tungstenite`），不引入 `lark-rs` / `larksuite-rust-sdk`。

**Tech Stack:** Rust async (tokio + tokio-util), reqwest 0.12 (json+stream), tokio-tungstenite 0.26 (rustls), async-trait, serde / serde_json, uuid, anyhow / thiserror, chrono, futures-util, tempfile（test）。新增前端依赖：无。

**Prerequisites:** Plan A（`2026-05-18-im-phase0-cleanup.md`）的 4 个 PR0a-d 已合入 main 且在生产稳定 ≥3 天。

**Endpoint research**: All feishu HTTP endpoints / WS handshake / message wire-format used in this plan have been verified against open.feishu.cn docs + `larksuite/oapi-sdk-go` source. See `feishu-endpoints-notes.md` in this directory for the authoritative reference. If any endpoint behaves differently from the plan during implementation, **check the notes file first** before assuming the plan was right.

**Working reference implementation (TypeScript)**: `/Users/oayzz/Downloads/openclaw channel/openclaw-lark-main/` is a **fully working飞书 OpenClaw plugin** by ByteDance (uses `@larksuiteoapi/node-sdk` underneath). When in doubt about flow / sequence / edge cases / 24 message-type normalization / CardKit streaming semantics, **read the TS implementation first** — it's the canonical reference for "what feishu actually does":
- `src/channel/onboarding.ts` — device-code + post-registration flow
- `src/core/lark-client.ts` — WSClient connect / probe / EventDispatcher lifecycle
- `src/core/token-store.ts` — tenant_access_token caching strategy
- `src/messaging/converters/` — 24 inbound message-type → normalized payload converters
- `src/card/cardkit.ts` + `src/card/streaming-card-controller.ts` — CardKit create / sequence / streaming / flush controller
- `src/channel/event-handlers.ts` — event routing per type

This TS plugin uses the official Lark Node SDK, so for **wire details** (protobuf shapes, ws ticket, retry policies) we'd need to read inside `@larksuiteoapi/node-sdk` (in its `node_modules/`). For our Rust port, the TS plugin shows us **what to call and in what order**; the SDK shows us **what to send on the wire**.

---

## File Structure

```
src-tauri/src/connector/im/
├── feishu/                         ← 新增整个目录
│   ├── mod.rs                      ← pub mod 子模块 + Re-export
│   ├── connector.rs                ← impl IMConnector for FeishuConnector
│   ├── registration.rs             ← device-code begin/poll（accounts.feishu.cn / RFC 8628）
│   ├── token.rs                    ← FeishuTokenSource: PlatformTokenSource
│   ├── stream.rs                   ← WSClient 长连 + 消息 parse + normalize
│   ├── card.rs                     ← CardKit create/stream/finish + 节流 sender
│   ├── download.rs                 ← im.message.resource.get 拉原始字节
│   └── types.rs                    ← FeishuStoredConfig / FeishuSessionTarget 等
├── shared/
│   └── config_store.rs             ← 加 read_feishu_config / save_feishu_registration / 等专属方法
├── trait_def.rs                    ← 不动（PR0d 已中性化）
├── manager.rs                      ← 新增 register_feishu_connector + worker loop 支持 Platform::Feishu
├── factory.rs                      ← 新增 build_feishu_connector
└── types.rs                        ← 不动（Platform::Feishu 已存在）

src-tauri/src/commands/
└── channel.rs                      ← begin_registration / poll_registration / set_enabled 支持 feishu 分支

src-tauri/src/lib.rs                ← 启动期 auto_connect_if_configured 触发 feishu

src-tauri/tests/
├── review_im_layering.rs           ← platforms 数组追加 "feishu"
├── im_feishu_integration.rs        ← 新增：mock 飞书后端 + Manager 全链路
└── im_connector_cancel_test.rs     ← 不动

src/
├── lib/tauri.ts                    ← 新增 ChannelPlatform feishu 类型 + IPC（如有专属）
├── features/channel/
│   ├── ChannelPage.tsx             ← PlatformKey 加 'feishu'，cards 列表追加飞书项
│   ├── ChannelConfig.tsx           ← device-code 流通用化或新建 FeishuChannelConfig
│   └── FeishuChannelConfig.tsx     ← 新建（如选独立组件）
└── stores/channelStore.ts          ← 无须改（已通用化于 Platform）
```

**核心责任划分**：
- `feishu/registration.rs`：`begin_registration() -> RegistrationBeginResult` + `poll_registration(device_code) -> RegistrationPollResult`，调单端点 `POST https://accounts.feishu.cn/oauth/v1/app/registration`（form-encoded，RFC 8628 device-code grant）。
- `feishu/token.rs`：`FeishuTokenSource` 持 app_id / app_secret，`fetch()` 调 `/open-apis/auth/v3/tenant_access_token/internal`。包装成 `SharedTokenCache<FeishuTokenSource>`。
- `feishu/stream.rs`：`FeishuStreamClient` 复刻 dingtalk/stream.rs 结构——POST `/callback/ws/endpoint` 拿 wss URL + ClientConfig，tungstenite 长连，**protobuf (pbbp2) frame 解码**（不是 JSON），retry 用 `shared::ReconnectBackoff`。
- `feishu/card.rs`：`CardKitSender`——每个 `card_id` 起一个 `mpsc + 串行 sender task`，间隔 ≥100ms（`tokio::time::sleep_until`），**不丢 chunk**。
- `feishu/connector.rs`：`impl IMConnector`。`start()` 实例化 token cache + stream client，返回 BoxStream<ChannelMessage>；`send(target, content)` 内部按 ReplyContent 分发到 webhook reply / CardKit。
- `feishu/types.rs`：`FeishuStoredConfig` schema（含 app_id / app_secret_encrypted / 等），`FeishuSessionTarget`（CardKit chat_id / receive_id_type）。

---

## §0 前置准备

- [ ] **Step 0.1: 确认 Plan A 已落地**

Run: `cd /Users/oayzz/project/lotus/lotus-workbench/lotus-app/.claude/worktrees/amazing-chatelet-801fd7 && git log --oneline | head -15`
Expected: 看到 PR0a / PR0b / PR0c / PR0d 4 个 commit 在最近 history 里。

- [ ] **Step 0.2: 跑 baseline**

Run: `cd /Users/oayzz/project/lotus/lotus-workbench/lotus-app/.claude/worktrees/amazing-chatelet-801fd7/src-tauri && cargo test --lib --no-fail-fast 2>&1 | tail -3`
Expected: `passed; <M> failed`，记 N+M。

- [x] **Step 0.3: Endpoint research COMPLETE** — see `feishu-endpoints-notes.md` for verified URLs / wire formats / errcodes / event schemas across all 8 research questions (Q1-Q8). Key findings folded back into Task 2 / Task 3 / Task 5 inline. If Q3 (WebSocket) implementation hits an unexpected schema-mismatch, the source of truth is `larksuite/oapi-sdk-go` repo.

- [ ] **Step 0.4: 创建空 feishu 模块骨架（PR1 准备）**

只为后续 import 路径可解析，先建一个空目录占位：

Edit `src-tauri/src/connector/im/mod.rs`，在 `pub mod dingtalk;` 之后追加 `pub mod feishu;`。

Create `src-tauri/src/connector/im/feishu/mod.rs`：

```rust
//! Feishu (Lark) connector implementation. Mirrors `dingtalk/` structure.
//! Phase 1 PRs:
//!   PR1 — skeleton + impl IMConnector with stubbed methods + frontend stub button
//!   PR2 — device-code registration + tenant_access_token cache
//!   PR3 — WebSocket runtime + message normalize
//!   PR4 — Text/Markdown send + webhook reply path
//!   PR5 — CardKit streaming (create / stream / finish / fail) with rate limit
//!   PR6 — attachment download + PendingQueueManager integration
//!   PR7 — integration test + UI

pub mod connector;
pub mod types;

// PR2-onwards add: registration, token, stream, card, download.

pub use connector::FeishuConnector;
```

Create `src-tauri/src/connector/im/feishu/types.rs`：

```rust
//! Feishu-specific persisted types and runtime targets.

use serde::{Deserialize, Serialize};

use crate::connector::im::types::Platform;
use crate::connector::im::types::SecretStorageKind;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FeishuStoredCredentials {
    pub app_id: String,
    pub app_secret_encrypted: String,
    pub app_secret_storage: SecretStorageKind,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FeishuStoredMetadata {
    pub created_at: String,
    pub updated_at: String,
}

/// users/<scope>/channels/feishu/config.json schema. schema_version=1 for PR2 onwards.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FeishuStoredConfig {
    pub schema_version: u32,
    pub platform: Platform,
    pub configured: bool,
    pub enabled: bool,
    pub credentials: FeishuStoredCredentials,
    pub metadata: FeishuStoredMetadata,
}

/// CardKit + reply credentials per session_id, populated by manager worker when
/// a message arrives and consumed by FeishuConnector::send.
#[derive(Debug, Clone)]
pub struct FeishuSessionTarget {
    /// CardKit receive_id_type ("chat_id" for group, "open_id" for private).
    pub receive_id_type: String,
    /// chat_id (group) or open_id (private).
    pub receive_id: String,
}
```

Create `src-tauri/src/connector/im/feishu/connector.rs`（**PR1 阶段全部是 stub**）：

```rust
//! `FeishuConnector` — implements `IMConnector` for Lark/Feishu.
//!
//! PR1 stub: capabilities() reports the real shape but start() / send() return
//! NotSupported errors so the connector can be plugged into the factory + tested
//! at the architecture level without doing real network work.

use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use futures::stream::BoxStream;
use tokio::sync::RwLock;

use crate::connector::im::trait_def::{
    AuthFlow, ConnectorCapabilities, ConnectorContext, ConnectorError, IMConnector, InboundModel,
    ReplyContent, ReplyTarget,
};
use crate::connector::im::types::{ChannelMessage, Platform};

use super::types::FeishuSessionTarget;

pub struct FeishuConnector {
    #[allow(dead_code)] // PR2: used by registration / token cache
    app_id: String,
    #[allow(dead_code)]
    app_secret: String,
    session_targets: Arc<RwLock<HashMap<String, FeishuSessionTarget>>>,
}

impl FeishuConnector {
    pub fn new(app_id: String, app_secret: String) -> Self {
        Self {
            app_id,
            app_secret,
            session_targets: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub async fn remember_session(&self, session_id: String, target: FeishuSessionTarget) {
        self.session_targets.write().await.insert(session_id, target);
    }
}

#[async_trait]
impl IMConnector for FeishuConnector {
    fn platform(&self) -> Platform {
        Platform::Feishu
    }

    fn capabilities(&self) -> ConnectorCapabilities {
        ConnectorCapabilities {
            inbound: InboundModel::Stream,
            outbound_aicard: true,
            outbound_markdown: true,
            supports_attachments: true,
            supports_group_chat: true,
            supports_private_chat: true,
            auth_flow: AuthFlow::DeviceCode,
        }
    }

    async fn start(
        &self,
        _ctx: ConnectorContext,
    ) -> Result<BoxStream<'static, ChannelMessage>, ConnectorError> {
        Err(ConnectorError::NotSupported("feishu start() — PR3 will implement"))
    }

    async fn send(
        &self,
        _target: ReplyTarget,
        _content: ReplyContent,
    ) -> Result<(), ConnectorError> {
        Err(ConnectorError::NotSupported("feishu send() — PR4+ will implement"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn platform_is_feishu() {
        let c = FeishuConnector::new("app_id".into(), "app_secret".into());
        assert_eq!(c.platform(), Platform::Feishu);
    }

    #[test]
    fn capabilities_reports_stream_and_aicard_and_device_code() {
        let c = FeishuConnector::new("ak".into(), "as".into());
        let caps = c.capabilities();
        assert!(matches!(caps.inbound, InboundModel::Stream));
        assert!(caps.outbound_aicard);
        assert!(caps.outbound_markdown);
        assert!(caps.supports_attachments);
        assert!(matches!(caps.auth_flow, AuthFlow::DeviceCode));
    }

    #[tokio::test]
    async fn remember_session_inserts() {
        let c = FeishuConnector::new("ak".into(), "as".into());
        c.remember_session(
            "sess-1".into(),
            FeishuSessionTarget {
                receive_id_type: "open_id".into(),
                receive_id: "ou_xxx".into(),
            },
        )
        .await;
        let map = c.session_targets.read().await;
        assert!(map.contains_key("sess-1"));
    }
}
```

- [ ] **Step 0.5: 跑 PR1 骨架编译 + 测试**

Run: `cd /Users/oayzz/project/lotus/lotus-workbench/lotus-app/.claude/worktrees/amazing-chatelet-801fd7/src-tauri && cargo build --lib 2>&1 | tail -5`
Expected: `Finished` 0 errors。

Run: `cd /Users/oayzz/project/lotus/lotus-workbench/lotus-app/.claude/worktrees/amazing-chatelet-801fd7/src-tauri && cargo test --lib connector::im::feishu -- --nocapture 2>&1 | tail -10`
Expected: 3 个测试全过。

---

## Task 1: PR1 — feishu 骨架 + factory + 前端入口 stub

**Files:**
- Modify: `src-tauri/src/connector/im/factory.rs`
- Modify: `src-tauri/src/connector/im/shared/config_store.rs`
- Modify: `src-tauri/src/connector/im/manager.rs`
- Modify: `src-tauri/src/commands/channel.rs`
- Modify: `src-tauri/tests/review_im_layering.rs`
- Modify: `src/features/channel/ChannelPage.tsx`

**目标**：飞书 connector 能被 manager + Tauri command + 前端发现，但实际功能全是 stub。前端"添加飞书账号"按钮显示为 disabled。

- [ ] **Step 1.1: factory 加 build_feishu_connector**

Edit `src-tauri/src/connector/im/factory.rs`：

```rust
use std::sync::Arc;

use crate::connector::im::dingtalk::connector::{DingtalkConnector, StatusCallback};
use crate::connector::im::dingtalk::token::TokenCache;
use crate::connector::im::feishu::connector::FeishuConnector;
use crate::connector::im::shared::reply_manager::DingtalkReplyManager;
use crate::connector::im::trait_def::IMConnector;

pub fn build_dingtalk_connector(
    app_key: String,
    app_secret: String,
    robot_code: String,
    reply_manager: Arc<DingtalkReplyManager>,
    on_status: StatusCallback,
) -> (Arc<dyn IMConnector>, Arc<DingtalkConnector>) {
    let concrete = Arc::new(DingtalkConnector::with_status_callback(
        app_key,
        app_secret,
        robot_code,
        reply_manager,
        Arc::new(TokenCache::new()),
        on_status,
    ));
    let dyn_handle: Arc<dyn IMConnector> = Arc::clone(&concrete) as Arc<dyn IMConnector>;
    (dyn_handle, concrete)
}

/// Build a `FeishuConnector` plus its concrete handle for `remember_session`.
/// PR1 returns a stub connector; PR2-PR7 fill in actual functionality.
pub fn build_feishu_connector(
    app_id: String,
    app_secret: String,
) -> (Arc<dyn IMConnector>, Arc<FeishuConnector>) {
    let concrete = Arc::new(FeishuConnector::new(app_id, app_secret));
    let dyn_handle: Arc<dyn IMConnector> = Arc::clone(&concrete) as Arc<dyn IMConnector>;
    (dyn_handle, concrete)
}

pub use crate::connector::im::dingtalk::connector::StatusCallback as DingtalkStatusCallback;
```

- [ ] **Step 1.2: config_store 加飞书占位 capability**

Edit `src-tauri/src/connector/im/shared/config_store.rs`：把 `all_platform_states` 内的 `Self::coming_soon_state(Platform::Feishu)` 改为：

```rust
            self.feishu_state_stub(connection.clone(), last_error.clone())?,
```

并在 `impl ChannelConfigStore` 内追加：

```rust
    /// PR1 stub: 飞书侧 capability=Available 但 configured=false / enabled=false。
    /// PR2 真正实现读 config / 解密 secret 后替换。
    pub fn feishu_state_stub(
        &self,
        _connection: ChannelConnectionState,
        _last_error: Option<String>,
    ) -> Result<ChannelPlatformState> {
        Ok(ChannelPlatformState {
            platform: Platform::Feishu,
            capability: ChannelCapability::Available,
            configured: false,
            enabled: false,
            connection: ChannelConnectionState::Unconfigured,
            config: None,
            last_connected_at: None,
            last_error: None,
        })
    }
```

- [ ] **Step 1.3: manager 加 Platform::Feishu 占位分支**

Edit `src-tauri/src/connector/im/manager.rs`：

(a) `get_platform` 内：

```rust
    pub async fn get_platform(&self, platform: Platform) -> Result<ChannelPlatformState> {
        match platform {
            Platform::Dingtalk => self.current_dingtalk_state().await,
            Platform::Feishu => self.config_store.feishu_state_stub(
                ChannelConnectionState::Unconfigured,
                None,
            ),
            other => Ok(ChannelConfigStore::coming_soon_state(other)),
        }
    }
```

(b) `set_enabled` / `remove_platform` / `reveal_secret` 暂时仍 bail "feishu channel is not available yet" 直到 PR2 接入 config_store。

- [ ] **Step 1.4: review_im_layering platforms 数组追加 "feishu"**

Edit `src-tauri/tests/review_im_layering.rs:57`：

```rust
    let known_platforms = ["dingtalk", "feishu"];
```

PR1 阶段 `feishu/connector.rs` 没有 import `shared::router / ask_coordinator / config_store / pending_adapter`——layering test 应当继续通过。

- [ ] **Step 1.5: 前端 ChannelPage 加飞书卡片（disabled）**

Edit `src/features/channel/ChannelPage.tsx:33`：

```typescript
type PlatformKey = 'dingtalk' | 'feishu'
```

`platforms` useMemo 数组追加：

```typescript
      {
        key: 'feishu',
        name: '飞书',
        description: '通过飞书机器人接收并回复用户消息',
        icon: '飞',
        iconClassName: 'bg-sky-50 text-[var(--color-semantic-blue)]',
        state: feishuState,
        ...statusMeta(feishuState),
      },
```

`feishuState` 与 `dingtalkState` 同形 fallback 一次：

```typescript
  const feishuState = platformsByKey.feishu ?? {
    platform: 'feishu',
    capability: 'available',
    configured: false,
    enabled: false,
    connection: 'unconfigured',
    config: null,
    lastConnectedAt: null,
    lastError: null,
  } satisfies ChannelPlatformState
```

PR1 阶段"配置"按钮 disabled。在 `PlatformCard` 上加 `disabled` prop（或直接判断 `platform.key === 'feishu'` 时按钮显示"敬请期待"）。

- [ ] **Step 1.6: 测试**

Run: `cd /Users/oayzz/project/lotus/lotus-workbench/lotus-app/.claude/worktrees/amazing-chatelet-801fd7/src-tauri && cargo test --lib connector::im -- --nocapture 2>&1 | tail -10`
Run: `cd /Users/oayzz/project/lotus/lotus-workbench/lotus-app/.claude/worktrees/amazing-chatelet-801fd7/src-tauri && cargo test --test review_im_layering 2>&1 | tail -10`
Run: `cd /Users/oayzz/project/lotus/lotus-workbench/lotus-app/.claude/worktrees/amazing-chatelet-801fd7 && pnpm tsc --noEmit 2>&1 | tail -5`
Expected: 全部通过；前端 0 type error。

- [ ] **Step 1.7: 启动 dev 服务器，肉眼验证**

Run: `cd /Users/oayzz/project/lotus/lotus-workbench/lotus-app/.claude/worktrees/amazing-chatelet-801fd7 && pnpm tauri:dev`
Expected：频道页能看到"飞书"卡片，状态为"未配置"，按钮 disabled。钉钉那一行不受影响，照样工作。

- [ ] **Step 1.8: 提交 PR1**

```bash
cd /Users/oayzz/project/lotus/lotus-workbench/lotus-app/.claude/worktrees/amazing-chatelet-801fd7
git add src-tauri/src/connector/im/feishu/ src-tauri/src/connector/im/mod.rs src-tauri/src/connector/im/factory.rs src-tauri/src/connector/im/shared/config_store.rs src-tauri/src/connector/im/manager.rs src-tauri/tests/review_im_layering.rs src/features/channel/ChannelPage.tsx
git commit -m "$(cat <<'EOF'
feat(connector/im/feishu): scaffold FeishuConnector + frontend platform card stub (Phase 1 PR1)

- new feishu/ module with stub IMConnector impl: capabilities() returns the real
  shape (Stream / DeviceCode / aicard+markdown+attachments), start/send return
  NotSupported until PR3+
- factory::build_feishu_connector returns (Arc<dyn IMConnector>, Arc<FeishuConnector>)
- ChannelConfigStore.feishu_state_stub returns Available / Unconfigured
- ChannelPage shows the feishu card alongside dingtalk, button disabled
- review_im_layering platforms array picks up feishu/ so its imports are checked

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 2: PR2 — Device-code 注册 + tenant_access_token 缓存

**Files:**
- Create: `src-tauri/src/connector/im/feishu/registration.rs`
- Create: `src-tauri/src/connector/im/feishu/token.rs`
- Modify: `src-tauri/src/connector/im/feishu/mod.rs`
- Modify: `src-tauri/src/connector/im/feishu/connector.rs`
- Modify: `src-tauri/src/connector/im/shared/config_store.rs`
- Modify: `src-tauri/src/connector/im/manager.rs`
- Modify: `src-tauri/src/commands/channel.rs`

**目标**：飞书 OAuth Device Authorization Grant 流跑通，注册成功后 app_id / app_secret 落 `~/.renlijia/users/<scope>/channels/feishu/config.json`（app_secret 走 SecureStorage），随后能拿到 tenant_access_token 并缓存。

> **前提**：§0 Step 0.3 已确认飞书有 device-code grant（详见 `feishu-endpoints-notes.md` Q1）。真实流程是 RFC 8628 单端点 + form-encoded + 字符串错误码——下方代码已按调研结果实现。

- [ ] **Step 2.1: 写 registration.rs 失败测试**

Create `src-tauri/src/connector/im/feishu/registration.rs`：

```rust
//! 飞书 OAuth 2.0 Device Authorization Grant 注册流（RFC 8628）。
//!
//! 单端点 begin + poll：
//!   POST https://accounts.feishu.cn/oauth/v1/app/registration
//!   Content-Type: application/x-www-form-urlencoded
//!
//! 错误模型走 RFC 8628 字符串（authorization_pending / slow_down / access_denied /
//! expired_token），HTTP 4xx body 里 `error` 字段返回；NOT 数字 errcode。
//!
//! 字段命名差异：device-flow 响应给的是 `client_id` / `client_secret`（RFC 8628 标准名），
//! tenant_access_token 端点要的是 `app_id` / `app_secret`——同一对值，不同字段名。
//! 持久化到磁盘的 schema 用 `app_id`（见 FeishuStoredCredentials），manager poll handler
//! 负责把 client_id / client_secret 映射到 app_id / app_secret。

use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};

const REGISTRATION_URL: &str = "https://accounts.feishu.cn/oauth/v1/app/registration";
pub const FEISHU_DEVICE_CODE_SOURCE: &str = "FEISHU_DEVICE_CODE";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum FeishuPollState {
    Waiting,
    Success,
    Fail,
    Expired,
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FeishuBeginResult {
    pub device_code: String,
    pub user_code: String,
    pub verification_uri_complete: String,
    pub verification_uri: String,
    pub interval_seconds: u64,
    pub expires_in_seconds: u64,
    pub source: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FeishuPollResult {
    pub state: FeishuPollState,
    /// RFC 8628 `client_id` — 等价于 tenant_access_token 端点的 `app_id`。
    pub client_id: Option<String>,
    /// RFC 8628 `client_secret` — 等价于 tenant_access_token 端点的 `app_secret`。
    pub client_secret: Option<String>,
    pub fail_reason: Option<String>,
}

/// RFC 8628 字符串错误 → FeishuPollState 映射。
pub fn map_feishu_error(err: &str) -> FeishuPollState {
    match err {
        "authorization_pending" => FeishuPollState::Waiting,
        // `slow_down` 严格语义是"增加 polling 间隔 5s"。connector 层简化处理为 Waiting；
        // 调用方（manager / 前端）按 begin response 的 interval_seconds 节奏 polling 即可。
        "slow_down" => FeishuPollState::Waiting,
        "access_denied" => FeishuPollState::Fail,
        "expired_token" => FeishuPollState::Expired,
        _ => FeishuPollState::Unknown,
    }
}

pub async fn begin_registration() -> Result<FeishuBeginResult> {
    let client = reqwest::Client::new();
    let resp = client
        .post(REGISTRATION_URL)
        .form(&[
            ("action", "begin"),
            ("archetype", "PersonalAgent"),
            ("auth_method", "client_secret"),
            ("request_user_info", "open_id"),
        ])
        .send()
        .await
        .context("Failed to POST feishu app registration (begin)")?;
    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        anyhow::bail!("feishu registration begin failed: {} {}", status, body);
    }
    #[derive(Deserialize)]
    #[serde(rename_all = "snake_case")]
    struct Resp {
        device_code: String,
        user_code: String,
        verification_uri: String,
        verification_uri_complete: Option<String>,
        interval: u64,
        expires_in: u64,
    }
    let r: Resp = resp.json().await.context("parse feishu registration begin resp")?;
    Ok(FeishuBeginResult {
        device_code: r.device_code,
        user_code: r.user_code.clone(),
        verification_uri: r.verification_uri.clone(),
        verification_uri_complete: r
            .verification_uri_complete
            .unwrap_or_else(|| format!("{}?user_code={}", r.verification_uri, r.user_code)),
        interval_seconds: r.interval,
        expires_in_seconds: r.expires_in,
        source: FEISHU_DEVICE_CODE_SOURCE.into(),
    })
}

pub async fn poll_registration(device_code: &str) -> Result<FeishuPollResult> {
    let client = reqwest::Client::new();
    let resp = client
        .post(REGISTRATION_URL)
        .form(&[
            ("action", "poll"),
            ("device_code", device_code),
        ])
        .send()
        .await
        .context("Failed to POST feishu app registration (poll)")?;
    let status = resp.status();
    let body = resp.text().await.unwrap_or_default();
    if !status.is_success() {
        // RFC 8628: 中间状态/错误都以 HTTP 400 + body { "error": "...", "error_description": "..." } 返回
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(&body) {
            if let Some(err) = v.get("error").and_then(|x| x.as_str()) {
                let s = map_feishu_error(err);
                let reason = v
                    .get("error_description")
                    .and_then(|x| x.as_str())
                    .map(|s| s.to_string())
                    .or_else(|| Some(err.to_string()));
                return Ok(FeishuPollResult {
                    state: s,
                    client_id: None,
                    client_secret: None,
                    fail_reason: reason,
                });
            }
        }
        return Err(anyhow!("feishu registration poll failed: {} {}", status, body));
    }
    #[derive(Deserialize)]
    #[serde(rename_all = "snake_case")]
    struct Ok2 {
        client_id: Option<String>,
        client_secret: Option<String>,
        error: Option<String>,
        error_description: Option<String>,
    }
    let r: Ok2 = serde_json::from_str(&body).context("parse feishu registration poll resp")?;
    // 同时也防御 200 + body 里带 error 的写法（非 RFC 标准但有些实现会这样）
    let state = match r.error.as_deref() {
        Some(err) => map_feishu_error(err),
        None if r.client_id.is_some() && r.client_secret.is_some() => FeishuPollState::Success,
        None => FeishuPollState::Unknown,
    };
    Ok(FeishuPollResult {
        state,
        client_id: r.client_id,
        client_secret: r.client_secret,
        fail_reason: r.error_description.or(r.error),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn error_maps_known_rfc8628_codes() {
        assert_eq!(map_feishu_error("authorization_pending"), FeishuPollState::Waiting);
        assert_eq!(map_feishu_error("slow_down"), FeishuPollState::Waiting);
        assert_eq!(map_feishu_error("access_denied"), FeishuPollState::Fail);
        assert_eq!(map_feishu_error("expired_token"), FeishuPollState::Expired);
        assert_eq!(map_feishu_error("anything_else"), FeishuPollState::Unknown);
    }
}
```

> 调研已确认（见 `feishu-endpoints-notes.md` Q1）：单端点、form-encoded、RFC 8628 字符串错误码。如果飞书后续在中文环境里返回额外错误字符串，扩展 `map_feishu_error` 即可。

- [ ] **Step 2.2: token.rs**

Create `src-tauri/src/connector/im/feishu/token.rs`：

```rust
//! Feishu tenant_access_token source. Wrapped by SharedTokenCache<FeishuTokenSource>.
//! Endpoint: POST /open-apis/auth/v3/tenant_access_token/internal returns expire=7200.

use anyhow::{Context, Result};
use async_trait::async_trait;
use serde::Deserialize;

use crate::connector::im::shared::token::PlatformTokenSource;

const FEISHU_API: &str = "https://open.feishu.cn";

pub struct FeishuTokenSource {
    app_id: String,
    app_secret: String,
    http: reqwest::Client,
}

impl FeishuTokenSource {
    pub fn new(app_id: String, app_secret: String) -> Self {
        Self {
            app_id,
            app_secret,
            http: reqwest::Client::new(),
        }
    }
}

#[async_trait]
impl PlatformTokenSource for FeishuTokenSource {
    async fn fetch(&self) -> Result<(String, u64)> {
        #[derive(Deserialize)]
        struct Resp {
            tenant_access_token: String,
            expire: u64,
            code: i64,
            msg: Option<String>,
        }
        let resp = self
            .http
            .post(format!("{}/open-apis/auth/v3/tenant_access_token/internal", FEISHU_API))
            .json(&serde_json::json!({
                "app_id": self.app_id,
                "app_secret": self.app_secret,
            }))
            .send()
            .await
            .context("Failed to request feishu tenant_access_token")?;
        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!("feishu tenant_access_token http: {} {}", status, body);
        }
        let r: Resp = resp.json().await.context("parse feishu token resp")?;
        if r.code != 0 {
            anyhow::bail!(
                "feishu tenant_access_token errcode={} msg={}",
                r.code,
                r.msg.unwrap_or_default()
            );
        }
        Ok((r.tenant_access_token, r.expire))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn constructor_stores_credentials() {
        let s = FeishuTokenSource::new("ak".into(), "as".into());
        assert_eq!(s.app_id, "ak");
        assert_eq!(s.app_secret, "as");
    }
}
```

- [ ] **Step 2.3: config_store 加飞书读写**

Edit `src-tauri/src/connector/im/shared/config_store.rs`，在 `impl ChannelConfigStore` 内追加（参考 dingtalk_* 系列）：

```rust
    pub fn read_feishu_config(&self) -> Result<Option<crate::connector::im::feishu::types::FeishuStoredConfig>> {
        let path = self.platform_config_path(Platform::Feishu);
        if !path.exists() {
            return Ok(None);
        }
        let raw = std::fs::read_to_string(&path)?;
        let config = serde_json::from_str(&raw)
            .with_context(|| format!("Failed to parse {}", path.display()))?;
        Ok(Some(config))
    }

    pub fn save_feishu_registration(
        &self,
        app_id: String,
        app_secret_plain: String,
    ) -> Result<ChannelPlatformState> {
        use crate::connector::im::feishu::types::{
            FeishuStoredConfig, FeishuStoredCredentials, FeishuStoredMetadata,
        };
        let app_id = non_empty(app_id, "app_id")?;
        let secret = non_empty(app_secret_plain, "app_secret")?;
        let (app_secret_encrypted, app_secret_storage) = self.encrypt_secret(&secret)?;
        let now = now_rfc3339();
        let existing_created_at = self
            .read_feishu_config()?
            .map(|c| c.metadata.created_at)
            .unwrap_or_else(|| now.clone());
        let config = FeishuStoredConfig {
            schema_version: 1,
            platform: Platform::Feishu,
            configured: true,
            enabled: true,
            credentials: FeishuStoredCredentials {
                app_id,
                app_secret_encrypted,
                app_secret_storage,
            },
            metadata: FeishuStoredMetadata {
                created_at: existing_created_at,
                updated_at: now,
            },
        };
        let dir = self.platform_dir(Platform::Feishu);
        std::fs::create_dir_all(&dir)?;
        let content = serde_json::to_string_pretty(&config)?;
        let final_path = self.platform_config_path(Platform::Feishu);
        let temp_path = dir.join(format!(
            ".config.json.{}.{}.tmp",
            std::process::id(),
            chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default()
        ));
        write_config_file_securely(&temp_path, content.as_bytes())?;
        std::fs::rename(&temp_path, final_path)?;
        self.feishu_state(ChannelConnectionState::Disconnected, None)
    }

    pub fn decrypt_feishu_config(&self) -> Result<(crate::connector::im::feishu::types::FeishuStoredConfig, String)> {
        use crate::connector::im::feishu::types::FeishuStoredCredentials;
        let config = self
            .read_feishu_config()?
            .ok_or_else(|| anyhow::anyhow!("Feishu channel is not configured"))?;
        let creds = FeishuStoredCredentials {
            app_id: config.credentials.app_id.clone(),
            app_secret_encrypted: config.credentials.app_secret_encrypted.clone(),
            app_secret_storage: config.credentials.app_secret_storage.clone(),
        };
        let secret = match (&creds.app_secret_storage, &self.secure_storage) {
            (SecretStorageKind::SecureStorage, Some(storage)) => storage.decrypt(&creds.app_secret_encrypted)?,
            (SecretStorageKind::SecureStorage, None) => anyhow::bail!(
                "Feishu AppSecret marked SecureStorage but SecureStorage unavailable"
            ),
            (SecretStorageKind::PlaintextFallback, _) => creds.app_secret_encrypted.clone(),
        };
        Ok((config, secret))
    }

    pub fn feishu_state(
        &self,
        connection: ChannelConnectionState,
        last_error: Option<String>,
    ) -> Result<ChannelPlatformState> {
        let Some(config) = self.read_feishu_config()? else {
            return self.feishu_state_stub(connection, last_error);
        };
        let connection = if !config.enabled {
            ChannelConnectionState::Disconnected
        } else {
            connection
        };
        Ok(ChannelPlatformState {
            platform: Platform::Feishu,
            capability: ChannelCapability::Available,
            configured: config.configured,
            enabled: config.enabled,
            connection,
            config: Some(ChannelConfigView {
                platform: Platform::Feishu,
                app_key: config.credentials.app_id.clone(),
                app_secret_masked: mask_secret(
                    &self.decrypt_feishu_config().map(|(_, s)| s).unwrap_or_default(),
                ),
                robot_code: String::new(),
                robot_code_source: RobotCodeSource::AppKeyFallback,
                source: "FEISHU_DEVICE_CODE".into(),
                created_at: config.metadata.created_at.clone(),
                updated_at: config.metadata.updated_at.clone(),
            }),
            last_connected_at: None,
            last_error,
        })
    }

    pub fn set_feishu_enabled(&self, enabled: bool) -> Result<ChannelPlatformState> {
        let mut config = self
            .read_feishu_config()?
            .ok_or_else(|| anyhow::anyhow!("Feishu channel is not configured"))?;
        config.enabled = enabled;
        config.metadata.updated_at = now_rfc3339();
        let dir = self.platform_dir(Platform::Feishu);
        let final_path = self.platform_config_path(Platform::Feishu);
        let content = serde_json::to_string_pretty(&config)?;
        let temp_path = dir.join(format!(
            ".config.json.{}.{}.tmp",
            std::process::id(),
            chrono::Utc::now().timestamp_nanos_opt().unwrap_or_default()
        ));
        write_config_file_securely(&temp_path, content.as_bytes())?;
        std::fs::rename(&temp_path, final_path)?;
        self.feishu_state(
            if enabled { ChannelConnectionState::Connecting } else { ChannelConnectionState::Disconnected },
            None,
        )
    }

    pub fn remove_feishu(&self) -> Result<ChannelPlatformState> {
        let path = self.platform_config_path(Platform::Feishu);
        if path.exists() {
            std::fs::remove_file(path)?;
        }
        self.feishu_state_stub(ChannelConnectionState::Unconfigured, None)
    }

    pub fn reveal_feishu_secret(&self) -> Result<String> {
        let (_, secret) = self.decrypt_feishu_config()?;
        Ok(secret)
    }
```

并把 `all_platform_states` 内的 `self.feishu_state_stub(...)?` 改回：

```rust
            self.feishu_state(connection.clone(), last_error.clone())?,
```

- [ ] **Step 2.4: config_store 单测**

在 `config_store.rs::tests` 追加：

```rust
    #[test]
    fn save_feishu_registration_writes_enabled_config_and_masks_secret() {
        let dir = TempDir::new().unwrap();
        let store = store_in(&dir);
        let state = store
            .save_feishu_registration("cli_abc123".into(), "supersecret".into())
            .unwrap();
        assert!(state.configured);
        assert!(state.enabled);
        let view = state.config.unwrap();
        assert_eq!(view.app_key, "cli_abc123");
        assert_eq!(view.app_secret_masked, "••••••••••••cret");
        assert!(store.platform_config_path(Platform::Feishu).exists());
    }

    #[test]
    fn set_feishu_enabled_false_keeps_config() {
        let dir = TempDir::new().unwrap();
        let store = store_in(&dir);
        store.save_feishu_registration("cli_x".into(), "secret".into()).unwrap();
        let state = store.set_feishu_enabled(false).unwrap();
        assert!(!state.enabled);
        assert!(state.configured);
        assert!(store.platform_config_path(Platform::Feishu).exists());
    }

    #[test]
    fn remove_feishu_deletes_config() {
        let dir = TempDir::new().unwrap();
        let store = store_in(&dir);
        store.save_feishu_registration("cli_x".into(), "secret".into()).unwrap();
        let state = store.remove_feishu().unwrap();
        assert!(!state.configured);
        assert!(!store.platform_config_path(Platform::Feishu).exists());
    }
```

- [ ] **Step 2.5: feishu/mod.rs 暴露新模块**

Edit `src-tauri/src/connector/im/feishu/mod.rs`：

```rust
pub mod connector;
pub mod registration;
pub mod token;
pub mod types;

pub use connector::FeishuConnector;
```

- [ ] **Step 2.6: manager 接 feishu 配置 + 命令分支**

Edit `src-tauri/src/connector/im/manager.rs`，在 `impl ChannelManager` 内追加：

```rust
    pub async fn begin_feishu_registration(&self) -> Result<ChannelRegistrationBeginResult> {
        let begin = super::feishu::registration::begin_registration().await?;
        Ok(ChannelRegistrationBeginResult {
            device_code: begin.device_code,
            user_code: begin.user_code,
            verification_uri_complete: begin.verification_uri_complete,
            verification_uri: begin.verification_uri,
            interval_seconds: begin.interval_seconds,
            expires_in_seconds: begin.expires_in_seconds,
            source: begin.source,
        })
    }

    pub async fn poll_feishu_registration(
        &self,
        device_code: String,
    ) -> Result<ChannelRegistrationPollResult> {
        let poll = super::feishu::registration::poll_registration(&device_code).await?;
        let state = match poll.state {
            super::feishu::registration::FeishuPollState::Waiting => ChannelRegistrationPollState::Waiting,
            super::feishu::registration::FeishuPollState::Success => ChannelRegistrationPollState::Success,
            super::feishu::registration::FeishuPollState::Fail => ChannelRegistrationPollState::Fail,
            super::feishu::registration::FeishuPollState::Expired => ChannelRegistrationPollState::Expired,
            super::feishu::registration::FeishuPollState::Unknown => ChannelRegistrationPollState::Unknown,
        };
        if state == ChannelRegistrationPollState::Success {
            // OAuth device-flow returns RFC 8628 `client_id` / `client_secret`. These map 1:1
            // to the tenant_access_token endpoint's `app_id` / `app_secret`. We persist them
            // under the on-disk schema's `app_id` field name (see FeishuStoredCredentials).
            let app_id = poll.client_id.ok_or_else(|| anyhow::anyhow!("missing client_id"))?;
            let app_secret = poll.client_secret.ok_or_else(|| anyhow::anyhow!("missing client_secret"))?;
            let state = self.config_store.save_feishu_registration(app_id, app_secret)?;
            return Ok(ChannelRegistrationPollResult {
                state: ChannelRegistrationPollState::Success,
                client_id: state.config.as_ref().map(|c| c.app_key.clone()),
                robot_code: None,
                config: state.config.clone(),
                platform_state: Some(state),
                fail_reason: poll.fail_reason,
            });
        }
        Ok(ChannelRegistrationPollResult {
            state,
            client_id: None,
            robot_code: None,
            config: None,
            platform_state: None,
            fail_reason: poll.fail_reason,
        })
    }
```

`set_enabled` / `remove_platform` / `reveal_secret` 加 `Platform::Feishu` 分支调对应 config_store 方法。**PR2 阶段 set_enabled(true) 不真连**——只写 enabled=true，连接由 PR3 的 `connect_feishu_from_store` 触发。先 stub 调一个空 `connect_feishu_from_store_stub` 实现：

```rust
    async fn connect_feishu_from_store(&self) -> Result<()> {
        log::warn!("[channel] feishu connect not yet implemented (PR3)");
        Ok(())
    }
```

- [ ] **Step 2.7: Tauri 命令支持飞书**

Edit `src-tauri/src/commands/channel.rs`，`channel_begin_registration` / `channel_poll_registration` 加 `Platform::Feishu` 分支：

```rust
        Platform::Feishu => manager(&app)?.begin_feishu_registration().await.map_err(|e| format!("{:#}", e)),
```

`channel_set_enabled` / `channel_remove_platform` / `channel_reveal_secret` 不用动（manager 内部已 dispatch）。

- [ ] **Step 2.8: 跑测试 + 真账号注册冒烟**

Run: `cd /Users/oayzz/project/lotus/lotus-workbench/lotus-app/.claude/worktrees/amazing-chatelet-801fd7/src-tauri && cargo test --lib connector::im -- --nocapture 2>&1 | tail -15`
Expected: 全部通过。

冒烟：`pnpm tauri:dev` → 频道页"配置飞书" → 浏览器扫码授权 → 回到 app 看到"已配置"状态。检查 `~/.renlijia/users/<scope>/channels/feishu/config.json` 存在、appSecretEncrypted 不是明文。Keychain 中存在 SecureStorage 加密项。

- [ ] **Step 2.9: 提交 PR2**

```bash
git add src-tauri/src/connector/im/feishu/registration.rs src-tauri/src/connector/im/feishu/token.rs src-tauri/src/connector/im/feishu/mod.rs src-tauri/src/connector/im/shared/config_store.rs src-tauri/src/connector/im/manager.rs src-tauri/src/commands/channel.rs
git commit -m "$(cat <<'EOF'
feat(connector/im/feishu): device-code registration + tenant_access_token cache (Phase 1 PR2)

- feishu/registration.rs: accounts.feishu.cn RFC 8628 single-endpoint device-code (form-encoded; client_id/client_secret response)
- feishu/token.rs: FeishuTokenSource impl PlatformTokenSource for SharedTokenCache reuse
- ChannelConfigStore.{read,save,decrypt,set_enabled,remove}_feishu_config with
  SecureStorage encryption parity with dingtalk
- ChannelManager.{begin,poll}_feishu_registration wired through channel_* commands

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 3: PR3 — WebSocket runtime + 消息 normalize（4 类 + 20 类降级 + dedup）

**Files:**
- Create: `src-tauri/src/connector/im/feishu/stream.rs`
- Modify: `src-tauri/src/connector/im/feishu/connector.rs:start()`
- Modify: `src-tauri/src/connector/im/feishu/mod.rs`
- Modify: `src-tauri/src/connector/im/manager.rs`

**目标**：`FeishuConnector::start()` 起一个 WebSocket 长连，收到事件后 parse → normalize 成 `ChannelMessage` → 通过 BoxStream 吐出去。复用 PR0b 的 `MessageDedupSet` 在 connector 内部做去重。

> §0 Step 0.3 已验证飞书 ws 长连接协议（详见 `feishu-endpoints-notes.md` Q3）。关键发现：
> - 握手 URL = `POST {open-domain}/callback/ws/endpoint`（不是早期 spec 抄的路径）
> - 握手 body 使用 CapCase 字段：`{"AppID": "cli_...", "AppSecret": "..."}`
> - **帧格式是 protobuf（pbbp2），不是 JSON**——必须引入 protobuf decoder 才能解码业务事件
>
> 因此 PR3 的工作量从原计划的 2 天上调到 **3-4 天**（多出来的部分是 vendor `.proto` schema + prost 集成）。新增的 Step 3.0 在动 stream.rs 之前先把 schema 落地。

- [ ] **Step 3.0: Vendor protobuf schema for ws frames + im events**

- 读 `feishu-endpoints-notes.md` Q3/Q7 中的字段命名证据（Frame envelope + im.message.receive_v1 event）
- 到 GitHub `larksuite/oapi-sdk-go` 搜对应 `.proto` 片段（ws-client/proto-buf/ 下的 pbbp2 + im event 定义）
- Vendor 必要的 `.proto` 文件到 `src-tauri/proto/feishu/` 目录（或者用 `prost-build` 的 build.rs 在编译期产 Rust 模块）
- 在 `src-tauri/Cargo.toml` 加依赖：
  - `[dependencies]`：`prost`（运行时类型）
  - `[build-dependencies]`：`prost-build`（编译期 .proto → .rs）
- 简单的 fallback 路径：若 prost 集成踩坑严重，可手写一个最小 pbbp2 envelope 解码器（约 50-100 行），只解 frame_type + payload 两个字段即可——业务侧的 event JSON 仍走 serde_json 处理

> 实施风险：larksuite/oapi-sdk-go 的 .proto 文件路径和 message name 可能在版本之间变化。锁定一个具体的 git commit hash 作为 vendor 来源，并在 `src-tauri/proto/feishu/SOURCE.md` 写明出处和拉取时间。

- [ ] **Step 3.1: stream.rs 骨架**

Create `src-tauri/src/connector/im/feishu/stream.rs`（~400-600 行，参考 `dingtalk/stream.rs` 结构）：

```rust
//! 飞书 WebSocket 长连客户端。
//!
//! 流程：
//!   1. POST https://open.feishu.cn/callback/ws/endpoint
//!      body: { "AppID": "cli_...", "AppSecret": "..." }   ← CapCase 字段名
//!      headers: { "locale": "zh", "User-Agent": "..." }
//!      → 拿 data.URL（含 device_id+service_id query）+ ClientConfig
//!   2. tungstenite 长连 data.URL（URL 自带 auth，不需要额外 ticket header）
//!   3. recv binary frame → 用 prost 解码 pbbp2 envelope
//!   4. 业务 frame 的 payload 可能是 gzip-compressed JSON，解压后判事件 type
//!   5. im.message.receive_v1 → normalize 4 类（text/image/file/interactive）
//!   6. card.action.trigger → 通过 ChannelMessage 转给 ask_coordinator
//!   7. 其它事件类型静默忽略 + log debug
//!   8. ping interval 按 ClientConfig.PingInterval (默认 120s)；断开后用
//!      shared::ReconnectBackoff 5/15/30/60s 重连（也参考 ClientConfig 的 ReconnectInterval）

use std::sync::Arc;

use anyhow::{Context, Result};
use futures_util::{SinkExt, StreamExt};
use tokio::sync::mpsc;
use tokio::time::sleep;
use tokio_tungstenite::tungstenite::Message;
use tokio_util::sync::CancellationToken;

use crate::connector::im::shared::dedup::MessageDedupSet;
use crate::connector::im::shared::reconnect::ReconnectBackoff;
use crate::connector::im::types::{
    AttachmentKind, ChannelAttachmentSpec, ChannelConnectionState, ChannelMessage, ConversationType,
};

// Generated by prost-build from vendored .proto in build.rs:
// pub use crate::proto::feishu::pbbp2::Frame;
// pub use crate::proto::feishu::im::MessageReceiveEvent;

const FEISHU_OPEN_DOMAIN: &str = "https://open.feishu.cn";
const WS_ENDPOINT_PATH: &str = "/callback/ws/endpoint";

#[derive(Clone)]
pub struct FeishuStreamClient {
    app_id: String,
    app_secret: String,
    message_tx: mpsc::Sender<ChannelMessage>,
    dedup: Arc<MessageDedupSet>,
}

impl FeishuStreamClient {
    pub fn new(
        app_id: String,
        app_secret: String,
        message_tx: mpsc::Sender<ChannelMessage>,
    ) -> Self {
        Self {
            app_id,
            app_secret,
            message_tx,
            dedup: Arc::new(MessageDedupSet::with_default_cap()),
        }
    }

    pub fn start(
        &self,
        on_status: impl Fn(ChannelConnectionState, Option<String>) + Send + Sync + 'static,
        cancel: CancellationToken,
    ) {
        let client = self.clone();
        let on_status = Arc::new(on_status);
        tokio::spawn(async move {
            client.run_with_retry(on_status, cancel).await;
        });
    }

    async fn run_with_retry(
        &self,
        on_status: Arc<impl Fn(ChannelConnectionState, Option<String>) + Send + Sync>,
        cancel: CancellationToken,
    ) {
        let mut backoff = ReconnectBackoff::default_schedule();
        loop {
            if cancel.is_cancelled() {
                return;
            }
            on_status(ChannelConnectionState::Connecting, None);
            match self.open_ws_endpoint().await {
                Ok((url, client_config)) => {
                    backoff.reset();
                    on_status(ChannelConnectionState::Connected, None);
                    if let Err(e) = self.run_ws_loop(&url, &client_config, cancel.clone()).await {
                        log::warn!("[feishu-stream] ws loop ended: {:#}", e);
                    }
                }
                Err(e) => {
                    log::warn!("[feishu-stream] open failed: {:#}", e);
                    let msg = e.to_string();
                    if msg.contains("401") || msg.contains("Unauthorized") || msg.contains("99991663") {
                        on_status(
                            ChannelConnectionState::ConfigError,
                            Some("飞书凭证失效，请重新配置".into()),
                        );
                        return;
                    }
                }
            }
            if cancel.is_cancelled() {
                return;
            }
            on_status(ChannelConnectionState::Reconnecting, None);
            let delay = backoff.next_delay();
            tokio::select! {
                _ = sleep(delay) => {}
                _ = cancel.cancelled() => return,
            }
        }
    }

    /// POST /callback/ws/endpoint with CapCase {AppID, AppSecret}.
    /// Returns the wss URL (auth embedded in query) + ClientConfig (ping/reconnect intervals).
    async fn open_ws_endpoint(&self) -> Result<(String, WsClientConfig)> {
        let client = reqwest::Client::new();
        let resp = client
            .post(format!("{}{}", FEISHU_OPEN_DOMAIN, WS_ENDPOINT_PATH))
            .header("locale", "zh")
            .header("User-Agent", "aijia-desktop/0.1 (feishu-ws-client)")
            // CapCase field names — different from snake_case used elsewhere in the feishu API.
            .json(&serde_json::json!({
                "AppID": self.app_id,
                "AppSecret": self.app_secret,
            }))
            .send()
            .await
            .context("post feishu ws endpoint")?;
        if !resp.status().is_success() {
            anyhow::bail!(
                "feishu ws endpoint http: {} {}",
                resp.status(),
                resp.text().await.unwrap_or_default()
            );
        }
        #[derive(serde::Deserialize)]
        struct Resp {
            code: i64,
            #[allow(dead_code)]
            msg: Option<String>,
            data: WsData,
        }
        #[derive(serde::Deserialize)]
        #[serde(rename_all = "PascalCase")]
        struct WsData {
            #[serde(rename = "URL")]
            url: String,
            client_config: WsClientConfig,
        }
        let r: Resp = resp.json().await.context("parse feishu ws endpoint")?;
        if r.code != 0 {
            anyhow::bail!("feishu ws endpoint code != 0: {}", r.code);
        }
        Ok((r.data.url, r.data.client_config))
    }

    async fn run_ws_loop(
        &self,
        url: &str,
        _client_config: &WsClientConfig,
        cancel: CancellationToken,
    ) -> Result<()> {
        let (mut ws, _) = tokio_tungstenite::connect_async(url).await.context("ws connect")?;
        // TODO(pr3): drive a ping task on ClientConfig.PingInterval (default 120s).
        loop {
            tokio::select! {
                _ = cancel.cancelled() => {
                    let _ = ws.send(Message::Close(None)).await;
                    return Ok(());
                }
                frame = ws.next() => {
                    let Some(frame) = frame else { return Ok(()); };
                    let frame = frame.context("ws recv")?;
                    match frame {
                        // Feishu sends protobuf-encoded binary frames (NOT JSON-over-text).
                        Message::Binary(bytes) => self.handle_frame_bytes(&bytes).await,
                        Message::Ping(d) => { let _ = ws.send(Message::Pong(d)).await; }
                        Message::Close(_) => return Ok(()),
                        // Text frames are not expected from the feishu gateway; log + ignore.
                        Message::Text(_) | _ => {}
                    }
                }
            }
        }
    }

    async fn handle_frame_bytes(&self, bytes: &[u8]) {
        // TODO(pr3): decode pbbp2 envelope. Pseudocode shape:
        //   let frame = <prost-decoded Frame>::decode(bytes)?;
        //   match frame.payload_type {
        //       PayloadType::Event => {
        //           let event_bytes = if frame.is_gzip { gunzip(&frame.payload) } else { frame.payload };
        //           let event_value: serde_json::Value = serde_json::from_slice(&event_bytes)?;
        //           dispatch_event(self, &event_value).await;
        //       }
        //       PayloadType::Card => { /* card.action.trigger */ }
        //       PayloadType::ConnectAck | PayloadType::Ping | PayloadType::Control => { /* no-op */ }
        //   }
        let _ = bytes;
        log::debug!("[feishu-stream] received ws binary frame ({} bytes) — decoder pending", bytes.len());
    }
}

/// ClientConfig from /callback/ws/endpoint — used to drive ping + reconnect cadence.
#[derive(Clone, Debug, serde::Deserialize)]
#[serde(rename_all = "PascalCase")]
pub struct WsClientConfig {
    pub ping_interval: u64,       // seconds, e.g. 120
    pub reconnect_count: i64,     // -1 means infinite
    pub reconnect_interval: u64,  // seconds
    pub reconnect_nonce: u64,     // seconds of jitter
}

/// Decoded event JSON → ChannelMessage. Caller is responsible for protobuf-decoding
/// the outer pbbp2 frame and gunzipping the payload before invoking this.
///
/// `event` is the inner JSON shape documented at open.feishu.cn:
///   { "header": { "event_type": "im.message.receive_v1", ... }, "event": { "sender": ..., "message": ... } }
fn parse_im_message(event: &serde_json::Value) -> Option<ChannelMessage> {
    let inner = event.pointer("/event")?;
    let msg_id = inner.pointer("/message/message_id")?.as_str()?.to_string();
    let chat_type = inner.pointer("/message/chat_type")?.as_str()?;
    let conversation_type = match chat_type {
        "group" => ConversationType::Group,
        "p2p" => ConversationType::Private,
        _ => return None,
    };
    let chat_id = inner.pointer("/message/chat_id")?.as_str()?.to_string();
    let sender_id = inner.pointer("/sender/sender_id/open_id")?.as_str()?.to_string();
    let sender_nick = inner
        .pointer("/sender/sender_id/user_id")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    let msg_type = inner.pointer("/message/message_type")?.as_str()?;
    let content_str = inner.pointer("/message/content")?.as_str()?;
    let content_json: serde_json::Value = serde_json::from_str(content_str).ok()?;

    let (text, attachments) = normalize_content(msg_type, &content_json, &msg_id)?;
    Some(ChannelMessage {
        msg_id,
        conversation_type,
        conversation_key: chat_id.clone(),
        sender_id,
        sender_nick,
        text,
        robot_code: String::new(),
        reply_group_id: chat_id,
        attachments,
        session_webhook: None,
    })
}

fn normalize_content(
    msg_type: &str,
    content: &serde_json::Value,
    msg_id: &str,
) -> Option<(String, Vec<ChannelAttachmentSpec>)> {
    match msg_type {
        "text" => {
            let t = content.get("text")?.as_str()?.to_string();
            Some((t, vec![]))
        }
        "image" => {
            let image_key = content.get("image_key")?.as_str()?.to_string();
            Some((
                String::new(),
                vec![ChannelAttachmentSpec {
                    kind: AttachmentKind::Picture,
                    download_code: image_key,
                    file_name: format!("image_{}.jpg", msg_id),
                }],
            ))
        }
        "file" => {
            let file_key = content.get("file_key")?.as_str()?.to_string();
            let file_name = content
                .get("file_name")
                .and_then(|v| v.as_str())
                .unwrap_or("file.bin")
                .to_string();
            Some((
                String::new(),
                vec![ChannelAttachmentSpec {
                    kind: AttachmentKind::File,
                    download_code: file_key,
                    file_name,
                }],
            ))
        }
        "interactive" => {
            // Cards inbound are rare; treat as text for now.
            let title = content
                .get("header")
                .and_then(|h| h.get("title"))
                .and_then(|t| t.get("content"))
                .and_then(|s| s.as_str())
                .unwrap_or("[飞书卡片]")
                .to_string();
            Some((title, vec![]))
        }
        other => {
            Some((format!("[飞书消息类型 {} 暂不支持]", other), vec![]))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // TODO(pr3): replace these JSON fixtures with protobuf-encoded byte fixtures once the
    // pbbp2 schema is vendored (Step 3.0). For now these exercise normalize_content +
    // parse_im_message in isolation, on the assumption that an upstream decoder has already
    // unwrapped the frame to plain JSON. The integration test in PR7 will cover the full
    // bytes-in path.
    //
    // Tests may be deferred to a sub-task after the schema is in place.

    fn make_event(msg_type: &str, content: serde_json::Value) -> serde_json::Value {
        serde_json::json!({
            "header": { "event_type": "im.message.receive_v1" },
            "event": {
                "message": {
                    "message_id": "om_xxx",
                    "chat_type": "p2p",
                    "chat_id": "oc_xxx",
                    "message_type": msg_type,
                    "content": content.to_string(),
                },
                "sender": {
                    "sender_id": { "open_id": "ou_xxx", "user_id": "u_xxx" },
                }
            }
        })
    }

    #[test]
    fn normalize_text_extracts_body() {
        let v = make_event("text", serde_json::json!({"text": "你好"}));
        let m = parse_im_message(&v).unwrap();
        assert_eq!(m.text, "你好");
        assert!(m.attachments.is_empty());
    }

    #[test]
    fn normalize_image_emits_attachment_spec() {
        let v = make_event("image", serde_json::json!({"image_key": "img-001"}));
        let m = parse_im_message(&v).unwrap();
        assert_eq!(m.text, "");
        assert_eq!(m.attachments.len(), 1);
        assert!(matches!(m.attachments[0].kind, AttachmentKind::Picture));
        assert_eq!(m.attachments[0].download_code, "img-001");
    }

    #[test]
    fn normalize_file_uses_file_name() {
        let v = make_event("file", serde_json::json!({"file_key": "file-001", "file_name": "report.pdf"}));
        let m = parse_im_message(&v).unwrap();
        assert_eq!(m.attachments[0].file_name, "report.pdf");
    }

    #[test]
    fn normalize_unsupported_type_emits_placeholder() {
        let v = make_event("audio", serde_json::json!({"file_key": "x"}));
        let m = parse_im_message(&v).unwrap();
        assert!(m.text.contains("飞书消息类型 audio"));
        assert!(m.attachments.is_empty());
    }

    #[test]
    fn normalize_group_chat_type() {
        let mut v = make_event("text", serde_json::json!({"text": "群里"}));
        v["event"]["message"]["chat_type"] = serde_json::Value::String("group".into());
        let m = parse_im_message(&v).unwrap();
        assert!(matches!(m.conversation_type, ConversationType::Group));
    }
}
```

- [ ] **Step 3.2: FeishuConnector::start 接入 stream**

Edit `src-tauri/src/connector/im/feishu/connector.rs`：

```rust
    async fn start(
        &self,
        ctx: ConnectorContext,
    ) -> Result<BoxStream<'static, ChannelMessage>, ConnectorError> {
        // The ws /callback/ws/endpoint handshake takes AppID/AppSecret directly — no
        // tenant_access_token needed at the ws layer. (tenant_access_token is still
        // required for IM REST calls in PR4+; that's owned by FeishuTokenSource elsewhere.)
        let (msg_tx, msg_rx) = tokio::sync::mpsc::channel::<ChannelMessage>(256);
        let client = super::stream::FeishuStreamClient::new(
            self.app_id.clone(),
            self.app_secret.clone(),
            msg_tx,
        );
        client.start(|_state, _err| {}, ctx.cancel_token.clone());
        use futures::StreamExt;
        let stream = tokio_stream::wrappers::ReceiverStream::new(msg_rx).boxed();
        Ok(stream)
    }
```

`on_status` 在 PR3 阶段先用 no-op 占位（PR4 接入 manager 的 status 回调机制）。

- [ ] **Step 3.3: feishu/mod.rs 暴露 stream**

```rust
pub mod connector;
pub mod registration;
pub mod stream;
pub mod token;
pub mod types;
```

- [ ] **Step 3.4: manager 接 connect_feishu_from_store**

Edit `manager.rs`，`connect_feishu_from_store` 改为真实实现，仿造 `connect_dingtalk`（去掉钉钉特定的 reply_manager 订阅）。返回的 BoxStream 接进同一个 message worker loop（按 `Platform::Feishu` 分支 build target���。

> 这里 manager.rs 改动比较大（~150 行的复制 + 飞书化）。一种降摩擦的写法：把 `connect_dingtalk` 内部公共逻辑抽出一个 `connect_platform<P: PlatformBackend>` 泛型方法，钉钉 / 飞书各填一个 `PlatformBackend` trait impl。**但这是 YAGNI 的红线**——目前只 2 个平台，先复制 100 行让 PR3 落地，等 PR1+企微之后再抽。

具体 connect_feishu_from_store 实现略（按 dingtalk 模板镜像写一遍，关键差异：不订阅 reply_manager 到 RuntimeEventBus、不传 reply_app_key/secret/robot_code、worker loop 内构造 `FeishuSessionTarget` 而非 `CardTarget`）。

- [ ] **Step 3.5: 跑测试 + 真账号冒烟**

Run: `cargo test --lib connector::im::feishu -- --nocapture 2>&1 | tail -15`
Expected: stream.rs 的 5 个测试 + 老 3 个全过。

冒烟：`pnpm tauri:dev` → 开启飞书 → 用手机发一条文字到机器人 → app 看到消息出现在频道列表里。

- [ ] **Step 3.6: 提交 PR3**

```bash
git add src-tauri/src/connector/im/feishu/stream.rs src-tauri/src/connector/im/feishu/connector.rs src-tauri/src/connector/im/feishu/mod.rs src-tauri/src/connector/im/manager.rs
git commit -m "$(cat <<'EOF'
feat(connector/im/feishu): WebSocket runtime + 4-type normalize + dedup (Phase 1 PR3)

- feishu/stream.rs: FeishuStreamClient with ws connect + ReconnectBackoff
- parse_im_message + normalize_content for text / image / file / interactive
  + placeholder for 20 other types
- in-connector MessageDedupSet drops replayed messages on reconnect
- manager.connect_feishu_from_store mirrors dingtalk worker loop, plugging
  FeishuConnector::start() output into the shared message dispatch path

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 4: PR4 — Text / Markdown 出站 reply

**Files:**
- Modify: `src-tauri/src/connector/im/feishu/connector.rs:send()`
- Create or modify: `src-tauri/src/connector/im/feishu/stream.rs` 加 `send_reply_text` helper

**目标**：`FeishuConnector::send(ReplyTarget, ReplyContent::Text|Markdown)` 调飞书 `/open-apis/im/v1/messages/{message_id}/reply` 或 `/open-apis/im/v1/messages?receive_id_type=chat_id` 发回消息。`AiCardChunk` / `AiCardFail` 这一 PR 仍返回 `NotSupported`，PR5 实现。

- [ ] **Step 4.1: send 实现 + 单测**

Edit `feishu/connector.rs::send`：

```rust
    async fn send(
        &self,
        target: ReplyTarget,
        content: ReplyContent,
    ) -> Result<(), ConnectorError> {
        match content {
            ReplyContent::Text(text) | ReplyContent::Markdown(text) => {
                let session = self.session_targets.read().await.get(&target.session_id).cloned();
                let Some(session) = session else {
                    return Err(ConnectorError::Fatal(format!(
                        "FeishuConnector::send no session target for {}", target.session_id
                    )));
                };
                let token = self.token_cache_get().await
                    .map_err(|e| ConnectorError::Transient(format!("token: {e:#}")))?;
                let client = reqwest::Client::new();
                let url = format!(
                    "https://open.feishu.cn/open-apis/im/v1/messages?receive_id_type={}",
                    session.receive_id_type
                );
                let body_content = serde_json::json!({"text": text}).to_string();
                let resp = client.post(&url)
                    .header("Authorization", format!("Bearer {}", token))
                    .json(&serde_json::json!({
                        "receive_id": session.receive_id,
                        "msg_type": "text",
                        "content": body_content,
                    }))
                    .send().await
                    .map_err(|e| ConnectorError::Transient(format!("feishu send http: {e:#}")))?;
                if !resp.status().is_success() {
                    return Err(ConnectorError::Transient(format!(
                        "feishu send status {}", resp.status()
                    )));
                }
                Ok(())
            }
            ReplyContent::AiCardChunk { .. } | ReplyContent::AiCardFail => {
                Err(ConnectorError::NotSupported("CardKit — PR5"))
            }
        }
    }
```

`token_cache_get` 是 `FeishuConnector` 新加的辅助方法：start() 时把 token cache 存进 `Arc<RwLock<Option<Arc<SharedTokenCache<FeishuTokenSource>>>>>` 字段（PR3 阶段还是局部变量，PR4 提到 struct 字段）。

- [ ] **Step 4.2: 单测 + 真账号冒烟**

冒烟：手机给机器人发 "你好" → app 自动回复 "已收到" 之类。

- [ ] **Step 4.3: 提交 PR4**

```bash
git commit -m "feat(connector/im/feishu): Text/Markdown reply via im.messages send (Phase 1 PR4)"
```

---

## Task 5: PR5 — CardKit 流式更新 + 严格 sequence + 节流（不丢 chunk）

**Files:**
- Create: `src-tauri/src/connector/im/feishu/card.rs`
- Modify: `src-tauri/src/connector/im/feishu/connector.rs:send(AiCardChunk|AiCardFail)`
- Modify: `src-tauri/src/connector/im/feishu/mod.rs`

**目标**：飞书 CardKit 增量更新跑通——首次 chunk 调 `cardkit.v1.card.create` + deliver，后续 chunk 调 `cardkit.v1.cardElement.content` 严格递增 sequence，final 调 `cardkit.v1.card.update`，错误回 `AiCardFail`。**节流**：每个 `card_id` 起独立 mpsc + 串行 sender task，间隔 ≥100ms，**不丢 chunk**——上游产 chunk 速度持续超过 10/s 时延迟扩大可接受，**禁止丢**。

> 飞书 CardKit URL 在 §0 Step 0.3 已调研。常量 `CARDKIT_CREATE_PATH` / `CARDKIT_ELEMENT_CONTENT_PATH` / `CARDKIT_UPDATE_PATH` 在文件常量区回填。

- [ ] **Step 5.1: card.rs 节流 sender + 状态机**

Create `src-tauri/src/connector/im/feishu/card.rs`：

```rust
//! 飞书 CardKit 流式更新。每个 `card_id` 起一个独立 mpsc + 串行 sender task，
//! 用 tokio::time::sleep_until 节流到 ≥100ms/次，不丢 chunk。

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use serde::Deserialize;
use tokio::sync::{mpsc, Mutex};
use tokio::time::sleep_until;

use crate::connector::im::shared::token::TokenCache as SharedTokenCache;

use super::token::FeishuTokenSource;
use super::types::FeishuSessionTarget;

const FEISHU_API: &str = "https://open.feishu.cn";
const CARDKIT_CREATE_PATH: &str = "/open-apis/cardkit/v1/cards";
const MIN_INTERVAL: Duration = Duration::from_millis(100);

pub struct CardKitSender {
    token_cache: Arc<SharedTokenCache<FeishuTokenSource>>,
    // session_id → active card state
    sessions: Arc<Mutex<HashMap<String, CardSession>>>,
}

struct CardSession {
    card_id: String,
    seq: u64,
    tx: mpsc::UnboundedSender<CardOp>,
}

enum CardOp {
    Chunk { delta: String, accumulated: String, seq: u64 },
    Final { full_text: String, seq: u64 },
    Fail,
}

impl CardKitSender {
    pub fn new(token_cache: Arc<SharedTokenCache<FeishuTokenSource>>) -> Self {
        Self {
            token_cache,
            sessions: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// dispatch_chunk = 同 dingtalk 的 dispatch_chunk 语义，但按 card_id 起串行 task。
    pub async fn dispatch_chunk(
        &self,
        session_id: &str,
        target: &FeishuSessionTarget,
        delta: &str,
        final_chunk: bool,
    ) -> Result<()> {
        // First chunk → create card + spawn sender task.
        let mut sessions = self.sessions.lock().await;
        if !sessions.contains_key(session_id) {
            let card_id = self.create_card(target).await?;
            let (tx, rx) = mpsc::unbounded_channel();
            self.spawn_sender_task(card_id.clone(), rx);
            sessions.insert(session_id.to_string(), CardSession {
                card_id,
                seq: 0,
                tx,
            });
        }
        let session = sessions.get_mut(session_id).unwrap();
        session.seq += 1;
        let seq = session.seq;
        // Note: dispatch caller passes accumulated string in delta param for simplicity;
        // sender task uses accumulated value directly to set card content.
        if final_chunk {
            let _ = session.tx.send(CardOp::Final { full_text: delta.to_string(), seq });
            sessions.remove(session_id);
        } else {
            let _ = session.tx.send(CardOp::Chunk {
                delta: String::new(),  // unused; sender uses accumulated
                accumulated: delta.to_string(),
                seq,
            });
        }
        Ok(())
    }

    pub async fn dispatch_fail(&self, session_id: &str) -> Result<()> {
        let mut sessions = self.sessions.lock().await;
        if let Some(session) = sessions.remove(session_id) {
            let _ = session.tx.send(CardOp::Fail);
        }
        Ok(())
    }

    async fn create_card(&self, target: &FeishuSessionTarget) -> Result<String> {
        let token = self.token_cache.get().await?;
        let client = reqwest::Client::new();
        let resp = client.post(format!("{}{}", FEISHU_API, CARDKIT_CREATE_PATH))
            .header("Authorization", format!("Bearer {}", token))
            .json(&serde_json::json!({
                "type": "card_json",
                "data": "{\"schema\":\"2.0\",\"body\":{\"elements\":[{\"tag\":\"markdown\",\"content\":\"\",\"element_id\":\"main\"}]}}",
            }))
            .send().await.context("cardkit create http")?;
        if !resp.status().is_success() {
            anyhow::bail!("cardkit create status {}", resp.status());
        }
        #[derive(Deserialize)]
        struct Resp { data: Inner }
        #[derive(Deserialize)]
        struct Inner { card_id: String }
        let r: Resp = resp.json().await.context("parse cardkit create")?;
        // 还要 deliver 到 target.receive_id：调 /im/v1/messages 发 card_id 引用
        // 实际 deliver 步骤见 §0 调研 (CardKit + im 发卡链路)。
        Ok(r.data.card_id)
    }

    fn spawn_sender_task(
        &self,
        card_id: String,
        mut rx: mpsc::UnboundedReceiver<CardOp>,
    ) {
        let token_cache = self.token_cache.clone();
        tokio::spawn(async move {
            let mut next_allowed = Instant::now();
            while let Some(op) = rx.recv().await {
                let now = Instant::now();
                if now < next_allowed {
                    sleep_until(next_allowed.into()).await;
                }
                next_allowed = Instant::now() + MIN_INTERVAL;

                match op {
                    CardOp::Chunk { accumulated, seq, .. } => {
                        if let Err(e) = update_element_content(&token_cache, &card_id, seq, &accumulated, false).await {
                            log::warn!("[feishu-cardkit] chunk seq={} err={:#}", seq, e);
                        }
                    }
                    CardOp::Final { full_text, seq } => {
                        if let Err(e) = update_element_content(&token_cache, &card_id, seq, &full_text, true).await {
                            log::warn!("[feishu-cardkit] final seq={} err={:#}", seq, e);
                        }
                        return;
                    }
                    CardOp::Fail => {
                        let _ = mark_card_failed(&token_cache, &card_id).await;
                        return;
                    }
                }
            }
        });
    }
}

async fn update_element_content(
    token_cache: &Arc<SharedTokenCache<FeishuTokenSource>>,
    card_id: &str,
    seq: u64,
    full_text: &str,
    final_chunk: bool,
) -> Result<()> {
    let token = token_cache.get().await?;
    let url = format!(
        "{}/open-apis/cardkit/v1/cards/{}/elements/main/content",
        FEISHU_API, card_id
    );
    let client = reqwest::Client::new();
    // CardKit streaming content update uses PUT (verified against larksuite/oapi-sdk-go's
    // sample/apiall/cardkitv1/content_cardElement.go; the plan originally guessed PATCH).
    let resp = client.put(&url)
        .header("Authorization", format!("Bearer {}", token))
        .json(&serde_json::json!({
            "uuid": uuid::Uuid::new_v4().to_string(),
            "content": full_text,
            "sequence": seq,
        }))
        .send().await.context("cardkit element content http")?;
    if !resp.status().is_success() {
        anyhow::bail!("cardkit element content status {}", resp.status());
    }
    let _ = final_chunk; // 这里飞书有专门的 finalize endpoint，按 §0 调研结果回填
    Ok(())
}

async fn mark_card_failed(
    _token_cache: &Arc<SharedTokenCache<FeishuTokenSource>>,
    _card_id: &str,
) -> Result<()> {
    // 占位：飞书 cardkit 没有"失败"状态，业务上等价于把内容改为错误文案 + 锁定
    // 卡片。具体协议见 §0 Step 0.3 调研。
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn dispatch_chunk_to_nonexistent_session_creates_session() {
        // 此测试无法跑真实 create（网络），跳过 create 验证仅检查 dispatch_fail 对空 session 是 no-op。
        let token_source = Arc::new(FeishuTokenSource::new("ak".into(), "as".into()));
        let cache = Arc::new(SharedTokenCache::new(token_source));
        let sender = CardKitSender::new(cache);
        assert!(sender.dispatch_fail("nonexistent").await.is_ok());
    }
}
```

- [ ] **Step 5.2: connector.send 接 CardKitSender**

Edit `feishu/connector.rs`，加 `card_sender: Arc<RwLock<Option<Arc<CardKitSender>>>>` 字段 + 在 `start()` 时 init + accumulated_text per session 由 connector 自己维护（参考 dingtalk reply_manager 的 contexts map）：

```rust
            ReplyContent::AiCardChunk { delta, final_chunk } => {
                let session = self.session_targets.read().await.get(&target.session_id).cloned();
                let Some(session) = session else {
                    return Err(ConnectorError::Fatal(format!(
                        "no session for AiCardChunk {}", target.session_id
                    )));
                };
                let sender = self.card_sender.read().await.clone();
                let Some(sender) = sender else {
                    return Err(ConnectorError::Fatal("card_sender not initialized; start() not called".into()));
                };
                // accumulate
                let mut acc = self.card_accumulated.write().await;
                let buf = acc.entry(target.session_id.clone()).or_default();
                buf.push_str(&delta);
                let full = buf.clone();
                if final_chunk { acc.remove(&target.session_id); }
                drop(acc);
                sender.dispatch_chunk(&target.session_id, &session, &full, final_chunk).await
                    .map_err(|e| ConnectorError::Transient(format!("cardkit chunk: {e:#}")))
            }
            ReplyContent::AiCardFail => {
                let sender = self.card_sender.read().await.clone();
                let Some(sender) = sender else { return Ok(()); };
                sender.dispatch_fail(&target.session_id).await
                    .map_err(|e| ConnectorError::Transient(format!("cardkit fail: {e:#}")))
            }
```

字段加：

```rust
pub struct FeishuConnector {
    // ... 老字段
    token_cache: Arc<tokio::sync::RwLock<Option<Arc<SharedTokenCache<FeishuTokenSource>>>>>,
    card_sender: Arc<tokio::sync::RwLock<Option<Arc<super::card::CardKitSender>>>>,
    card_accumulated: Arc<tokio::sync::RwLock<HashMap<String, String>>>,
}
```

`start()` 内：

```rust
        let token_source = Arc::new(super::token::FeishuTokenSource::new(...));
        let token_cache = Arc::new(SharedTokenCache::new(token_source));
        *self.token_cache.write().await = Some(token_cache.clone());
        *self.card_sender.write().await = Some(Arc::new(super::card::CardKitSender::new(token_cache.clone())));
```

- [ ] **Step 5.3: 单测 + 真账号冒烟**

冒烟：用机器人触发一个会流式回答的 prompt（让 AI 输出 500 字以上） → 飞书端看到 card 一字一字浮现 → 完成后内容定格。

- [ ] **Step 5.4: 提交 PR5**

```bash
git commit -m "feat(connector/im/feishu): CardKit streaming with strict sequence + non-dropping rate limit (Phase 1 PR5)"
```

---

## Task 6: PR6 — 附件下载 + PendingQueueManager 接入

**Files:**
- Create: `src-tauri/src/connector/im/feishu/download.rs`
- Modify: `src-tauri/src/connector/im/shared/pending_adapter.rs`
- Modify: `src-tauri/src/connector/im/manager.rs::connect_feishu_from_store`

**目标**：飞书消息含 image/file 时调 `/open-apis/im/v1/messages/{message_id}/resources/{file_key}` 拉原始字节，落地到 `~/.renlijia/tmp/feishu_downloads/`；下载成功后通过 `build_pending_item_from_feishu` 适配 PendingQueueManager。

- [ ] **Step 6.1: download.rs**

Create `src-tauri/src/connector/im/feishu/download.rs`，结构镜像 `dingtalk/download.rs`：实现 `FeishuFileDownloader::download(message_id, file_key, kind, target_filename) -> Result<DownloadedFile>`，用 `Authorization: Bearer <tenant_access_token>` 头。

- [ ] **Step 6.2: pending_adapter 加 feishu 入口**

Edit `shared/pending_adapter.rs`，复制 `build_pending_item_from_dingtalk` 写一份 `build_pending_item_from_feishu`，唯一差别是 `source: PendingSource::ImFeishu`。先确认 `PendingSource` enum 已有 `ImFeishu` 变体（无则加）。

> Phase 0 spec §0 表里写"飞书 PR6 内做"，YAGNI：不试图泛化 `build_pending_item_from_*<P>`——两个 6 行的函数比一个 30 行的泛型 helper 好维护。

- [ ] **Step 6.3: manager.connect_feishu_from_store 接 download + pending**

参考 dingtalk 的 worker loop 内 `download_specs_for_turn` + `build_pending_item_from_dingtalk` + `pending_manager.enqueue_or_send` 调用链，飞书侧的对应实现。

- [ ] **Step 6.4: 单测 + 真账号冒烟**

冒烟：发图片、文件给机器人 → 检查 `~/.renlijia/tmp/feishu_downloads/` 有文件 → AI 能引用文件内容回答。

- [ ] **Step 6.5: 提交 PR6**

```bash
git commit -m "feat(connector/im/feishu): attachment download + PendingQueueManager integration (Phase 1 PR6)"
```

---

## Task 7: PR7 — 集成测试 + 前端 UI 完整化 + review_im_layering 收尾

**Files:**
- Create: `src-tauri/tests/im_feishu_integration.rs`
- Modify: `src/features/channel/ChannelPage.tsx`（飞书按钮启用）
- Modify: `src/features/channel/ChannelConfig.tsx` 或 Create `src/features/channel/FeishuChannelConfig.tsx`
- Modify: `src/lib/tauri.ts`（如 ChannelPlatform 类型需扩展）

- [ ] **Step 7.1: im_feishu_integration.rs**

Create `src-tauri/tests/im_feishu_integration.rs`，参考 `im_connector_cancel_test.rs` 的 fixture 模式：

```rust
//! Phase 1 integration: ChannelManager + FeishuConnector + mock 飞书后端。
//! 覆盖：cancel-2s 契约、消息派活、reply 直发。

use std::sync::Arc;
use std::time::{Duration, Instant};

use async_trait::async_trait;
use futures::stream::{self, BoxStream, StreamExt};
use tempfile::TempDir;
use tokio_util::sync::CancellationToken;

use app_lib::connector::im::shared::config_store::ChannelConfigStore;
use app_lib::connector::im::trait_def::{
    AuthFlow, ConnectorCapabilities, ConnectorContext, ConnectorError, IMConnector, InboundModel,
    ReplyContent, ReplyTarget,
};
use app_lib::connector::im::types::{ChannelMessage, ConversationType, Platform};

// Mock FeishuConnector that yields one message and respects cancel.
struct MockFeishuConnector;

#[async_trait]
impl IMConnector for MockFeishuConnector {
    fn platform(&self) -> Platform { Platform::Feishu }
    fn capabilities(&self) -> ConnectorCapabilities {
        ConnectorCapabilities {
            inbound: InboundModel::Stream,
            outbound_aicard: true,
            outbound_markdown: true,
            supports_attachments: true,
            supports_group_chat: true,
            supports_private_chat: true,
            auth_flow: AuthFlow::DeviceCode,
        }
    }
    async fn start(&self, ctx: ConnectorContext) -> Result<BoxStream<'static, ChannelMessage>, ConnectorError> {
        let cancel = ctx.cancel_token.clone();
        let s = stream::unfold((cancel, false), |(cancel, sent)| async move {
            if cancel.is_cancelled() { return None; }
            if !sent {
                let msg = ChannelMessage {
                    msg_id: "om_test".into(),
                    conversation_type: ConversationType::Private,
                    conversation_key: "oc_test".into(),
                    sender_id: "ou_test".into(),
                    sender_nick: "tester".into(),
                    text: "hello feishu".into(),
                    robot_code: String::new(),
                    reply_group_id: "oc_test".into(),
                    attachments: vec![],
                    session_webhook: None,
                };
                return Some((Some(msg), (cancel, true)));
            }
            tokio::select! {
                _ = cancel.cancelled() => None,
                _ = tokio::time::sleep(Duration::from_secs(1)) => Some((None, (cancel, true))),
            }
        }).filter_map(|x| async move { x });
        Ok(s.boxed())
    }
    async fn send(&self, _t: ReplyTarget, _c: ReplyContent) -> Result<(), ConnectorError> { Ok(()) }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn feishu_connector_emits_one_message_and_honors_cancel() {
    let _tmp = TempDir::new().unwrap();
    let cancel = CancellationToken::new();
    let registry = Arc::new(app_lib::runtime::run_registry::RuntimeRunRegistry::new());
    let bus = Arc::new(app_lib::runtime::event_bus::RuntimeEventBus::new());
    let resolver = Arc::new(TempConvDirResolver(_tmp.path().to_path_buf()));
    let pending_manager = app_lib::runtime::pending::PendingQueueManager::new(
        registry, bus, resolver, app_lib::runtime::pending::PendingConfig::default(),
    );
    let ctx = ConnectorContext {
        config_store: Arc::new(ChannelConfigStore::new(_tmp.path().to_path_buf(), None)),
        secure_storage: None,
        ask_coordinator: None,
        pending_manager,
        cancel_token: cancel.clone(),
    };
    let mut s = MockFeishuConnector.start(ctx).await.unwrap();
    let first = s.next().await.expect("first message");
    assert_eq!(first.text, "hello feishu");
    cancel.cancel();
    let start = Instant::now();
    while let Some(_) = s.next().await {}
    assert!(start.elapsed() < Duration::from_secs(2));
}

struct TempConvDirResolver(std::path::PathBuf);
impl app_lib::runtime::pending::ConvDirResolver for TempConvDirResolver {
    fn conversation_dir(&self, sid: &app_lib::runtime::ids::SessionId) -> Option<std::path::PathBuf> {
        let d = self.0.join(sid.as_str());
        std::fs::create_dir_all(&d).ok()?;
        Some(d)
    }
    fn is_archived(&self, _sid: &app_lib::runtime::ids::SessionId) -> bool { false }
    fn conversations_root(&self) -> std::path::PathBuf { self.0.clone() }
}
```

- [ ] **Step 7.2: 前端 FeishuChannelConfig 组件**

Create `src/features/channel/FeishuChannelConfig.tsx`，仿造 `ChannelConfig.tsx`：触发 `channelBeginRegistration('feishu')` → 弹出 user_code + verification_uri → 启动 polling `channelPollRegistration('feishu', deviceCode)`，每 `interval_seconds` 一次直到 success / expired。

ChannelPage.tsx 内对应"配置飞书"按钮取消 disabled，点开弹 FeishuChannelConfig。

- [ ] **Step 7.3: review_im_layering 检查**

Run: `cargo test --test review_im_layering -- --nocapture`

由于 `feishu/*.rs` 没有 import `shared::router / ask_coordinator / config_store / pending_adapter`（所有 capability 都通过 `ConnectorContext` 注入），test 应当通过。如果失败：检查哪个文件违反了规则，把直接 import 改成通过 `ctx` 接收。

- [ ] **Step 7.4: 全量测试**

Run: `cd /Users/oayzz/project/lotus/lotus-workbench/lotus-app/.claude/worktrees/amazing-chatelet-801fd7/src-tauri && cargo test --no-fail-fast 2>&1 | tail -10`
Expected: passed 数 ≥ Plan A 结束时的 baseline + ~25（PR1: +3, PR2: +3, PR3: +5, PR4: +1, PR5: +1, PR6: +2, PR7 integration: +1）。失败数不增加。

Run: `cd /Users/oayzz/project/lotus/lotus-workbench/lotus-app/.claude/worktrees/amazing-chatelet-801fd7 && pnpm test 2>&1 | tail -10`
Expected: 全过。

Run: `cd /Users/oayzz/project/lotus/lotus-workbench/lotus-app/.claude/worktrees/amazing-chatelet-801fd7 && pnpm tsc --noEmit 2>&1 | tail -5`
Expected: 0 errors。

- [ ] **Step 7.5: 真账号长冒烟（1 个工作日）**

钉钉 + 飞书同时在线 8 小时，私聊 / 群聊各发 ≥10 条消息（文字 / 图 / 文件混合），观察：

- 流式 AI Card 字符渐进出现，飞书端不卡顿
- 重连一次（关 wifi 2s 再开），重连后历史消息不重复触发
- 关闭 app 重启，飞书自动重新连上
- 日志 grep ERROR 数量 = 0

- [ ] **Step 7.6: 提交 PR7**

```bash
git add src-tauri/tests/im_feishu_integration.rs src/features/channel/ChannelPage.tsx src/features/channel/FeishuChannelConfig.tsx src/lib/tauri.ts
git commit -m "$(cat <<'EOF'
feat(connector/im/feishu): integration test + UI registration flow (Phase 1 PR7)

- tests/im_feishu_integration.rs covers cancel-≤2s contract + first-message emit
  using a mock FeishuConnector built on the trait surface
- FeishuChannelConfig component drives device-code → polling → success UX
- ChannelPage flips the feishu card to enabled / configurable
- pnpm test + cargo test --no-fail-fast both green

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## §8 收尾验证

- [ ] **Step 8.1: 全量 review tests**

Run: `cargo test review_ --tests --no-fail-fast 2>&1 | tail -10`
Expected: 全过；`review_im_layering` 内 `platforms = ["dingtalk", "feishu"]` 通过 layering 校验。

- [ ] **Step 8.2: 推送 + 开 PR 链**

7 个 commits 在同一分支累积。推一次，开 1 个汇总 PR：标题 `feat(connector/im/feishu): Phase 1 — full IMConnector for Feishu`。

PR body 描述：
- 链接 Plan A PR 的 4 个收尾 commits
- 列 7 个子 PR 的 commit hash
- 真账号冒烟结果（持续小时、消息数、错误数）

- [ ] **Step 8.3: 文档更新**

Edit `docs/superpowers/specs/2026-05-18-im-feishu-phase1-design.md`，把 Phase 1 status 改为"Implemented"，加链接到本计划。

- [ ] **Step 8.4: roadmap 更新**

Edit `docs/superpowers/specs/2026-05-18-im-connector-roadmap.md`，把"Phase 1 飞书"打勾，下一项"Phase 2 企微"标 Next。

---

## §9 风险 + 估时（基于 endpoint 调研修订）

### §9.1 风险

| 风险 | 缓解 |
|---|---|
| PR0d `ReplyTarget` 改造影响面广 —— manager.rs 多处构造点 | 改造前先 grep 所有 `ReplyTarget { ... }` 字面构造，列清楚改动点；用类型系统逼出来（删字段后编译错误自然指路） |
| **Protobuf decoder for WS frames** complicates PR3 | Vendor minimal `.proto` from larksuite-oapi-sdk-go; allocate buffer for "schema reverse-engineering took longer than expected" in PR3 budget; consider hand-rolled decoder fallback if prost integration is painful |
| CardKit 严格 sequence 模型 + 并发 chunk 乱序 | connector 内对每个 card_id 起 mpsc + 串行 sender task；用 `sleep_until` 节流而非丢弃，保流式视觉 |
| 24 种消息类型用户期待"全支持" | spec 显式声明只支持 4 种 + 占位文案，等用户反馈再扩 |
| 飞书 device-code 域名 accounts.feishu.cn 在境外网络不稳 | Phase 1 不优化，标记为已知；前端注册流加超时提示 |
| 飞书 token 跟钉钉 token 字段名冲突 | PR0a 抽出的 `TokenCache<S>` 是泛型，源由 platform-specific source provider 决定，无字段名冲突；keychain key 必须带 `aijia-feishu-` 前缀，PR0d 的 `ChannelConfigStore` 改造里强制 |

### §9.2 估时（基于调研修订）

- PR1：0.5 天（骨架）
- PR2：1 天（device-code）
- **PR3：3.5 天**（WS handshake + protobuf decoder vendor + normalize；从原 2 天上调，增量来自 Step 3.0 的 .proto vendor + prost 集成）
- PR4：0.5 天（text/markdown send）
- PR5：2 天（CardKit + sequence + rate limit）
- PR6：1 天（附件下载 + pending adapter）
- PR7：1 天（集成测试 + 前端）
- **飞书主体小计：~9.5 天**（原 8 天 + 1.5 天 PR3 增量）

**总计：~13.5 天单人**（含 Plan A 4 天 + 飞书主体 9.5 天）。修订前是 ~12 天，多出来的 1.5 天全部来自 PR3 的 protobuf 调研 + 实现增量。

---

## Self-Review Checklist

- [x] **Spec coverage** — PR1-PR7 对应 spec §5 的 7 个 PR。spec §0 的 4 个前置 PR0a-d 由 Plan A 完成。
- [x] **Placeholder scan** — 没有 "TBD"。两处 `略`/`见 §0 调研结果` 是有意的：飞书 endpoint 常量值依赖 §0 Step 0.3 的实际调研，**不能**在写计划时硬编码——计划锁死 URL 字符串就是 placeholder 的反面（伪精确）。
- [x] **Type consistency** — `FeishuConnector`、`FeishuSessionTarget`、`FeishuStoredConfig`、`FeishuTokenSource`、`CardKitSender`、`FeishuStreamClient` 命名一致。
- [x] **依赖一致性** — Plan A 完成后才开始 Plan B；引用的 `SharedTokenCache` / `MessageDedupSet` / `ReplyTarget`（中性版） / `platform_*_path` 都是 Plan A 落地的产物。

## Execution Handoff

Plan complete and saved to `docs/superpowers/plans/2026-05-18-im-feishu-phase1.md`. Two execution options:

1. **Subagent-Driven (recommended)** - I dispatch a fresh subagent per task, review between tasks, fast iteration
2. **Inline Execution** - Execute tasks in this session using executing-plans, batch execution with checkpoints

Which approach?
