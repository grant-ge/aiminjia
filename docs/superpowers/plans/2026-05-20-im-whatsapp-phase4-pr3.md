# Phase 4 WhatsApp PR3 — 扫码登录（begin/poll_registration + UI 接入）

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 把 PR1/PR2 的骨架升级成真能扫码——用户点"添加 WhatsApp 账号"按钮 → 扫 QR → 写 config.json + 状态进 Connected。**不**实现入站消息处理（PR4）和出站（PR5）。

**Architecture:** `begin_registration` 起 wa-rs `Bot::builder()` + `bot.run()` 异步 task，**立即返回**不等 QR；`on_event` 闭包捕获 `Arc<Mutex<PairingState>>`，事件到达后 lock + 写状态机。`poll_registration` 每 2s 被前端调用一次，读 PairingState 映射到 `ChannelRegistrationPollState`。`Event::PairSuccess` 时写 `config.json`。复用现有 `RegistrationModal mode='qr_url'`（跟 wechat 同款）。

**Tech Stack:** `wa-rs = "0.2"` (PR1 已加)。伴生 crate `wa-rs-sqlite-storage` / `wa-rs-tokio-transport` / `wa-rs-ureq-http` 已被 wa-rs default features 拉进 build graph（PR1 实测）。前端复用 `RegistrationModal` + `QrCodeCanvas`。

**Spec 来源：** `docs/superpowers/specs/2026-05-18-im-whatsapp-phase4-design.md` §3.6（扫码流程）+ §3.7（Tauri 命令分支）+ §3.8（RegistrationModal mode='qr_url'）+ §3.9（重新扫码）+ §3.10（allowFrom 字段已在 PR2 加好）+ §9.1（首次扫码风险 banner）。

---

## File Structure（PR3 范围）

新建：
- `src-tauri/src/connector/im/whatsapp/runtime.rs` — Bot 构造 + event handler 闭包（拆出来避免 connector.rs 太长）。~150 行。
- `src/features/channel/WhatsappChannelConfig.tsx` — 前端 channel 配置卡片 + 首次扫码风险 banner + 触发 RegistrationModal。~250 行。
- `src/features/channel/WhatsappRiskBanner.tsx` — 一次性风险 banner 组件（"我已知晓"勾选 + 继续按钮）。~80 行。

修改：
- `src-tauri/src/connector/im/whatsapp/connector.rs` — `begin_registration` / `poll_registration` 真实现（不再返 NotSupported）。
- `src-tauri/src/connector/im/whatsapp/mod.rs` — 加 `pub mod runtime;`
- `src-tauri/src/connector/im/manager.rs` — 加 `register_whatsapp_connector` + `begin_whatsapp_registration` + `poll_whatsapp_registration` + `set_whatsapp_connection_state`
- `src-tauri/src/commands/channel.rs` — `channel_begin_registration` / `channel_poll_registration` 加 `Platform::Whatsapp` arm
- `src-tauri/src/lib.rs` — 启动期注册 whatsapp connector（如果 config.json 存在则自动起 Bot 复用既有 session）
- `src/lib/tauri.ts` — 不动（已有的 `channel_begin_registration` / `channel_poll_registration` 是 platform-neutral）
- `src/features/channel/ChannelConfig.tsx` — 路由到 `WhatsappChannelConfig`（如果有 generic dispatcher 的话；否则前端只新增组件，dispatcher 路由由 PR8 完成）
- `src/features/channel/ChannelPage.tsx` — whatsapp 平台卡片"添加账号"按钮 wire 到 WhatsappChannelConfig

不动：
- `factory.rs`（PR1 已加 `build_whatsapp_connector`）
- `types.rs`（PairingState 已加在 PR2）
- `session.rs` / `config.rs`（PR2 已写好）
- `Cargo.toml`
- 其它平台的 connector

---

## Task 1: connector.rs 加 `runtime.rs` mod 引用 + 准备 register_session 入口

**Files:**
- Modify: `src-tauri/src/connector/im/whatsapp/mod.rs`

- [ ] **Step 1: 加 runtime mod 引用**

Edit `src-tauri/src/connector/im/whatsapp/mod.rs`：插入 `pub mod runtime;` 在 `pub mod session;` 之前（保持 alphabetical）：

```rust
pub mod config;
pub mod connector;
pub mod runtime;
pub mod session;
pub mod types;

pub use connector::WhatsAppConnector;
```

- [ ] **Step 2: 编译——预期 build fail（runtime.rs 还没建）**

Run: `cd src-tauri && cargo build --lib 2>&1 | tail -5`
Expected: `error[E0583]: file not found for module \`runtime\``

跟 PR1 Task 3 同款套路：Task 1 单独 commit 不可行（编译破），让 Task 2 一起 commit。

- [ ] **Step 3: 不 commit，进 Task 2**

---

## Task 2: 写 runtime.rs Bot 构造 + event handler

**Files:**
- Create: `src-tauri/src/connector/im/whatsapp/runtime.rs`

- [ ] **Step 1: 先写 runtime.rs 起手 + 测试块**

Create `src-tauri/src/connector/im/whatsapp/runtime.rs`：

```rust
//! Bot 构造 + event handler 闭包。spec v3 §3.6。
//!
//! 拆出来避免 connector.rs 太长。本模块只暴露 `start_bot(...)` 一个入口；
//! 内部构造 wa-rs Bot 并起 bot.run()，返回 `JoinHandle<()>` 给 connector
//! 存到 `bot_handle` 字段。

use std::path::Path;
use std::sync::Arc;
use std::time::Instant;

use tokio::sync::Mutex;
use tokio::task::JoinHandle;
use wa_rs::bot::Bot;
use wa_rs::store::SqliteStore;
use wa_rs::TokioRuntime;
use wa_rs_tokio_transport::TokioWebSocketTransportFactory;
use wa_rs_ureq_http::UreqHttpClient;
use wacore::types::events::Event;

use super::config::{self, WhatsAppChannelConfig};
use super::session::WhatsAppPaths;
use super::types::PairingState;

/// 起 Bot 并返回 bot.run() 的 JoinHandle。调用方负责存储这个 handle
/// （connector 把它放进 `bot_handle: Mutex<Option<JoinHandle<()>>>`）。
///
/// `pairing_state` 是 connector 持有的 Arc<Mutex<PairingState>>；
/// 本函数克隆它进 on_event 闭包，事件到达后 lock + 写状态。
pub async fn start_bot(
    paths: WhatsAppPaths,
    pairing_state: Arc<Mutex<PairingState>>,
) -> anyhow::Result<JoinHandle<()>> {
    let db_path = paths.session_db();
    let db_path_str = db_path
        .to_str()
        .ok_or_else(|| anyhow::anyhow!("session.db path is not valid UTF-8: {db_path:?}"))?;
    let backend = Arc::new(SqliteStore::new(db_path_str).await?);

    let paths_for_closure = paths.clone();
    let state_for_closure = Arc::clone(&pairing_state);

    let mut bot = Bot::builder()
        .with_backend(backend)
        .with_transport_factory(TokioWebSocketTransportFactory::new())
        .with_http_client(UreqHttpClient::new())
        .with_runtime(TokioRuntime)
        .on_event(move |event, _client| {
            let paths = paths_for_closure.clone();
            let pairing_state = Arc::clone(&state_for_closure);
            async move {
                handle_event(event, &paths, pairing_state).await;
            }
        })
        .build()
        .await?;

    // 起 PairingState → AwaitingQr（在 spawn 之前写，避免 race）
    {
        let mut state = pairing_state.lock().await;
        *state = PairingState::AwaitingQr { started_at: Instant::now() };
    }

    Ok(bot.run().await?)
}

/// 处理 wa-rs 事件。PR3 只处理 pairing 相关的 3 个 event；其它 event drop。
/// PR4 入站 worker 会再加 Event::Message 的处理。
async fn handle_event(
    event: Event,
    paths: &WhatsAppPaths,
    pairing_state: Arc<Mutex<PairingState>>,
) {
    match event {
        Event::PairingQrCode { code, timeout } => {
            log::info!("[whatsapp] received PairingQrCode (timeout={:?})", timeout);
            let mut state = pairing_state.lock().await;
            *state = PairingState::QrIssued {
                code,
                expires_at: Instant::now() + timeout,
            };
        }
        Event::PairSuccess(success) => {
            let jid = success.id.to_string();
            let push_name = success.business_name.clone(); // wa-rs PairSuccess 里这字段最接近 displayName
            log::info!("[whatsapp] PairSuccess jid={} push_name={}", jid, push_name);

            // 写 config.json
            let cfg = WhatsAppChannelConfig {
                schema_version: 1,
                jid: jid.clone(),
                push_name: push_name.clone(),
                paired_at: chrono::Utc::now().to_rfc3339(),
                allow_from: None,
            };
            if let Err(e) = config::write(&paths.config_path(), &cfg) {
                log::error!("[whatsapp] failed to write config.json after PairSuccess: {e:#}");
                // 配对已成功但元数据写失败——状态机仍标 Connected，下次启动时
                // wa-rs SqliteStore 里有凭证就够用。
            }

            let mut state = pairing_state.lock().await;
            *state = PairingState::Connected { jid, push_name };
        }
        Event::PairError(err) => {
            log::warn!("[whatsapp] PairError: {}", err.error);
            // 不动 PairingState；让 poll_registration 在 QrIssued.expires_at 到期时
            // 自然返 Expired。本设计避免把 PairingState 加 Failed 变体（spec v3 §3.5）。
        }
        Event::Connected(_) => {
            log::info!("[whatsapp] Connected (post-pairing or returning session)");
            // 已配对会话恢复时也会到这里；如果 pairing_state 还是 Idle（启动期复用
            // 既有 session.db），读 config.json 把 PairingState 推到 Connected。
            let mut state = pairing_state.lock().await;
            if matches!(*state, PairingState::Idle | PairingState::AwaitingQr { .. }) {
                if let Ok(Some(cfg)) = config::read(&paths.config_path()) {
                    *state = PairingState::Connected {
                        jid: cfg.jid,
                        push_name: cfg.push_name,
                    };
                }
            }
        }
        _ => {
            // PR4 才处理 Event::Message 等。
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn tmp_paths() -> (TempDir, WhatsAppPaths) {
        let dir = TempDir::new().unwrap();
        let base = dir.path().join("channels").join("whatsapp");
        let paths = WhatsAppPaths::new(&base);
        paths.ensure_base_dir().unwrap();
        (dir, paths)
    }

    /// 当 PairingState 不是 QrIssued / Connected 时，PairingQrCode 应该把它推到
    /// QrIssued 并填 code。
    #[tokio::test]
    async fn handle_event_pairing_qr_code_sets_qr_issued() {
        let (_dir, paths) = tmp_paths();
        let state = Arc::new(Mutex::new(PairingState::AwaitingQr {
            started_at: Instant::now(),
        }));
        let event = Event::PairingQrCode {
            code: "1@test_qr".into(),
            timeout: std::time::Duration::from_secs(60),
        };
        handle_event(event, &paths, Arc::clone(&state)).await;
        let s = state.lock().await;
        match &*s {
            PairingState::QrIssued { code, .. } => assert_eq!(code, "1@test_qr"),
            other => panic!("expected QrIssued, got {other:?}"),
        }
    }

    /// PairSuccess 应该：(1) 写 config.json (2) 推 PairingState 到 Connected
    #[tokio::test]
    async fn handle_event_pair_success_writes_config_and_sets_connected() {
        use wa_rs::types::Jid;
        use wacore::types::events::PairSuccess;

        let (_dir, paths) = tmp_paths();
        let state = Arc::new(Mutex::new(PairingState::QrIssued {
            code: "1@test".into(),
            expires_at: Instant::now() + std::time::Duration::from_secs(60),
        }));
        let event = Event::PairSuccess(PairSuccess {
            id: Jid::from_string("8613800138000@s.whatsapp.net").unwrap(),
            lid: Jid::from_string("8613800138000@lid").unwrap(),
            business_name: "Alice".into(),
            platform: "android".into(),
        });
        handle_event(event, &paths, Arc::clone(&state)).await;

        // config.json 写了
        let cfg = config::read(&paths.config_path()).unwrap().expect("config should exist");
        assert_eq!(cfg.jid, "8613800138000@s.whatsapp.net");
        assert_eq!(cfg.push_name, "Alice");

        // PairingState 是 Connected
        let s = state.lock().await;
        match &*s {
            PairingState::Connected { jid, push_name } => {
                assert_eq!(jid, "8613800138000@s.whatsapp.net");
                assert_eq!(push_name, "Alice");
            }
            other => panic!("expected Connected, got {other:?}"),
        }
    }

    /// Connected event 时如果 PairingState 还是 Idle/AwaitingQr 且 config.json 存在，
    /// 应该读 config 推到 Connected（启动期复用既有 session 场景）。
    #[tokio::test]
    async fn handle_event_connected_recovers_state_from_config() {
        use wacore::types::events::Connected;

        let (_dir, paths) = tmp_paths();
        // 写一个已存在的 config.json 模拟"老用户"
        let cfg = WhatsAppChannelConfig {
            schema_version: 1,
            jid: "8613912345678@s.whatsapp.net".into(),
            push_name: "Bob".into(),
            paired_at: "2026-05-19T10:00:00Z".into(),
            allow_from: None,
        };
        config::write(&paths.config_path(), &cfg).unwrap();

        let state = Arc::new(Mutex::new(PairingState::Idle));
        handle_event(Event::Connected(Connected {}), &paths, Arc::clone(&state)).await;

        let s = state.lock().await;
        match &*s {
            PairingState::Connected { jid, push_name } => {
                assert_eq!(jid, "8613912345678@s.whatsapp.net");
                assert_eq!(push_name, "Bob");
            }
            other => panic!("expected Connected from config recovery, got {other:?}"),
        }
    }

    /// PairError 不应该改变 PairingState（spec v3 §3.5 砍掉 Failed 变体）。
    #[tokio::test]
    async fn handle_event_pair_error_does_not_change_state() {
        use wa_rs::types::Jid;
        use wacore::types::events::PairError;

        let (_dir, paths) = tmp_paths();
        let started_at = Instant::now();
        let state = Arc::new(Mutex::new(PairingState::AwaitingQr { started_at }));
        let event = Event::PairError(PairError {
            id: Jid::from_string("8613800138000@s.whatsapp.net").unwrap(),
            lid: Jid::from_string("8613800138000@lid").unwrap(),
            business_name: String::new(),
            platform: String::new(),
            error: "socket timeout".into(),
        });
        handle_event(event, &paths, Arc::clone(&state)).await;

        let s = state.lock().await;
        // 状态仍是 AwaitingQr（虽然 started_at 是新 Instant，但变体没变）
        assert!(matches!(*s, PairingState::AwaitingQr { .. }));
    }
}
```

⚠️ **重要 caveat**：上面测试代码引用了 `wa_rs::types::Jid` / `wacore::types::events::{Connected, PairSuccess, PairError}` 等具体类型——这些类型签名和构造方式我**没有 100% 验证过 wa-rs 0.2 的实际 API**。实施时**先**：

1. 跑 `cd src-tauri && cargo check 2>&1 | head -30` 看 use 是否能 resolve
2. 如果某些类型路径错（比如 `Jid::from_string` 实际是 `Jid::new` 或 `Jid::parse`），**先 grep wa-rs crate 看实际签名**：`find ~/.cargo/registry/src -path '*wa-rs-0.2*/src/lib.rs' -o -path '*wacore*/src/types/events.rs' 2>/dev/null | head` 然后读对应文件
3. 调整测试代码用真实的 API
4. 如果 `wa_rs::store::SqliteStore` 路径错，看 `wa-rs-sqlite-storage` 的 re-export

如果**调整超过 30 分钟还在解 import 路径**，停下来报 BLOCKED 让 controller 决定。

- [ ] **Step 2: 编译，确认 use 路径都对**

Run: `cd src-tauri && cargo check --lib 2>&1 | tail -20`
Expected: 编译成功，或者 transitive use 路径错时 fail 明确指出哪个 import resolve 不到——按 caveat 调整。

- [ ] **Step 3: 跑测试**

Run: `cd src-tauri && cargo test --lib connector::im::whatsapp::runtime:: 2>&1 | tail -10`
Expected: 4 tests pass。

如果测试构造 PairSuccess/PairError 失败（字段不对），按 wacore 实际定义调整。**关键事实**：spec 已经 grep 过 wacore，`PairSuccess { id, lid, business_name, platform }` 是真实字段；implementer 可能要把它从 unit struct 改成对应的真 struct（PR3 brainstorm 阶段读 wacore/src/types/events.rs 已确认这 4 字段存在）。

- [ ] **Step 4: Task 1 + Task 2 一起 commit**

```bash
git add src-tauri/src/connector/im/whatsapp/mod.rs \
        src-tauri/src/connector/im/whatsapp/runtime.rs
git commit -m "$(cat <<'EOF'
feat(connector/im/whatsapp): PR3 加 runtime.rs Bot 构造 + event handler

spec v3 §3.6。`start_bot()` 入口构造 wa-rs Bot::builder() + SqliteStore，
on_event 闭包捕获 Arc<Mutex<PairingState>>，事件到达 lock + 写状态。

支持 4 个 event：
- PairingQrCode → 写 QrIssued
- PairSuccess → 写 config.json + 推 Connected
- PairError → 不动状态（让 poll 走过期路径）
- Connected → 启动期复用既有 session 时从 config.json 恢复状态

4 个 unit test 覆盖：QR 推 QrIssued / PairSuccess 写 config + Connected /
Connected 启动期恢复 / PairError 不改状态。

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

记录 commit SHA。

---

## Task 3: connector.rs 实现 begin_registration + poll_registration

**Files:**
- Modify: `src-tauri/src/connector/im/whatsapp/connector.rs`

PR2 完成时 `begin_registration` / `poll_registration` 都是 trait default `NotSupported`（不在 connector.rs 里 override）。本 task 加 override 实现。

- [ ] **Step 1: 编辑 connector.rs，在 impl IMConnector 里加两个方法**

在 `impl IMConnector for WhatsAppConnector { ... }` 块内、`stop()` 方法之前，加：

```rust
    async fn begin_registration(
        &self,
        _req: &crate::connector::im::trait_def::RegistrationRequest,
    ) -> Result<crate::connector::im::trait_def::RegistrationBegin, ConnectorError> {
        // 此方法被 manager.begin_whatsapp_registration 调用。
        // 调用方负责传 `paths`，但 trait 签名没暴露 paths——所以 manager
        // 在 begin_whatsapp_registration 里 **不通过 trait** 调用本方法，
        // 而是直接调用 connector 的具体方法 `start_pairing_session(paths)`
        // （下面 inherent impl）。这里 trait 的 begin_registration 仍返
        // NotSupported，因为 connector 单独看没有路径上下文。
        Err(ConnectorError::NotSupported(
            "whatsapp::begin_registration — 走 connector.start_pairing_session(paths)",
        ))
    }

    async fn poll_registration(
        &self,
        _req: &crate::connector::im::trait_def::PollRequest,
    ) -> Result<crate::connector::im::trait_def::RegistrationPoll, ConnectorError> {
        // 同 begin_registration 注释：走 connector.poll_pairing_state()。
        Err(ConnectorError::NotSupported(
            "whatsapp::poll_registration — 走 connector.poll_pairing_state()",
        ))
    }
```

然后在 `impl WhatsAppConnector { ... }` inherent block 加两个具体方法 `start_pairing_session(paths)` + `poll_pairing_state()`，这两个是 manager 真实调用的入口：

```rust
    /// 起一次 pairing 会话。**Manager 入口**：manager.begin_whatsapp_registration
    /// 解析 scope → 路径 → 调本方法。
    pub async fn start_pairing_session(
        &self,
        paths: super::session::WhatsAppPaths,
    ) -> anyhow::Result<()> {
        // 启动备份兜底（spec v3 §3.3）
        let _backed_up = super::session::backup_session_db_if_present(&paths)?;
        paths.ensure_base_dir()?;

        // 起 Bot
        let handle = super::runtime::start_bot(paths, Arc::clone(&self.pairing_state)).await?;

        // 存 join handle
        *self.bot_handle.lock().await = Some(handle);
        Ok(())
    }

    /// Manager 入口：拉一次 PairingState 当前快照，给 poll_whatsapp_registration 用。
    pub async fn poll_pairing_state(&self) -> super::types::PairingState {
        self.pairing_state.lock().await.clone()
    }
```

⚠️ `super::types::PairingState` 当前 `#[derive(Debug, Clone)]`（PR2 已加 Clone）。verify 是否真有 Clone derive；如果没有，加它。

Run: `grep '^#\[derive' src-tauri/src/connector/im/whatsapp/types.rs | head -3` 确认 PairingState 有 `Clone` derive。

- [ ] **Step 2: 跑 connector tests + 编译**

Run: `cd src-tauri && cargo build --lib 2>&1 | tail -5`
Expected: `Finished`。

Run: `cd src-tauri && cargo test --lib connector::im::whatsapp::connector:: 2>&1 | tail -15`
Expected: PR2 既有的 6 个测试还过。注意：`start_still_returns_not_supported_in_pr2` 测试断言 `start()` returns NotSupported——本任务不动 `start()`（PR4 才动），所以这条仍过。

- [ ] **Step 3: Commit**

```bash
git add src-tauri/src/connector/im/whatsapp/connector.rs
git commit -m "$(cat <<'EOF'
feat(connector/im/whatsapp): PR3 加 start_pairing_session + poll_pairing_state

inherent methods（不走 trait）：manager 直接调，因为需要传 paths 参数，
trait IMConnector::begin/poll_registration 没暴露 paths 上下文。

start_pairing_session：
  1. backup_session_db_if_present（启动备份兜底）
  2. ensure_base_dir
  3. runtime::start_bot(paths, pairing_state) 起 Bot::run() task
  4. 存 JoinHandle 到 self.bot_handle

poll_pairing_state：返回 PairingState 当前快照（Clone）。

trait 的 begin/poll_registration 仍返 NotSupported，message 指向具体方法。

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 4: manager.rs 加 register_whatsapp_connector + begin/poll_whatsapp_registration

**Files:**
- Modify: `src-tauri/src/connector/im/manager.rs`

这是 PR3 最复杂的一步——manager 是 5000+ 行的大文件，要在恰当位置插入 4 个新方法。

- [ ] **Step 1: 加 register_whatsapp_connector**

参考 `register_wechat_connector`（行 922-944）的形态。在 `register_telegram_connector` 之后、`register_wechat_connector` 之前（或就近 wechat 之后）加：

```rust
    async fn register_whatsapp_connector(
        &self,
        on_status: super::factory::WhatsappStatusCallback,
    ) -> Arc<super::whatsapp::connector::WhatsAppConnector> {
        let (dyn_conn, concrete) = super::factory::build_whatsapp_connector(on_status);
        let mut map = self.connectors.write().await;
        map.insert(Platform::Whatsapp, dyn_conn);
        concrete
    }
```

签名跟 wechat 比少了 ilink_bot_id / base_url 等参数——因为 wa-rs 的 SqliteStore 自管所有凭证，不需要 manager 传入。

- [ ] **Step 2: 加 begin_whatsapp_registration**

参考 `begin_wechat_registration`（行 2387-2400）的形态。在 wechat 那块附近加：

```rust
    /// Phase 4 PR3：起 WhatsApp 扫码会话。Manager 解析 scope → paths → 调
    /// connector.start_pairing_session。如果已配对（config.json 存在）则
    /// **删 config + session.db 走重新扫码**（spec v3 §3.9）。
    pub async fn begin_whatsapp_registration(&self) -> Result<ChannelRegistrationBeginResult> {
        // 1. 解析 paths
        let paths = self.resolve_whatsapp_paths()?;

        // 2. 检查是否重新扫码场景：config.json 存在 → 删 config + session.db
        if paths.config_path().exists() {
            log::info!("[whatsapp] config.json exists — clearing for re-pairing");
            // 先 stop 旧 connector（如果有）
            if let Some(conn) = self.connectors.read().await.get(&Platform::Whatsapp).cloned() {
                let _ = conn.stop().await;
            }
            super::whatsapp::session::delete_for_reauth(&paths)?;
        }

        // 3. 注册或拿到 concrete connector
        let on_status = self.make_whatsapp_status_callback();
        let concrete = self.register_whatsapp_connector(on_status).await;

        // 4. 起 pairing session
        concrete.start_pairing_session(paths).await?;

        Ok(ChannelRegistrationBeginResult {
            device_code: "whatsapp".to_string(),         // 单账号约定常量
            user_code: String::new(),
            verification_uri_complete: String::new(),     // QR 还没生成；poll 时返回
            verification_uri: String::new(),
            interval_seconds: 2,
            expires_in_seconds: 60,                       // wa-rs PairingQrCode 默认 timeout
            source: "whatsapp_web".to_string(),
        })
    }
```

- [ ] **Step 3: 加 poll_whatsapp_registration**

参考 `poll_wechat_registration`（行 2407-2557）的形态。简化版：

```rust
    /// Phase 4 PR3：拉一次 WhatsApp PairingState 当前快照。
    pub async fn poll_whatsapp_registration(
        &self,
        _device_code: String,                            // 单账号下忽略；仅保持 trait 一致性
    ) -> Result<ChannelRegistrationPollResult> {
        let conn_arc = self
            .whatsapp_concrete_or_err()
            .ok_or_else(|| anyhow::anyhow!("whatsapp connector not registered"))?;
        let state = conn_arc.poll_pairing_state().await;

        use super::whatsapp::types::PairingState;
        use std::time::Instant;

        let result = match state {
            PairingState::Idle | PairingState::AwaitingQr { .. } => {
                ChannelRegistrationPollResult {
                    state: ChannelRegistrationPollState::Waiting,
                    client_id: None,
                    robot_code: None,
                    config: None,
                    platform_state: None,
                    fail_reason: None,
                }
            }
            PairingState::QrIssued { code, expires_at } => {
                if Instant::now() >= expires_at {
                    ChannelRegistrationPollResult {
                        state: ChannelRegistrationPollState::Expired,
                        client_id: None,
                        robot_code: None,
                        config: None,
                        platform_state: None,
                        fail_reason: Some("QR code expired".into()),
                    }
                } else {
                    // 把 QR string 通过 fail_reason JSON envelope 返回，跟 wechat 同款约定
                    let payload = serde_json::json!({
                        "kind": "qr",
                        "qr_url": code,
                        "expires_in_seconds": (expires_at - Instant::now()).as_secs(),
                    });
                    ChannelRegistrationPollResult {
                        state: ChannelRegistrationPollState::Waiting,
                        client_id: None,
                        robot_code: None,
                        config: None,
                        platform_state: None,
                        fail_reason: Some(payload.to_string()),
                    }
                }
            }
            PairingState::Connected { jid, push_name } => {
                log::info!("[whatsapp] pairing success: jid={} push_name={}", jid, push_name);
                self.set_whatsapp_connection_state(
                    ChannelConnectionState::Connected,
                    None,
                ).await;
                let payload = serde_json::json!({
                    "kind": "whatsapp_success",
                    "jid": jid,
                    "push_name": push_name,
                });
                ChannelRegistrationPollResult {
                    state: ChannelRegistrationPollState::Success,
                    client_id: None,
                    robot_code: None,
                    config: None,
                    platform_state: None,                  // PR3 暂不返完整 platform_state；前端通过 channel:platform-state 拿
                    fail_reason: Some(payload.to_string()),
                }
            }
        };
        Ok(result)
    }
```

- [ ] **Step 4: 加 set_whatsapp_connection_state + whatsapp_concrete_or_err + resolve_whatsapp_paths + make_whatsapp_status_callback 辅助方法**

```rust
    fn resolve_whatsapp_paths(&self) -> Result<super::whatsapp::session::WhatsAppPaths> {
        let dir = self.config_store.platform_dir(Platform::Whatsapp);
        Ok(super::whatsapp::session::WhatsAppPaths::new(dir))
    }

    fn whatsapp_concrete_or_err(&self) -> Option<Arc<super::whatsapp::connector::WhatsAppConnector>> {
        // PR3 简化：每次 begin 时重建 connector，concrete handle 不持久化。
        // PR4 时 manager 拿 dyn handle 也可以驱动入站。为了 poll 能拿到 concrete
        // 类型，临时再 build 一份是错的（会丢 PairingState）。
        //
        // 正确实现：register_whatsapp_connector 时缓存 concrete 到 self 字段
        // （类似 self.cached_telegram_concrete: Arc<RwLock<Option<Arc<TelegramConnector>>>>）。
        //
        // 看 manager.rs 行 ~96-100 附近 telegram 怎么缓存的，照搬。
        unimplemented!("see comment: cache concrete handle in self field, mirror telegram")
    }

    fn make_whatsapp_status_callback(&self) -> super::factory::WhatsappStatusCallback {
        // 仿 register_wechat_connector 的回调风格（manager 实例化时通过
        // Arc::clone(&self) capture）。具体形态参考 register_wechat_connector
        // 里 on_status 闭包是如何构造的（搜索 "set_wechat_connection_state(state" 上下文）。
        let app_handle = self.app_handle.clone();
        Arc::new(move |state, last_error| {
            let _ = app_handle.emit(
                "channel:platform-state",
                &ChannelPlatformStatePayload {
                    state: ChannelPlatformState {
                        platform: Platform::Whatsapp,
                        capability: ChannelCapability::Available,
                        configured: matches!(state, ChannelConnectionState::Connected),
                        enabled: matches!(state, ChannelConnectionState::Connected),
                        connection: state,
                        config: None,
                        last_connected_at: None,
                        last_error,
                    },
                },
            );
        })
    }

    async fn set_whatsapp_connection_state(
        &self,
        connection: ChannelConnectionState,
        last_error: Option<String>,
    ) {
        log::info!(
            "[channel/whatsapp] set_whatsapp_connection_state connection={:?} last_error={:?}",
            connection, last_error
        );
        self.platform_state_mutate(Platform::Whatsapp, |s| {
            s.connection = connection.clone();
            s.last_error = last_error.clone();
        }).await;

        let _ = self.app_handle.emit(
            "channel:platform-state",
            &ChannelPlatformStatePayload {
                state: ChannelPlatformState {
                    platform: Platform::Whatsapp,
                    capability: ChannelCapability::Available,
                    configured: matches!(connection, ChannelConnectionState::Connected),
                    enabled: matches!(connection, ChannelConnectionState::Connected),
                    connection,
                    config: None,
                    last_connected_at: None,
                    last_error,
                },
            },
        );
    }
```

⚠️ **重要 caveat**：`whatsapp_concrete_or_err` 标了 `unimplemented!()`——这是 plan 故意留的设计缺口，implementer **必须**：

1. 先 grep `self\.telegram_concrete\|fn telegram_concrete\|cached_telegram` 等关键字找 telegram 是怎么缓存的
2. 在 `ChannelManager` struct 加 `cached_whatsapp_concrete: RwLock<Option<Arc<super::whatsapp::connector::WhatsAppConnector>>>` 字段（位置见 manager.rs ~80-100 行）
3. `register_whatsapp_connector` 末尾把 concrete 写进这个字段
4. `whatsapp_concrete_or_err()` 读这个字段返回 clone

如果 telegram 用的模式不一样（比如直接从 connectors map 里 downcast），照搬 telegram 的方式。**不要自己设计**。

- [ ] **Step 5: 编译**

Run: `cd src-tauri && cargo build --lib 2>&1 | tail -10`
Expected: `Finished`。如果 `unimplemented!()` 触发——说明 implementer 没替换它，要回去重做 Step 4 的 caveat。

- [ ] **Step 6: 跑回归测试**

Run: `cd src-tauri && cargo test --lib connector::im 2>&1 | grep -E '^test result' | tail -3`
Expected: 0 NEW failures（pre-existing 8 download failures保持不变）。

- [ ] **Step 7: Commit**

```bash
git add src-tauri/src/connector/im/manager.rs
git commit -m "$(cat <<'EOF'
feat(connector/im/whatsapp): PR3 manager 接入扫码 + 状态机

spec v3 §3.6 / §3.9。

新加 6 个 manager 方法：
- register_whatsapp_connector：用 factory build + 写 connectors map +
  缓存 concrete 到 cached_whatsapp_concrete（仿 telegram 模式）
- begin_whatsapp_registration：解析 paths → 检查 config.json 是否存在
  → 走重新扫码路径（stop 旧 + delete_for_reauth）→ 起 pairing session
- poll_whatsapp_registration：读 PairingState 映射到
  ChannelRegistrationPollState。QR string 通过 fail_reason JSON
  envelope ({kind:"qr", qr_url}) 返回，跟 wechat 同款约定。
  Success 时 emit channel:platform-state Connected。
- set_whatsapp_connection_state：状态变化 emit 给前端
- resolve_whatsapp_paths / make_whatsapp_status_callback：辅助

device_code 单账号下用常量 "whatsapp"（不需要真实 session_id）。

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 5: channel.rs Tauri 命令加 Platform::Whatsapp arm

**Files:**
- Modify: `src-tauri/src/commands/channel.rs`

最简单的一步。

- [ ] **Step 1: 加 begin / poll 分支**

Edit `src-tauri/src/commands/channel.rs:41-89`，在 `Platform::Wechat` arm 之后、`other =>` 之前各加一个 `Platform::Whatsapp` arm：

```rust
        Platform::Wechat => manager(&app)?
            .begin_wechat_registration()
            .await
            .map_err(|e| format!("{:#}", e)),
        Platform::Whatsapp => manager(&app)?
            .begin_whatsapp_registration()
            .await
            .map_err(|e| format!("{:#}", e)),
        other => Err(format!(
            "{} channel registration is not available yet",
            other.as_str()
        )),
```

poll 同形。

- [ ] **Step 2: 编译**

Run: `cd src-tauri && cargo build --lib 2>&1 | tail -3`
Expected: `Finished`。

- [ ] **Step 3: Commit**

```bash
git add src-tauri/src/commands/channel.rs
git commit -m "$(cat <<'EOF'
feat(commands/channel): PR3 channel_begin/poll_registration 加 whatsapp arm

机械抄 wechat 同款分支。manager.begin_whatsapp_registration /
poll_whatsapp_registration 在 PR3 Task 4 已加。

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 6: 前端 WhatsappRiskBanner 组件

**Files:**
- Create: `src/features/channel/WhatsappRiskBanner.tsx`

- [ ] **Step 1: 写 banner 组件**

Create `src/features/channel/WhatsappRiskBanner.tsx`:

```tsx
import { useState } from 'react'
import { Button } from '@/components/ui/button'
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog'
import { Checkbox } from '@/components/ui/checkbox'

interface Props {
  open: boolean
  onAccept: () => void
  onCancel: () => void
}

/**
 * 首次扫码风险 banner。spec §9.1。
 *
 * 用户必须勾选"我已了解上述风险"才能点继续。每次扫码弹一次，不持久化
 * （重新扫码会再弹）。
 */
export function WhatsappRiskBanner({ open, onAccept, onCancel }: Props) {
  const [acknowledged, setAcknowledged] = useState(false)

  return (
    <Dialog
      open={open}
      onOpenChange={(o) => {
        if (!o) onCancel()
      }}
    >
      <DialogContent className="max-w-lg overflow-hidden">
        <DialogHeader>
          <DialogTitle>关于 AIjia 接入 WhatsApp 的说明</DialogTitle>
        </DialogHeader>
        <DialogDescription className="space-y-3 text-sm text-foreground">
          <p>
            AIjia 接入 WhatsApp 采用与开源项目{' '}
            <a
              href="https://docs.openclaw.ai/channels/whatsapp"
              target="_blank"
              rel="noreferrer"
              className="underline text-primary"
            >
              OpenClaw
            </a>{' '}
            相同的方案：通过 <b>WhatsApp Web 多设备协议</b>把 AIjia 作为一台"已链接设备"接入你的 WhatsApp 账号（与你手机上的"已链接的设备 → 链接设备"是同一套机制）。
          </p>
          <h4 className="font-semibold mt-2">为什么不是官方 WhatsApp Business API</h4>
          <p>
            官方 Cloud API 需要 Meta Business 认证、企业资质、模板预审和公网 webhook，对个人和中小团队门槛极高。OpenClaw 等开源方案选择 WhatsApp Web 协议，是当前唯一对个人用户友好的接入方式。
          </p>
          <h4 className="font-semibold mt-2">这个方案的风险（与 OpenClaw 完全相同）</h4>
          <ul className="list-disc list-inside space-y-1">
            <li><b>WhatsApp 官方未授权</b>这种接入方式，属于 TOS 灰区</li>
            <li>账号有被 WhatsApp <b>限速 / 封禁</b>的风险，<b>风险由你自行承担</b></li>
            <li>实测中，<b>虚拟号</b>（Google Voice 等）被封概率显著高于真实手机号</li>
            <li>群发、频繁主动外呼、异常高频回复都会增加封号风险</li>
          </ul>
          <h4 className="font-semibold mt-2">强烈建议</h4>
          <ul className="list-disc list-inside space-y-1">
            <li>使用<b>真实手机号</b>，不要用虚拟号</li>
            <li>不在 AIjia 中<b>群发</b>或频繁主动外呼</li>
            <li>用于 AI 辅助对话场景，不用于营销 / 推广</li>
            <li>重要号码请勿接入，建议用<b>专门的工作号</b></li>
          </ul>
        </DialogDescription>
        <div className="flex items-center gap-2 mt-4">
          <Checkbox
            id="ack"
            checked={acknowledged}
            onCheckedChange={(v) => setAcknowledged(v === true)}
          />
          <label htmlFor="ack" className="text-sm select-none cursor-pointer">
            我已了解上述风险，继续扫码
          </label>
        </div>
        <DialogFooter className="mt-4">
          <Button variant="ghost" onClick={onCancel}>
            取消
          </Button>
          <Button onClick={onAccept} disabled={!acknowledged}>
            继续
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}
```

⚠️ **caveat**：`@/components/ui/dialog` / `@/components/ui/button` / `@/components/ui/checkbox` 应该都存在（其他平台用过）。如果某个 import 不存在，`grep '@/components/ui/' src/features/channel/*.tsx` 看其它平台用的什么。

- [ ] **Step 2: 编译 + lint**

Run: `pnpm exec tsc --noEmit 2>&1 | grep -E 'WhatsappRiskBanner|error TS' | head -5`
Expected: 0 errors related to this file。

- [ ] **Step 3: Commit**

```bash
git add src/features/channel/WhatsappRiskBanner.tsx
git commit -m "$(cat <<'EOF'
feat(channel/whatsapp): PR3 加首次扫码风险 banner（spec §9.1）

明确归因 OpenClaw 同款方案 + TOS 灰区 + 封号风险 + 建议（真实手机号/
不群发/工作号），用户勾"我已了解"才能继续。每次扫码弹一次，不持久化。

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 7: 前端 WhatsappChannelConfig 组件

**Files:**
- Create: `src/features/channel/WhatsappChannelConfig.tsx`

- [ ] **Step 1: 看现有 wechat 同款实现作参考**

Run: `cat src/features/channel/WechatChannelConfig.tsx`

学习它怎么调 `channel_begin_registration({ platform: 'wechat' })` + 弹 RegistrationModal + 解析 `fail_reason` JSON envelope。

- [ ] **Step 2: 写 WhatsappChannelConfig**

Create `src/features/channel/WhatsappChannelConfig.tsx`：

```tsx
import { useState } from 'react'
import { Button } from '@/components/ui/button'
import { invoke } from '@/lib/tauri'
import { RegistrationModal, type RegistrationPollState } from '@/components/registration/RegistrationModal'
import { WhatsappRiskBanner } from './WhatsappRiskBanner'
import { useNotificationStore } from '@/stores/notificationStore'

type Phase = 'idle' | 'risk_banner' | 'modal'

interface BeginResult {
  deviceCode: string
  verificationUriComplete: string
  expiresInSeconds: number
  source: string
}

interface PollResult {
  state: 'waiting' | 'success' | 'fail' | 'expired' | 'unknown'
  failReason: string | null
}

interface QrPayload {
  kind: 'qr'
  qr_url: string
  expires_in_seconds: number
}

interface SuccessPayload {
  kind: 'whatsapp_success'
  jid: string
  push_name: string
}

export function WhatsappChannelConfig() {
  const [phase, setPhase] = useState<Phase>('idle')
  const [qrUrl, setQrUrl] = useState<string>('')
  const [expireSec, setExpireSec] = useState<number>(60)
  const push = useNotificationStore((s) => s.push)

  async function handleAdd() {
    setPhase('risk_banner')
  }

  async function handleRiskAccepted() {
    setPhase('modal')
    try {
      const begin = await invoke<BeginResult>('channel_begin_registration', {
        platform: 'whatsapp',
      })
      // begin.verificationUriComplete 是空字符串；第一次 poll 才能拿到真 QR
      setExpireSec(begin.expiresInSeconds)
    } catch (e) {
      push({ context: 'toast', kind: 'error', message: `添加失败：${String(e)}` })
      setPhase('idle')
    }
  }

  async function pollOnce(): Promise<RegistrationPollState> {
    try {
      const result = await invoke<PollResult>('channel_poll_registration', {
        platform: 'whatsapp',
        deviceCode: 'whatsapp',
      })
      if (result.state === 'success') {
        if (result.failReason) {
          try {
            const payload = JSON.parse(result.failReason) as SuccessPayload
            push({
              context: 'toast',
              kind: 'success',
              message: `WhatsApp 已连接：${payload.push_name} (${payload.jid.split('@')[0]})`,
            })
          } catch {
            push({ context: 'toast', kind: 'success', message: 'WhatsApp 已连接' })
          }
        }
        return 'confirmed'
      }
      if (result.state === 'expired') {
        return 'expired'
      }
      // waiting：检查 failReason 里有没有 QR
      if (result.failReason) {
        try {
          const payload = JSON.parse(result.failReason) as QrPayload
          if (payload.kind === 'qr') {
            setQrUrl(payload.qr_url)
            setExpireSec(payload.expires_in_seconds)
          }
        } catch {
          // failReason 不是 JSON，可能是错误描述；忽略
        }
      }
      return 'waiting'
    } catch (e) {
      console.error('[whatsapp] poll failed:', e)
      return 'waiting'
    }
  }

  function handleConfirmed() {
    setPhase('idle')
    setQrUrl('')
  }

  function handleCancel() {
    setPhase('idle')
    setQrUrl('')
  }

  return (
    <>
      <Button onClick={handleAdd}>添加 WhatsApp 账号</Button>

      <WhatsappRiskBanner
        open={phase === 'risk_banner'}
        onAccept={handleRiskAccepted}
        onCancel={() => setPhase('idle')}
      />

      {phase === 'modal' && qrUrl && (
        <RegistrationModal
          mode="qr_url"
          title="扫码登录 WhatsApp"
          qrUrl={qrUrl}
          expireSeconds={expireSec}
          pollState={pollOnce}
          onConfirmed={handleConfirmed}
          onCancel={handleCancel}
        />
      )}
      {phase === 'modal' && !qrUrl && (
        <RegistrationModal
          mode="qr_url"
          title="扫码登录 WhatsApp"
          qrUrl=""
          expireSeconds={expireSec}
          pollState={pollOnce}
          onConfirmed={handleConfirmed}
          onCancel={handleCancel}
        />
      )}
    </>
  )
}
```

⚠️ **重要**：`invoke` 的具体导入路径以及 `useNotificationStore.push` 的参数 shape 可能跟实际仓库有差异——参考 `WechatChannelConfig.tsx` 的真实用法照抄。

- [ ] **Step 3: 加 ChannelPage 路由**

Edit `src/features/channel/ChannelPage.tsx`（或类似 dispatcher）：当用户点 whatsapp 卡片"添加账号"时打开 `WhatsappChannelConfig`。

具体路由方式取决于现有 dispatcher 形态——`grep -n 'WechatChannelConfig\|TelegramChannelConfig\|WecomChannelConfig' src/features/channel/ChannelPage.tsx` 看现有路由模式，照搬。

- [ ] **Step 4: 跑前端检查**

Run: `pnpm exec tsc --noEmit 2>&1 | tail -5`
Expected: 0 new errors。

Run: `pnpm lint src/features/channel/WhatsappChannelConfig.tsx src/features/channel/WhatsappRiskBanner.tsx 2>&1 | tail -10`
Expected: 0 errors。

- [ ] **Step 5: Commit**

```bash
git add src/features/channel/WhatsappChannelConfig.tsx \
        src/features/channel/ChannelPage.tsx
git commit -m "$(cat <<'EOF'
feat(channel/whatsapp): PR3 加 WhatsappChannelConfig + ChannelPage 路由

spec v3 §3.6 / §3.8 / §9.1。

WhatsappChannelConfig 三阶段：
- idle：显示"添加 WhatsApp 账号"按钮
- risk_banner：spec §9.1 风险确认对话框
- modal：扫码 RegistrationModal mode='qr_url'

QR string 通过 poll 的 failReason JSON envelope ({kind:"qr",qr_url})
传递。Success 时 ({kind:"whatsapp_success",jid,push_name}) toast 通知。
跟 wechat 同款约定，复用 RegistrationModal 不新加 mode。

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 8: 启动期自动重连（lib.rs）

**Files:**
- Modify: `src-tauri/src/lib.rs`

桌面 app 启动期如果 `config.json` 已存在，应该自动起 Bot 复用既有 session（不需要再扫码）。

- [ ] **Step 1: 看启动期其他平台怎么自动重连**

Run: `grep -n 'register_telegram_connector\|register_wechat_connector\|connect_from_store' src-tauri/src/lib.rs | head -10`

参考 wechat 的 `connect_wechat_from_store` 启动期模式（如果有）；或者参考 dingtalk/telegram。

- [ ] **Step 2: 加 connect_whatsapp_from_store**

在 manager.rs 加一个 helper：

```rust
    /// 启动期：如果 config.json 存在，直接起 Bot 复用既有 session。
    /// 不存在则不动（用户需要走扫码才能初次连）。
    pub async fn connect_whatsapp_from_store(&self) -> Result<()> {
        let paths = self.resolve_whatsapp_paths()?;
        if !paths.config_path().exists() {
            log::info!("[whatsapp] no config.json — skipping auto-connect");
            return Ok(());
        }
        let on_status = self.make_whatsapp_status_callback();
        let concrete = self.register_whatsapp_connector(on_status).await;
        concrete.start_pairing_session(paths).await?;
        // PR3：Connected 状态由 runtime.rs 的 Event::Connected handler 推到
        // PairingState::Connected；manager 的 connection state 由 on_status
        // 回调驱动。
        Ok(())
    }
```

然后在 lib.rs 启动期 manager setup 那段加调用：

```rust
// 类似已有的 connect_wechat_from_store 调用，加一行：
if let Err(e) = mgr.connect_whatsapp_from_store().await {
    log::warn!("[whatsapp] auto-connect failed at startup: {e:#}");
}
```

- [ ] **Step 3: 编译 + 测试**

Run: `cd src-tauri && cargo build --lib 2>&1 | tail -3`
Expected: `Finished`。

- [ ] **Step 4: Commit**

```bash
git add src-tauri/src/connector/im/manager.rs src-tauri/src/lib.rs
git commit -m "$(cat <<'EOF'
feat(connector/im/whatsapp): PR3 启动期自动重连

manager.connect_whatsapp_from_store：如 config.json 存在则起 Bot
复用既有 session.db 凭证，不需用户重新扫码。Event::Connected 回调
会把 PairingState::Idle → Connected。

lib.rs 启动期加调用，仿 wechat connect_from_store 模式。

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 9: 收尾校验

**Files:**（无修改）

- [ ] **Step 1: 全 PR3 测试**

Run: `cd src-tauri && cargo test --lib connector::im::whatsapp 2>&1 | tail -15`
Expected: 27 PR2 baseline + 4 PR3 runtime = 31 tests pass.

- [ ] **Step 2: 全 IM 回归**

Run: `cd src-tauri && cargo test --lib connector::im 2>&1 | grep -E '^test result' | tail -3`
Expected: 0 new failures。

- [ ] **Step 3: review_im_layering**

Run: `cd src-tauri && cargo test --test review_im_layering 2>&1 | tail -5`
Expected: 3 passed.

- [ ] **Step 4: Clippy on PR3 files**

Run: `cd src-tauri && cargo clippy --lib --message-format=short 2>&1 | grep -E 'src/connector/im/whatsapp/|src/commands/channel.rs' | head -10`
Expected: 0 warnings on PR3-touched files.

- [ ] **Step 5: Cargo fmt**

Run: `cd src-tauri && cargo fmt -- --check 2>&1 | head -5`
Expected: clean. If diff, fmt + commit.

- [ ] **Step 6: 前端 lint + tsc**

Run: `pnpm exec tsc --noEmit 2>&1 | tail -3`
Expected: 0 errors.

Run: `pnpm lint 2>&1 | tail -3`
Expected: baseline 不变。

- [ ] **Step 7: 实测——dev server 起来 + 用户场景**

Run: `pnpm tauri:dev`（在 background）。控制台跑：
- 点 WhatsApp 卡片 → 出现风险 banner
- 勾"我已了解" + 继续 → QR 弹出（**真实手机扫码可选；本步只验证 UI 流程跑通**）
- 等 60s QR 过期 → "已过期"提示出现

如果时间紧，**只验证编译 + 测试通过 + dev server 起不崩**就够；真扫码测试留给用户在 app 里手动跑。

---

## Self-Review

### 1. Spec 覆盖（v3 §3 + §9.1）

| spec 子段 | task | 状态 |
|---|---|---|
| §3.6 扫码流程 | Task 2 runtime.rs + Task 3 connector.rs + Task 4 manager.rs | ✅ |
| §3.7 Tauri 命令 | Task 5 channel.rs whatsapp arm | ✅ |
| §3.8 RegistrationModal qr_url | Task 7 WhatsappChannelConfig | ✅ |
| §3.9 重新扫码 | Task 4 begin_whatsapp_registration 检查 config.json + delete_for_reauth | ✅ |
| §3.10 allow_from | **PR2 已加字段**；PR3 不实现 UI 编辑（留 PR8） | ⚠️ deferred |
| §3.11 reaction | PR6 范围 | N/A |
| §3.12 quoted reply | PR4 范围 | N/A |
| §9.1 风险 banner | Task 6 WhatsappRiskBanner | ✅ |
| 启动期自动重连 | Task 8 connect_whatsapp_from_store | ✅ |

### 2. Placeholder scan

- Task 4 Step 4 `whatsapp_concrete_or_err` 有 `unimplemented!()` —— 是 plan **故意**留的设计缺口，要 implementer 去查 telegram 模式补全。在 caveat 段落已明确说明。**不是 placeholder——这是"必须查 + 抄"指令**。
- Task 2 Step 1 wa-rs API 类型路径 caveat —— 同上，让 implementer 实测调整。

其他无 placeholder。

### 3. 类型一致性

- `WhatsAppPaths` / `WhatsAppChannelConfig` / `PairingState` 全大写 W 一致（Rust 类型名规范）
- `Platform::Whatsapp` 大写 W 小写其余（已有 enum，PR1 加的）
- `device_code` 常量 `"whatsapp"` 在 begin（manager.rs）和前端 poll deviceCode 参数都用同样字符串
- `fail_reason` JSON envelope 三种 kind：`"qr"` / `"whatsapp_success"`（success）/ 错误描述字符串。前端 try-catch 解析。

---

## Execution Handoff

Plan 完成并保存到 `docs/superpowers/plans/2026-05-20-im-whatsapp-phase4-pr3.md`。

**估时**：9 个 task / ~700 行新代码（含测试）/ 实际 1-2 小时（subagent-driven）。

两种执行方式：

**1. Subagent-Driven（推荐，跟 PR1/PR2 一样）**

**2. Inline Execution**

哪种？
