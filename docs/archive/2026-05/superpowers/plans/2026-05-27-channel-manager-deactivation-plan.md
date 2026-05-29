# ChannelManager 注册时机重构 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 修复"setup 时未登录的用户登录后频道功能仍然提示『频道功能未初始化』"的 bug；顺带把所有"用户失活"路径（手动登出 / token 过期 / 改密）统一到一个 deactivation handler 链，保证 ChannelManager 在切换用户时被干净销毁、不污染新账号。

**Architecture:**
- 把 `Arc<ChannelManager>` 从直接 `app.manage()` 改为放在可替换 slot `ChannelManagerSlot(Mutex<Option<Arc<ChannelManager>>>)`，所有 IPC 通过 slot 取实例。
- ChannelManager 加 `shutdown()`，标记 inactive + cancel 所有 worker + 关 WS + 清 `channel_session_ids` 中本实例占用的 session ids。
- AuthManager 加 `Vec<Arc<dyn AuthDeactivationHandler>>` 钩子，所有 3 处失活点（`logout` / `change_password` / `refresh_auth_info` 内部 401 自动失活）统一触发。
- setup 阶段无条件注册空 slot + 注册 3 个 handler（ChannelManager / CurrentUserStorage / FileManager workspace 重置）；`cloud_login` 成功后调用 `ensure_channel_manager_registered(&app)` helper 装配实例。

**Tech Stack:** Rust / Tauri 2.x / tokio / `async_trait` / `tokio::sync::Mutex`

**Out of scope（本次不解决）:** 其它 11 个 user-scoped 服务（AppStorage / RuntimeRepositoryFacade / EmployeeStore / AgentRegistry / SkillRegistry / AgentRuntime / PermissionStore / McpConfigStore / McpServerManager / PendingQueueManager / FileManager.workspacePath）的切账号问题。Logout 后仍建议用户重启 app 切账号——详见 `docs/decisions/runtime-decisions.md` follow-up issue（本 plan 不写该文档，只在 commit message 中提及）。

---

## File Structure

| 文件 | 操作 | 职责 |
|---|---|---|
| `src-tauri/src/auth/deactivation.rs` | 新增 | `AuthDeactivationHandler` trait 定义 |
| `src-tauri/src/auth/mod.rs` | 改 | 加 handler 注册 + 3 处失活点触发 + 模块导出 |
| `src-tauri/src/connector/im/channel_manager_slot.rs` | 新增 | `ChannelManagerSlot` 容器类型 |
| `src-tauri/src/connector/im/mod.rs` | 改 | 导出 `ChannelManagerSlot` |
| `src-tauri/src/connector/im/manager.rs` | 改 | 加 `inactive: AtomicBool` + `shutdown()` + 入口 inactive 检查 |
| `src-tauri/src/commands/channel.rs` | 改 | `manager(&app)` 改为从 slot 取 |
| `src-tauri/src/commands/auth.rs` | 改 | `cloud_login` 加 `AppHandle` 参数 + 调 helper；`cloud_logout` 瘦身 |
| `src-tauri/src/lib.rs` | 改 | setup 注册空 slot + handler 注册；抽出 `ensure_channel_manager_registered` helper |
| `src-tauri/tests/channel_deactivation_integration_test.rs` | 新增 | 集成测试：未登录启动 → 登录 → 频道可用；登录 → 登出 → slot None；切账号 → 新实例 |

---

## Task 1: 定义 `AuthDeactivationHandler` trait

**Files:**
- Create: `src-tauri/src/auth/deactivation.rs`
- Modify: `src-tauri/src/auth/mod.rs`（加 `pub mod deactivation;` 与 `pub use deactivation::AuthDeactivationHandler;`）

- [ ] **Step 1: 写测试**

Create `src-tauri/src/auth/deactivation.rs`:

```rust
//! Auth deactivation hook — services that hold user-scoped runtime state
//! register a handler so `AuthManager` can fan out a single signal whenever
//! the active user is invalidated (manual logout, password change, server-
//! initiated refresh-token revocation).
//!
//! Handlers MUST be idempotent — they may be called even when the user was
//! already deactivated (e.g. logout after a 401 has already cleared state).
//! Handlers MUST NOT panic; errors should be logged and swallowed so one
//! misbehaving handler does not break the chain.

use async_trait::async_trait;

#[async_trait]
pub trait AuthDeactivationHandler: Send + Sync {
    /// Called after `AuthManager` has cleared in-memory + persisted auth
    /// state. The handler runs OUTSIDE any `AuthManager` lock, so it is
    /// safe to call back into the app handle / storage.
    async fn on_deactivated(&self);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    struct Counter(Arc<AtomicUsize>);

    #[async_trait]
    impl AuthDeactivationHandler for Counter {
        async fn on_deactivated(&self) {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
    }

    #[tokio::test]
    async fn handler_increments_counter() {
        let counter = Arc::new(AtomicUsize::new(0));
        let h: Arc<dyn AuthDeactivationHandler> = Arc::new(Counter(counter.clone()));
        h.on_deactivated().await;
        h.on_deactivated().await;
        assert_eq!(counter.load(Ordering::SeqCst), 2);
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd src-tauri && cargo test --lib auth::deactivation 2>&1 | tail -20`
Expected: 编译错误，`module deactivation not declared`

- [ ] **Step 3: 加 module 声明 + re-export**

Edit `src-tauri/src/auth/mod.rs`，在已有 `pub mod client;`/`pub mod state;` 等附近加：

```rust
pub mod deactivation;

pub use deactivation::AuthDeactivationHandler;
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cd src-tauri && cargo test --lib auth::deactivation -- --nocapture`
Expected: `test auth::deactivation::tests::handler_increments_counter ... ok`

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/auth/deactivation.rs src-tauri/src/auth/mod.rs
git commit -m "feat(auth): add AuthDeactivationHandler trait

Establishes a fan-out hook called by AuthManager whenever the active
user is invalidated (logout, change_password, server-initiated 401
auto-deactivation). Subsequent commits wire ChannelManager and other
user-scoped services into this chain."
```

---

## Task 2: `AuthManager` 持有并触发 deactivation handlers

**Files:**
- Modify: `src-tauri/src/auth/mod.rs`（在 `AuthManager` struct / new / logout / change_password / refresh_auth_info 内部 401 五处）

- [ ] **Step 1: 写测试**

在 `src-tauri/src/auth/mod.rs` 文件底部已有 `#[cfg(test)] mod tests` 中追加（如果没有则新建）：

```rust
#[cfg(test)]
mod deactivation_chain_tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct Counting(Arc<AtomicUsize>);

    #[async_trait::async_trait]
    impl crate::auth::AuthDeactivationHandler for Counting {
        async fn on_deactivated(&self) {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
    }

    #[tokio::test]
    async fn logout_triggers_registered_handlers() {
        let am = AuthManager::for_test();
        let counter = Arc::new(AtomicUsize::new(0));
        am.register_deactivation_handler(Arc::new(Counting(counter.clone())));
        // Pre-populate state so logout has something to clear
        am.set_state_for_test(test_cloud_auth());
        am.logout().await;
        assert_eq!(counter.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn change_password_triggers_handlers_on_state_clear() {
        // Same shape as above but driving change_password's clear path.
        // If change_password requires server roundtrip in tests, expose
        // a `clear_state_for_test()` and assert handlers fire after it.
        let am = AuthManager::for_test();
        let counter = Arc::new(AtomicUsize::new(0));
        am.register_deactivation_handler(Arc::new(Counting(counter.clone())));
        am.set_state_for_test(test_cloud_auth());
        am.clear_state_and_fire_handlers_for_test().await;
        assert_eq!(counter.load(Ordering::SeqCst), 1);
    }
}
```

(如果 `AuthManager::for_test` / `set_state_for_test` / `test_cloud_auth` / `clear_state_and_fire_handlers_for_test` 不存在，本 step 同时新增；放在 `impl AuthManager` 的 `#[cfg(test)]` 块内即可。)

- [ ] **Step 2: Run test to verify it fails**

Run: `cd src-tauri && cargo test --lib auth::deactivation_chain_tests 2>&1 | tail -20`
Expected: 编译错误，`no method named register_deactivation_handler`

- [ ] **Step 3: 改 `AuthManager` struct + 注册 + 触发**

在 `AuthManager` struct 中加字段：

```rust
deactivation_handlers: tokio::sync::RwLock<Vec<Arc<dyn AuthDeactivationHandler>>>,
```

在 `AuthManager::new` 初始化：

```rust
deactivation_handlers: tokio::sync::RwLock::new(Vec::new()),
```

加方法：

```rust
pub async fn register_deactivation_handler(&self, h: Arc<dyn AuthDeactivationHandler>) {
    self.deactivation_handlers.write().await.push(h);
}

/// Internal: fire all handlers AFTER `state` write lock is released and
/// persistence has been cleared. Each handler runs sequentially; panics
/// inside a handler are caught and logged so the chain does not abort.
async fn fire_deactivation_handlers(&self) {
    let handlers = self.deactivation_handlers.read().await.clone();
    for h in handlers {
        // We don't have catch_unwind for async; rely on handler contract
        // (must not panic). Log any future-level errors via tracing if
        // handlers return Result in a follow-up.
        h.on_deactivated().await;
    }
}
```

在 3 处失活点结尾追加 `self.fire_deactivation_handlers().await;`：

1. `logout()` — 在 `self.clear_persisted_auth();` 后
2. `change_password()` — 在 `self.clear_persisted_auth();` 后
3. `refresh_auth_info` 内部 401 路径 — 在已有 `*self.state.write().await = None; self.clear_persisted_auth();` 后

（搜索 `clear_persisted_auth()` 调用点确认 3 处都加到，line 115 的 `restore()` 失败路径**不加**——`restore` 失败时 state 本来就还没生效，无需通知 handlers。）

加测试辅助：

```rust
#[cfg(test)]
impl AuthManager {
    pub fn for_test() -> Self { /* 复用现有 new(...) 或加一个 ::default()，省略具体实现 */ }
    pub fn set_state_for_test(&self, auth: state::CloudAuth) {
        // block_on 或者 sync 版本：因为测试在 tokio runtime 内，直接 try_write
        *self.state.try_write().unwrap() = Some(auth);
    }
    pub async fn clear_state_and_fire_handlers_for_test(&self) {
        *self.state.write().await = None;
        self.clear_persisted_auth();
        self.fire_deactivation_handlers().await;
    }
}

#[cfg(test)]
fn test_cloud_auth() -> state::CloudAuth {
    use chrono::Utc;
    use chrono::Duration;
    state::CloudAuth {
        access_token: "test".into(),
        access_expires_at: Utc::now() + Duration::hours(1),
        refresh_token: "test".into(),
        refresh_expires_at: Utc::now() + Duration::hours(24),
        session_key: "test".into(),
        session_key_expires_at: Utc::now() + Duration::hours(24),
        user: state::UserInfo { id: 1, username: "test".into(), name: "test".into() /* fill required fields */ },
        tenant: state::TenantInfo { id: 1, name: "test".into() /* fill required fields */ },
    }
}
```

如果 `state::UserInfo` / `state::TenantInfo` 字段更多，按编译报错补齐。

- [ ] **Step 4: Run test to verify it passes**

Run: `cd src-tauri && cargo test --lib auth::deactivation_chain_tests -- --nocapture`
Expected: 两条测试都 `... ok`

- [ ] **Step 5: 验证全量 auth 模块测试不退化**

Run: `cd src-tauri && cargo test --lib auth::`
Expected: 全部 PASS

- [ ] **Step 6: Commit**

```bash
git add src-tauri/src/auth/mod.rs
git commit -m "feat(auth): fan out deactivation handlers from logout/change_pw/auto-401

AuthManager now fires AuthDeactivationHandler chain after clearing
state at all three deactivation sites (manual logout, change_password,
refresh_auth_info auto-401). Restore() failure path intentionally
skipped — state was never live so there is nothing to notify."
```

---

## Task 3: 新增 `ChannelManagerSlot` 容器

**Files:**
- Create: `src-tauri/src/connector/im/channel_manager_slot.rs`
- Modify: `src-tauri/src/connector/im/mod.rs`

- [ ] **Step 1: 写测试**

Create `src-tauri/src/connector/im/channel_manager_slot.rs`:

```rust
//! Replaceable container for the active `ChannelManager`.
//!
//! Setup-time registration cannot satisfy "user not logged in at startup"
//! and "switch user without restart" simultaneously — `tauri::App::manage`
//! refuses to overwrite. The slot indirection lets us swap instances at
//! runtime while keeping a stable type registration in app state.

use std::sync::Arc;
use tokio::sync::Mutex;

use super::ChannelManager;

pub struct ChannelManagerSlot {
    inner: Mutex<Option<Arc<ChannelManager>>>,
}

impl ChannelManagerSlot {
    pub fn new() -> Self {
        Self { inner: Mutex::new(None) }
    }

    /// Read-only snapshot. Returns the current instance if any.
    pub async fn current(&self) -> Option<Arc<ChannelManager>> {
        self.inner.lock().await.clone()
    }

    /// Atomically replace the instance, returning the previous value so the
    /// caller can drive `shutdown()` on it.
    pub async fn replace(&self, new: Option<Arc<ChannelManager>>) -> Option<Arc<ChannelManager>> {
        let mut guard = self.inner.lock().await;
        std::mem::replace(&mut *guard, new)
    }
}

impl Default for ChannelManagerSlot {
    fn default() -> Self { Self::new() }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn new_slot_is_empty() {
        let slot = ChannelManagerSlot::new();
        assert!(slot.current().await.is_none());
    }

    #[tokio::test]
    async fn replace_returns_previous() {
        let slot = ChannelManagerSlot::new();
        // We can't easily instantiate a real ChannelManager here without all
        // its deps, so just smoke-test the None -> None path.
        let prev = slot.replace(None).await;
        assert!(prev.is_none());
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd src-tauri && cargo test --lib connector::im::channel_manager_slot 2>&1 | tail -20`
Expected: `module channel_manager_slot not declared`

- [ ] **Step 3: 在 `mod.rs` 加声明 + re-export**

Edit `src-tauri/src/connector/im/mod.rs`，在 `pub use manager::ChannelManager;` 附近追加：

```rust
pub mod channel_manager_slot;
pub use channel_manager_slot::ChannelManagerSlot;
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cd src-tauri && cargo test --lib connector::im::channel_manager_slot -- --nocapture`
Expected: 两条测试 `... ok`

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/connector/im/channel_manager_slot.rs src-tauri/src/connector/im/mod.rs
git commit -m "feat(channel): add ChannelManagerSlot for replaceable registration

Allows runtime swap of the active ChannelManager (needed for 'login
after setup-time guest start' and 'switch user without restart')."
```

---

## Task 4: `ChannelManager` 加 `inactive` 标志与 `shutdown()`

**Files:**
- Modify: `src-tauri/src/connector/im/manager.rs`

- [ ] **Step 1: 写测试**

在 `src-tauri/src/connector/im/manager.rs` 文件底部 `#[cfg(test)] mod tests {}` 内追加：

```rust
#[tokio::test]
async fn shutdown_marks_inactive_and_stops_all_streams() {
    use super::*;
    let cm = build_test_channel_manager_for_shutdown().await;
    // Sanity: not inactive initially.
    assert!(!cm.is_inactive());
    // Simulate a connected platform by inserting a fake stream cancel into
    // platform_state so shutdown actually does something.
    {
        let mut states = cm.platform_state.write().await;
        let mut s = PerPlatformState::unconfigured();
        let cancel = tokio_util::sync::CancellationToken::new();
        s.stream_cancel = Some(cancel.clone());
        states.insert(Platform::Dingtalk, s);
        // Keep cancel alive in scope to check it was cancelled.
        drop(states);
        cm.shutdown().await;
        assert!(cm.is_inactive());
        assert!(cancel.is_cancelled());
    }
}

#[tokio::test]
async fn shutdown_is_idempotent() {
    use super::*;
    let cm = build_test_channel_manager_for_shutdown().await;
    cm.shutdown().await;
    cm.shutdown().await; // must not panic / hang
    assert!(cm.is_inactive());
}

#[tokio::test]
async fn inactive_blocks_channel_session_id_registration() {
    use super::*;
    let cm = build_test_channel_manager_for_shutdown().await;
    cm.shutdown().await;
    // Attempt to register a session via the same helper workers use.
    cm.register_channel_session_for_test("sess-test".into());
    let ids = cm.channel_session_ids.read().unwrap();
    assert!(
        !ids.contains("sess-test"),
        "inactive manager must reject new session ids"
    );
}
```

如果 `build_test_channel_manager_for_shutdown` 不存在则同步加：复用现有任意已存在的 `build_test_channel_manager` / `for_test`；本 task 不要求新写出完整 manager，只要构造足够让 `shutdown` 路径通过的最小实例即可（platform_state 用 `Default::default()`，其它字段填空/默认值）。如果实在没有现成 helper，直接 `unimplemented!()` 占位，运行测试看编译，编译通过后再补 helper（TDD 红→绿小步）。

- [ ] **Step 2: Run test to verify it fails**

Run: `cd src-tauri && cargo test --lib connector::im::manager::tests::shutdown_marks_inactive_and_stops_all_streams 2>&1 | tail -30`
Expected: 编译错误，`no method named shutdown` / `is_inactive` / `register_channel_session_for_test`

- [ ] **Step 3: 加 `inactive` 字段**

在 `ChannelManager` struct 中加：

```rust
inactive: Arc<std::sync::atomic::AtomicBool>,
```

在 `ChannelManager::new(...)` 初始化：

```rust
inactive: Arc::new(std::sync::atomic::AtomicBool::new(false)),
```

加方法：

```rust
pub fn is_inactive(&self) -> bool {
    self.inactive.load(std::sync::atomic::Ordering::SeqCst)
}

#[cfg(test)]
pub fn register_channel_session_for_test(&self, session_id: String) {
    if self.is_inactive() { return; }
    self.channel_session_ids.write().unwrap().insert(session_id);
}
```

- [ ] **Step 4: 实现 `shutdown()`**

在 `impl ChannelManager` 中追加：

```rust
/// Best-effort shutdown — marks inactive, cancels all per-platform streams,
/// awaits worker tasks with a 3s overall budget, and clears any session
/// ids this instance owns from the shared `channel_session_ids` registry.
///
/// Idempotent: subsequent calls are no-ops once `inactive` is set.
pub async fn shutdown(&self) {
    use std::sync::atomic::Ordering;
    use std::time::Duration;

    // Idempotency gate: only the first caller does the work.
    if self.inactive.swap(true, Ordering::SeqCst) {
        log::debug!("[channel] shutdown: already inactive, skipping");
        return;
    }
    log::info!("[channel] shutdown: begin");

    // Step 1: collect per-platform cancel tokens + task handles, replacing
    // them with empty slots so subsequent set_enabled / connect attempts
    // see a clean slate. We don't await tasks while holding the write lock.
    let mut to_join: Vec<(Platform, tokio::task::JoinHandle<()>)> = Vec::new();
    {
        let mut states = self.platform_state.write().await;
        for (platform, slot) in states.iter_mut() {
            if let Some(token) = slot.stream_cancel.take() {
                token.cancel();
            }
            if let Some(handle) = slot.message_task.take() {
                to_join.push((platform.clone(), handle));
            }
            slot.stream_generation.fetch_add(1, Ordering::SeqCst);
        }
    }

    // Step 2: await all workers with a global 3s budget. Anything still
    // running gets dropped — the inactive flag prevents user-visible side
    // effects from those zombies.
    let join_all = async {
        for (platform, handle) in to_join {
            if let Err(e) = handle.await {
                log::warn!(
                    "[channel/{}] shutdown worker join failed: {}",
                    platform.as_str(),
                    e
                );
            }
        }
    };
    if tokio::time::timeout(Duration::from_secs(3), join_all).await.is_err() {
        log::warn!("[channel] shutdown: worker join exceeded 3s budget, dropping");
    }

    // Step 3: drop our entries from the shared session-id registry. We
    // intentionally do NOT clear the whole set — a future owner may have
    // already registered new sessions. Instead, drain everything the local
    // conversations cache claims is ours.
    let owned: Vec<String> = {
        let convs = self.conversations.read().await;
        convs.iter().map(|c| c.session_id.clone()).collect()
    };
    {
        let mut ids = self
            .channel_session_ids
            .write()
            .expect("channel_session_ids poisoned");
        for sid in owned {
            ids.remove(&sid);
        }
    }

    log::info!("[channel] shutdown: complete");
}
```

> 注意：`self.conversations` 当前是 `Arc<RwLock<Vec<...>>>`（std 或 tokio？查现状），上面用 `.read().await` 假定 tokio；若是 std 则去掉 `.await`。提交前用 `cargo check` 跟着编译器错误调整。

- [ ] **Step 5: 加入口 inactive 检查**

在所有"会因 inactive 应该被忽略"的入口加 `if self.is_inactive() { return; }` 早返。最小集合（这次不求全，只挡核心副作用）：

1. `register` 触发新 session（搜 `channel_session_ids_ref ... .insert(`，line 712 / 1433 / 2884，在 insert 前后判断 `self.inactive`——但 spawn 出去的 worker 闭包没有 `self`，需把 `Arc<AtomicBool>` clone 进去）。

具体改法：构造 worker 时把 `let inactive = Arc::clone(&self.inactive);` 一同 `move` 进去，在 insert 前判断：

```rust
if inactive.load(std::sync::atomic::Ordering::SeqCst) {
    log::debug!("[channel] worker observed inactive flag, dropping event");
    continue;
}
let mut ids = channel_session_ids_ref
    .write()
    .expect("channel_session_ids poisoned");
ids.insert(...);
```

2. `set_enabled` / `begin_*_registration` / `poll_*_registration`（用户在 inactive 期间不该再触发新连接） — 在方法入口加：

```rust
if self.is_inactive() {
    return Err(anyhow::anyhow!("channel manager inactive"));
}
```

只挡 5 个入口（`set_enabled`, `begin_dingtalk_registration`, `begin_feishu_registration`, `begin_wechat_registration`, `begin_whatsapp_registration`）即可，其它读操作在 inactive 时返回旧数据也无害（很快会被 slot.replace 整个换掉）。

- [ ] **Step 6: Run test to verify it passes**

Run: `cd src-tauri && cargo test --lib connector::im::manager::tests::shutdown -- --nocapture`
Expected: 三条 shutdown 相关测试 `... ok`

- [ ] **Step 7: 验证完整 manager 测试不退化**

Run: `cd src-tauri && cargo test --lib connector::im::manager::`
Expected: 现有所有 `manager::tests::*` 仍 PASS

- [ ] **Step 8: Commit**

```bash
git add src-tauri/src/connector/im/manager.rs
git commit -m "feat(channel): add shutdown() + inactive gate to ChannelManager

shutdown() marks the instance inactive, cancels all per-platform
streams, awaits workers with a 3s budget, and removes owned session
ids from the shared channel_session_ids registry. Idempotent.

inactive flag gates 5 mutating entry points (set_enabled + 4 register
flows) and is checked in worker session-id insertion to prevent a
zombie worker from polluting a new user's account."
```

---

## Task 5: 抽出 `ensure_channel_manager_registered` helper

**Files:**
- Modify: `src-tauri/src/lib.rs`（把 `if let Some(paths) = current_user_storage.resolve_paths() { ... app.manage(channel_manager); }` 的整块 setup 装配逻辑抽到独立函数）

- [ ] **Step 1: 写测试（行为契约）**

集成测试比单元测试合适，留到 Task 9 一起加。本步**只做重构**，不新增测试，靠现有 `review_` 测试守住接口不变。

Run: `cd src-tauri && cargo test --tests review_ --no-fail-fast 2>&1 | tail -10`
记录基线（应该全 PASS）。

- [ ] **Step 2: 抽函数**

在 `src-tauri/src/lib.rs` 文件底部（main `pub fn run()` 之外）加：

```rust
/// Idempotent helper: ensure an active `ChannelManager` exists in the slot
/// matching the currently-active user scope. Safe to call from setup (when
/// the user was already logged in at boot) and from `cloud_login` (post-
/// login activation).
///
/// Behaviour:
/// - No active user scope → no-op (slot stays None, IPC commands keep
///   returning "频道功能未初始化，请先登录").
/// - Slot already holds an instance for the SAME scope → no-op.
/// - Slot empty OR holds a different scope → shutdown old (best effort,
///   spawn-detached) and install a fresh instance.
pub async fn ensure_channel_manager_registered(app: &tauri::AppHandle) {
    use std::sync::Arc;
    use tauri::Manager;

    let cus = app
        .state::<Arc<crate::storage::CurrentUserStorage>>()
        .inner()
        .clone();
    let Some(paths) = cus.resolve_paths() else {
        log::debug!("[channel] ensure: no active user scope, slot stays empty");
        return;
    };
    let current_scope = cus.scope();

    let slot = app
        .state::<Arc<crate::connector::im::ChannelManagerSlot>>()
        .inner()
        .clone();

    // Fast path: same scope already installed.
    if let Some(existing) = slot.current().await {
        if existing.active_scope() == current_scope {
            log::debug!("[channel] ensure: instance already matches active scope");
            return;
        }
    }

    // Build new instance — same wiring as the old inline setup block.
    let chat_adapter_ref = app
        .state::<Arc<crate::transport::tauri_commands::chat::TauriChatCommandAdapter>>()
        .inner()
        .clone();
    let gateway_ref = app
        .state::<Arc<crate::llm::gateway::LlmGateway>>()
        .inner()
        .clone();

    let reply_manager = Arc::new(crate::connector::im::DingtalkReplyManager::new());
    let judge = Arc::new(crate::connector::im::ask_coordinator::GatewayAskReplyJudge::new(
        gateway_ref,
        crate::models::settings::AppSettings::default(),
    ));
    let channel_session_ids = app
        .state::<Arc<std::sync::RwLock<std::collections::HashSet<String>>>>()
        .inner()
        .clone();
    let ask_coordinator = Arc::new(
        crate::connector::im::ask_coordinator::IMAskCoordinator::new(
            channel_session_ids.clone()
                as Arc<dyn crate::connector::im::ask_coordinator::ChannelSessionRegistry>,
            reply_manager.clone()
                as Arc<dyn crate::connector::im::ask_coordinator::AskOutputSink>,
            chat_adapter_ref.permission_control_plane(),
            chat_adapter_ref.interaction_control_plane(),
            judge,
        ),
    );

    let new_cm = Arc::new(crate::connector::im::ChannelManager::new(
        app.clone(),
        chat_adapter_ref,
        app.state::<Arc<crate::storage::file_store::RuntimeRepositoryFacade>>()
            .inner()
            .conversation_store_arc(),
        app.state::<Option<Arc<crate::storage::crypto::SecureStorage>>>()
            .inner()
            .clone(),
        paths.channels_dir(),
        Some(ask_coordinator),
        reply_manager,
        channel_session_ids,
        app.state::<Arc<crate::runtime::pending::PendingQueueManager>>()
            .inner()
            .clone(),
    ));

    // Swap in, fire-and-forget shutdown on old.
    let prev = slot.replace(Some(new_cm.clone())).await;
    if let Some(old) = prev {
        tokio::spawn(async move {
            log::info!("[channel] ensure: shutting down previous instance");
            old.shutdown().await;
        });
    }

    // Hydrate + auto-connect on the new instance.
    let cm_clone = new_cm.clone();
    tauri::async_runtime::spawn(async move {
        cm_clone.hydrate_conversations().await;
        cm_clone.auto_connect_if_configured().await;
    });
    log::info!("[channel] ensure: installed new ChannelManager for scope");
}
```

`active_scope()` 需要在 `ChannelManager` 加 getter（path 推出来的，或在 `new` 时传入 scope 并保存）。最简单：让 `ChannelManager::new` 多接一个 `scope: Option<UserScope>` 参数，存到字段，加：

```rust
pub fn active_scope(&self) -> Option<UserScope> { self.active_scope.clone() }
```

设置 helper 传入 `cus.scope()`。所有现有 `ChannelManager::new` 调用点同步加这个参数（lib.rs 中的 setup 旧路径 + 任何测试 helper）。

- [ ] **Step 3: setup 改为只 manage slot，不直接装配实例**

替换 `src-tauri/src/lib.rs` 第 807-860 行（原 `// Initialize ChannelManager for IM channel integration` 整块）为：

```rust
// Slot is registered unconditionally — actual instance is installed by
// ensure_channel_manager_registered() either now (if logged in at boot)
// or later via the AuthManager deactivation/login hooks.
app.manage(Arc::new(crate::connector::im::ChannelManagerSlot::new()));

// Boot-time bring-up: only effective when current_user_storage already
// has a scope (restored from disk in setup above).
{
    let handle = app.handle().clone();
    tauri::async_runtime::spawn(async move {
        crate::ensure_channel_manager_registered(&handle).await;
    });
}
```

> ⚠️ `Arc<std::sync::RwLock<HashSet<String>>>` 即 `channel_session_ids` 现在已经在 setup 中创建（line 671 附近）但没 `app.manage()` 注册。helper 需要从 state 拿，所以 setup 中必须 `app.manage(channel_session_ids.clone());`——加在原 `let channel_session_ids = Arc::new(...);` 后立即 `app.manage(channel_session_ids.clone());`。

- [ ] **Step 4: 编译并跑回归**

Run: `cd src-tauri && cargo build 2>&1 | tail -20`
Expected: 编译通过

Run: `cd src-tauri && cargo test --tests review_ --no-fail-fast 2>&1 | tail -10`
Expected: 与 Step 1 基线一致

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/lib.rs src-tauri/src/connector/im/manager.rs
git commit -m "refactor(channel): extract ensure_channel_manager_registered helper

Slot is now registered unconditionally at setup; the actual
ChannelManager is installed via this helper, callable both from
setup (boot-time bring-up) and from cloud_login. The helper is
idempotent and replaces stale instances on scope change."
```

---

## Task 6: 加 3 个具体 `AuthDeactivationHandler` 实现

**Files:**
- Modify: `src-tauri/src/lib.rs`（在 setup 末尾、`auth_manager` 已 `app.manage` 之后注册三个 handler）

- [ ] **Step 1: 写 handlers**

在 `src-tauri/src/lib.rs` 文件底部（或新建 `src-tauri/src/runtime/deactivation_handlers.rs`，本任务为节省 import 直接放 lib.rs）加：

```rust
struct ChannelManagerDeactivator {
    slot: Arc<crate::connector::im::ChannelManagerSlot>,
}

#[async_trait::async_trait]
impl crate::auth::AuthDeactivationHandler for ChannelManagerDeactivator {
    async fn on_deactivated(&self) {
        let prev = self.slot.replace(None).await;
        if let Some(cm) = prev {
            // Detach: don't make logout wait on the 3s shutdown budget.
            tokio::spawn(async move {
                log::info!("[channel] deactivation: shutting down current instance");
                cm.shutdown().await;
            });
        }
    }
}

struct CurrentUserStorageDeactivator {
    cus: Arc<crate::storage::CurrentUserStorage>,
}

#[async_trait::async_trait]
impl crate::auth::AuthDeactivationHandler for CurrentUserStorageDeactivator {
    async fn on_deactivated(&self) {
        self.cus.deactivate();
    }
}

struct FileManagerWorkspaceResetter {
    file_mgr: Arc<crate::storage::file_manager::FileManager>,
    home: Arc<crate::storage::AiJiaHome>,
}

#[async_trait::async_trait]
impl crate::auth::AuthDeactivationHandler for FileManagerWorkspaceResetter {
    async fn on_deactivated(&self) {
        self.file_mgr.update_workspace_path(self.home.root());
    }
}
```

- [ ] **Step 2: setup 中注册**

在 `src-tauri/src/lib.rs` 的 setup 中，已 `app.manage(auth_manager)` 后追加（auth_manager 还在作用域中——它在 `app.manage` 时被 clone 进 state，本地 `auth_manager: Arc<auth::AuthManager>` 仍可用）：

```rust
let slot_ref = app
    .state::<Arc<crate::connector::im::ChannelManagerSlot>>()
    .inner()
    .clone();
let cus_ref = app
    .state::<Arc<crate::storage::CurrentUserStorage>>()
    .inner()
    .clone();
let file_mgr_ref = app
    .state::<Arc<crate::storage::file_manager::FileManager>>()
    .inner()
    .clone();
let home_ref = aijia_home.clone();
let am = app.state::<Arc<crate::auth::AuthManager>>().inner().clone();
tauri::async_runtime::block_on(async {
    am.register_deactivation_handler(Arc::new(ChannelManagerDeactivator {
        slot: slot_ref,
    })).await;
    am.register_deactivation_handler(Arc::new(CurrentUserStorageDeactivator {
        cus: cus_ref,
    })).await;
    am.register_deactivation_handler(Arc::new(FileManagerWorkspaceResetter {
        file_mgr: file_mgr_ref,
        home: home_ref,
    })).await;
});
```

> ⚠️ 顺序很重要：必须在 `app.manage(crate::connector::im::ChannelManagerSlot::new())` 与 `app.manage(auth_manager)` 都完成之后。如果原来 setup 中 `auth_manager` 在 line 717 就被 `app.manage` 了，slot 在 Task 5 已被加到那之后，本注册块放在原 `// Initialize ChannelManager` 块的位置（即 line 807 附近）即可。

- [ ] **Step 3: 编译**

Run: `cd src-tauri && cargo build 2>&1 | tail -20`
Expected: 编译通过

- [ ] **Step 4: Commit**

```bash
git add src-tauri/src/lib.rs
git commit -m "feat(auth): register 3 deactivation handlers in setup

Wires ChannelManager / CurrentUserStorage / FileManager-workspace
into the AuthManager deactivation chain. Now any of the 3 logout
paths (manual / change_pw / auto-401) cleans up all three subsystems
in a single fan-out, without needing IPC-specific bookkeeping."
```

---

## Task 7: `cloud_login` 调用 `ensure_channel_manager_registered`

**Files:**
- Modify: `src-tauri/src/commands/auth.rs`

- [ ] **Step 1: 加参数 + 调用**

Edit `src-tauri/src/commands/auth.rs`，把 `cloud_login` 签名加 `app: tauri::AppHandle`：

```rust
#[tauri::command]
pub async fn cloud_login(
    app: tauri::AppHandle,
    auth: State<'_, Arc<AuthManager>>,
    cus: State<'_, Arc<CurrentUserStorage>>,
    home: State<'_, Arc<AiJiaHome>>,
    file_mgr: State<'_, Arc<crate::storage::file_manager::FileManager>>,
    username: String,
    password: String,
) -> Result<CloudAuthInfo, String> {
    // ... existing body unchanged through `cus.activate_scope(scope.clone())` ...

    // After workspace_path / scope.json / active_account writes:
    crate::ensure_channel_manager_registered(&app).await;

    Ok(/* existing return value */)
}
```

具体插入位置：第 128 行 `}` 之前（紧挨 `Ok(result)` 之前），加上 `crate::ensure_channel_manager_registered(&app).await;`。

- [ ] **Step 2: `cloud_logout` 瘦身（依赖 handler 链）**

把 `cloud_logout` 改为：

```rust
#[tauri::command]
pub async fn cloud_logout(
    auth: State<'_, Arc<AuthManager>>,
) -> Result<(), String> {
    auth.logout().await;  // handlers fire cus.deactivate + file_mgr reset + channel shutdown
    Ok(())
}
```

删除原 `cus`/`home`/`file_mgr` 三个参数（保留向后兼容性的话保留参数但不用——本次直接删，IPC schema 变化由 Tauri 静态分析 + 前端类型同步处理）。

- [ ] **Step 3: 前端类型同步**

Run: `grep -rn "cloud_logout\|cloudLogout" src/lib/tauri.ts`
确认前端调用形态。如果前端用 `invoke('cloud_logout', { ... })` 传了 `cus/home/file_mgr` 之类的参数（不会，这些是后端 State 自动注入），则前端不用改；否则也不用改。Tauri State 参数不出现在 JS 侧调用签名里。

Run: `pnpm exec tsc --noEmit 2>&1 | tail -10`
Expected: 无类型错误

- [ ] **Step 4: 编译 + 跑测试**

Run: `cd src-tauri && cargo build 2>&1 | tail -20`
Expected: 通过

Run: `cd src-tauri && cargo test --lib commands::`
Expected: 全 PASS（无 commands::auth 旧版固定 4 参数签名的测试假设）

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/commands/auth.rs
git commit -m "feat(auth): cloud_login installs ChannelManager via helper; logout slim

cloud_login now triggers ensure_channel_manager_registered after
activating the user scope, fixing the 'app started while logged out
→ login → channel still uninitialized' bug.

cloud_logout no longer duplicates cleanup logic — AuthManager's
deactivation chain owns cus.deactivate + file_mgr workspace reset +
channel shutdown."
```

---

## Task 8: `commands/channel.rs` 从 slot 取实例

**Files:**
- Modify: `src-tauri/src/commands/channel.rs`

- [ ] **Step 1: 改 `manager(&app)` 辅助函数**

替换 `src-tauri/src/commands/channel.rs` line 15-19：

```rust
async fn manager(app: &AppHandle) -> Result<Arc<ChannelManager>, String> {
    let slot = app
        .try_state::<Arc<crate::connector::im::ChannelManagerSlot>>()
        .ok_or_else(|| "频道功能未初始化，请先登录".to_string())?
        .inner()
        .clone();
    slot.current()
        .await
        .ok_or_else(|| "频道功能未初始化，请先登录".to_string())
}
```

由于函数变成了 `async`，所有调用点（同一文件内 `manager(&app)?` 调用）必须改为 `manager(&app).await?`。

逐个搜索修复：

```bash
grep -n "manager(&app)" src-tauri/src/commands/channel.rs
```

把每处 `manager(&app)?` 改为 `manager(&app).await?`。

- [ ] **Step 2: line 139 类似的 `try_state::<Arc<ChannelManager>>` 直接调用也要改**

Search:

```bash
grep -n "try_state::<Arc<ChannelManager>>" src-tauri/src/commands/channel.rs
```

行 139 原来用 `app.try_state::<Arc<ChannelManager>>()` 自行兜底——改为：

```rust
let conversations = match app
    .try_state::<Arc<crate::connector::im::ChannelManagerSlot>>()
{
    Some(slot) => match slot.inner().clone().current().await {
        Some(cm) => cm.get_conversations(/* args */).await.unwrap_or_default(),
        None => Vec::new(),
    },
    None => Vec::new(),
};
```

（保持原"无 manager 时返回空 Vec"的语义。）

- [ ] **Step 3: 编译**

Run: `cd src-tauri && cargo build 2>&1 | tail -30`
Expected: 通过

- [ ] **Step 4: 跑 channel command 相关测试**

Run: `cd src-tauri && cargo test --lib commands::channel`
Expected: PASS（如无）

Run: `cd src-tauri && cargo test --tests review_ --no-fail-fast 2>&1 | tail -10`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/commands/channel.rs
git commit -m "refactor(channel-cmd): resolve manager via ChannelManagerSlot

All channel_* IPC commands now look up the active ChannelManager via
slot.current().await instead of app.state::<Arc<ChannelManager>>().
Same error text preserved ('频道功能未初始化，请先登录') for None paths."
```

---

## Task 9: 集成测试

**Files:**
- Create: `src-tauri/tests/channel_deactivation_integration_test.rs`

- [ ] **Step 1: 写完整集成测试**

Create `src-tauri/tests/channel_deactivation_integration_test.rs`:

```rust
//! End-to-end契约: AuthManager deactivation chain + ChannelManagerSlot.
//!
//! Verifies three scenarios that motivated the refactor:
//! 1. App boots logged-out → user logs in → channel becomes available.
//! 2. App boots logged-in → user logs out → channel reports uninitialized.
//! 3. Auto-401 deactivation has the same effect as manual logout.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use aijia::auth::{AuthDeactivationHandler, AuthManager};

struct Probe(Arc<AtomicUsize>);

#[async_trait::async_trait]
impl AuthDeactivationHandler for Probe {
    async fn on_deactivated(&self) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
}

#[tokio::test]
async fn logout_triggers_handler_chain() {
    let am = AuthManager::for_test();
    let counter = Arc::new(AtomicUsize::new(0));
    am.register_deactivation_handler(Arc::new(Probe(counter.clone()))).await;
    am.set_state_for_test(/* helper from Task 2 */ todo!());
    am.logout().await;
    assert_eq!(counter.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn auto_401_deactivation_triggers_handler_chain() {
    let am = AuthManager::for_test();
    let counter = Arc::new(AtomicUsize::new(0));
    am.register_deactivation_handler(Arc::new(Probe(counter.clone()))).await;
    am.set_state_for_test(todo!());
    // Drive the same internal "server rejected refresh" path via the test
    // helper added in Task 2 (clear_state_and_fire_handlers_for_test).
    am.clear_state_and_fire_handlers_for_test().await;
    assert_eq!(counter.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn slot_replace_returns_previous_instance() {
    use aijia::connector::im::ChannelManagerSlot;
    let slot = ChannelManagerSlot::new();
    assert!(slot.current().await.is_none());
    let prev = slot.replace(None).await;
    assert!(prev.is_none());
}
```

> ⚠️ 实际的 Tauri AppHandle 在集成测试里很难造，所以"setup → login → channel ready"的端到端路径只能在手动 QA 验证。本集成测试覆盖 AuthManager 钩子链与 slot 容器即可。Step 2 的 manual QA 清单弥补端到端覆盖。

- [ ] **Step 2: Run tests**

Run: `cd src-tauri && cargo test --test channel_deactivation_integration_test`
Expected: 3 条全 PASS

- [ ] **Step 3: 手动 QA 清单（写到 PR description）**

```
- [ ] 场景 A：删除 ~/.renlijia/global/auth/ → 启动 app → 进入登录页 → 登录 → 进入频道页 → 不应看到「频道功能未初始化」错误，应看到平台列表
- [ ] 场景 B：登录态启动 → 进入频道页（应正常） → 退出登录 → 重新进入频道页 → 应看到「频道功能未初始化」错误
- [ ] 场景 C：登录态启动 → 钉钉/飞书已连接 → 修改服务端用户密码使 refresh token 失效 → 等 15min 自动 401 触发 → 不重启 app 重新登录 → 进入频道页 → 应能看到平台列表（新实例）
- [ ] 场景 D：切账号——A 用户登录 → 配置钉钉 → 登出 → B 用户登录 → 频道页应显示 B 的配置而非 A 的
```

- [ ] **Step 4: Commit**

```bash
git add src-tauri/tests/channel_deactivation_integration_test.rs
git commit -m "test(channel): integration tests for deactivation chain + slot

Covers AuthManager handler fan-out and ChannelManagerSlot replace
semantics. End-to-end login/logout cycles validated via manual QA
checklist in the PR description."
```

---

## Task 10: 全量回归

- [ ] **Step 1: Rust 全量编译 + 测试**

Run: `cd src-tauri && cargo build`
Expected: 0 warning 0 error

Run: `cd src-tauri && cargo test --no-fail-fast 2>&1 | tail -30`
Expected: 与本次改动前基线一致或新增测试 PASS，无回归

- [ ] **Step 2: 前端 lint + test**

Run: `pnpm lint`
Expected: 无新增 lint 错误

Run: `pnpm exec vitest run src/lib/tauri.events.test.ts src/hooks/useStreaming.integration.test.tsx src/stores/chatStore.test.ts src/stores/channelStore.test.ts`
Expected: 全 PASS

- [ ] **Step 3: review_ 架构约束**

Run: `cd src-tauri && cargo test review_ --tests --no-fail-fast`
Expected: 全 PASS

- [ ] **Step 4: 启动跑通**

Run: `pnpm tauri:dev`
按手动 QA 清单（Task 9 Step 3）走一遍 4 个场景。

- [ ] **Step 5: 最终 commit/PR**

```bash
git add -A
git status  # 应该是干净的，所有改动已 commit
git log --oneline -10
```

如有遗留改动追加 `chore: cleanup` commit；否则直接进入 PR 流程。

---

## Self-Review

**1. Spec coverage:**
- ✅ ChannelManager slot 容器（Task 3）
- ✅ ChannelManager shutdown + inactive（Task 4）
- ✅ AuthDeactivationHandler trait（Task 1）
- ✅ AuthManager 3 处失活点触发（Task 2）
- ✅ 3 个 handler 注册（Task 6）
- ✅ cloud_login 装配 helper（Task 7）
- ✅ cloud_logout 瘦身（Task 7）
- ✅ commands/channel.rs slot 适配（Task 8）
- ✅ setup 改造（Task 5）
- ✅ 集成测试（Task 9）+ 全量回归（Task 10）

**2. Placeholder scan:** Task 9 step 1 中 `todo!()` 是允许的——表示需要复用 Task 2 中创建的 test helper，集成测试编写时必须替换为真实调用。其它任务无 TODO/TBD。

**3. Type consistency:**
- `ChannelManagerSlot::current() -> Option<Arc<ChannelManager>>`：在 Task 3 / 5 / 6 / 8 一致
- `ChannelManagerSlot::replace(Option<Arc<ChannelManager>>) -> Option<Arc<ChannelManager>>`：Task 3 / 6 一致
- `ChannelManager::shutdown(&self) -> ()` (async)：Task 4 / 5 / 6 一致
- `ChannelManager::is_inactive(&self) -> bool`：Task 4 / 4 step 5 一致
- `ChannelManager::active_scope(&self) -> Option<UserScope>`：Task 5 / 5 调用一致
- `AuthDeactivationHandler::on_deactivated(&self)` (async)：Task 1 / 2 / 6 / 9 一致
- `AuthManager::register_deactivation_handler(Arc<dyn AuthDeactivationHandler>)` (async)：Task 2 / 6 / 9 一致
- `ensure_channel_manager_registered(&AppHandle)` (async)：Task 5 / 7 一致

无类型漂移。

---
