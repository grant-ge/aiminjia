# Phase 4 WhatsApp PR1 — 骨架 + Cargo 依赖 + capability 字段补齐

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 落地 `src-tauri/src/connector/im/whatsapp/` 编译可通过的骨架 + `whatsapp-rust = "0.6"` 依赖 + `ConnectorCapabilities::outbound_text_streaming` 新字段 + 修复 `InboundModel::Webhook` 注释把 whatsapp 错列在 Webhook 的笔误，**不**实现扫码 / 收发 / 媒体（PR2-PR8 做）。

**Architecture:** 镜像 `feishu/connector.rs` 的"PR1 stub connector"模式——`WhatsAppConnector` 结构体只持 `on_status` 回调，`impl IMConnector` 的 `start` / `send` / `begin_registration` / `poll_registration` 全部返回 `ConnectorError::NotSupported(...)`，让 PR2-PR8 按 spec §10.2 顺序逐步填实。Factory 函数 `build_whatsapp_connector` 加进 `factory.rs`，但 **manager.rs 不动**（manager wiring 留到 PR3 扫码登录时再做）。

**Tech Stack:** Rust crate [`wa-rs = "0.2"`](https://crates.io/crates/wa-rs)（[homunbot/wa-rs](https://github.com/homunbot/wa-rs)，是 [jlucaso1/whatsapp-rust](https://github.com/jlucaso1/whatsapp-rust) 的 stable-Rust fork——移除 `#![feature(portable_simd)]` 和 `if_let_chains`，能在 stable 1.77.2 编过；2026-05-20 实测 OK）。**不**在 PR1 引入伴生 crate `wa-rs-sqlite-storage / -tokio-transport / -ureq-http`，那些到 PR2 真的开始构造 Bot 时才加，避免 PR1 拉一堆未用的 deps 让编译时间膨胀。

**Spec 来源：** `docs/superpowers/specs/2026-05-18-im-whatsapp-phase4-design.md` §10.2 PR1 行。

**与 spec 的偏离（已验证仓库现状）：**
- spec 写 PR1 要"加 `Platform::Whatsapp` enum 变体（含 as_str / from_str / all）"——**仓库已存在**（`types.rs:12,23,34,46`），本 PR 跳过这步
- spec 把 `outbound_text_streaming` capability 字段归到"Phase 3 PR1.5"前置——但 Phase 3 PR1.5 还没动工，本 PR1 顺手加（5 行改动 + 6 个调用点更新），不然 capability 写不出来
- spec 把 `ChannelConnectionState::NeedsReauth` 归到"Phase 3 PR1.5"前置——**仓库已存在**（`types.rs:71`），跳过
- spec §1 表说 `inbound = Stream`，但 `trait_def.rs:28` 的 `InboundModel::Webhook` doc 注释把 whatsapp 错列在 webhook 类，本 PR1 顺手修文字
- **spec §0.3 / §10.2 / §10.3 写的 `whatsapp-rust = "0.1"`（后 cargo search 实测最新是 0.6）实际无法在 stable Rust 编**——`wacore-binary` 用 `#![feature(portable_simd)]`、`wacore` 用 `if_let_chains`，stable 编不过；`default-features = false` 也救不了 wacore。**改用 `wa-rs = "0.2"` fork**（spec §0.5 已登记"必要时给 upstream 提 PR"，这是该风险的具体体现）。Spec 文档会在 PR1 完成后单独更新（task 列表追加）

---

## File Structure（PR1 范围）

新建：
- `src-tauri/src/connector/im/whatsapp/mod.rs` — module 入口 + 文档注释 + 子模块声明
- `src-tauri/src/connector/im/whatsapp/connector.rs` — `WhatsAppConnector` struct + stub `impl IMConnector`
- `src-tauri/src/connector/im/whatsapp/types.rs` — PR1 只有 `WhatsAppSessionTarget`（PR3+ 填充其他）

修改：
- `src-tauri/Cargo.toml` — `[dependencies]` 节加 `wa-rs = "0.2"`
- `src-tauri/src/connector/im/mod.rs` — `pub mod whatsapp;`
- `src-tauri/src/connector/im/trait_def.rs` — `ConnectorCapabilities` 加字段 `outbound_text_streaming: bool`；修 `InboundModel::Webhook` 注释；同步 `tests` 中构造点
- `src-tauri/src/connector/im/factory.rs` — 加 `WhatsappStatusCallback` 类型别名 + `build_whatsapp_connector` 函数
- `src-tauri/src/connector/im/dingtalk/connector.rs` — `capabilities()` 加 `outbound_text_streaming: true`（dingtalk 走 AI Card，是 streaming text 平台）
- `src-tauri/src/connector/im/feishu/connector.rs` — `capabilities()` 加 `outbound_text_streaming: true`（feishu 走 CardKit streaming）
- `src-tauri/src/connector/im/wecom/connector.rs` — `capabilities()` 加 `outbound_text_streaming: false`
- `src-tauri/src/connector/im/wechat/connector.rs` — `capabilities()` 加 `outbound_text_streaming: false`
- `src-tauri/src/connector/im/telegram/connector.rs` — `capabilities()` 加 `outbound_text_streaming: false`

新建测试（PR1 自带）：
- `src-tauri/src/connector/im/whatsapp/connector.rs` 内嵌 `#[cfg(test)] mod tests`：验证 `capabilities()` 返回的字段跟 spec §1 表一致

不动（PR1 不碰）：
- `manager.rs`（无 wiring）
- `tests/review_im_layering.rs` 的 `known_platforms` 数组（PR8 加 whatsapp）
- 前端任何文件
- `Cargo.lock`（让 `cargo build` 自动产）

---

## Task 1: Cargo 加 wa-rs 依赖 + 探测可编译

**Files:**
- Modify: `src-tauri/Cargo.toml`（`[dependencies]` 节末尾追加 1 行）

**背景**：原计划用 `whatsapp-rust = "0.6"` 上游 crate，2026-05-20 实测发现它的 `wacore-binary` 用 `#![feature(portable_simd)]`、`wacore` 用 `if_let_chains`，**stable Rust 编不过**。改用 [`wa-rs = "0.2"`](https://crates.io/crates/wa-rs)（[homunbot/wa-rs](https://github.com/homunbot/wa-rs) fork，移除 nightly 特性，stable 兼容）。在 `/tmp/wa-rs-probe` 隔离 cargo 工程实测过：44s 编过，无 unstable feature 错误。

- [ ] **Step 1: 读 Cargo.toml 确认插入位置**

Run: `grep -n '^\[dependencies\]\|^\[dev-dependencies\]' src-tauri/Cargo.toml`
Expected: 至少两行——`[dependencies]` 起点和 `[dev-dependencies]` 起点。

- [ ] **Step 2: 在 `[dev-dependencies]` 节之前插入 wa-rs**

Edit `src-tauri/Cargo.toml`：在 `[dev-dependencies]` 节起始行之前插入

```toml
# WhatsApp Web (multi-device) connector — OpenClaw-equivalent route
# (Baileys/whatsmeow protocol). See docs/superpowers/specs/2026-05-18-im-whatsapp-phase4-design.md.
#
# Using `wa-rs` (homunbot/wa-rs), a fork of jlucaso1/whatsapp-rust that
# removes `#![feature(portable_simd)]` and `if_let_chains` so the crate
# compiles on stable Rust (upstream whatsapp-rust requires nightly).
# Companion crates (wa-rs-sqlite-storage / -tokio-transport / -ureq-http)
# pulled in PR2 when Bot::builder() actually constructs an instance.
wa-rs = "0.2"
```

- [ ] **Step 3: 编译验证（只编译，不跑测试）**

Run: `cd src-tauri && cargo build --lib 2>&1 | tail -20`
Expected: `Finished \`dev\` profile [unoptimized + debuginfo] target(s) in ...`（首次会下载依赖，可能 1-2 分钟）。

**If you see compile errors**: report BLOCKED with the full error tail. Do NOT attempt MSRV bump or feature flag tweaks—we already validated `wa-rs = "0.2"` builds cleanly on stable in an isolated probe; failure here likely means cargo workspace constraint conflicts with our existing deps. Controller will decide.

- [ ] **Step 4: Commit**

```bash
git add src-tauri/Cargo.toml src-tauri/Cargo.lock
git commit -m "$(cat <<'EOF'
chore(connector/im/whatsapp): PR1 加 wa-rs 0.2 依赖

Phase 4 PR1 第一步。wa-rs = jlucaso1/whatsapp-rust 的 stable-Rust
fork（homunbot/wa-rs），移除 nightly-only 的 portable_simd + if_let_chains。
本质仍是 OpenClaw 同款方案（WhatsApp Web multi-device 协议，whatsmeow +
Baileys 移植）。

原 plan 写的 whatsapp-rust = "0.6" 实测无法在 stable Rust 编（wacore
依赖链有 #![feature(portable_simd)] 和 if_let_chains，仓库 MSRV 1.77.2
+ stable toolchain）。fork 维护活跃度低（7 stars / 4 commits），上线前
考虑 vendor 一份或给 upstream 提 PR。

伴生 crate（sqlite-storage / tokio-transport / ureq-http）留到 PR2 真的
Bot::builder() 时再加，避免 PR1 拉无用 deps。

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 2: 给 ConnectorCapabilities 加 outbound_text_streaming 字段

**Files:**
- Modify: `src-tauri/src/connector/im/trait_def.rs:44-53`（struct 定义）+ `:165-176`（测试构造）+ `:28`（注释笔误）

- [ ] **Step 1: 写失败测试——验证 trait 模块层面字段已加**

Edit `src-tauri/src/connector/im/trait_def.rs`，把 `#[cfg(test)] mod tests` 块（行 161-185）的 `capabilities_can_be_constructed` 测试改成断言新字段存在：

```rust
    #[test]
    fn capabilities_can_be_constructed() {
        let c = ConnectorCapabilities {
            inbound: InboundModel::Stream,
            outbound_aicard: true,
            outbound_text_streaming: true,
            outbound_markdown: true,
            supports_attachments: true,
            supports_group_chat: true,
            supports_private_chat: true,
            auth_flow: AuthFlow::DeviceCode,
        };
        // Streaming text capability must be independent from aicard.
        assert!(c.outbound_text_streaming);
    }
```

- [ ] **Step 2: 运行测试验证它失败（编译失败：字段不存在）**

Run: `cd src-tauri && cargo test --lib connector::im::trait_def::tests::capabilities_can_be_constructed 2>&1 | tail -10`
Expected: `error[E0560]: struct \`ConnectorCapabilities\` has no field named \`outbound_text_streaming\``

- [ ] **Step 3: 加字段定义**

Edit `src-tauri/src/connector/im/trait_def.rs:44-53`，在 `outbound_aicard` 字段下方加一行：

```rust
#[derive(Debug, Clone)]
pub struct ConnectorCapabilities {
    pub inbound: InboundModel,
    pub outbound_aicard: bool,
    /// Connector can stream a single text reply incrementally (e.g. by
    /// editing a previously sent message). When `true`, the manager can
    /// route `ReplyContent::AiCardChunk` to this connector even though
    /// `outbound_aicard` is `false` — the connector renders the chunked
    /// stream as edits to a placeholder text message. Set by:
    /// - dingtalk (true, via native AI Card)
    /// - feishu (true, via CardKit streaming)
    /// - whatsapp (true, via send_text + edit_message; see Phase 4 spec §6)
    /// - wecom / wechat / telegram (false; final-only or static text)
    pub outbound_text_streaming: bool,
    pub outbound_markdown: bool,
    pub supports_attachments: bool,
    pub supports_group_chat: bool,
    pub supports_private_chat: bool,
    pub auth_flow: AuthFlow,
}
```

- [ ] **Step 4: 修 InboundModel::Webhook 注释笔误**

Edit `src-tauri/src/connector/im/trait_def.rs:27-30`：

把
```rust
    /// HTTP webhook pushed by the platform (wecom / telegram / whatsapp).
    /// The connector registers a path with the shared webhook server.
    Webhook,
```

改为
```rust
    /// HTTP webhook pushed by the platform (wecom).
    /// The connector registers a path with the shared webhook server.
    /// (Telegram uses long-poll = `Stream`; WhatsApp uses WebSocket = `Stream`.)
    Webhook,
```

- [ ] **Step 5: 跑 trait 测试，确认通过**

Run: `cd src-tauri && cargo test --lib connector::im::trait_def::tests 2>&1 | tail -10`
Expected: `test result: ok. 2 passed; 0 failed`

- [ ] **Step 6: 跑全 lib 编译，列出所有"missing field"报错**

Run: `cd src-tauri && cargo build --lib 2>&1 | grep -E 'missing field|--> src' | head -40`
Expected: 5 处缺字段错（dingtalk / feishu / wecom / wechat / telegram 各一处 `capabilities()`）。如果数量不对，停下来人工核对——可能有别的地方也构造了 `ConnectorCapabilities`。

- [ ] **Step 7: 在 5 个已有 connector 的 `capabilities()` 加新字段**

逐个 Edit，参照下表：

| 文件 | 行号附近 | 新字段值 | 理由 |
|---|---|---|---|
| `src-tauri/src/connector/im/dingtalk/connector.rs` | grep `fn capabilities` | `outbound_text_streaming: true,` | AI Card 是 streaming text 的范式 |
| `src-tauri/src/connector/im/feishu/connector.rs` | grep `fn capabilities` | `outbound_text_streaming: true,` | CardKit 100ms throttle streaming |
| `src-tauri/src/connector/im/wecom/connector.rs` | grep `fn capabilities` | `outbound_text_streaming: false,` | aibot 只能 final-only |
| `src-tauri/src/connector/im/wechat/connector.rs` | grep `fn capabilities` | `outbound_text_streaming: false,` | iLink 静默累积 = final-only |
| `src-tauri/src/connector/im/telegram/connector.rs` | grep `fn capabilities` | `outbound_text_streaming: false,` | Bot API editMessageText 有但 spec 不用 |

每处插在 `outbound_aicard: ...,` 之后、`outbound_markdown: ...,` 之前——保持字段顺序跟 struct 定义一致，让 reviewer 一眼能比对。

- [ ] **Step 8: 跑全 lib 编译 + 测试**

Run: `cd src-tauri && cargo build --lib 2>&1 | tail -5`
Expected: `Finished` 无 error。

Run: `cd src-tauri && cargo test --lib connector::im:: 2>&1 | tail -5`
Expected: `test result: ok. ... passed; 0 failed`（具体 passed 数随仓库变；只看 0 failed）。

- [ ] **Step 9: Commit**

```bash
git add src-tauri/src/connector/im/trait_def.rs \
        src-tauri/src/connector/im/dingtalk/connector.rs \
        src-tauri/src/connector/im/feishu/connector.rs \
        src-tauri/src/connector/im/wecom/connector.rs \
        src-tauri/src/connector/im/wechat/connector.rs \
        src-tauri/src/connector/im/telegram/connector.rs
git commit -m "$(cat <<'EOF'
feat(connector/im): 加 outbound_text_streaming capability 字段

Phase 4 PR1 第二步。WhatsApp 通过 send_text + edit_message 是
非 AI Card 平台中第一个支持 streaming text 的，trait 需要新字段
让 manager 路由 ReplyContent::AiCardChunk。

5 个已有 connector 同步：dingtalk/feishu = true（AI Card / CardKit），
wecom/wechat/telegram = false（final-only）。

顺手修 InboundModel::Webhook 注释错把 telegram/whatsapp 列在
webhook 类（实际都是 Stream）。

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 3: 建 whatsapp 模块骨架（mod.rs + types.rs）

**Files:**
- Create: `src-tauri/src/connector/im/whatsapp/mod.rs`
- Create: `src-tauri/src/connector/im/whatsapp/types.rs`
- Modify: `src-tauri/src/connector/im/mod.rs`（追加 `pub mod whatsapp;`）

- [ ] **Step 1: 建 whatsapp/types.rs（PR1 只放 WhatsAppSessionTarget）**

Create `src-tauri/src/connector/im/whatsapp/types.rs`：

```rust
//! WhatsApp connector 内部类型。PR1 只放 reply target；PR2-PR8 按 spec §2
//! 逐步加 PairingState / MessageRef / 内部 JID newtype 等。

/// 反查表条目：把内部 session_id 映射回 WhatsApp JID（"86138...@s.whatsapp.net"）。
/// 入站消息到达时由 parser 写入，出站 send() 时读取。私聊 only 所以一个
/// session_id 唯一对应一个对端 JID。
#[derive(Debug, Clone)]
pub struct WhatsAppSessionTarget {
    /// 对端 WhatsApp JID（e.g. `8613800138000@s.whatsapp.net`）
    pub peer_jid: String,
}
```

- [ ] **Step 2: 建 whatsapp/mod.rs**

Create `src-tauri/src/connector/im/whatsapp/mod.rs`：

```rust
//! WhatsApp connector implementation —— OpenClaw 同款方案
//! ([docs.openclaw.ai/channels/whatsapp](https://docs.openclaw.ai/channels/whatsapp))
//! via Rust crate [whatsapp-rust](https://github.com/jlucaso1/whatsapp-rust)
//! (whatsmeow + Baileys 协议移植)。
//!
//! ⚠️ 协议是 WhatsApp Web 多设备协议，**TOS 灰区**。账号有被 WhatsApp
//! 限速 / 封禁的风险，必须在前端首次扫码时显示 §9.1 风险 banner，
//! 用户勾选"已知晓"才能进入扫码界面。
//!
//! Phase 4 PR 切分（spec §10.2）：
//!   PR1 —— 骨架 + Cargo deps + capability 字段（**本 PR**）
//!   PR2 —— Bot 生命周期 + SqliteStore + _pairing 路径
//!   PR3 —— 扫码登录（begin/poll_registration + PairingState 状态机）
//!   PR4 —— 入站（bot.run() worker + Event::Message dispatch + parser）
//!   PR5 —— 出站 text/markdown + 错误映射
//!   PR6 —— 出站 AI Card 占位 + 增量编辑
//!   PR7 —— 入站媒体 download_media
//!   PR8 —— 集成测试 + UI + banner
//!
//! 详见 `docs/superpowers/specs/2026-05-18-im-whatsapp-phase4-design.md`。

pub mod connector;
pub mod types;

pub use connector::WhatsAppConnector;
```

- [ ] **Step 3: 把 whatsapp 子 mod 加进 im/mod.rs**

Edit `src-tauri/src/connector/im/mod.rs`，在 `pub mod telegram;` 行之后加一行：

```rust
pub mod telegram;
pub mod whatsapp;
pub mod wecom;
```

（按字母序插在 telegram 后 wecom 前。注意原文件 `wecom` 在 `telegram` 后，本步骤是夹进去。先 `grep -n '^pub mod' src-tauri/src/connector/im/mod.rs` 确认实际顺序，按 alphabetical 插。）

- [ ] **Step 4: 编译——connector.rs 还没建，预期 build 报错**

Run: `cd src-tauri && cargo build --lib 2>&1 | tail -10`
Expected: `error[E0583]: file not found for module \`connector\`` 指向 `whatsapp/mod.rs`。这预期，Task 4 解决。

- [ ] **Step 5: 暂不 commit**

Task 3 + Task 4 共同构成"模块骨架"的一个原子提交。等 Task 4 完成再 commit。

---

## Task 4: 写 WhatsAppConnector stub + 单测

**Files:**
- Create: `src-tauri/src/connector/im/whatsapp/connector.rs`

- [ ] **Step 1: 先写测试（TDD）**

Create `src-tauri/src/connector/im/whatsapp/connector.rs` 起手 = 测试块 + 用到的 use（让"先红再绿"显式）：

```rust
//! `WhatsAppConnector` —— PR1 stub。实现 `IMConnector` trait 让仓库能
//! 编过，所有业务方法返回 `ConnectorError::NotSupported(...)`。PR2-PR8
//! 按 spec §10.2 顺序填实。
//!
//! 镜像 `feishu/connector.rs` 的 PR1 模式。`with_status_callback` 接受
//! manager 注入的连接状态回调，PR2 拿到 Bot 句柄后才会实际调用它。

use std::sync::Arc;

use async_trait::async_trait;
use futures::stream::BoxStream;

use crate::connector::im::trait_def::{
    AuthFlow, ConnectorCapabilities, ConnectorContext, ConnectorError, IMConnector, InboundModel,
    ReplyContent, ReplyTarget,
};
use crate::connector::im::types::{ChannelConnectionState, ChannelMessage, Platform};

pub type WhatsappStatusCallback =
    Arc<dyn Fn(ChannelConnectionState, Option<String>) + Send + Sync + 'static>;

pub struct WhatsAppConnector {
    /// 状态回调。PR1 持有但不调用；PR2 拿到 `Bot::run()` 句柄后才会从
    /// event loop 里以 Connected / Reconnecting / NeedsReauth 触发。
    #[allow(dead_code)]
    on_status: WhatsappStatusCallback,
}

impl WhatsAppConnector {
    pub fn new() -> Self {
        Self::with_status_callback(Arc::new(|_state, _err| {}))
    }

    pub fn with_status_callback(on_status: WhatsappStatusCallback) -> Self {
        Self { on_status }
    }
}

impl Default for WhatsAppConnector {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl IMConnector for WhatsAppConnector {
    fn platform(&self) -> Platform {
        Platform::Whatsapp
    }

    fn capabilities(&self) -> ConnectorCapabilities {
        // 跟 spec §1 capability 表逐字对齐。PR1 必须立刻交付正确的 capabilities，
        // 否则 manager 无法在不动 platform 模块的情况下决定路由策略。
        ConnectorCapabilities {
            inbound: InboundModel::Stream,
            outbound_aicard: false,
            outbound_text_streaming: true,
            outbound_markdown: false,
            supports_attachments: true,
            supports_group_chat: false,
            supports_private_chat: true,
            auth_flow: AuthFlow::QRCode,
        }
    }

    async fn start(
        &self,
        _ctx: ConnectorContext,
    ) -> Result<BoxStream<'static, ChannelMessage>, ConnectorError> {
        Err(ConnectorError::NotSupported(
            "whatsapp::start — PR2 Bot 生命周期未实现",
        ))
    }

    async fn send(
        &self,
        _target: ReplyTarget,
        _content: ReplyContent,
    ) -> Result<(), ConnectorError> {
        Err(ConnectorError::NotSupported(
            "whatsapp::send — PR5 出站未实现",
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capabilities_match_phase4_spec() {
        let c = WhatsAppConnector::new();
        let caps = c.capabilities();
        // spec §1 表逐字对齐 —— 这是 capability 表的契约测试。
        assert_eq!(caps.inbound, InboundModel::Stream, "inbound = Stream (WS 长连)");
        assert!(!caps.outbound_aicard, "outbound_aicard = false (不发原生 AI 卡片)");
        assert!(
            caps.outbound_text_streaming,
            "outbound_text_streaming = true (走 edit_message 路径)"
        );
        assert!(!caps.outbound_markdown, "outbound_markdown = false (仅 *粗体* / _斜体_)");
        assert!(caps.supports_attachments, "supports_attachments = true (IMAGE/FILE 双向)");
        assert!(!caps.supports_group_chat, "MVP 私聊 only");
        assert!(caps.supports_private_chat);
        assert_eq!(caps.auth_flow, AuthFlow::QRCode);
    }

    #[test]
    fn platform_is_whatsapp() {
        let c = WhatsAppConnector::new();
        assert_eq!(c.platform(), Platform::Whatsapp);
    }

    #[tokio::test]
    async fn start_returns_not_supported_in_pr1() {
        let c = WhatsAppConnector::new();
        let ctx = test_ctx();
        let err = c.start(ctx).await.unwrap_err();
        match err {
            ConnectorError::NotSupported(msg) => assert!(msg.contains("PR2")),
            other => panic!("PR1 start should be NotSupported, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn send_returns_not_supported_in_pr1() {
        let c = WhatsAppConnector::new();
        let err = c
            .send(
                ReplyTarget {
                    session_id: "sess-1".into(),
                    external_conversation_key: "8613800138000@s.whatsapp.net".into(),
                },
                ReplyContent::Text("hi".into()),
            )
            .await
            .unwrap_err();
        match err {
            ConnectorError::NotSupported(msg) => assert!(msg.contains("PR5")),
            other => panic!("PR1 send should be NotSupported, got {other:?}"),
        }
    }

    fn test_ctx() -> ConnectorContext {
        use crate::connector::im::shared::config_store::ChannelConfigStore;
        use crate::runtime::pending::PendingQueueManager;
        use tokio_util::sync::CancellationToken;

        ConnectorContext {
            config_store: Arc::new(ChannelConfigStore::in_memory_for_tests()),
            secure_storage: None,
            ask_coordinator: None,
            pending_manager: Arc::new(PendingQueueManager::in_memory_for_tests()),
            cancel_token: CancellationToken::new(),
        }
    }
}
```

⚠️ `ChannelConfigStore::in_memory_for_tests` 和 `PendingQueueManager::in_memory_for_tests` 是假定**已存在**的测试辅助（feishu / telegram 应该用同样模式）。**先确认**：
```bash
grep -rn 'in_memory_for_tests' src-tauri/src/connector/im/shared/config_store.rs src-tauri/src/runtime/pending/ 2>/dev/null
```
**如果不存在**，把 `test_ctx()` 改成手动构造 minimal context——查 `feishu/connector.rs` 或 `telegram/connector.rs` 里的测试看怎么构造 `ConnectorContext`，照搬即可（这点是仓库私事实，plan 不预写代码，照搬就行）。

- [ ] **Step 2: 跑测试验证 4 个 case 全过**

Run: `cd src-tauri && cargo test --lib connector::im::whatsapp:: 2>&1 | tail -10`
Expected:
```
running 4 tests
test connector::im::whatsapp::connector::tests::platform_is_whatsapp ... ok
test connector::im::whatsapp::connector::tests::capabilities_match_phase4_spec ... ok
test connector::im::whatsapp::connector::tests::start_returns_not_supported_in_pr1 ... ok
test connector::im::whatsapp::connector::tests::send_returns_not_supported_in_pr1 ... ok
test result: ok. 4 passed; 0 failed
```

- [ ] **Step 3: 跑全 lib 测试确认没有其他回归**

Run: `cd src-tauri && cargo test --lib 2>&1 | tail -5`
Expected: `test result: ok. ... passed; 0 failed`（passed 数无所谓，0 failed 必须）。

- [ ] **Step 4: Commit Task 3 + Task 4**

```bash
git add src-tauri/src/connector/im/mod.rs \
        src-tauri/src/connector/im/whatsapp/
git commit -m "$(cat <<'EOF'
feat(connector/im/whatsapp): PR1 骨架（stub IMConnector + capabilities）

Phase 4 PR1 模块骨架。mirror feishu/ 的 PR1 stub 模式：
- whatsapp/mod.rs / connector.rs / types.rs 三个文件
- impl IMConnector：start / send 返回 NotSupported（PR2/PR5 填实）
- capabilities() 按 spec §1 逐字对齐：Stream / QRCode / 私聊 only /
  outbound_text_streaming=true / 双向附件
- 单测 4 个：capabilities 契约 / platform / start stub / send stub

不动 manager.rs（manager wiring 留到 PR3 扫码登录 +
register_whatsapp_connector）。

OpenClaw 同款方案，TOS 灰区，封号风险已在 mod.rs doc + spec §9.1
登记，PR8 前端 banner 必须告知用户。

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 5: factory.rs 加 build_whatsapp_connector + 类型别名

**Files:**
- Modify: `src-tauri/src/connector/im/factory.rs`

- [ ] **Step 1: 看 factory.rs 末尾结构**

Run: `tail -30 src-tauri/src/connector/im/factory.rs`
Expected: 看到 `build_telegram_connector` 函数（带 `pub fn ... -> anyhow::Result<...>` 签名）。

- [ ] **Step 2: 在文件末尾追加 whatsapp factory 函数**

Edit `src-tauri/src/connector/im/factory.rs`，在文件**最末**追加：

```rust

/// Build a `WhatsAppConnector` plus its concrete handle. PR1 stub —— concrete
/// 类型 PR2-PR8 会带上 Bot/SqliteStore 等内部状态。Manager wiring（包括
/// 注册路径、register_whatsapp_connector、reply_forwarder）留到 PR3。
pub fn build_whatsapp_connector(
    on_status: WhatsappStatusCallback,
) -> (
    Arc<dyn IMConnector>,
    Arc<crate::connector::im::whatsapp::connector::WhatsAppConnector>,
) {
    use crate::connector::im::whatsapp::connector::WhatsAppConnector;
    let concrete = Arc::new(WhatsAppConnector::with_status_callback(on_status));
    let dyn_handle: Arc<dyn IMConnector> = Arc::clone(&concrete) as Arc<dyn IMConnector>;
    (dyn_handle, concrete)
}

pub type WhatsappStatusCallback =
    Arc<dyn Fn(ChannelConnectionState, Option<String>) + Send + Sync + 'static>;
```

⚠️ 注意：`WhatsappStatusCallback` 已经在 `whatsapp/connector.rs` 里有一份完全相同的定义（Task 4 Step 1）——这里**重复定义**是有意的，跟 `TelegramStatusCallback` 等其它平台一致（types 跟 connector 模块各持一份别名），让外部使用方（manager / commands）只 import factory 就够。如果 reviewer 觉得是 DRY 违规，留 follow-up issue，PR1 不动其他平台的同款模式。

- [ ] **Step 3: 编译验证**

Run: `cd src-tauri && cargo build --lib 2>&1 | tail -10`
Expected: `Finished` 无 error。

- [ ] **Step 4: 跑全 lib 测试**

Run: `cd src-tauri && cargo test --lib 2>&1 | tail -5`
Expected: `0 failed`。

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/connector/im/factory.rs
git commit -m "$(cat <<'EOF'
feat(connector/im/whatsapp): PR1 加 factory::build_whatsapp_connector

镜像其他平台的 factory 模式 —— manager 不直接 new
WhatsAppConnector，必走 factory，让 PR3 manager wiring 时
review_im_layering.rs 第 2 个测试不破。

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 6: 跑 review_im_layering + cargo clippy 收尾

**Files:** （无修改，只跑校验）

- [ ] **Step 1: 跑架构约束测试**

Run: `cd src-tauri && cargo test --test review_im_layering 2>&1 | tail -10`
Expected: 3 个测试全过。`platforms` 数组里**不含** `"whatsapp"` 是有意的（PR8 加），这里不动它。

- [ ] **Step 2: 跑 cargo clippy 检查 PR1 新代码**

Run: `cd src-tauri && cargo clippy --lib -- -D warnings 2>&1 | tail -20`
Expected: `Finished` 0 warnings。如果有 warning，是新代码的就改（典型是 `unused_imports`、`dead_code`——后者用 `#[allow(dead_code)]` 注释为什么是 PR2 才用）；如果不是新代码触发的（pre-existing），原样保留，记到 task #1 comment。

- [ ] **Step 3: 跑 cargo fmt 检查**

Run: `cd src-tauri && cargo fmt -- --check 2>&1 | head -20`
Expected: 无输出。如果有 diff，运行 `cargo fmt` 修，并把 diff 一起 commit 到下一步。

- [ ] **Step 4: 跑前端 lint（确保 PR1 没意外触碰前端）**

Run: `pnpm exec tsc --noEmit 2>&1 | tail -5`
Expected: `Found 0 errors`。

Run: `pnpm lint 2>&1 | tail -10`
Expected: 无新 error。

- [ ] **Step 5: 收尾 commit（如果 cargo fmt 改了什么）**

```bash
# 仅当 cargo fmt 改了文件
git status
git diff --stat
# 如果有改动：
git add -u
git commit -m "$(cat <<'EOF'
style(connector/im/whatsapp): cargo fmt

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

如果没改动，跳过。

- [ ] **Step 6: 更新 TaskList**

调 TaskUpdate 把 Phase 4 task 状态推进。具体：把任务 #1 的 description 补一行 "PR1 完成 commit hashes: <列 3-4 个 hash>"，status 仍保 `in_progress`（PR2-PR8 没做）。

---

## Self-Review

**1. Spec coverage（PR1 范围）**

按 spec §10.2 PR1 行的内容："`im/whatsapp/` 目录 + `Platform::Whatsapp` enum 变体（含 `as_str / from_str / all`）+ capabilities + factory 入口 + `Cargo.toml` 加 `whatsapp-rust = "0.1"` 依赖"。逐项核：

| spec 项 | 本 plan task | 状态 |
|---|---|---|
| `im/whatsapp/` 目录 | Task 3 + Task 4 | ✅ |
| `Platform::Whatsapp` enum 变体 + `as_str/from_str/all` | （仓库已存在） | ✅ 不需做 |
| capabilities | Task 4 Step 1 `capabilities()` 实现 | ✅ |
| factory 入口 | Task 5 | ✅ |
| `Cargo.toml` whatsapp-rust 依赖 | Task 1 | ✅ |

跟 spec 偏差 4 处：① crate version 0.1 → 0.6（crates.io 实测）② `outbound_text_streaming` 字段从 Phase 3 PR1.5 前置移到 PR1（PR1 必须有这个字段才能写 capabilities）③ 伴生 crate 延后到 PR2 ④ `Platform::Whatsapp` / `NeedsReauth` 已存在跳过。这 4 处在 plan 头部"与 spec 的偏离"明确登记。

**2. Placeholder scan**

搜 plan 全文：
- ✅ 无 "TBD" / "TODO" / "implement later"
- ✅ 无 "Add appropriate error handling"
- ✅ 无 "Write tests for the above"
- ✅ 无 "Similar to Task N"
- Task 4 Step 1 有一段 ⚠️ 说"先确认 `in_memory_for_tests` 存不存在；不存在的话查 feishu 测试照搬"——这是**有意为之**的私事实查询，不算 placeholder（plan 不应该把仓库私 API 名 hard-code 进去）

**3. Type consistency**

- `WhatsappStatusCallback` 别名：Task 4 Step 1 在 `whatsapp/connector.rs` 定义，Task 5 Step 2 在 `factory.rs` 再定义（**重复但 intentional**，跟 telegram/wecom 同款模式）。reviewer 可能挑，plan 已在 Task 5 Step 2 注释里解释。
- `WhatsAppConnector` 大小写：所有地方都是 `WhatsApp`（驼峰），不是 `Whatsapp`。Platform enum 是 `Whatsapp`（Rust enum 惯例 + 已存在），不动。
- 子模块名：`mod.rs:11 pub mod connector;` ↔ `mod.rs:12 pub mod types;` ↔ `connector.rs:5 use crate::connector::im::whatsapp::types::...`：本 PR types.rs 只 export `WhatsAppSessionTarget`，PR1 connector.rs **不引用** types.rs（PR4 parser 才会用 SessionTarget）——所以 PR1 types.rs 的 `WhatsAppSessionTarget` 实际是死代码。Task 3 Step 1 应该加 `#[allow(dead_code)]`。

**修一处类型一致性问题**：在 Task 3 Step 1 的 `types.rs` 内容首行 `#[derive(...)]` 上方加 `#[allow(dead_code)]`：

```rust
#[allow(dead_code)] // PR4 parser 会用；PR1 引入是为了让模块文件结构跟 spec §2 对齐
#[derive(Debug, Clone)]
pub struct WhatsAppSessionTarget {
```

（plan 已就地更新——上面 Task 3 Step 1 重新读一遍，将 `WhatsAppSessionTarget` 的 derive 上方加 `#[allow(dead_code)]`。）

---

## Execution Handoff

Plan 完成并保存到 `docs/superpowers/plans/2026-05-20-im-whatsapp-phase4-pr1.md`。两种执行方式：

**1. Subagent-Driven（推荐）** —— 每个 Task 派一个新 subagent（haiku 跑前 5 个 task，sonnet 跑 Task 4 + Task 6 因为涉及推理）；我在每 Task 之间 review diff + 跑测试，失败立刻让 subagent 改。这样 PR1 整体 1-2 小时内可落，每步可中断。

**2. Inline Execution** —— 我在当前 session 顺序跑 6 个 task，每 2 个 task 一个 checkpoint 让你 review。中途打断成本更高，但 token 占用低。

哪种？
