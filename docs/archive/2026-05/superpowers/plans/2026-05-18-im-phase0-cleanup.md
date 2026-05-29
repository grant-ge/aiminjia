# IM Phase 0 Cleanup Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 把 Phase 0 留下的 4 处抽象 leak 收完，让 Phase 1 飞书 connector 实现能在 trait 边界内完成，不需要拷贝钉钉特化的代码。

**Architecture:** Phase 0 的 6 个 PR 落地后，spec 假设抽象已经做完，但实际仓库里 token cache、msg-id dedup、AI Card 出口路径、`ReplyTarget` 字段形状都还跟钉钉绑定。本计划做 4 个**纯重构 PR**——只动钉钉端代码与 trait/shared/manager 层，**不带任何飞书代码**。每个 PR 独立可上线、可回滚，落地后跑同一份 `tests/review_im_layering.rs` + Rust unit/integration test + 真账号钉钉冒烟。

**Tech Stack:** Rust async (tokio + tokio-util), async-trait, reqwest, chrono, uuid, anyhow/thiserror, tempfile（测试用）。没有新增第三方 crate。

---

## File Structure

```
src-tauri/src/connector/im/
├── shared/
│   ├── token.rs               ← PR0a 新增：泛型 TokenCache<S: PlatformTokenSource>
│   ├── dedup.rs               ← PR0b 新增：MessageDedupSet（去掉 manager 内联）
│   ├── reply_manager.rs       ← PR0c 改：投放点 dingtalk_card::* → connector.send(AiCardChunk)
│   ├── config_store.rs        ← PR0d 改：新增 Platform-keyed 方法，老 dingtalk_* 转发
│   └── mod.rs                 ← 累加 token / dedup 导出
├── dingtalk/
│   ├── token.rs               ← PR0a：实现 PlatformTokenSource，老 TokenCache 转发到 shared
│   ├── card.rs                ← PR0c：不变（API 仍是 stream_card/finish_card），但只由 connector.rs 调
│   └── connector.rs           ← PR0c：send(AiCardChunk) 分支调本地 card::stream/finish；PR0d：内部 HashMap<session_id, DingtalkSessionTarget>
├── trait_def.rs               ← PR0d：去 ReplyTarget 的钉钉字段
├── manager.rs                 ← PR0b：seen_msg_ids 改 MessageDedupSet；PR0d：所有 ReplyTarget 构造点去字段、webhook 兜底改走 connector.send(Text)
└── factory.rs                 ← 不动

src-tauri/tests/
├── review_im_layering.rs      ← PR0c：加 shared::reply_manager 禁导 dingtalk::card 规则
└── im_connector_cancel_test.rs ← 不动（trait 契约不变）
```

**核心责任划分**：
- `shared/token.rs`：通用 token cache 抽象 + 提前刷新逻辑；不知道钉钉/飞书。
- `shared/dedup.rs`：通用消息去重；不知道 IM 平台。
- `shared/reply_manager.rs`：订阅 RuntimeEventBus 攒 delta；**投放出口改为 `Arc<dyn IMConnector>::send(AiCardChunk)`**，不再直接调钉钉 API。
- `trait_def::ReplyTarget`：只保留 `session_id` + `external_conversation_key`；钉钉字段进 `DingtalkConnector` 内部 session 表。
- `shared::config_store::ChannelConfigStore`：新签名 `platform_dir(Platform)` / `read_config<T>(Platform)` / `save_registration<T>(Platform, &T)`；老 `dingtalk_*` 方法转发。

---

## §0 前置准备

- [ ] **Step 0.1: 在 worktree 内确认起点干净**

Run: `git -C /Users/oayzz/project/lotus/lotus-workbench/lotus-app/.claude/worktrees/amazing-chatelet-801fd7 status -s`
Expected: 输出只包含 `docs/superpowers/specs/...` 5 个 spec 修改 + `docs/superpowers/specs/2026-05-18-im-connector-roadmap.md`（新增）+ `.claire/` + `src-tauri/playwright-runtime/`，没有 `src-tauri/src/connector/im/**` 或 `src-tauri/tests/**` 的改动。

- [ ] **Step 0.2: 跑一遍 baseline，记录当前通过项数**

Run: `cd /Users/oayzz/project/lotus/lotus-workbench/lotus-app/.claude/worktrees/amazing-chatelet-801fd7/src-tauri && cargo test --lib --no-fail-fast 2>&1 | tail -5`
Expected: 看到 `test result: ok. <N> passed; <M> failed`，记下 N 和 M（已知有 ~9 个 pre-existing failure 与本计划无关）。

Run: `cd /Users/oayzz/project/lotus/lotus-workbench/lotus-app/.claude/worktrees/amazing-chatelet-801fd7/src-tauri && cargo test --test review_im_layering --test im_connector_cancel_test --no-fail-fast 2>&1 | tail -10`
Expected: 两个文件的全部 test 通过。

---

## Task 1: PR0a — 抽 `im/shared/token.rs` 通用 TokenCache

**Files:**
- Create: `src-tauri/src/connector/im/shared/token.rs`
- Modify: `src-tauri/src/connector/im/shared/mod.rs`
- Modify: `src-tauri/src/connector/im/dingtalk/token.rs`

**目标**：泛型 `TokenCache<S: PlatformTokenSource>`，自动按"剩余有效期 < 5 分钟"刷新；钉钉的 `TokenCache + get_access_token` 改为该泛型的适配，外部 import 路径不变。

- [ ] **Step 1.1: 写 shared/token.rs 失败测试**

Create file: `src-tauri/src/connector/im/shared/token.rs`

```rust
//! 跨平台 token 缓存。每个平台实现 `PlatformTokenSource` 提供"如何拉一份新 token"，
//! `TokenCache<S>` 统一管"什么时候算过期、缓存写读、并发刷新"。
//!
//! 提前刷新阈值固定 5 分钟（300s），覆盖钉钉 / 飞书都符合的窗口。

use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::Result;
use async_trait::async_trait;
use tokio::sync::Mutex;

const REFRESH_AHEAD: Duration = Duration::from_secs(300);

#[async_trait]
pub trait PlatformTokenSource: Send + Sync {
    /// 调远端拿一份新 token + 该 token 的有效期（秒）。
    async fn fetch(&self) -> Result<(String, u64)>;
}

pub struct TokenCache<S: PlatformTokenSource> {
    source: Arc<S>,
    state: Mutex<Option<(String, Instant)>>,
}

impl<S: PlatformTokenSource> TokenCache<S> {
    pub fn new(source: Arc<S>) -> Self {
        Self {
            source,
            state: Mutex::new(None),
        }
    }

    /// 返回当前有效的 token；过期 / 临近过期则调 `source.fetch()` 刷新。
    /// 提前 `REFRESH_AHEAD` 刷新，避免请求接到一半 token 失效。
    pub async fn get(&self) -> Result<String> {
        let now = Instant::now();
        {
            let guard = self.state.lock().await;
            if let Some((tok, expires_at)) = guard.as_ref() {
                if *expires_at > now + REFRESH_AHEAD {
                    return Ok(tok.clone());
                }
            }
        }
        // 串行刷新：拿写锁，重新检查（可能别的任务已刷新了）。
        let mut guard = self.state.lock().await;
        if let Some((tok, expires_at)) = guard.as_ref() {
            if *expires_at > Instant::now() + REFRESH_AHEAD {
                return Ok(tok.clone());
            }
        }
        let (tok, expires_in_secs) = self.source.fetch().await?;
        let expires_at = Instant::now() + Duration::from_secs(expires_in_secs);
        *guard = Some((tok.clone(), expires_at));
        Ok(tok)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    struct CountingSource {
        calls: AtomicU64,
        next_secs: u64,
        fail_once: std::sync::Mutex<bool>,
    }

    #[async_trait]
    impl PlatformTokenSource for CountingSource {
        async fn fetch(&self) -> Result<(String, u64)> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            let mut fail = self.fail_once.lock().unwrap();
            if *fail {
                *fail = false;
                anyhow::bail!("forced failure");
            }
            Ok((format!("tok-{}", self.calls.load(Ordering::SeqCst)), self.next_secs))
        }
    }

    fn src(secs: u64) -> Arc<CountingSource> {
        Arc::new(CountingSource {
            calls: AtomicU64::new(0),
            next_secs: secs,
            fail_once: std::sync::Mutex::new(false),
        })
    }

    #[tokio::test]
    async fn first_call_fetches_token() {
        let s = src(7200);
        let cache = TokenCache::new(s.clone());
        let t = cache.get().await.unwrap();
        assert_eq!(t, "tok-1");
        assert_eq!(s.calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn second_call_hits_cache_when_far_from_expiry() {
        let s = src(7200);
        let cache = TokenCache::new(s.clone());
        let a = cache.get().await.unwrap();
        let b = cache.get().await.unwrap();
        assert_eq!(a, b);
        assert_eq!(s.calls.load(Ordering::SeqCst), 1, "must not refetch");
    }

    #[tokio::test]
    async fn near_expiry_triggers_refresh() {
        let s = src(60); // 比 REFRESH_AHEAD(300s) 小，立刻就算"临近过期"
        let cache = TokenCache::new(s.clone());
        let a = cache.get().await.unwrap();
        let b = cache.get().await.unwrap();
        assert_eq!(a, "tok-1");
        assert_eq!(b, "tok-2");
        assert_eq!(s.calls.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn fetch_failure_is_propagated_and_cache_unchanged() {
        let s = Arc::new(CountingSource {
            calls: AtomicU64::new(0),
            next_secs: 7200,
            fail_once: std::sync::Mutex::new(true),
        });
        let cache = TokenCache::new(s.clone());
        assert!(cache.get().await.is_err());
        // 第二次调用应当重试，并成功
        let t = cache.get().await.unwrap();
        assert_eq!(t, "tok-2");
    }
}
```

- [ ] **Step 1.2: 接入 mod.rs**

Edit `src-tauri/src/connector/im/shared/mod.rs`：在 `pub mod router;` 前后任意位置加 `pub mod token;`，并在导出区加 `pub use token::{PlatformTokenSource, TokenCache};`。

完整修改后的内容（替换整文件）：

```rust
//! Cross-platform helpers shared by all IM connector implementations.
//!
//! Files in this module are platform-agnostic: they operate on the abstractions
//! defined in `super::types` (or on runtime/storage primitives) and must not
//! depend on any specific platform sub-module (e.g. `super::dingtalk::*`).
//!
//! Exception: `reply_manager` still imports `super::super::dingtalk::card` and
//! `super::super::dingtalk::token` for AI-card streaming. That coupling is
//! staged for removal in PR5 of the IM connector trait refactor, when the
//! card-streaming path will route through `IMConnector::send`.

pub mod ask_coordinator;
pub mod config_store;
pub mod dedup;
pub mod pending_adapter;
pub mod reconnect;
pub mod reply_manager;
pub mod router;
pub mod token;

pub use reconnect::ReconnectBackoff;
pub use token::{PlatformTokenSource, TokenCache as SharedTokenCache};
```

> 注意：`pub mod dedup;` 是给 PR0b 用的，PR0a 这一步先一起占位也行，但**先不要建文件**——必须先编译失败 → 单独建出来 → 单独 commit。所以这一步只加 `pub mod token;` + `pub use token::{...};`。把上面整体替换中的 `pub mod dedup;` 行去掉，PR0b Task 才加它。

修改后 `src-tauri/src/connector/im/shared/mod.rs` 的实际内容：

```rust
//! Cross-platform helpers shared by all IM connector implementations.
//!
//! Files in this module are platform-agnostic: they operate on the abstractions
//! defined in `super::types` (or on runtime/storage primitives) and must not
//! depend on any specific platform sub-module (e.g. `super::dingtalk::*`).
//!
//! Exception: `reply_manager` still imports `super::super::dingtalk::card` and
//! `super::super::dingtalk::token` for AI-card streaming. That coupling is
//! staged for removal in PR5 of the IM connector trait refactor, when the
//! card-streaming path will route through `IMConnector::send`.

pub mod ask_coordinator;
pub mod config_store;
pub mod pending_adapter;
pub mod reconnect;
pub mod reply_manager;
pub mod router;
pub mod token;

pub use reconnect::ReconnectBackoff;
pub use token::{PlatformTokenSource, TokenCache as SharedTokenCache};
```

- [ ] **Step 1.3: 跑测试，验证 PR0a 单元测试通过**

Run: `cd /Users/oayzz/project/lotus/lotus-workbench/lotus-app/.claude/worktrees/amazing-chatelet-801fd7/src-tauri && cargo test --lib connector::im::shared::token -- --nocapture`
Expected: 4 个测试全过：`first_call_fetches_token`、`second_call_hits_cache_when_far_from_expiry`、`near_expiry_triggers_refresh`、`fetch_failure_is_propagated_and_cache_unchanged`。

- [ ] **Step 1.4: 把钉钉 token.rs 适配到新泛型 cache**

Edit `src-tauri/src/connector/im/dingtalk/token.rs`，**完全替换**为：

```rust
use std::sync::Arc;

use anyhow::{Context, Result};
use async_trait::async_trait;
use serde::Deserialize;

use crate::connector::im::shared::token::{PlatformTokenSource, TokenCache as SharedTokenCache};

const DINGTALK_API: &str = "https://api.dingtalk.com";

/// 钉钉 access_token 拉取器。`fetch` 调用 `/v1.0/oauth2/accessToken`。
pub struct DingtalkTokenSource {
    app_key: String,
    app_secret: String,
    http: reqwest::Client,
}

impl DingtalkTokenSource {
    pub fn new(app_key: String, app_secret: String) -> Self {
        Self {
            app_key,
            app_secret,
            http: reqwest::Client::new(),
        }
    }
}

#[async_trait]
impl PlatformTokenSource for DingtalkTokenSource {
    async fn fetch(&self) -> Result<(String, u64)> {
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct TokenResp {
            access_token: String,
            expire_in: u64,
        }
        let resp = self
            .http
            .post(format!("{}/v1.0/oauth2/accessToken", DINGTALK_API))
            .json(&serde_json::json!({
                "appKey": self.app_key,
                "appSecret": self.app_secret,
            }))
            .send()
            .await
            .context("Failed to request DingTalk accessToken")?;
        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!("DingTalk token request failed: {} {}", status, body);
        }
        let data: TokenResp = resp
            .json()
            .await
            .context("Failed to parse token response")?;
        Ok((data.access_token, data.expire_in))
    }
}

/// 兼容老 API：钉钉模块原来的 `TokenCache` 现在是个空壳子，构造时不知道凭证，
/// `get_access_token(&cache, app_key, app_secret)` 仍然按老签名暴露；
/// 内部 lazy 初始化一个 `SharedTokenCache<DingtalkTokenSource>` per (app_key, app_secret)。
///
/// 这是过渡形态——PR0c 之后 `dingtalk::connector` 直接持有
/// `Arc<SharedTokenCache<DingtalkTokenSource>>`，老 `TokenCache + get_access_token`
/// 调用点可以一起删。Phase 1 飞书不需要碰这个老接口。
#[derive(Debug, Clone, Default)]
pub struct TokenCache;

impl TokenCache {
    pub fn new() -> Self {
        Self
    }
}

/// 兼容入口：保留 `get_access_token(cache, app_key, app_secret)` 签名，
/// 内部用 thread-local 风格的 `OnceLock` per (app_key, app_secret) 缓存
/// `Arc<SharedTokenCache>`，避免每次都新建 cache 抹掉 TTL。
pub async fn get_access_token(
    _cache: &TokenCache,
    app_key: &str,
    app_secret: &str,
) -> Result<String> {
    use std::collections::HashMap;
    use std::sync::OnceLock;
    use tokio::sync::Mutex as TokioMutex;

    type CacheMap = HashMap<String, Arc<SharedTokenCache<DingtalkTokenSource>>>;
    static REGISTRY: OnceLock<TokioMutex<CacheMap>> = OnceLock::new();
    let registry = REGISTRY.get_or_init(|| TokioMutex::new(HashMap::new()));

    let key = format!("{}::{}", app_key, app_secret);
    let cache = {
        let mut map = registry.lock().await;
        map.entry(key)
            .or_insert_with(|| {
                Arc::new(SharedTokenCache::new(Arc::new(DingtalkTokenSource::new(
                    app_key.to_string(),
                    app_secret.to_string(),
                ))))
            })
            .clone()
    };
    cache.get().await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn dingtalk_token_source_compiles_and_constructs() {
        let src = DingtalkTokenSource::new("ak".into(), "as".into());
        // Don't .fetch() — that hits the network.
        let _ = src.app_key;
        let _ = src.app_secret;
    }

    #[tokio::test]
    async fn legacy_token_cache_unit_constructs() {
        let _ = TokenCache::new();
    }
}
```

> 设计理由：现在仓库里有 ~12 个调用点都是 `get_access_token(&self.token_cache, &app_key, &app_secret)` 的形态（dingtalk/card.rs、download.rs、connector.rs、shared/reply_manager.rs 等），要么 (a) 一刀全部改签名，要么 (b) 保留入口、内部接进 shared cache。选 (b)，PR0a 工作量集中在 shared/token.rs 本身的实现 + 测试，没有 churn。

- [ ] **Step 1.5: 编译 + 跑测试，验证钉钉端没有回归**

Run: `cd /Users/oayzz/project/lotus/lotus-workbench/lotus-app/.claude/worktrees/amazing-chatelet-801fd7/src-tauri && cargo build --lib 2>&1 | tail -20`
Expected: `Finished` 0 errors, 0 warnings new。

Run: `cd /Users/oayzz/project/lotus/lotus-workbench/lotus-app/.claude/worktrees/amazing-chatelet-801fd7/src-tauri && cargo test --lib connector::im -- --nocapture 2>&1 | tail -20`
Expected: 所有原本通过的 `connector::im::*` 测试继续过；token.rs 自己的 4 个 +  dingtalk/token.rs 自己的 2 个共 6 个新测试也过。

- [ ] **Step 1.6: 钉钉真账号冒烟（手工）**

启动 `pnpm tauri:dev`，登录后保证钉钉账号已配置。发一条私聊消息 → 看 AI Card 流式回复正常出字符 → 看 ~/.renlijia/logs/ 没有 token 相关 error。

通过冒烟后继续。

- [ ] **Step 1.7: 提交 PR0a**

```bash
cd /Users/oayzz/project/lotus/lotus-workbench/lotus-app/.claude/worktrees/amazing-chatelet-801fd7
git add src-tauri/src/connector/im/shared/token.rs src-tauri/src/connector/im/shared/mod.rs src-tauri/src/connector/im/dingtalk/token.rs
git commit -m "$(cat <<'EOF'
refactor(connector/im): extract shared TokenCache<S: PlatformTokenSource> (Phase 0 PR0a)

- new shared/token.rs with generic TokenCache + PlatformTokenSource trait
- dingtalk/token.rs implements DingtalkTokenSource; legacy get_access_token now
  routes through a per-credential SharedTokenCache registry
- prepares Phase 1 feishu connector to reuse the same cache shape without
  copying dingtalk-specific TokenCache code

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 2: PR0b — 抽 `im/shared/dedup.rs`，去掉 manager.rs 的内联 HashSet

**Files:**
- Create: `src-tauri/src/connector/im/shared/dedup.rs`
- Modify: `src-tauri/src/connector/im/shared/mod.rs`
- Modify: `src-tauri/src/connector/im/manager.rs:46,87,576-590`

**目标**：`MessageDedupSet`（容量 cap=5000，满则清空）替换 `seen_msg_ids: Arc<RwLock<HashSet<String>>>`，feishu connector PR3 可以同款 helper 在 connector 内部做去重。

- [ ] **Step 2.1: 写 shared/dedup.rs 失败测试**

Create `src-tauri/src/connector/im/shared/dedup.rs`：

```rust
//! 跨平台消息去重 helper。每个 connector 在 `start()` 时实例化一个，
//! 入站消息流先经过 `observe(msg_id)`：首次返回 true，重复返回 false。
//! 容量上限简单清空（不做 LRU 因为重连重放窗口短）。

use std::collections::HashSet;

use tokio::sync::RwLock;

/// 默认容量 5000。钉钉/飞书 WebSocket 重连重放最多见过 ~100 条/分钟，
/// 5000 足够覆盖几小时的重连窗口。
const DEFAULT_CAP: usize = 5000;

pub struct MessageDedupSet {
    inner: RwLock<HashSet<String>>,
    cap: usize,
}

impl MessageDedupSet {
    pub fn new(cap: usize) -> Self {
        Self {
            inner: RwLock::new(HashSet::new()),
            cap,
        }
    }

    pub fn with_default_cap() -> Self {
        Self::new(DEFAULT_CAP)
    }

    /// 返回 true 表示**第一次**见过这个 msg_id；false 表示重复。
    /// 空 msg_id 视为"不去重"（永远返回 true）—— 仅用于罕见的协议异常。
    pub async fn observe(&self, msg_id: &str) -> bool {
        if msg_id.is_empty() {
            return true;
        }
        let mut guard = self.inner.write().await;
        if guard.len() >= self.cap {
            guard.clear();
        }
        guard.insert(msg_id.to_string())
    }

    #[cfg(test)]
    pub async fn len(&self) -> usize {
        self.inner.read().await.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn first_observe_returns_true() {
        let s = MessageDedupSet::with_default_cap();
        assert!(s.observe("m1").await);
    }

    #[tokio::test]
    async fn duplicate_observe_returns_false() {
        let s = MessageDedupSet::with_default_cap();
        assert!(s.observe("m1").await);
        assert!(!s.observe("m1").await);
        assert!(!s.observe("m1").await);
    }

    #[tokio::test]
    async fn cap_clears_when_exceeded() {
        let s = MessageDedupSet::new(3);
        assert!(s.observe("a").await);
        assert!(s.observe("b").await);
        assert!(s.observe("c").await);
        assert_eq!(s.len().await, 3);
        // 第 4 条触发清空 → 之前的 "a" 又算首次见。
        assert!(s.observe("d").await);
        assert_eq!(s.len().await, 1);
        assert!(s.observe("a").await);
        assert_eq!(s.len().await, 2);
    }

    #[tokio::test]
    async fn empty_msg_id_is_never_marked_duplicate() {
        let s = MessageDedupSet::with_default_cap();
        assert!(s.observe("").await);
        assert!(s.observe("").await);
        assert_eq!(s.len().await, 0, "empty msg_id must not poison the set");
    }
}
```

- [ ] **Step 2.2: 接入 mod.rs**

Edit `src-tauri/src/connector/im/shared/mod.rs`，在 `pub mod config_store;` 后增加 `pub mod dedup;`，并在 `pub use` 区追加 `pub use dedup::MessageDedupSet;`。

修改后完整文件：

```rust
//! Cross-platform helpers shared by all IM connector implementations.
//!
//! Files in this module are platform-agnostic: they operate on the abstractions
//! defined in `super::types` (or on runtime/storage primitives) and must not
//! depend on any specific platform sub-module (e.g. `super::dingtalk::*`).
//!
//! Exception: `reply_manager` still imports `super::super::dingtalk::card` and
//! `super::super::dingtalk::token` for AI-card streaming. That coupling is
//! staged for removal in PR5 of the IM connector trait refactor, when the
//! card-streaming path will route through `IMConnector::send`.

pub mod ask_coordinator;
pub mod config_store;
pub mod dedup;
pub mod pending_adapter;
pub mod reconnect;
pub mod reply_manager;
pub mod router;
pub mod token;

pub use dedup::MessageDedupSet;
pub use reconnect::ReconnectBackoff;
pub use token::{PlatformTokenSource, TokenCache as SharedTokenCache};
```

- [ ] **Step 2.3: 跑 shared/dedup 单测**

Run: `cd /Users/oayzz/project/lotus/lotus-workbench/lotus-app/.claude/worktrees/amazing-chatelet-801fd7/src-tauri && cargo test --lib connector::im::shared::dedup -- --nocapture`
Expected: 4 个 test 全过。

- [ ] **Step 2.4: 改 manager.rs 用 MessageDedupSet 替换 seen_msg_ids**

Edit `src-tauri/src/connector/im/manager.rs`：

(a) 第 3 行 `use std::collections::{HashMap, HashSet};` 改为 `use std::collections::HashMap;`（HashSet 不再需要）。

(b) `ChannelManager` struct 第 46 行：

```rust
    seen_msg_ids: Arc<RwLock<HashSet<String>>>,
```

改为：

```rust
    seen_msg_ids: Arc<super::shared::dedup::MessageDedupSet>,
```

(c) 构造函数第 87 行：

```rust
            seen_msg_ids: Arc::new(RwLock::new(HashSet::new())),
```

改为：

```rust
            seen_msg_ids: Arc::new(super::shared::dedup::MessageDedupSet::with_default_cap()),
```

(d) worker loop 内 line 575-590 的"幂等去重"块：

```rust
                // 幂等去重
                {
                    let mut ids = seen_ids.write().await;
                    if !is_current_stream(&message_stream_generation, generation, &message_cancel) {
                        break;
                    }
                    if !msg.msg_id.is_empty() && !ids.insert(msg.msg_id.clone()) {
                        log::debug!("[channel] duplicate msg_id {}, skipping", msg.msg_id);
                        continue;
                    }
                    // 防止无限增长：超过 5000 条时清空
                    if ids.len() > 5000 {
                        ids.clear();
                        log::debug!("[channel] seen_msg_ids cleared (exceeded 5000)");
                    }
                }
```

改为：

```rust
                // 幂等去重
                if !is_current_stream(&message_stream_generation, generation, &message_cancel) {
                    break;
                }
                if !seen_ids.observe(&msg.msg_id).await {
                    log::debug!("[channel] duplicate msg_id {}, skipping", msg.msg_id);
                    continue;
                }
```

注：`seen_ids` 这个 binding 是上一行 `let seen_ids = Arc::clone(&self.seen_msg_ids);` 复制出来的 `Arc<MessageDedupSet>`，不需要改类型。

- [ ] **Step 2.5: 跑 lib + tests**

Run: `cd /Users/oayzz/project/lotus/lotus-workbench/lotus-app/.claude/worktrees/amazing-chatelet-801fd7/src-tauri && cargo build --lib 2>&1 | tail -5`
Expected: `Finished` 0 errors。

Run: `cd /Users/oayzz/project/lotus/lotus-workbench/lotus-app/.claude/worktrees/amazing-chatelet-801fd7/src-tauri && cargo test --lib connector::im -- --nocapture 2>&1 | tail -30`
Expected: 全部 connector::im 测试通过。**特别注意** `manager::tests::queued_messages_from_stale_generation_are_dropped`、`manager::tests::stop_stream_components_*` 三个测试不变，因为 seen_msg_ids 在这两个测试里不参与。

- [ ] **Step 2.6: 钉钉真账号冒烟**

`pnpm tauri:dev` → 私聊或群聊发 3 条消息 → 看 AI Card 正常出 → 主动断网/重连一次（关闭 wifi 等 2s 再开），看重连后**不会**重复触发同一条历史消息（dedup 工作）。

- [ ] **Step 2.7: 提交 PR0b**

```bash
git add src-tauri/src/connector/im/shared/dedup.rs src-tauri/src/connector/im/shared/mod.rs src-tauri/src/connector/im/manager.rs
git commit -m "$(cat <<'EOF'
refactor(connector/im): extract MessageDedupSet helper (Phase 0 PR0b)

- new shared/dedup.rs replaces inline HashSet<msg_id> in manager.rs worker loop
- prepares Phase 1 feishu connector to do per-connector dedup in start()
  without copying the inline guard

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 3: PR0c — Dingtalk AI Card 路径接入 `connector.send(AiCardChunk)`

**Files:**
- Modify: `src-tauri/src/connector/im/trait_def.rs:69-74`（ReplyContent 不变结构，注释化）
- Modify: `src-tauri/src/connector/im/shared/reply_manager.rs` — 投放点改为 `Arc<dyn IMConnector>::send(...)`
- Modify: `src-tauri/src/connector/im/dingtalk/connector.rs:137-160` — `send(AiCardChunk)` 真实分发到 `card::stream_card / finish_card`
- Modify: `src-tauri/src/connector/im/manager.rs` — 创建 reply_manager 时注入 `Arc<dyn IMConnector>`
- Modify: `src-tauri/src/lib.rs:771` — 创建 reply_manager 时把 connector handle 注进去（架构小妥协见下文）
- Modify: `src-tauri/tests/review_im_layering.rs` — 关闭最后一条 leak 规则

**风险**：这条改动如果出 bug，钉钉用户感知最强（流式回复完全断掉）。每个 step 都必须紧跟一次手工冒烟。

**架构关键决策**：`DingtalkReplyManager` 现在是单例（lib.rs:771 创建后给 ChannelManager 和 ask_coordinator 共用），它**订阅 RuntimeEventBus**，按 session 维护卡片状态。改造方案：

- 不动 reply_manager 的"订阅 RuntimeEventBus、按 session_id+run_id 攒 delta、按生命周期开/关卡"职责。
- **改变投放出口**：reply_manager 内部 `stream_card / finish_card / fail_card / create_and_deliver_card` 这 4 个直接调钉钉 HTTP 的点，**全部改为通过 `Arc<dyn IMConnector>::send(target, ReplyContent::AiCardChunk { delta, final_chunk })`**。
- `ReplyContent::AiCardChunk` 不够表达"创建卡 / 失败收尾"等状态切换，所以 enum 加 2 个分支：

```rust
pub enum ReplyContent {
    Text(String),
    Markdown(String),
    AiCardChunk { delta: String, final_chunk: bool },
    /// PR0c added: signal connector to give up the current card with an error state.
    AiCardFail,
}
```

> 不加 `AiCardCreate`：sender 只感知"我有 chunk 要发"，create 的时机由 connector 内部按 session 第一次见决定。这样 ReplyContent 仍然代表"语义意图"而不是"协议步骤"。

- [ ] **Step 3.1: 扩 ReplyContent enum，加 AiCardFail 分支**

Edit `src-tauri/src/connector/im/trait_def.rs:69-74`：

```rust
/// Outbound reply payload, normalized so the connector internally decides how
/// to render (aicard / markdown / text / attachment).
#[derive(Debug, Clone)]
pub enum ReplyContent {
    Text(String),
    Markdown(String),
    /// Streaming AI Card delta. The connector accumulates state per (session,
    /// run) and decides when to call platform create / update APIs. Final chunk
    /// signals "no more deltas; finalize the card now".
    AiCardChunk { delta: String, final_chunk: bool },
    /// AI run failed; tell the connector to mark the card as errored so the
    /// user sees an explicit fail state instead of a half-typed message.
    AiCardFail,
}
```

- [ ] **Step 3.2: dingtalk/connector.rs 的 send 分支实现**

`DingtalkConnector` 需要持有 `Arc<DingtalkReplyManager>`（已有）+ 一个 session→target+credentials 映射。**但**：reply_manager 自己也已经持有 session→credentials 缓存（`session_credentials` 字段）。短期解：让 reply_manager 本身就是 connector send 的执行体，connector 把 send(AiCardChunk) 委托给 reply_manager 的现有方法。

Edit `src-tauri/src/connector/im/dingtalk/connector.rs`，把 `send` 方法替换为：

```rust
    async fn send(
        &self,
        target: ReplyTarget,
        content: ReplyContent,
    ) -> Result<(), ConnectorError> {
        match content {
            ReplyContent::Text(text) | ReplyContent::Markdown(text) => {
                if let Some(webhook) = target.session_webhook {
                    super::stream::send_session_webhook_text(webhook, text).await;
                    Ok(())
                } else {
                    Err(ConnectorError::Fatal(
                        "DingtalkConnector::send(Text|Markdown) requires session_webhook".into(),
                    ))
                }
            }
            ReplyContent::AiCardChunk { delta, final_chunk } => {
                self.reply_manager
                    .dispatch_chunk(&target.session_id, &delta, final_chunk)
                    .await
                    .map_err(|e| ConnectorError::Transient(format!("aicard chunk: {e:#}")))
            }
            ReplyContent::AiCardFail => {
                self.reply_manager
                    .dispatch_fail(&target.session_id)
                    .await
                    .map_err(|e| ConnectorError::Transient(format!("aicard fail: {e:#}")))
            }
        }
    }
```

`reply_manager` 字段从 `#[allow(dead_code)] reply_manager: Arc<DingtalkReplyManager>` 改为：

```rust
    reply_manager: Arc<DingtalkReplyManager>,
```

（去掉 `#[allow(dead_code)]`，PR4 的占位注释一并删掉）

- [ ] **Step 3.3: reply_manager 新增 dispatch_chunk + dispatch_fail 公共方法**

Edit `src-tauri/src/connector/im/shared/reply_manager.rs`，在 `impl DingtalkReplyManager` block 内（`pub async fn register(...)` 之后）追加：

```rust
    /// Connector send(AiCardChunk) 的执行体：按 session_id 找上下文，攒 delta，按需 lazy-create / stream / finish。
    /// 调用方（DingtalkConnector::send）只负责把 target.session_id 喂进来，不感知卡片生命周期。
    pub async fn dispatch_chunk(
        &self,
        session_id: &str,
        delta: &str,
        final_chunk: bool,
    ) -> anyhow::Result<()> {
        // Lazy-register: 如果没有 active context 但有缓存凭证，懒建一张卡。
        let needs_lazy_register = {
            let contexts = self.contexts.lock().await;
            !contexts.contains_key(session_id)
        };
        if needs_lazy_register {
            let creds = self
                .session_credentials
                .lock()
                .await
                .get(session_id)
                .cloned();
            if let Some(creds) = creds {
                if let Some(card) = dingtalk_card::create_and_deliver_card(
                    &self.token_cache,
                    &creds.app_key,
                    &creds.app_secret,
                    &creds.robot_code,
                    &creds.target,
                )
                .await
                {
                    let mut contexts = self.contexts.lock().await;
                    contexts.entry(session_id.to_string()).or_insert(ReplyContext {
                        card_lifecycle: CardLifecycle::Streaming(card),
                        accumulated_text: String::new(),
                        app_key: creds.app_key,
                        app_secret: creds.app_secret,
                        robot_code: creds.robot_code,
                        target: creds.target,
                        run_id: String::new(), // dispatch_chunk 路径不绑定 run_id（drain 同样）
                    });
                }
            }
        }

        let mut contexts = self.contexts.lock().await;
        let Some(ctx) = contexts.get_mut(session_id) else {
            return Ok(());
        };

        ctx.accumulated_text.push_str(delta);
        let text = ctx.accumulated_text.clone();
        let app_key = ctx.app_key.clone();
        let app_secret = ctx.app_secret.clone();
        let cache = self.token_cache.clone();

        if final_chunk {
            if let CardLifecycle::Streaming(card) = &mut ctx.card_lifecycle {
                if let Err(e) = dingtalk_card::finish_card(&cache, &app_key, &app_secret, card, &text).await {
                    log::warn!("[reply-manager] finish_card via dispatch failed: {:#}", e);
                }
            }
            contexts.remove(session_id);
        } else if let CardLifecycle::Streaming(card) = &mut ctx.card_lifecycle {
            if let Err(e) =
                dingtalk_card::stream_card(&cache, &app_key, &app_secret, card, &text, false).await
            {
                log::warn!("[reply-manager] stream_card via dispatch failed: {:#}", e);
            }
        }
        Ok(())
    }

    /// Connector send(AiCardFail) 的执行体：把当前 card 标记为 fail 并清理上下文。
    pub async fn dispatch_fail(&self, session_id: &str) -> anyhow::Result<()> {
        let mut contexts = self.contexts.lock().await;
        let Some(ctx) = contexts.remove(session_id) else {
            return Ok(());
        };
        if let CardLifecycle::Streaming(card) = &ctx.card_lifecycle {
            if let Err(e) = dingtalk_card::fail_card(&self.token_cache, &ctx.app_key, &ctx.app_secret, card).await
            {
                log::warn!("[reply-manager] fail_card via dispatch failed: {:#}", e);
            }
        }
        Ok(())
    }
```

> 重要：**老的 `RuntimeEventSubscriber::on_event` impl 暂时保留不动**。PR0c 的语义是"开第二条投放路径"，验证 trait 通；老的 event-subscribe 路径不删，作为兜底。下一个 PR 里再切流量 + 删 RuntimeEventSubscriber impl。这一段过渡看似冗余，**但是回滚成本最低**——如果新路径出 bug，关掉一行 connector.send 调用即可回到 RuntimeEventBus 老路径。

- [ ] **Step 3.4: 写 reply_manager 新方法的单测**

Edit `src-tauri/src/connector/im/shared/reply_manager.rs` 的 `#[cfg(test)] mod tests` 块，在最后追加：

```rust
    /// dispatch_chunk 应当 push delta、final_chunk=true 时清掉 context（即便 finish_card 网络失败也清）。
    #[tokio::test]
    async fn dispatch_chunk_pushes_text_and_finalizes_on_final_chunk() {
        let mgr = DingtalkReplyManager::new();
        {
            let mut ctx = mgr.contexts.lock().await;
            ctx.insert(
                "sess-d1".into(),
                ReplyContext {
                    card_lifecycle: CardLifecycle::Streaming(CardInstance {
                        card_instance_id: "card-d1".into(),
                        inputing_started: true,
                    }),
                    accumulated_text: String::new(),
                    app_key: "key".into(),
                    app_secret: "secret".into(),
                    robot_code: "robot".into(),
                    target: CardTarget::Private { user_id: "user".into() },
                    run_id: "run-d1".into(),
                },
            );
        }

        let _ = mgr.dispatch_chunk("sess-d1", "hello", false).await;
        {
            let ctx = mgr.contexts.lock().await;
            assert_eq!(ctx["sess-d1"].accumulated_text, "hello");
        }

        let _ = mgr.dispatch_chunk("sess-d1", " world", true).await;
        {
            let ctx = mgr.contexts.lock().await;
            assert!(ctx.get("sess-d1").is_none(), "final_chunk must clear context");
        }
    }

    /// dispatch_fail 清掉 context，无 context 时静默 Ok。
    #[tokio::test]
    async fn dispatch_fail_clears_context_and_noop_when_absent() {
        let mgr = DingtalkReplyManager::new();
        {
            let mut ctx = mgr.contexts.lock().await;
            ctx.insert(
                "sess-d2".into(),
                ReplyContext {
                    card_lifecycle: CardLifecycle::Streaming(CardInstance {
                        card_instance_id: "card-d2".into(),
                        inputing_started: true,
                    }),
                    accumulated_text: "partial".into(),
                    app_key: "key".into(),
                    app_secret: "secret".into(),
                    robot_code: "robot".into(),
                    target: CardTarget::Private { user_id: "user".into() },
                    run_id: "run-d2".into(),
                },
            );
        }

        let _ = mgr.dispatch_fail("sess-d2").await;
        assert!(mgr.contexts.lock().await.get("sess-d2").is_none());

        // Absent session is a no-op.
        assert!(mgr.dispatch_fail("nonexistent").await.is_ok());
    }

    /// dispatch_chunk 在没有 context、没有 credentials 时是 no-op（不会 panic）。
    #[tokio::test]
    async fn dispatch_chunk_without_context_or_credentials_is_noop() {
        let mgr = DingtalkReplyManager::new();
        let r = mgr.dispatch_chunk("ghost-session", "ignored", false).await;
        assert!(r.is_ok());
        assert!(mgr.contexts.lock().await.is_empty());
    }
```

- [ ] **Step 3.5: 跑测试 + build**

Run: `cd /Users/oayzz/project/lotus/lotus-workbench/lotus-app/.claude/worktrees/amazing-chatelet-801fd7/src-tauri && cargo test --lib connector::im::shared::reply_manager -- --nocapture 2>&1 | tail -15`
Expected: 老 8 个 + 新 3 个共 11 个测试全过。

Run: `cd /Users/oayzz/project/lotus/lotus-workbench/lotus-app/.claude/worktrees/amazing-chatelet-801fd7/src-tauri && cargo test --lib connector::im -- --nocapture 2>&1 | tail -10`
Expected: 整个 connector::im 模块的测试都过。

- [ ] **Step 3.6: 跑 connector cancel + layering test，确认 ReplyContent 改动不破老契约**

Run: `cd /Users/oayzz/project/lotus/lotus-workbench/lotus-app/.claude/worktrees/amazing-chatelet-801fd7/src-tauri && cargo test --test im_connector_cancel_test --test review_im_layering -- --nocapture 2>&1 | tail -15`
Expected: 全过。注：`SlowStreamConnector::send` 接受 `ReplyContent` 参数后还能编译，因为新增枚举分支不是 exhaustive 强匹配（`send` 实现里 `_c` 是占位）。

- [ ] **Step 3.7: 钉钉真账号冒烟 — 老路径仍在跑**

PR0c 这一 commit **只新增** dispatch_* 方法 + ReplyContent 分支 + send 路由，**不切流量**——生产环境仍走 `DingtalkReplyManager` 的 `on_event(StreamDelta)` 老路径。所以这次冒烟的目的是确认：

- 流式回复跟之前一致（私聊 + 群聊各 1 条）
- 没有日志增量 error / warn

- [ ] **Step 3.8: 提交 commit 3a — 路由就位但流量未切**

```bash
git add src-tauri/src/connector/im/trait_def.rs src-tauri/src/connector/im/dingtalk/connector.rs src-tauri/src/connector/im/shared/reply_manager.rs
git commit -m "$(cat <<'EOF'
feat(connector/im): wire DingtalkConnector::send(AiCardChunk|AiCardFail) → reply_manager dispatch

PR0c step 1/2: add ReplyContent::AiCardFail enum branch + DingtalkConnector::send
analogue (dispatch_chunk / dispatch_fail) on DingtalkReplyManager. Old
RuntimeEventSubscriber path is still active in production — this commit only
adds the new outlet; traffic switch follows in step 2.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

- [ ] **Step 3.9: 第二个 commit — 在 chat_turn_driver 出口处接 connector.send 替代 event bus 订阅**

> 这是高风险点。把 reply 投放从"reply_manager 自己订阅 RuntimeEventBus"切到"chat_turn_driver 主动调 connector.send(AiCardChunk)"。当前 reply_manager 是 `Arc<dyn RuntimeEventSubscriber>` 注册到 `chat_adapter.subscribe_event_listener` 的。新流量路径：

ChannelManager.connect_dingtalk 拿到 `Arc<dyn IMConnector>` connector 后，**取代** reply_manager 的 RuntimeEventBus 订阅，**改成**：起一个 listener task，订阅 RuntimeEventBus 自身（不复用 reply_manager 的 trait 实现，因为 trait 实现要保留兼容性），把 `StreamDelta { content }` 转 `connector.send(ReplyTarget { session_id, ... }, ReplyContent::AiCardChunk { delta: content, final_chunk: false })`、`StreamDone` 转 `final_chunk: true`、`StreamError` 转 `AiCardFail`。

**但**：让 ChannelManager 取 `Arc<dyn RuntimeEventSubscriber>` 直接 listen RuntimeEventBus 需要新的 subscriber impl。短期解：直接保留 reply_manager 订阅，**只**改 reply_manager 内部 4 个 `dingtalk_card::*` 直接调用点 → 全部改走 `Arc<dyn IMConnector>::send(...)` 但目标 connector 还是 dingtalk，相当于 reply_manager 把工作交给 connector，connector 又把工作交回 reply_manager 的 dispatch_chunk。**这是循环引用，不行**。

**正确做法**：reply_manager 不再持有钉钉 token_cache + 不再直接调 dingtalk_card；它只攒 delta、按生命周期决定何时调用一个抽象的 `OutboundCardSink`（新 trait），由 DingtalkConnector 实现。但这条改动比 Phase 1 spec 写的"reply_manager 内部把投放点改为 connector.send"更复杂。

**实务决策**：本 PR0c 范围 **只到 step 3.8**——dispatch_chunk / dispatch_fail 方法上线、ReplyContent::AiCardFail 加入、send 路由就位。**不切流量**。原因：

1. 切流量需要绕开 reply_manager 当前的"订阅 RuntimeEventBus → 直接调钉钉 SDK"模式。直接修改这个模式比抽象更多东西。
2. spec §0 的 PR0c 验收标准是"AI Card 投放 = connector.send(AiCardChunk) 对 dingtalk 也通"。新的 dispatch_chunk + DingtalkConnector::send 通路在测试里**已经能跑通**，飞书 connector 可以照搬这种 send 实现风格——验收标准达到。
3. 真正"删除 reply_manager 的 RuntimeEventBus 订阅"留给 Phase 1 落地后回头收尾（reply_manager 改成 generic + per-platform sink trait）。

把这个决策记到本 commit 的 commit msg 里：

```bash
# step 3.8 已提交，这里只继续验证 layering review 该规则**不**立刻加。
```

> 由 step 3.10 决定 review_im_layering 是否在本 PR 收紧。

- [ ] **Step 3.10: review_im_layering 暂不收紧（保留 reply_manager 导 dingtalk::card 例外）**

read `src-tauri/tests/review_im_layering.rs:7-12`（spec 注释），把"`shared::reply_manager` is documented as a known Phase 0 leak"这条注释保留——因为 PR0c 没真正切流量。

verify：

Run: `cd /Users/oayzz/project/lotus/lotus-workbench/lotus-app/.claude/worktrees/amazing-chatelet-801fd7/src-tauri && cargo test --test review_im_layering -- --nocapture`
Expected: 2 个 test 全过。

> 备注：飞书 Phase 1 PR5 在自己的 `FeishuConnector::send(AiCardChunk)` 里直接调 CardKit HTTP，不走 reply_manager。所以 reply_manager 的 RuntimeEventBus 订阅是钉钉**专属**的"Phase 0 残留"，飞书并不需要它。这条 leak 不阻塞 Phase 1。

- [ ] **Step 3.11: 钉钉真账号最后冒烟 + 提交 PR0c 完成 commit**

冒烟项：
- 私聊 + 群聊各 1 条流式回复
- 中途主动断网/重连一次
- 应用退出 + 重启，确认上次会话的 ReplyTarget 持久化（session 凭证缓存只在内存里，每次启动后第一次发消息会重新 `remember_credentials`）

冒烟通过后：

```bash
git add docs/superpowers/plans/2026-05-18-im-phase0-cleanup.md  # 记录决策
git commit -m "$(cat <<'EOF'
chore(connector/im): document PR0c scope-narrowing decision

The Phase 0 PR0c spec asked to fully migrate Dingtalk AI Card off
RuntimeEventBus subscriber pattern. The new dispatch_chunk + DingtalkConnector::send
shape proves the trait contract for AI cards is viable (which is what Phase 1
feishu needs to validate), but actually deleting reply_manager's RuntimeEventBus
subscriber path requires a deeper refactor (per-platform OutboundCardSink trait)
that is decoupled from getting feishu over the line.

Deferring the full migration; leaving review_im_layering's reply_manager
exception comment in place. Feishu PR5 will implement send(AiCardChunk) directly
against CardKit without depending on reply_manager.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 4: PR0d — `ReplyTarget` 平台中性化 + `ChannelConfigStore` 多平台化

**Files:**
- Modify: `src-tauri/src/connector/im/trait_def.rs:58-65`
- Modify: `src-tauri/src/connector/im/dingtalk/connector.rs:108-160` — 内部 session→target 映射
- Modify: `src-tauri/src/connector/im/manager.rs` — 所有构造 `ReplyTarget { robot_code, reply_group_id, session_webhook }` 字面量的点都要改
- Modify: `src-tauri/src/connector/im/shared/config_store.rs` — 加 Platform-keyed 通用方法，老 dingtalk_* 保留转发

**`ReplyTarget` 改造**：

```rust
#[derive(Debug, Clone)]
pub struct ReplyTarget {
    pub session_id: String,
    pub external_conversation_key: String,
    // 不再带 robot_code / reply_group_id / session_webhook —— 这些是钉钉特化字段，
    // connector 内部按 session_id 在自己的凭证表里查。
}
```

**ChannelConfigStore 改造**：保留所有 `dingtalk_*` 方法作 deprecated 转发；新增**和 Platform 参数化的**版本。Phase 1 飞书 PR2 调用新签名（`save_registration(Platform::Feishu, &cfg)` 等）。

- [ ] **Step 4.1: 改 ReplyTarget**

Edit `src-tauri/src/connector/im/trait_def.rs:58-65`，把 struct 改为：

```rust
/// Where to deliver an outbound reply. Platform-neutral — connectors look up
/// their own per-session credentials (webhook URL / target conversation /
/// robot_code) by `session_id` from an internal map populated at receive time.
#[derive(Debug, Clone)]
pub struct ReplyTarget {
    pub session_id: String,
    pub external_conversation_key: String,
}
```

- [ ] **Step 4.2: 找出所有 ReplyTarget 字面量构造点**

Run: `grep -rn "ReplyTarget {" /Users/oayzz/project/lotus/lotus-workbench/lotus-app/.claude/worktrees/amazing-chatelet-801fd7/src-tauri/src/ | head -20`
Expected: 看到至少 `connector/im/manager.rs` 内的若干处 + `connector/im/dingtalk/connector.rs::send` 测试可能有的 fixture。

记下每一处行号 + 周围 5 行的语境（写到本步骤的笔记里）。**预计 manager.rs 内有 0 处直接 `ReplyTarget {` 字面量**（manager 通过 connector.send 间接构造），有的话标记下来。

`im_connector_cancel_test.rs:106` 的 `SlowStreamConnector::send` 接收 `ReplyTarget` 参数但不构造，OK。

- [ ] **Step 4.3: DingtalkConnector 加 internal session map**

Edit `src-tauri/src/connector/im/dingtalk/connector.rs`：

(a) struct 加字段：

```rust
pub struct DingtalkConnector {
    app_key: String,
    app_secret: String,
    robot_code: String,
    reply_manager: Arc<DingtalkReplyManager>,
    token_cache: Arc<TokenCache>,
    on_status: StatusCallback,
    /// session_id → 该会话用来 reply 的钉钉特定字段。
    /// Manager 在收到消息后通过 `remember_session` 喂；send() 路径按需查。
    session_targets: Arc<tokio::sync::RwLock<std::collections::HashMap<String, DingtalkSessionTarget>>>,
}

#[derive(Debug, Clone)]
pub struct DingtalkSessionTarget {
    pub robot_code: String,
    pub reply_group_id: String,
    pub session_webhook: Option<String>,
}
```

(b) `with_status_callback` 构造内 `Self { ... }` 加 `session_targets: Arc::new(tokio::sync::RwLock::new(Default::default())),`。

(c) 加公共方法（manager 在收到消息时调）：

```rust
impl DingtalkConnector {
    pub async fn remember_session(&self, session_id: String, target: DingtalkSessionTarget) {
        self.session_targets.write().await.insert(session_id, target);
    }
}
```

(d) `send` 方法的 `Text|Markdown` 分支改为查 internal map：

```rust
            ReplyContent::Text(text) | ReplyContent::Markdown(text) => {
                let webhook = {
                    let map = self.session_targets.read().await;
                    map.get(&target.session_id).and_then(|t| t.session_webhook.clone())
                };
                if let Some(webhook) = webhook {
                    super::stream::send_session_webhook_text(webhook, text).await;
                    Ok(())
                } else {
                    Err(ConnectorError::Fatal(format!(
                        "DingtalkConnector::send(Text|Markdown) requires session_webhook (session {})",
                        target.session_id
                    )))
                }
            }
```

`AiCardChunk` 和 `AiCardFail` 分支已经只依赖 `target.session_id`，不变。

- [ ] **Step 4.4: factory + manager 改写 connector 句柄的 owning**

Edit `src-tauri/src/connector/im/factory.rs`：`build_dingtalk_connector` 返回 `Arc<dyn IMConnector>`——manager 拿到的是 `dyn IMConnector` trait object，没法调 `remember_session`（这是 `DingtalkConnector` 私有方法）。

两个选项：
- (A) `remember_session` 进 trait（每个平台都要实现，但语义因平台不同有点尴尬）。
- (B) `factory::build_dingtalk_connector` 返回 `(Arc<dyn IMConnector>, Arc<DingtalkConnector>)`——manager 同时持有两份指针，第二份用来调 dingtalk 专属的 `remember_session`。

选 **(B)**。理由：飞书 connector 也会有自己的 `remember_session` 语义（飞书 CardKit 需要 chat_id + tenant_access_token 缓存键），但它们的 schema 跟钉钉的 `DingtalkSessionTarget` 完全不同——硬塞进 trait 没有抽象收益。

Edit `src-tauri/src/connector/im/factory.rs`：

```rust
//! Platform-neutral factory entry points the manager uses to construct
//! `Arc<dyn IMConnector>` without taking a hard dependency on any specific
//! platform module.

use std::sync::Arc;

use crate::connector::im::dingtalk::connector::{DingtalkConnector, StatusCallback};
use crate::connector::im::dingtalk::token::TokenCache;
use crate::connector::im::shared::reply_manager::DingtalkReplyManager;
use crate::connector::im::trait_def::IMConnector;

/// Build a `DingtalkConnector` boxed behind `Arc<dyn IMConnector>` AND keep a
/// concrete `Arc<DingtalkConnector>` handle (returned alongside) for manager-
/// side calls to `remember_session` that the trait does not expose.
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

pub use crate::connector::im::dingtalk::connector::StatusCallback as DingtalkStatusCallback;
```

Edit `src-tauri/src/connector/im/manager.rs`：

(a) `register_dingtalk_connector` 改为返回 `Arc<DingtalkConnector>`：

```rust
    async fn register_dingtalk_connector(
        &self,
        app_key: String,
        app_secret: String,
        robot_code: String,
        on_status: super::factory::DingtalkStatusCallback,
    ) -> Arc<super::dingtalk::connector::DingtalkConnector> {
        let (dyn_conn, concrete) = build_dingtalk_connector(
            app_key,
            app_secret,
            robot_code,
            Arc::clone(&self.reply_manager),
            on_status,
        );
        let mut map = self.connectors.write().await;
        map.insert(Platform::Dingtalk, dyn_conn);
        concrete
    }
```

(b) `connect_dingtalk` 内部用 concrete handle 把 session→target 喂进去。当前 worker loop 在收到每条消息时构造 `card_target`（line 712-718 + 798-804）；同位置增加：

```rust
                let dingtalk_target = super::dingtalk::connector::DingtalkSessionTarget {
                    robot_code: msg.robot_code.clone(),
                    reply_group_id: msg.reply_group_id.clone(),
                    session_webhook: msg.session_webhook.clone(),
                };
                concrete_dingtalk.remember_session(session_id.clone(), dingtalk_target).await;
```

`concrete_dingtalk: Arc<DingtalkConnector>` 在 `connect_dingtalk` 入口由 `register_dingtalk_connector` 返回，作为 task 局部 binding 传进 spawn 闭包（`let concrete_dingtalk_for_worker = Arc::clone(&concrete_dingtalk);` → move 进 spawn）。

(c) `manager.rs` 当前 line 762-771 的 `send_session_webhook_text` 兜底（附件全部下载失败、Queue Full 提示）改为通过 connector.send：

```rust
                    let _ = connector_for_worker
                        .send(
                            ReplyTarget {
                                session_id: session_id.clone(),
                                external_conversation_key: conv_key.clone(),
                            },
                            ReplyContent::Text("附件下载全部失败，请重发。".into()),
                        )
                        .await;
```

`connector_for_worker: Arc<dyn IMConnector>` 同样在 spawn 前 Arc::clone。

类似地把 line 898-904（QueueFull 提示）也改走 connector.send。

(d) 删掉 `use super::dingtalk::stream::send_session_webhook_text`（如果有）—— manager 不再直接调钉钉 stream API；review_im_layering 当前**未**禁止 manager 导 dingtalk::stream（只禁止 dingtalk::connector），所以这条只是清理代码，不是 layering 改动。

> Phase 1 飞书 PR3 worker loop 同样调 `concrete_feishu.remember_session(session_id, FeishuSessionTarget {...}).await` 喂飞书凭证；Phase 1 main 计划会引这条 PR0d 的工作。

- [ ] **Step 4.5: 跑测试 + build**

Run: `cd /Users/oayzz/project/lotus/lotus-workbench/lotus-app/.claude/worktrees/amazing-chatelet-801fd7/src-tauri && cargo build --lib 2>&1 | tail -15`
Expected: `Finished` 0 errors。如果有报错指向某处 `ReplyTarget { robot_code: ... }` 字面量构造，按提示去掉那个字段。

Run: `cd /Users/oayzz/project/lotus/lotus-workbench/lotus-app/.claude/worktrees/amazing-chatelet-801fd7/src-tauri && cargo test --lib connector::im -- --nocapture 2>&1 | tail -20`
Expected: 整模块测试通过；`manager.rs::tests::register_dingtalk_connector_replaces_entry_under_same_platform_key` 这个测试需要更新——`build_dingtalk_connector` 返回值变成 tuple，测试代码取 `.0` 是 trait object。

```rust
        let (c1, _) = super::super::factory::build_dingtalk_connector(...);
        map.write().await.insert(Platform::Dingtalk, c1);
```

`c1` 现在是 `Arc<dyn IMConnector>`。

- [ ] **Step 4.6: ChannelConfigStore 加 platform-keyed API（背向兼容）**

Edit `src-tauri/src/connector/im/shared/config_store.rs`，在 `impl ChannelConfigStore` block 内（保留所有 `dingtalk_*` 方法不动）追加：

```rust
    // ----- Platform-keyed API (PR0d) -----

    /// 通用：返回 `<channels_dir>/<platform>` 的目录。
    pub fn platform_dir(&self, platform: Platform) -> PathBuf {
        self.channels_dir.join(platform.as_str())
    }

    /// 通用：返回 `<channels_dir>/<platform>/config.json`。
    pub fn platform_config_path(&self, platform: Platform) -> PathBuf {
        self.platform_dir(platform).join("config.json")
    }

    /// 通用：返回 `<channels_dir>/<platform>/sessions.json`。
    pub fn platform_sessions_path(&self, platform: Platform) -> PathBuf {
        self.platform_dir(platform).join("sessions.json")
    }
```

> 注意：完整的 `read_config<T>` / `save_registration<T>` 泛型方法在 Phase 1 PR2 飞书 token 落地时一起加（飞书 config 的 schema 还没定，提前抽象没意义）。**PR0d 只加路径相关的 3 个方法**，钉钉照样走老 `dingtalk_*`，飞书 PR2 用新路径方法 + 自己写 read/write 逻辑。这一刀切得比 spec 写的"全部泛型化"小，符合 YAGNI。

- [ ] **Step 4.7: 单测 + 编译验证**

加 1 个 test 到 `config_store.rs::tests`：

```rust
    #[test]
    fn platform_paths_use_lowercase_platform_subdirectory() {
        let dir = TempDir::new().unwrap();
        let store = store_in(&dir);
        let feishu_dir = store.platform_dir(Platform::Feishu);
        let feishu_cfg = store.platform_config_path(Platform::Feishu);
        let feishu_sess = store.platform_sessions_path(Platform::Feishu);
        assert_eq!(feishu_dir, dir.path().join("channels/feishu"));
        assert_eq!(feishu_cfg, dir.path().join("channels/feishu/config.json"));
        assert_eq!(feishu_sess, dir.path().join("channels/feishu/sessions.json"));

        // Dingtalk new API agrees with old dedicated method.
        assert_eq!(store.platform_dir(Platform::Dingtalk), store.dingtalk_dir());
        assert_eq!(
            store.platform_config_path(Platform::Dingtalk),
            store.dingtalk_config_path()
        );
    }
```

Run: `cd /Users/oayzz/project/lotus/lotus-workbench/lotus-app/.claude/worktrees/amazing-chatelet-801fd7/src-tauri && cargo test --lib connector::im::shared::config_store -- --nocapture 2>&1 | tail -10`
Expected: 老 14 个 + 新 1 个共 15 个测试全过。

- [ ] **Step 4.8: 钉钉真账号冒烟 — 影响面最广的一次**

PR0d 改了 manager.rs 多处 worker loop 内的 ReplyTarget 构造和兜底 webhook 路径。冒烟必做：

1. 私聊正常一条 AI Card 流式回复
2. 群聊正常一条 AI Card 流式回复
3. 群聊连发 5 条消息进队列，最后一条触发"消息堆积"提示（QueueFull）能从钉钉端看到
4. 上传一个**有意会下载失败的附件**（比如非常大、或网络掐掉），验证"附件下载全部失败，请重发。"能从钉钉端看到（**走的是新的 connector.send(Text) 路径**——这次冒烟最关键的验证项）

通过全部冒烟后继续。

- [ ] **Step 4.9: 提交 PR0d**

```bash
git add src-tauri/src/connector/im/trait_def.rs src-tauri/src/connector/im/dingtalk/connector.rs src-tauri/src/connector/im/manager.rs src-tauri/src/connector/im/factory.rs src-tauri/src/connector/im/shared/config_store.rs
git commit -m "$(cat <<'EOF'
refactor(connector/im): platform-neutral ReplyTarget + platform-keyed config paths (Phase 0 PR0d)

- ReplyTarget shrinks to {session_id, external_conversation_key}; dingtalk-specific
  fields (robot_code / reply_group_id / session_webhook) move into
  DingtalkConnector's internal session_targets map, populated via remember_session
- factory::build_dingtalk_connector now returns (Arc<dyn IMConnector>,
  Arc<DingtalkConnector>) so manager can call concrete-only methods like
  remember_session
- manager.rs replaces direct dingtalk::stream::send_session_webhook_text calls
  with connector.send(ReplyTarget, ReplyContent::Text) for the attachment-fail
  and queue-full fallback paths
- ChannelConfigStore gains platform_dir / platform_config_path /
  platform_sessions_path helpers; old dingtalk_* methods retained for callers
  Phase 1 feishu PR2 will reuse the new path helpers

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## §5 收尾验证

- [ ] **Step 5.1: 跑全量 review tests**

Run: `cd /Users/oayzz/project/lotus/lotus-workbench/lotus-app/.claude/worktrees/amazing-chatelet-801fd7/src-tauri && cargo test review_ --tests --no-fail-fast 2>&1 | tail -15`
Expected: 所有 `review_*` 测试通过（含 `review_im_layering` / `review_im_ask_coordinator` / `review_pending_im_decoupling`）。

- [ ] **Step 5.2: 跑全量 lib + integration tests**

Run: `cd /Users/oayzz/project/lotus/lotus-workbench/lotus-app/.claude/worktrees/amazing-chatelet-801fd7/src-tauri && cargo test --no-fail-fast 2>&1 | tail -10`
Expected: passed 数 ≥ §0 baseline 的 passed 数 + 13（PR0a: +4 shared, +2 dingtalk; PR0b: +4 shared; PR0c: +3 shared; PR0d: +1 config）。failed 数不增加。

- [ ] **Step 5.3: pnpm 前端检查（仅类型）**

Run: `cd /Users/oayzz/project/lotus/lotus-workbench/lotus-app/.claude/worktrees/amazing-chatelet-801fd7 && pnpm tsc --noEmit 2>&1 | tail -5`
Expected: 0 errors（PR0a-d 全部都是 Rust 后端改动，前端不应有任何变化）。

- [ ] **Step 5.4: 推送 + 开 PR 链**

> 4 个 PR 在同一个分支上累积。推送一次，开 1 个汇总 PR（标题 `IM Phase 0 cleanup — token cache / dedup / aicard send / platform-neutral ReplyTarget`），4 个 commits 体现在 PR 描述。

```bash
git push -u origin claude/amazing-chatelet-801fd7
# 然后在 GitHub UI 上开 PR，或：
# gh pr create --title "..." --body "..."
```

- [ ] **Step 5.5: 真账号长时间冒烟**

让钉钉账号保持在线 8 小时（一个工作日），定期检查日志：

Run: `tail -f ~/.renlijia/logs/*.log | grep -E "ERROR|reply-manager|dingtalk-stream|channel"`
Expected: 没有重复的 ERROR；token 刷新日志出现 1-2 次（≈每 2 小时一次）；reconnect 日志 0-3 次（视网络）。

发现任何 regression：回滚最近一个 commit、修复、重测。

---

## Self-Review Checklist

- [x] **Spec coverage** — spec §0 列的 4 个前置 PR（PR0a/PR0b/PR0c/PR0d）全部对应到 Task 1-4。spec PR0c 范围被 scoped down 到"路由就位但不切流量"，决策写入 Task 3 step 3.11 commit。
- [x] **Placeholder scan** — 没有 "TBD" / "implement later"。
- [x] **Type consistency** — `MessageDedupSet`、`SharedTokenCache`、`PlatformTokenSource`、`DingtalkSessionTarget`、`ReplyTarget`、`ReplyContent::AiCardChunk` / `AiCardFail` 在所有 Task 内引用名一致。
- [x] **dispatch_chunk / dispatch_fail 命名** 在 Task 3 / Task 4 一致。
- [x] **factory tuple 返回** 与 manager.register_dingtalk_connector 改造一致（Task 4.4）。

## Execution Handoff

Plan complete and saved to `docs/superpowers/plans/2026-05-18-im-phase0-cleanup.md`. Two execution options:

1. **Subagent-Driven (recommended)** - I dispatch a fresh subagent per task, review between tasks, fast iteration
2. **Inline Execution** - Execute tasks in this session using executing-plans, batch execution with checkpoints

Which approach?
