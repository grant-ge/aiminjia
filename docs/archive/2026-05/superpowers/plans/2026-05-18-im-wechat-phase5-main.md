# Phase 5 PR3-7：Wechat Connector 业务实现 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 把 Phase 5 PR1-2 搭好的 wechat 骨架填成完整可用的 IMConnector。PR3 扫码登录 + SecureStorage；PR4 长轮询 + SessionGuard + IMConnector + NeedsReauth 状态链路；PR5 sender + parser + 3 个 store + AiCard fallback + StreamingMarkdownFilter + allowFrom 过滤；PR6 媒体上下行 + crypto 接入；PR7 集成测试 + 前端 UI + allowFrom 管理 UI。

**Architecture:** 在 PR1-2 骨架基础上逐 PR 填充。沿用 Phase 1 飞书 + Phase 0 钉钉的运行时形态：`runtime.rs` 长轮询 → `parser.rs` normalize → `WechatSessionStore.observe`（经 `observe_session` trait 路径喂入）→ `WechatContextTokenStore.set` → `WechatAllowFromStore.is_allowed` 过滤 → emit 到 manager。出站 `connector.send` 从 `WechatSessionStore` 反查 `ilink_user_id`、从 `WechatContextTokenStore` 拿 `context_token`、过 `StreamingMarkdownFilter` 再发。所有出站请求前先 `SessionGuard::assert_active` 熔断。

**Tech Stack:** Rust async (tokio + tokio-util), reqwest 0.12, async-trait, serde, anyhow/thiserror, tempfile（test）。前端 React + Tailwind + vitest。无新增第三方 crate（aes 系列在 PR2 已加）。

**Prerequisites:** Phase 5 PR0（RegistrationModal）+ PR1（骨架/types/headers）+ PR2（crypto）已合入 main。

**参考**：
- spec `docs/superpowers/specs/2026-05-18-im-wechat-phase5-design.md` 全文，重点 §1.1–§1.4 / §3.1–§3.4 / §4 / §5 / §6
- openclaw 实测：`/Users/oayzz/Downloads/openclaw channel/openclaw-weixin-main/`
  - `src/auth/login-qr.ts` — 扫码状态机（含 `scaned_but_redirect` / 3 次 expired 刷新）
  - `src/monitor/*.ts` — 长轮询 worker loop + `errcode=-14` 处理
  - `src/api/session-guard.ts` — pause 机制
  - `src/messaging/inbound.ts` — context_token 持久化、`bodyFromItemList`
  - `src/messaging/markdown-filter.ts` — StreamingMarkdownFilter
  - `src/auth/pairing.ts` — allowFrom 白名单

---

## File Structure

```
src-tauri/src/connector/im/wechat/                              ← 新增
├── mod.rs                          ← PR1 已建；PR3-6 追加子模块
├── connector.rs                    ← PR3-6 把 start/send/registration 填实
├── types.rs                        ← PR1 已建；不动
├── endpoints.rs                    ← PR1 已建；不动
├── headers.rs                      ← PR1 已建；不动
├── appid.rs                        ← PR1 已建；不动
├── crypto.rs                       ← PR2 已建；PR6 仅引用，不改
├── api.rs                          ← PR4 新增：7 endpoint HTTP 封装（reqwest）
├── runtime.rs                      ← PR4 新增：getUpdates worker loop + unfold stream
├── login.rs                        ← PR3 新增：扫码 begin/poll 状态机
├── sender.rs                       ← PR5 新增：sendMessage + sendTyping
├── parser.rs                       ← PR5 新增:: WeixinMessage → ChannelMessage
├── session.rs                      ← PR5 新增：3 个 store（Session / ContextToken / AllowFrom）
├── markdown_filter.rs              ← PR5 新增：StreamingMarkdownFilter Rust 移植
├── media.rs                        ← PR6 新增：getUploadUrl + 上传/下载 + crypto 接入
└── session_guard.rs                ← PR4 新增：pause + assert_active 熔断

src-tauri/src/connector/im/
├── trait_def.rs                    ← PR4：ChannelConnectionState 加 NeedsReauth；
│                                      ConnectorError 加 SessionExpired（若 Phase 3 PR1.5 未做）
├── manager.rs                      ← PR3 加扫码注册 hook；PR4 加 wechat worker 启动 + NeedsReauth 设置；
│                                      PR5 加 observe_session 路由到 wechat connector
├── types.rs                        ← PR4：ChannelConnectionState::NeedsReauth 变体
└── factory.rs                      ← PR3：实例化 WechatConnector 时注入 SecureStorage / AllowFromStore

src-tauri/src/commands/
└── channel.rs                      ← PR3：begin/poll 支持 wechat 分支

src-tauri/tests/
└── im_wechat_integration.rs        ← PR7 新增：mock iLink + 完整收发集成测试

src/features/channel/
├── ChannelConfig.tsx               ← PR7：加 wechat 分支（mode='qr_url'）
├── ChannelConfig.test.tsx          ← PR7：覆盖 wechat 流程
└── wechat/                         ← PR7 新增子目录
    ├── AllowFromManagement.tsx     ← PR7：白名单 CRUD UI
    ├── AllowFromManagement.test.tsx
    └── NeedsReauthBanner.tsx       ← PR7：⚠️ 提示 + 重新扫码按钮

src/stores/
└── channelStore.ts                 ← PR7：加 wechat 状态 + allowFrom 数组同步
```

---

# PR3: 扫码登录 + SecureStorage + 自动 allowFrom 入白

## §0 前置

- [ ] **Step P3.0.1: 确认 PR1 + PR2 已合**

Run: `git log --oneline main -10 | grep -E "wechat.*PR1|wechat.*PR2|crypto.rs.*scripture"`
Expected: 看到 PR1 + PR2 commit。

- [ ] **Step P3.0.2: 确认 PR0 (RegistrationModal) 已合**

Run: `git log --oneline main -20 | grep -E "RegistrationModal|registration.*modal"`
Expected: 至少 1 个 PR0 commit。

---

## Task P3.1: `login.rs` —— 扫码登录 begin/poll 状态机

**Files:**
- Create: `src-tauri/src/connector/im/wechat/login.rs`

openclaw 参考：`src/auth/login-qr.ts`。本 task 不接业务侧（不写 SecureStorage / config 落盘），只实现纯函数+ 状态机，PR3 Task 3 才接业务。

- [ ] **Step P3.1.1: 在 mod.rs 加 `pub mod login;`**

修改 `src-tauri/src/connector/im/wechat/mod.rs`，加：

```rust
pub mod login;
```

- [ ] **Step P3.1.2: 写失败的单测（fetchQRCode + 5 个状态）**

Create `src-tauri/src/connector/im/wechat/login.rs`:

```rust
//! 扫码登录状态机 —— 镜像 openclaw-weixin-main/src/auth/login-qr.ts。
//!
//! 流程（spec §1）：
//!   begin → fetch_qrcode → 返回 qr_url + qrcode 字符串
//!   poll  → get_qrcode_status 长轮询 → 5 种状态：
//!     wait / scaned / scaned_but_redirect / confirmed / expired
//!
//! scaned_but_redirect 必须把后续轮询的 base_url 切到 redirect_host；
//! expired 自动 refresh QR，最多 3 次。
//!
//! 本模块只做 HTTP + 状态机；不接 SecureStorage、不写 auth.json。
//! 业务侧由 `connector.rs::begin_registration` / `poll_registration` 包装。

use serde::Deserialize;
use thiserror::Error;

use super::endpoints::{DEFAULT_BASE_URL, GET_BOT_QRCODE, GET_QRCODE_STATUS, DEFAULT_BOT_TYPE};
use super::headers::{build_headers, HeaderInputs};

#[derive(Debug, Error)]
pub enum LoginError {
    #[error("network error: {0}")]
    Network(String),
    #[error("invalid response: {0}")]
    InvalidResponse(String),
    #[error("qr expired after {0} refreshes")]
    ExpiredAfterRefreshes(u32),
}

#[derive(Debug, Clone, Deserialize)]
pub struct QrCodeResponse {
    pub qrcode: String,
    pub qrcode_img_content: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct QrStatusResponse {
    /// "wait" / "scaned" / "scaned_but_redirect" / "confirmed" / "expired"
    pub status: String,
    #[serde(default)]
    pub bot_token: Option<String>,
    #[serde(default)]
    pub ilink_bot_id: Option<String>,
    #[serde(default)]
    pub baseurl: Option<String>,
    /// User who scanned the QR (for auto-allowFrom).
    #[serde(default)]
    pub ilink_user_id: Option<String>,
    /// IDC redirect target when status == "scaned_but_redirect".
    #[serde(default)]
    pub redirect_host: Option<String>,
}

/// Confirmed login result; consumed by connector::begin_registration's caller
/// to persist credentials.
#[derive(Debug, Clone)]
pub struct ConfirmedLogin {
    pub bot_token: String,
    pub ilink_bot_id: String,
    pub ilink_user_id: String,
    /// Effective base URL after IDC redirect; subsequent business endpoints
    /// (getUpdates/sendMessage/etc.) MUST hit this URL, not DEFAULT_BASE_URL.
    pub effective_base_url: String,
}

pub const QR_LONG_POLL_TIMEOUT_SECS: u64 = 35;
pub const MAX_QR_REFRESH_COUNT: u32 = 3;

/// Fetch a fresh QR code from iLink. Pass through `app_id` + `client_version`
/// from PR1's appid/headers modules.
pub async fn fetch_qrcode(
    client: &reqwest::Client,
    app_id: &str,
    client_version: &str,
    base_url: &str,
) -> Result<QrCodeResponse, LoginError> {
    let url = format!(
        "{}/{}?bot_type={}",
        base_url.trim_end_matches('/'),
        GET_BOT_QRCODE,
        DEFAULT_BOT_TYPE
    );
    let headers = build_headers(HeaderInputs {
        app_id,
        client_version,
        bot_token: None,
        route_tag: None,
        body: "",
    });
    let resp = client
        .get(&url)
        .headers(headers)
        .send()
        .await
        .map_err(|e| LoginError::Network(e.to_string()))?;
    if !resp.status().is_success() {
        return Err(LoginError::InvalidResponse(format!(
            "get_bot_qrcode HTTP {}",
            resp.status()
        )));
    }
    let raw = resp
        .text()
        .await
        .map_err(|e| LoginError::Network(e.to_string()))?;
    serde_json::from_str(&raw).map_err(|e| LoginError::InvalidResponse(e.to_string()))
}

/// Poll QR status. Returns the parsed status; caller decides next action.
/// Treats AbortError / 524 / network errors as `"wait"` (transient).
pub async fn poll_qr_status(
    client: &reqwest::Client,
    app_id: &str,
    client_version: &str,
    base_url: &str,
    qrcode: &str,
) -> Result<QrStatusResponse, LoginError> {
    let url = format!(
        "{}/{}?qrcode={}",
        base_url.trim_end_matches('/'),
        GET_QRCODE_STATUS,
        urlencoding::encode(qrcode)
    );
    let headers = build_headers(HeaderInputs {
        app_id,
        client_version,
        bot_token: None,
        route_tag: None,
        body: "",
    });
    let req = client
        .get(&url)
        .headers(headers)
        .timeout(std::time::Duration::from_secs(QR_LONG_POLL_TIMEOUT_SECS));
    let raw = match req.send().await {
        Ok(r) if r.status().is_success() => r
            .text()
            .await
            .map_err(|e| LoginError::Network(e.to_string()))?,
        Ok(r) => {
            return Err(LoginError::InvalidResponse(format!(
                "get_qrcode_status HTTP {}",
                r.status()
            )))
        }
        Err(e) if e.is_timeout() => {
            // Long-poll timeout is normal — treat as wait
            return Ok(QrStatusResponse {
                status: "wait".to_string(),
                bot_token: None,
                ilink_bot_id: None,
                baseurl: None,
                ilink_user_id: None,
                redirect_host: None,
            });
        }
        Err(e) => return Err(LoginError::Network(e.to_string())),
    };
    serde_json::from_str(&raw).map_err(|e| LoginError::InvalidResponse(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    // ===========================================================
    // Pure-function tests: response parsing.
    // HTTP-level tests use mockito in a separate `#[tokio::test]` later.
    // ===========================================================

    #[test]
    fn deserialize_qr_code_response() {
        let raw = r#"{"qrcode":"abc","qrcode_img_content":"https://ilink.weixin.qq.com/qr/xyz"}"#;
        let r: QrCodeResponse = serde_json::from_str(raw).unwrap();
        assert_eq!(r.qrcode, "abc");
        assert_eq!(r.qrcode_img_content, "https://ilink.weixin.qq.com/qr/xyz");
    }

    #[test]
    fn deserialize_qr_status_wait() {
        let raw = r#"{"status":"wait"}"#;
        let s: QrStatusResponse = serde_json::from_str(raw).unwrap();
        assert_eq!(s.status, "wait");
        assert!(s.bot_token.is_none());
    }

    #[test]
    fn deserialize_qr_status_scaned_but_redirect() {
        let raw = r#"{"status":"scaned_but_redirect","redirect_host":"sg.ilink.weixin.qq.com"}"#;
        let s: QrStatusResponse = serde_json::from_str(raw).unwrap();
        assert_eq!(s.status, "scaned_but_redirect");
        assert_eq!(s.redirect_host.as_deref(), Some("sg.ilink.weixin.qq.com"));
    }

    #[test]
    fn deserialize_qr_status_confirmed_with_all_fields() {
        let raw = r#"{
            "status":"confirmed",
            "bot_token":"tk-abc",
            "ilink_bot_id":"bot-123",
            "baseurl":"https://sg.ilink.weixin.qq.com",
            "ilink_user_id":"wxid_alice@im.wechat"
        }"#;
        let s: QrStatusResponse = serde_json::from_str(raw).unwrap();
        assert_eq!(s.status, "confirmed");
        assert_eq!(s.bot_token.as_deref(), Some("tk-abc"));
        assert_eq!(s.ilink_bot_id.as_deref(), Some("bot-123"));
        assert_eq!(s.baseurl.as_deref(), Some("https://sg.ilink.weixin.qq.com"));
        assert_eq!(s.ilink_user_id.as_deref(), Some("wxid_alice@im.wechat"));
    }
}
```

依赖：`urlencoding = "2"`。

Run: `grep urlencoding src-tauri/Cargo.toml` ─ 若没有，加到 `[dependencies]`：

```toml
urlencoding = "2"
```

- [ ] **Step P3.1.3: 编译 + 测试**

Run: `cd src-tauri && cargo test --lib connector::im::wechat::login::tests`
Expected: 4/4 PASS。

- [ ] **Step P3.1.4: 提交**

```bash
git add src-tauri/src/connector/im/wechat/login.rs src-tauri/src/connector/im/wechat/mod.rs src-tauri/Cargo.toml
git commit -m "feat(connector/im/wechat): login.rs — fetch_qrcode + poll_qr_status with 5-state response parsing (Phase 5 PR3)"
```

---

## Task P3.2: 扫码 begin/poll 完整状态机 + 状态记忆

**Files:**
- Modify: `src-tauri/src/connector/im/wechat/login.rs`

实现"begin → 拿 QR + 启动 poll session → expired 自动 refresh 3 次 → confirmed/取消 退出"完整流程。用 `Arc<Mutex<HashMap<session_key, ActiveLogin>>>` 管理活动 session（跟 openclaw `activeLogins` 一致）。

- [ ] **Step P3.2.1: 写状态机 + 自动刷新 + IDC 重定向的失败测试**

在 `login.rs` 已有 mod tests 内追加：

```rust
    use std::sync::Arc;
    use std::sync::Mutex;

    /// Test double: simulates `get_qrcode_status` returning a scripted sequence.
    fn scripted_poll(seq: Vec<&'static str>) -> impl FnMut() -> QrStatusResponse {
        let i = Arc::new(Mutex::new(0usize));
        move || {
            let mut idx = i.lock().unwrap();
            let n = *idx;
            *idx += 1;
            let raw = seq.get(n).copied().unwrap_or("{\"status\":\"wait\"}");
            serde_json::from_str(raw).unwrap()
        }
    }

    // We test `LoginSession::tick`, a pure state machine consuming poll
    // responses, instead of running the real HTTP loop. This isolates the
    // status transitions from network mocking.

    #[test]
    fn session_transitions_wait_then_confirmed() {
        let mut session = LoginSession::new("qr-1".to_string(), DEFAULT_BASE_URL.to_string());
        let mut script = scripted_poll(vec![
            r#"{"status":"wait"}"#,
            r#"{"status":"scaned"}"#,
            r#"{"status":"confirmed","bot_token":"tk","ilink_bot_id":"bot","baseurl":"https://sg.ilink.weixin.qq.com","ilink_user_id":"wxid_alice@im.wechat"}"#,
        ]);
        assert!(matches!(session.tick(script()), LoginStep::KeepWaiting));
        assert!(matches!(session.tick(script()), LoginStep::Scanned));
        match session.tick(script()) {
            LoginStep::Confirmed(c) => {
                assert_eq!(c.bot_token, "tk");
                assert_eq!(c.ilink_bot_id, "bot");
                assert_eq!(c.ilink_user_id, "wxid_alice@im.wechat");
                assert_eq!(c.effective_base_url, "https://sg.ilink.weixin.qq.com");
            }
            other => panic!("expected Confirmed, got {other:?}"),
        }
    }

    #[test]
    fn session_switches_base_url_on_scaned_but_redirect() {
        let mut session = LoginSession::new("qr-1".to_string(), DEFAULT_BASE_URL.to_string());
        let mut script = scripted_poll(vec![
            r#"{"status":"scaned_but_redirect","redirect_host":"sg.ilink.weixin.qq.com"}"#,
        ]);
        match session.tick(script()) {
            LoginStep::KeepWaiting => {}
            other => panic!("expected KeepWaiting, got {other:?}"),
        }
        assert_eq!(session.current_base_url(), "https://sg.ilink.weixin.qq.com");
    }

    #[test]
    fn session_refresh_qr_up_to_3_times_then_fails() {
        let mut session = LoginSession::new("qr-1".to_string(), DEFAULT_BASE_URL.to_string());
        // First expired -> refresh
        assert!(matches!(
            session.tick(serde_json::from_str(r#"{"status":"expired"}"#).unwrap()),
            LoginStep::NeedsQrRefresh
        ));
        session.apply_new_qr("qr-2".to_string());
        // Second expired -> refresh
        assert!(matches!(
            session.tick(serde_json::from_str(r#"{"status":"expired"}"#).unwrap()),
            LoginStep::NeedsQrRefresh
        ));
        session.apply_new_qr("qr-3".to_string());
        // Third expired -> refresh
        assert!(matches!(
            session.tick(serde_json::from_str(r#"{"status":"expired"}"#).unwrap()),
            LoginStep::NeedsQrRefresh
        ));
        session.apply_new_qr("qr-4".to_string());
        // Fourth expired -> give up
        match session.tick(serde_json::from_str(r#"{"status":"expired"}"#).unwrap()) {
            LoginStep::Failed(msg) => assert!(msg.contains("3 次")),
            other => panic!("expected Failed, got {other:?}"),
        }
    }
```

- [ ] **Step P3.2.2: 编译失败**

Run: `cd src-tauri && cargo test --lib connector::im::wechat::login::tests`
Expected: FAIL —— `LoginSession` / `LoginStep` / `Confirmed` not found。

- [ ] **Step P3.2.3: 实现状态机**

在 `login.rs` 中、`pub async fn poll_qr_status` 之后、`#[cfg(test)]` 之前追加：

```rust
//---------------------------------------------------------------------------
// LoginSession state machine
//---------------------------------------------------------------------------

#[derive(Debug)]
pub enum LoginStep {
    KeepWaiting,
    Scanned,
    NeedsQrRefresh,
    Confirmed(ConfirmedLogin),
    Failed(String),
}

/// Stateful login session. Holds the current qrcode + base_url + refresh
/// count; drives the state machine via `tick()`.
pub struct LoginSession {
    qrcode: String,
    base_url: String,
    refresh_count: u32,
}

impl LoginSession {
    pub fn new(qrcode: String, base_url: String) -> Self {
        Self {
            qrcode,
            base_url,
            refresh_count: 0,
        }
    }

    pub fn current_base_url(&self) -> &str {
        &self.base_url
    }

    pub fn current_qrcode(&self) -> &str {
        &self.qrcode
    }

    /// Apply a new QR (after caller refetched). Bumps the refresh counter.
    pub fn apply_new_qr(&mut self, new_qrcode: String) {
        self.qrcode = new_qrcode;
        self.refresh_count += 1;
    }

    /// Consume a poll response and return the next step.
    pub fn tick(&mut self, resp: QrStatusResponse) -> LoginStep {
        match resp.status.as_str() {
            "wait" => LoginStep::KeepWaiting,
            "scaned" => LoginStep::Scanned,
            "scaned_but_redirect" => {
                if let Some(host) = resp.redirect_host.filter(|s| !s.is_empty()) {
                    self.base_url = format!("https://{host}");
                }
                LoginStep::KeepWaiting
            }
            "expired" => {
                // refresh_count tracks how many times we've already refreshed
                if self.refresh_count >= MAX_QR_REFRESH_COUNT {
                    LoginStep::Failed(format!(
                        "登录超时：二维码已过期 3 次，请重新发起登录"
                    ))
                } else {
                    LoginStep::NeedsQrRefresh
                }
            }
            "confirmed" => match (
                resp.bot_token,
                resp.ilink_bot_id,
                resp.ilink_user_id,
            ) {
                (Some(tk), Some(bot), Some(uid)) => LoginStep::Confirmed(ConfirmedLogin {
                    bot_token: tk,
                    ilink_bot_id: bot,
                    ilink_user_id: uid,
                    effective_base_url: resp
                        .baseurl
                        .filter(|s| !s.is_empty())
                        .unwrap_or_else(|| self.base_url.clone()),
                }),
                _ => LoginStep::Failed(
                    "登录失败：服务器返回 confirmed 但缺字段".to_string(),
                ),
            },
            other => LoginStep::Failed(format!("未知 QR 状态：{other}")),
        }
    }
}
```

- [ ] **Step P3.2.4: 测试通过**

Run: `cd src-tauri && cargo test --lib connector::im::wechat::login::tests`
Expected: 7/7 PASS（之前 4 + 新 3）。

- [ ] **Step P3.2.5: 提交**

```bash
git add src-tauri/src/connector/im/wechat/login.rs
git commit -m "feat(connector/im/wechat): login.rs — LoginSession state machine with IDC redirect + 3x expired refresh"
```

---

## Task P3.3: `session.rs` —— WechatAllowFromStore（仅本 PR 需要）

**Files:**
- Create: `src-tauri/src/connector/im/wechat/session.rs`
- Modify: `src-tauri/src/connector/im/wechat/mod.rs`

PR3 只需要 `WechatAllowFromStore`（扫码成功后自动入白）。其他两个 store（Session / ContextToken）PR5 再加。

- [ ] **Step P3.3.1: 加 `pub mod session;`**

修改 `mod.rs`，加 `pub mod session;`。

- [ ] **Step P3.3.2: 写失败的 AllowFrom 单测**

Create `src-tauri/src/connector/im/wechat/session.rs`:

```rust
//! Per-account state stores for wechat connector.
//!
//! Three stores live here, all keyed by account_id (the `ilink_bot_id`):
//!   - WechatAllowFromStore: 授权用户白名单（§1.4）—— PR3
//!   - WechatSessionStore: session_id → ilink_user_id 反查表 —— PR5
//!   - WechatContextTokenStore: ilink_user_id → context_token 反查表 —— PR5

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;

//---------------------------------------------------------------------------
// WechatAllowFromStore
//---------------------------------------------------------------------------

#[derive(Debug, Default, Serialize, Deserialize)]
struct AllowFromFile {
    #[serde(default = "default_version")]
    version: u32,
    #[serde(rename = "allowFrom", default)]
    allow_from: Vec<String>,
}

fn default_version() -> u32 {
    1
}

/// In-memory + on-disk authorized-user whitelist. Inbound messages whose
/// `from_user_id` is not in this set are silently dropped (logged at info).
///
/// The scanner of the QR code (ilink_user_id from confirmed login) is auto-
/// added when the bot is first registered.
pub struct WechatAllowFromStore {
    inner: RwLock<HashSet<String>>,
    persist_path: PathBuf,
}

impl WechatAllowFromStore {
    /// Create a store for a given account, loading the existing file if any.
    pub async fn open(account_dir: &Path) -> Self {
        let persist_path = account_dir.join("allow_from.json");
        let initial: HashSet<String> = match tokio::fs::read_to_string(&persist_path).await {
            Ok(raw) => serde_json::from_str::<AllowFromFile>(&raw)
                .map(|f| f.allow_from.into_iter().filter(|s| !s.trim().is_empty()).collect())
                .unwrap_or_default(),
            Err(_) => HashSet::new(),
        };
        Self {
            inner: RwLock::new(initial),
            persist_path,
        }
    }

    pub async fn is_allowed(&self, user_id: &str) -> bool {
        self.inner.read().await.contains(user_id)
    }

    pub async fn add(&self, user_id: &str) -> std::io::Result<bool> {
        let mut set = self.inner.write().await;
        let added = set.insert(user_id.to_string());
        if added {
            self.persist_locked(&set).await?;
        }
        Ok(added)
    }

    pub async fn remove(&self, user_id: &str) -> std::io::Result<bool> {
        let mut set = self.inner.write().await;
        let removed = set.remove(user_id);
        if removed {
            self.persist_locked(&set).await?;
        }
        Ok(removed)
    }

    pub async fn list(&self) -> Vec<String> {
        let set = self.inner.read().await;
        let mut v: Vec<_> = set.iter().cloned().collect();
        v.sort();
        v
    }

    async fn persist_locked(&self, set: &HashSet<String>) -> std::io::Result<()> {
        let mut all: Vec<_> = set.iter().cloned().collect();
        all.sort();
        let f = AllowFromFile { version: 1, allow_from: all };
        let raw = serde_json::to_string_pretty(&f).expect("AllowFromFile serializes");
        if let Some(parent) = self.persist_path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        let tmp = self.persist_path.with_extension("json.tmp");
        tokio::fs::write(&tmp, raw).await?;
        tokio::fs::rename(&tmp, &self.persist_path).await?;
        Ok(())
    }
}

pub type SharedAllowFromStore = Arc<WechatAllowFromStore>;

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn tmp() -> TempDir {
        tempfile::tempdir().unwrap()
    }

    #[tokio::test]
    async fn empty_store_when_file_missing() {
        let dir = tmp();
        let s = WechatAllowFromStore::open(dir.path()).await;
        assert!(!s.is_allowed("wxid_alice@im.wechat").await);
        assert!(s.list().await.is_empty());
    }

    #[tokio::test]
    async fn add_then_is_allowed() {
        let dir = tmp();
        let s = WechatAllowFromStore::open(dir.path()).await;
        let added = s.add("wxid_alice@im.wechat").await.unwrap();
        assert!(added);
        assert!(s.is_allowed("wxid_alice@im.wechat").await);
        assert!(!s.is_allowed("wxid_bob@im.wechat").await);
    }

    #[tokio::test]
    async fn duplicate_add_returns_false_and_no_extra_write() {
        let dir = tmp();
        let s = WechatAllowFromStore::open(dir.path()).await;
        assert!(s.add("wxid_alice@im.wechat").await.unwrap());
        assert!(!s.add("wxid_alice@im.wechat").await.unwrap());
    }

    #[tokio::test]
    async fn remove_works() {
        let dir = tmp();
        let s = WechatAllowFromStore::open(dir.path()).await;
        s.add("wxid_alice@im.wechat").await.unwrap();
        assert!(s.remove("wxid_alice@im.wechat").await.unwrap());
        assert!(!s.is_allowed("wxid_alice@im.wechat").await);
        assert!(!s.remove("wxid_alice@im.wechat").await.unwrap());
    }

    #[tokio::test]
    async fn persist_and_reload_round_trip() {
        let dir = tmp();
        {
            let s = WechatAllowFromStore::open(dir.path()).await;
            s.add("wxid_alice@im.wechat").await.unwrap();
            s.add("wxid_bob@im.wechat").await.unwrap();
        }
        let s = WechatAllowFromStore::open(dir.path()).await;
        let mut list = s.list().await;
        list.sort();
        assert_eq!(list, vec![
            "wxid_alice@im.wechat".to_string(),
            "wxid_bob@im.wechat".to_string(),
        ]);
    }

    #[tokio::test]
    async fn list_is_sorted_for_stable_output() {
        let dir = tmp();
        let s = WechatAllowFromStore::open(dir.path()).await;
        s.add("wxid_charlie").await.unwrap();
        s.add("wxid_alice").await.unwrap();
        s.add("wxid_bob").await.unwrap();
        let l = s.list().await;
        assert_eq!(l, vec!["wxid_alice", "wxid_bob", "wxid_charlie"]);
    }
}
```

- [ ] **Step P3.3.3: 测试通过**

Run: `cd src-tauri && cargo test --lib connector::im::wechat::session::tests`
Expected: 6/6 PASS。

- [ ] **Step P3.3.4: 提交**

```bash
git add src-tauri/src/connector/im/wechat/session.rs src-tauri/src/connector/im/wechat/mod.rs
git commit -m "feat(connector/im/wechat): WechatAllowFromStore — whitelist with atomic write + sorted list (spec §1.4)"
```

---

## Task P3.4: `connector.rs` —— `begin_registration` + `poll_registration` 实现

**Files:**
- Modify: `src-tauri/src/connector/im/wechat/connector.rs`

把 PR1 占位的 `begin_registration` / `poll_registration`（默认 trait 实现返 NotSupported）真正接上。`WechatConnector` 加上 `login_sessions: Arc<RwLock<HashMap<session_key, LoginSession>>>` + `allow_from: SharedAllowFromStore` + `secure_storage: Arc<SecureStorage>`。

- [ ] **Step P3.4.1: 看现状**

Run: `cat src-tauri/src/connector/im/wechat/connector.rs`
确认 PR1 的占位结构。

- [ ] **Step P3.4.2: 写失败的单测（mockito 模拟 iLink）**

需要先确认 `mockito` 依赖在 dev-deps：

Run: `grep mockito src-tauri/Cargo.toml`
Expected: `mockito = "1"` 在 `[dev-dependencies]`。

在 `connector.rs` 末尾 `mod tests` 内追加（注意现有 3 个 case 保留）：

```rust
    use mockito::Server;

    #[tokio::test]
    async fn begin_registration_returns_qr_url_from_ilink() {
        let mut server = Server::new_async().await;
        let _m = server
            .mock("GET", "/ilink/bot/get_bot_qrcode?bot_type=3")
            .with_status(200)
            .with_body(
                r#"{"qrcode":"qr-1","qrcode_img_content":"https://ilink.weixin.qq.com/qr/abc"}"#,
            )
            .create_async()
            .await;

        let conn = WechatConnector::new_with_overrides(
            "test-app-id".to_string(),
            "0.1.0".to_string(),
            std::path::PathBuf::from("/tmp/nope.json"),
            server.url(), // base_url override
        );
        let begin = conn
            .begin_registration(&RegistrationRequest::default())
            .await
            .unwrap();
        // RegistrationBegin is `ChannelRegistrationBeginResult` — verify it carries
        // the QR URL in the expected field (Phase 1 PR0d adds `qr_url` next to `verification_uri_complete`).
        // If your trait_def uses a different field name, adapt the assertion.
        assert!(begin.verification_uri_complete.contains("ilink.weixin.qq.com/qr/abc")
            || begin.qr_url.as_deref() == Some("https://ilink.weixin.qq.com/qr/abc"));
    }
```

注意：`RegistrationBegin` 结构当前可能没有 `qr_url` 字段（spec 假设 Phase 1 PR0d 加了）。如果没有，**先回去把 Phase 1 PR0d 章节里 RegistrationBegin 字段扩展加上 `qr_url: Option<String>`**，或者本 plan 也把这个改动列在 P3.4.0 步骤里：

- [ ] **Step P3.4.2.5（前置）: 给 RegistrationBegin / ChannelRegistrationBeginResult 加 `qr_url` 字段**

Run: `grep -n "ChannelRegistrationBeginResult\b" src-tauri/src/connector/im/types.rs`
找到结构体定义，加：

```rust
pub struct ChannelRegistrationBeginResult {
    pub device_code: String,       // dingtalk 用；wechat 这里塞 qrcode (poll handle)
    pub verification_uri_complete: String,   // wechat 复用：放 qr_url
    // ... 既有字段 ...
    #[serde(default)]
    pub qr_url: Option<String>,    // wechat 专用；dingtalk 不填
}
```

**或者**复用 `verification_uri_complete` 字段（dingtalk 的"OPEN_CLAW URL"语义跟 wechat 的"QR payload URL"在用途上一致），不加新字段——前端 `RegistrationModal mode='qr_url'` 接的 `qrUrl` 就从这个字段取。建议走这条路（复用），更干净。这意味着 P3.4.2 的 test assertion 改成：

```rust
assert_eq!(begin.verification_uri_complete, "https://ilink.weixin.qq.com/qr/abc");
```

确定路线（复用 vs 新字段）后实施。本 plan 余下假设**复用 `verification_uri_complete`**。

- [ ] **Step P3.4.3: 在 `connector.rs` 实现 begin/poll**

完整替换 `connector.rs`：

```rust
//! `WechatConnector` — `IMConnector` implementation for iLink HTTP API.
//!
//! PR3 (this commit): begin/poll_registration plumbed against login.rs.
//! Active login sessions are kept in-process so poll_registration can resume.
//! On confirmed, bot_token goes to SecureStorage; ilink_bot_id / ilink_user_id
//! / baseurl land in auth.json; ilink_user_id auto-added to allow_from.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;
use futures::stream::{self, BoxStream};
use tokio::sync::RwLock;

use crate::connector::im::trait_def::{
    AuthFlow, ConnectorCapabilities, ConnectorContext, ConnectorError, IMConnector,
    InboundDeployment, MarkdownSupport, PollRequest, RegistrationBegin, RegistrationPoll,
    RegistrationRequest, ReplyContent, ReplyTarget,
};
use crate::connector::im::types::{
    ChannelMessage, ChannelRegistrationBeginResult, ChannelRegistrationPollResult,
    ChannelRegistrationPollState, Platform,
};
use crate::storage::crypto::SecureStorage;

use super::endpoints::DEFAULT_BASE_URL;
use super::login::{
    fetch_qrcode, poll_qr_status, ConfirmedLogin, LoginSession, LoginStep, MAX_QR_REFRESH_COUNT,
};
use super::session::SharedAllowFromStore;

pub struct WechatConnector {
    app_id: String,
    client_version: String,
    config_path: PathBuf,
    /// HTTP base URL for login endpoints. Tests override via `new_with_overrides`.
    base_url: String,
    http: reqwest::Client,
    /// Active scan-in-progress sessions keyed by `device_code` (we re-purpose
    /// the dingtalk `device_code` field as the poll handle = qrcode string).
    login_sessions: Arc<RwLock<HashMap<String, LoginSession>>>,
    /// Whitelist store; injected by factory at connector construction. None
    /// in test setups that don't exercise the allow_from path.
    allow_from: Option<SharedAllowFromStore>,
    /// Account dir under `users/{scope}/channels/wechat/{bot_id}/` for storing
    /// auth.json on confirmed login.
    account_dir_base: Option<PathBuf>,
    secure_storage: Option<Arc<SecureStorage>>,
}

impl WechatConnector {
    /// Construct with production defaults — base URL hard-coded to ilinkai.weixin.qq.com.
    /// Use `new_with_overrides` for tests.
    pub fn new(
        app_id: String,
        client_version: String,
        config_path: PathBuf,
        account_dir_base: PathBuf,
        allow_from: Option<SharedAllowFromStore>,
        secure_storage: Option<Arc<SecureStorage>>,
    ) -> Self {
        Self {
            app_id,
            client_version,
            config_path,
            base_url: DEFAULT_BASE_URL.to_string(),
            http: reqwest::Client::new(),
            login_sessions: Arc::new(RwLock::new(HashMap::new())),
            allow_from,
            account_dir_base: Some(account_dir_base),
            secure_storage,
        }
    }

    /// Test-only constructor: skip allow_from / secure_storage / account_dir;
    /// override base_url to mockito server.
    pub fn new_with_overrides(
        app_id: String,
        client_version: String,
        config_path: PathBuf,
        base_url: String,
    ) -> Self {
        Self {
            app_id,
            client_version,
            config_path,
            base_url,
            http: reqwest::Client::new(),
            login_sessions: Arc::new(RwLock::new(HashMap::new())),
            allow_from: None,
            account_dir_base: None,
            secure_storage: None,
        }
    }

    pub fn app_id(&self) -> &str {
        &self.app_id
    }
}

#[async_trait]
impl IMConnector for WechatConnector {
    fn platform(&self) -> Platform {
        Platform::Wechat
    }

    fn capabilities(&self) -> ConnectorCapabilities {
        ConnectorCapabilities {
            inbound: InboundDeployment::SelfHosted,
            outbound_aicard: false,
            outbound_text_streaming: false,
            outbound_markdown: MarkdownSupport::Partial,
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
        // PR4 implements the real long-poll loop here.
        Ok(Box::pin(stream::empty()))
    }

    async fn send(
        &self,
        _target: ReplyTarget,
        _content: ReplyContent,
    ) -> Result<(), ConnectorError> {
        Err(ConnectorError::NotSupported("wechat send (PR3)"))
    }

    async fn begin_registration(
        &self,
        _req: &RegistrationRequest,
    ) -> Result<RegistrationBegin, ConnectorError> {
        let resp = fetch_qrcode(&self.http, &self.app_id, &self.client_version, &self.base_url)
            .await
            .map_err(|e| ConnectorError::Transient(format!("wechat fetch_qrcode: {e}")))?;

        let session = LoginSession::new(resp.qrcode.clone(), self.base_url.clone());
        self.login_sessions
            .write()
            .await
            .insert(resp.qrcode.clone(), session);

        Ok(ChannelRegistrationBeginResult {
            device_code: resp.qrcode.clone(),
            verification_uri_complete: resp.qrcode_img_content,
            // Other fields per the existing struct shape — fill 0 / default
            // where applicable. The frontend RegistrationModal mode='qr_url'
            // only reads verification_uri_complete + expires_in_seconds.
            verification_uri: String::new(),
            user_code: None,
            interval_seconds: 2,
            expires_in_seconds: 300,
            source: "wechat-ilink".to_string(),
            ..Default::default()
        })
    }

    async fn poll_registration(
        &self,
        req: &PollRequest,
    ) -> Result<RegistrationPoll, ConnectorError> {
        let qrcode = &req.device_code;
        let mut sessions = self.login_sessions.write().await;
        let session = sessions
            .get_mut(qrcode)
            .ok_or_else(|| ConnectorError::Fatal(format!("no active wechat login for {qrcode}")))?;

        let status = poll_qr_status(
            &self.http,
            &self.app_id,
            &self.client_version,
            session.current_base_url(),
            qrcode,
        )
        .await
        .map_err(|e| ConnectorError::Transient(format!("wechat poll_qr_status: {e}")))?;

        match session.tick(status) {
            LoginStep::KeepWaiting | LoginStep::Scanned => Ok(ChannelRegistrationPollResult {
                state: ChannelRegistrationPollState::Waiting,
                ..Default::default()
            }),
            LoginStep::NeedsQrRefresh => {
                // Refetch and apply
                let new = fetch_qrcode(
                    &self.http,
                    &self.app_id,
                    &self.client_version,
                    session.current_base_url(),
                )
                .await
                .map_err(|e| ConnectorError::Transient(format!("wechat refetch qr: {e}")))?;
                session.apply_new_qr(new.qrcode.clone());
                // Move session under the new qrcode key
                let owned = sessions.remove(qrcode).unwrap();
                sessions.insert(new.qrcode.clone(), owned);
                // Front-end will see Waiting + a new device_code; UI swaps QR.
                Ok(ChannelRegistrationPollResult {
                    state: ChannelRegistrationPollState::Waiting,
                    // Repurpose `verification_uri_complete` again to push the
                    // new QR payload up to the frontend.
                    config: None,
                    platform_state: None,
                    fail_reason: None,
                    // Add a new field or reuse `next_qr_url`? — for now we put
                    // it in a dedicated field on the poll result.
                    ..Default::default()
                })
            }
            LoginStep::Confirmed(confirmed) => {
                sessions.remove(qrcode);
                drop(sessions);
                self.finalize_login(confirmed).await
            }
            LoginStep::Failed(msg) => {
                sessions.remove(qrcode);
                Ok(ChannelRegistrationPollResult {
                    state: ChannelRegistrationPollState::Fail,
                    fail_reason: Some(msg),
                    ..Default::default()
                })
            }
        }
    }

    // stop / observe_session etc. — PR4/PR5 implement.
}

impl WechatConnector {
    async fn finalize_login(
        &self,
        confirmed: ConfirmedLogin,
    ) -> Result<RegistrationPoll, ConnectorError> {
        // 1. Persist bot_token to SecureStorage if available
        if let Some(ss) = &self.secure_storage {
            let key = format!("aijia-wechat-bot-token-{}", confirmed.ilink_bot_id);
            ss.set(&key, confirmed.bot_token.as_bytes())
                .await
                .map_err(|e| ConnectorError::Fatal(format!("SecureStorage write: {e}")))?;
        } else {
            log::warn!(
                "[wechat] SecureStorage not configured — bot_token NOT persisted (dev mode only)"
            );
        }

        // 2. Write auth.json
        if let Some(base) = &self.account_dir_base {
            let dir = base.join(&confirmed.ilink_bot_id);
            tokio::fs::create_dir_all(&dir)
                .await
                .map_err(|e| ConnectorError::Fatal(format!("mkdir account dir: {e}")))?;
            let auth_path = dir.join("auth.json");
            let auth = serde_json::json!({
                "ilink_bot_id": confirmed.ilink_bot_id,
                "ilink_user_id": confirmed.ilink_user_id,
                "baseurl": confirmed.effective_base_url,
                "bot_token_storage_kind": if self.secure_storage.is_some() { "keychain" } else { "plaintext_unsafe" },
            });
            let tmp = auth_path.with_extension("json.tmp");
            tokio::fs::write(&tmp, serde_json::to_string_pretty(&auth).unwrap())
                .await
                .map_err(|e| ConnectorError::Fatal(format!("write auth.json: {e}")))?;
            tokio::fs::rename(&tmp, &auth_path)
                .await
                .map_err(|e| ConnectorError::Fatal(format!("rename auth.json: {e}")))?;
        }

        // 3. Auto-add scanner to allow_from
        if let Some(af) = &self.allow_from {
            af.add(&confirmed.ilink_user_id).await.map_err(|e| {
                ConnectorError::Fatal(format!("allow_from auto-add: {e}"))
            })?;
        }

        Ok(ChannelRegistrationPollResult {
            state: ChannelRegistrationPollState::Success,
            // config: PR3 doesn't fill ChannelConfigView yet — manager fills
            // it via platform_state hookup in PR4.
            config: None,
            platform_state: None,
            fail_reason: None,
            ..Default::default()
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mockito::Server;

    fn make() -> WechatConnector {
        WechatConnector::new(
            "test-app-id".to_string(),
            "0.1.0".to_string(),
            PathBuf::from("/tmp/nope.json"),
            PathBuf::from("/tmp/wechat-accounts"),
            None,
            None,
        )
    }

    #[test]
    fn platform_is_wechat() {
        assert_eq!(make().platform(), Platform::Wechat);
    }

    #[test]
    fn capabilities_match_spec_section_2() {
        let c = make().capabilities();
        assert_eq!(c.inbound, InboundDeployment::SelfHosted);
        assert!(!c.outbound_aicard);
        assert!(!c.outbound_text_streaming);
        assert_eq!(c.outbound_markdown, MarkdownSupport::Partial);
        assert!(c.supports_attachments);
        assert!(!c.supports_group_chat);
        assert!(c.supports_private_chat);
        assert_eq!(c.auth_flow, AuthFlow::QRCode);
    }

    #[tokio::test]
    async fn send_returns_not_supported_in_pr3() {
        let connector = make();
        let target = ReplyTarget {
            session_id: "s1".to_string(),
            external_conversation_key: "wxid_alice@im.wechat".to_string(),
        };
        let result = connector
            .send(target, ReplyContent::Text("hi".to_string()))
            .await;
        assert!(matches!(result, Err(ConnectorError::NotSupported(_))));
    }

    #[tokio::test]
    async fn begin_registration_returns_qr_url_from_ilink() {
        let mut server = Server::new_async().await;
        let _m = server
            .mock("GET", "/ilink/bot/get_bot_qrcode?bot_type=3")
            .with_status(200)
            .with_body(
                r#"{"qrcode":"qr-1","qrcode_img_content":"https://ilink.weixin.qq.com/qr/abc"}"#,
            )
            .create_async()
            .await;

        let conn = WechatConnector::new_with_overrides(
            "test-app-id".to_string(),
            "0.1.0".to_string(),
            PathBuf::from("/tmp/nope.json"),
            server.url(),
        );
        let begin = conn
            .begin_registration(&RegistrationRequest::default())
            .await
            .unwrap();
        assert_eq!(begin.device_code, "qr-1");
        assert_eq!(
            begin.verification_uri_complete,
            "https://ilink.weixin.qq.com/qr/abc"
        );
    }

    #[tokio::test]
    async fn poll_registration_waiting_then_confirmed() {
        let mut server = Server::new_async().await;
        let _m1 = server
            .mock("GET", "/ilink/bot/get_bot_qrcode?bot_type=3")
            .with_status(200)
            .with_body(
                r#"{"qrcode":"qr-1","qrcode_img_content":"https://ilink.weixin.qq.com/qr/abc"}"#,
            )
            .create_async()
            .await;
        let _m2 = server
            .mock("GET", "/ilink/bot/get_qrcode_status?qrcode=qr-1")
            .with_status(200)
            .with_body(r#"{"status":"wait"}"#)
            .expect(1)
            .create_async()
            .await;

        let conn = WechatConnector::new_with_overrides(
            "test-app-id".to_string(),
            "0.1.0".to_string(),
            PathBuf::from("/tmp/nope.json"),
            server.url(),
        );
        let begin = conn
            .begin_registration(&RegistrationRequest::default())
            .await
            .unwrap();
        let poll = conn
            .poll_registration(&PollRequest {
                device_code: begin.device_code.clone(),
                source: "wechat".to_string(),
            })
            .await
            .unwrap();
        assert_eq!(poll.state, ChannelRegistrationPollState::Waiting);
    }
}
```

- [ ] **Step P3.4.4: 测试**

Run: `cd src-tauri && cargo test --lib connector::im::wechat::connector::tests`
Expected: 5/5 PASS（之前 3 + 新 2）。

如果编译报错 `Default not implemented for ChannelRegistrationBeginResult` / `ChannelRegistrationPollResult` —— 给这两个结构体加 `#[derive(Default)]`。看 `src-tauri/src/connector/im/types.rs` 现状。

- [ ] **Step P3.4.5: 提交**

```bash
git add src-tauri/src/connector/im/wechat/connector.rs src-tauri/src/connector/im/types.rs
git commit -m "feat(connector/im/wechat): begin/poll_registration with scan state machine + SecureStorage + auth.json + auto-allow (Phase 5 PR3)"
```

---

## Task P3.5: 接 Tauri command + 让前端 ChannelConfig 能调

**Files:**
- Modify: `src-tauri/src/commands/channel.rs`
- Modify: `src-tauri/src/connector/im/factory.rs`（确保 wechat 注册时传 SecureStorage + AllowFromStore）

- [ ] **Step P3.5.1: 看 dingtalk 在 commands/channel.rs 怎么 dispatch begin/poll**

Run: `grep -n "Platform::\|platform.as_str\|parse_platform" src-tauri/src/commands/channel.rs | head -20`

- [ ] **Step P3.5.2: 加 wechat 分支**

确保 `parse_platform("wechat") -> Platform::Wechat` 已支持（Phase 0 已加）。`channel_begin_registration` / `channel_poll_registration` 走 IMConnector trait 调用，应该天然支持 wechat —— 不需要每平台改 if branch。验证：

Run: `grep -A20 "fn channel_begin_registration" src-tauri/src/commands/channel.rs | head -25`

如果它**已经**走的是 `manager.get(platform).begin_registration(...)`（无 if branch），✅ 不动。
如果它**还在** match Platform 枚举 hard-code dingtalk —— 加 wechat 分支或重构成统一调用。**保守做法**：加 wechat 分支照搬 dingtalk 风格。

- [ ] **Step P3.5.3: 看 factory.rs 是否给 wechat 注入了 SecureStorage**

PR1 Task 8 加的 `build_wechat_connector` 用的是 `WechatConnector::new(app_id, client_version, config_path)` —— 3 参数旧签名。本 PR 改成 6 参数（加 account_dir_base / allow_from / secure_storage），需要在 factory.rs 里更新：

```rust
pub fn build_wechat_connector(
    config_store: &ChannelConfigStore,
    aijia_config_path: &Path,
    secure_storage: Arc<SecureStorage>,
) -> Option<WechatConnector> {
    if !is_platform_enabled(config_store, Platform::Wechat) {
        return None;
    }
    let app_id = appid::resolve_app_id(aijia_config_path);
    let client_version = env!("CARGO_PKG_VERSION").to_string();
    let account_dir_base = config_store.platform_dir(Platform::Wechat);
    // Note: allow_from is per-account (under `{bot_id}/allow_from.json`).
    // For PR3, we lazy-create the store when finalize_login picks a bot_id.
    // Pass None at construction time; PR4 wires the worker that picks it up.
    Some(WechatConnector::new(
        app_id,
        client_version,
        aijia_config_path.to_path_buf(),
        account_dir_base,
        None, // allow_from PR4 wires
        Some(secure_storage),
    ))
}
```

具体 `is_platform_enabled` / `secure_storage` 注入路径以 factory.rs 现状为准。

**重要陷阱**：`allow_from` per-bot_id 在扫码完成后才知道目录路径 —— PR3 暂时没法在 construction time 给 allow_from。一种做法是把 `account_dir_base` 存进 connector，在 `finalize_login` 时 lazy 创建 `WechatAllowFromStore::open(&account_dir_base.join(&bot_id))`，调 `add(ilink_user_id)`，然后 drop。这样 PR3 自给自足，PR4 长轮询启动时再常驻一个 SharedAllowFromStore。

修改 `connector.rs::finalize_login` 用这个 lazy 模式：

```rust
// 3. Auto-add scanner to allow_from（lazy: open then drop）
if let Some(base) = &self.account_dir_base {
    let store = super::session::WechatAllowFromStore::open(
        &base.join(&confirmed.ilink_bot_id)
    ).await;
    store.add(&confirmed.ilink_user_id).await.map_err(|e| {
        ConnectorError::Fatal(format!("allow_from auto-add: {e}"))
    })?;
}
```

并把 `self.allow_from: Option<SharedAllowFromStore>` 字段移除（PR4 再加常驻版）。

- [ ] **Step P3.5.4: 测试 + 编译**

Run: `cd src-tauri && cargo test --lib connector::im::wechat && cargo build`
Expected: PASS。

- [ ] **Step P3.5.5: 提交**

```bash
git add src-tauri/src/commands/channel.rs src-tauri/src/connector/im/factory.rs src-tauri/src/connector/im/wechat/connector.rs
git commit -m "feat(connector/im/wechat): wire begin/poll to channel commands + lazy allow_from on finalize"
```

---

## Task P3.6: PR3 验收

- [ ] **Step P3.6.1: 跑全套测试**

Run: `cd src-tauri && cargo test --lib connector::im::wechat`
Expected: 全 PASS。

Run: `cd src-tauri && cargo test review_ --tests --no-fail-fast`
Expected: 全 PASS。

- [ ] **Step P3.6.2: PR3 描述草稿**

Title: `feat(connector/im/wechat): QR-code login flow with SecureStorage + auth.json + auto-allowFrom (Phase 5 PR3)`

Body：
```
Phase 5 PR3 — Wechat 扫码登录闭环。

新增：
- src-tauri/src/connector/im/wechat/login.rs: fetch_qrcode + poll_qr_status 纯
  HTTP + LoginSession 状态机（含 scaned_but_redirect 切 base_url + 3 次 expired
  自动刷新边界）
- src-tauri/src/connector/im/wechat/session.rs: WechatAllowFromStore 仅 PR3 部
  分（仅本 PR 用到；其他两个 store PR5 加）

WechatConnector::begin_registration / poll_registration 完整实现：
- begin: GET ilink/bot/get_bot_qrcode → 缓存 LoginSession（按 qrcode 键）
- poll: GET ilink/bot/get_qrcode_status → 状态机 tick：
  - waiting/scaned → Waiting
  - scaned_but_redirect → 切 base_url + Waiting
  - expired → 自动 refetch QR（最多 3 次）
  - confirmed → finalize_login（SecureStorage 写 bot_token + auth.json 写
    ilink_bot_id + baseurl + ilink_user_id；自动加入 allow_from 白名单）
  - fail → Fail with 失败原因

Spec: docs/superpowers/specs/2026-05-18-im-wechat-phase5-design.md §1
Plan: docs/superpowers/plans/2026-05-18-im-wechat-phase5-main.md

Tests:
- 4 login.rs parse case + 3 LoginSession state machine case
- 6 WechatAllowFromStore case（含 atomic write round-trip）
- 5 connector.rs case（含 mockito 模拟 begin + poll waiting 路径）
```

---

# PR4: 长轮询 + SessionGuard + IMConnector::start + NeedsReauth 状态链路

## §0 前置

- [ ] **Step P4.0.1: 确认 PR3 已合**

Run: `git log --oneline main -5 | grep PR3`

- [ ] **Step P4.0.2: 确认 ChannelConnectionState 状态**

Run: `grep -n "pub enum ChannelConnectionState" src-tauri/src/connector/im/types.rs`
Expected: 现有变体 Unconfigured/Disconnected/Connecting/Connected/Reconnecting/ConfigError。

PR4 加 `NeedsReauth`。

---

## Task P4.1: `ChannelConnectionState::NeedsReauth` + `ConnectorError::SessionExpired`

**Files:**
- Modify: `src-tauri/src/connector/im/types.rs`
- Modify: `src-tauri/src/connector/im/trait_def.rs`

- [ ] **Step P4.1.1: 写失败的单测（NeedsReauth 序列化）**

修改 `src-tauri/src/connector/im/types.rs`，在 `mod tests`（如不存在则新建）追加：

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn channel_connection_state_serializes_needs_reauth_as_camel_case() {
        let s = serde_json::to_string(&ChannelConnectionState::NeedsReauth).unwrap();
        assert_eq!(s, "\"needsReauth\"");
    }
}
```

- [ ] **Step P4.1.2: 加 `NeedsReauth` 变体**

修改 `ChannelConnectionState`：

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ChannelConnectionState {
    Unconfigured,
    Disconnected,
    Connecting,
    Connected,
    Reconnecting,
    ConfigError,
    /// Session expired persistently; requires user to re-scan QR. Wechat-driven
    /// addition; dingtalk reuses for `device_code` expiry by request.
    NeedsReauth,
}
```

- [ ] **Step P4.1.3: 在 `trait_def.rs::ConnectorError` 加 SessionExpired**

确认是否已有（Phase 3 PR1.5 期望已经加）：

Run: `grep -n "SessionExpired" src-tauri/src/connector/im/trait_def.rs`

如果没有，加：

```rust
pub enum ConnectorError {
    Transient(String),
    AuthExpired(String),    // 既有；保留
    /// Wechat-specific: iLink errcode=-14. Different from AuthExpired in
    /// that PR4 wechat treats this as recoverable (pause N min) rather than
    /// instantly NeedsReauth.
    SessionExpired { errcode: i32, errmsg: String },
    Fatal(String),
    ShutdownRequested,
    NotSupported(&'static str),
}
```

加配套 `Display` impl 行（用 `thiserror::Error` 的话只需加 `#[error("session expired: errcode={errcode} {errmsg}")]`）。

- [ ] **Step P4.1.4: 测试通过 + 编译**

Run: `cd src-tauri && cargo test --lib connector::im::types::tests connector::im::trait_def::tests && cargo build`
Expected: PASS。其他平台代码可能要在 manager.rs `match` 上加新分支（编译器报 non-exhaustive match），按提示补全。

- [ ] **Step P4.1.5: 前端类型同步**

Run: `grep -n "ChannelConnectionState\|needsReauth" src/lib/tauri.ts`

如果 `ChannelConnectionState` 是字符串联合类型，加 `'needsReauth'`：

```ts
export type ChannelConnectionState =
  | 'unconfigured'
  | 'disconnected'
  | 'connecting'
  | 'connected'
  | 'reconnecting'
  | 'configError'
  | 'needsReauth'  // new in Phase 5 PR4
```

- [ ] **Step P4.1.6: 提交**

```bash
git add src-tauri/src/connector/im/types.rs src-tauri/src/connector/im/trait_def.rs src-tauri/src/connector/im/manager.rs src/lib/tauri.ts
git commit -m "feat(connector/im): add NeedsReauth state + SessionExpired error (Phase 5 PR4 prep)"
```

---

## Task P4.2: `session_guard.rs` —— pause + assert_active 熔断

**Files:**
- Create: `src-tauri/src/connector/im/wechat/session_guard.rs`
- Modify: `src-tauri/src/connector/im/wechat/mod.rs`

- [ ] **Step P4.2.1: 加 `pub mod session_guard;`**

- [ ] **Step P4.2.2: 写失败的单测**

Create `src-tauri/src/connector/im/wechat/session_guard.rs`:

```rust
//! Per-account `SessionGuard` —— pause + assert_active 熔断（spec §1.2）。
//!
//! When `getUpdates` returns `errcode = -14`, we `pause(account, N min)` instead
//! of jumping straight to NeedsReauth. All outbound requests guard themselves
//! with `assert_active(account)`. After K consecutive pauses without a
//! successful poll in between, the worker escalates to NeedsReauth and ends
//! the stream.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use thiserror::Error;
use tokio::sync::RwLock;

/// Default pause duration when `errcode=-14` is returned. Chosen to be long
/// enough that we don't hammer the server while a true session expiry settles,
/// but short enough that benign one-off blips recover before the user notices.
/// Adjust based on openclaw实测 if needed.
pub const DEFAULT_PAUSE_DURATION: Duration = Duration::from_secs(5 * 60);

/// Number of consecutive pauses (with no successful poll between them) before
/// escalating to NeedsReauth.
pub const MAX_PAUSE_BEFORE_REAUTH: u32 = 3;

#[derive(Debug, Error)]
#[error("session paused for account {0}, {1:?} remaining")]
pub struct SessionPaused(pub String, pub Duration);

struct PauseEntry {
    until: Instant,
    consecutive_count: u32,
}

pub struct SessionGuard {
    inner: RwLock<HashMap<String, PauseEntry>>,
}

impl Default for SessionGuard {
    fn default() -> Self {
        Self::new()
    }
}

impl SessionGuard {
    pub fn new() -> Self {
        Self {
            inner: RwLock::new(HashMap::new()),
        }
    }

    /// Mark an account as paused for `dur`. Increments the consecutive-pause
    /// counter; caller may use `consecutive_pause_count` to decide when to
    /// escalate to NeedsReauth.
    pub async fn pause(&self, account_id: &str, dur: Duration) {
        let mut map = self.inner.write().await;
        let entry = map.entry(account_id.to_string()).or_insert(PauseEntry {
            until: Instant::now(),
            consecutive_count: 0,
        });
        entry.until = Instant::now() + dur;
        entry.consecutive_count += 1;
    }

    /// Returns `Some(remaining)` if the account is currently paused, else `None`.
    pub async fn remaining_pause(&self, account_id: &str) -> Option<Duration> {
        let map = self.inner.read().await;
        let entry = map.get(account_id)?;
        let now = Instant::now();
        if entry.until > now {
            Some(entry.until - now)
        } else {
            None
        }
    }

    /// Number of consecutive pauses since the last `reset_consecutive`.
    pub async fn consecutive_pause_count(&self, account_id: &str) -> u32 {
        let map = self.inner.read().await;
        map.get(account_id).map(|e| e.consecutive_count).unwrap_or(0)
    }

    /// Call on every successful getUpdates response to reset the escalation counter.
    pub async fn reset_consecutive(&self, account_id: &str) {
        let mut map = self.inner.write().await;
        if let Some(entry) = map.get_mut(account_id) {
            entry.consecutive_count = 0;
        }
    }

    /// Used by outbound APIs (sendMessage/sendTyping/getUploadUrl) to refuse
    /// when the account is paused.
    pub async fn assert_active(&self, account_id: &str) -> Result<(), SessionPaused> {
        match self.remaining_pause(account_id).await {
            None => Ok(()),
            Some(rem) => Err(SessionPaused(account_id.to_string(), rem)),
        }
    }
}

pub type SharedSessionGuard = Arc<SessionGuard>;

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn fresh_account_is_active() {
        let g = SessionGuard::new();
        assert!(g.assert_active("acc1").await.is_ok());
        assert_eq!(g.consecutive_pause_count("acc1").await, 0);
    }

    #[tokio::test]
    async fn pause_then_assert_active_fails() {
        let g = SessionGuard::new();
        g.pause("acc1", Duration::from_secs(60)).await;
        let err = g.assert_active("acc1").await.unwrap_err();
        assert_eq!(err.0, "acc1");
        assert!(err.1 <= Duration::from_secs(60));
        assert!(err.1 > Duration::from_secs(58));
    }

    #[tokio::test]
    async fn remaining_pause_returns_none_after_expiry() {
        let g = SessionGuard::new();
        g.pause("acc1", Duration::from_millis(10)).await;
        tokio::time::sleep(Duration::from_millis(20)).await;
        assert!(g.remaining_pause("acc1").await.is_none());
        assert!(g.assert_active("acc1").await.is_ok());
    }

    #[tokio::test]
    async fn consecutive_count_increments_each_pause() {
        let g = SessionGuard::new();
        g.pause("acc1", Duration::from_secs(60)).await;
        g.pause("acc1", Duration::from_secs(60)).await;
        g.pause("acc1", Duration::from_secs(60)).await;
        assert_eq!(g.consecutive_pause_count("acc1").await, 3);
    }

    #[tokio::test]
    async fn reset_consecutive_clears_counter() {
        let g = SessionGuard::new();
        g.pause("acc1", Duration::from_secs(60)).await;
        g.pause("acc1", Duration::from_secs(60)).await;
        g.reset_consecutive("acc1").await;
        assert_eq!(g.consecutive_pause_count("acc1").await, 0);
    }

    #[tokio::test]
    async fn per_account_isolation() {
        let g = SessionGuard::new();
        g.pause("acc1", Duration::from_secs(60)).await;
        assert!(g.assert_active("acc2").await.is_ok());
        assert!(g.assert_active("acc1").await.is_err());
    }
}
```

- [ ] **Step P4.2.3: 测试通过**

Run: `cd src-tauri && cargo test --lib connector::im::wechat::session_guard::tests`
Expected: 6/6 PASS。

- [ ] **Step P4.2.4: 提交**

```bash
git add src-tauri/src/connector/im/wechat/session_guard.rs src-tauri/src/connector/im/wechat/mod.rs
git commit -m "feat(connector/im/wechat): SessionGuard with pause + assert_active + consecutive_count escalation (spec §1.2)"
```

---

## Task P4.3: `api.rs` —— 7 endpoint HTTP 封装 + `getUpdates` 含 `longpolling_timeout_ms` 自适应

**Files:**
- Create: `src-tauri/src/connector/im/wechat/api.rs`
- Modify: `src-tauri/src/connector/im/wechat/mod.rs`

PR4 重点：`get_updates` 是长轮询主入口，必须正确处理 `errcode = -14`、`longpolling_timeout_ms`、`AbortError` 三种边界。

- [ ] **Step P4.3.1: 加 `pub mod api;`**

- [ ] **Step P4.3.2: 写失败的单测（mockito）**

Create `src-tauri/src/connector/im/wechat/api.rs`:

```rust
//! 7 个 iLink endpoint 的 HTTP 封装（spec §1.3 / §2）。
//!
//! - get_updates (POST 长轮询): 收 errcode=-14 → SessionExpired；
//!   收到 longpolling_timeout_ms 返回给 caller；client timeout 视为 wait。
//! - send_message / send_typing / get_upload_url / get_config: 普通 POST。
//! - QR endpoints 在 login.rs。

use std::time::Duration;

use serde::de::DeserializeOwned;
use thiserror::Error;

use super::endpoints;
use super::headers::{build_headers, HeaderInputs};
use super::types::{
    BaseInfo, GetConfigResp, GetUpdatesReq, GetUpdatesResp, GetUploadUrlReq, GetUploadUrlResp,
    SendMessageReq, SendTypingReq, WeixinMessage, SESSION_EXPIRED_ERRCODE,
};

#[derive(Debug, Error)]
pub enum WechatApiError {
    #[error("session expired: errcode={errcode} {errmsg}")]
    SessionExpired { errcode: i32, errmsg: String },
    #[error("transient error: {0}")]
    Transient(String),
    #[error("fatal: {0}")]
    Fatal(String),
}

pub const DEFAULT_LONGPOLL_TIMEOUT_MS: u64 = 35_000;
pub const DEFAULT_API_TIMEOUT_MS: u64 = 15_000;
pub const DEFAULT_CONFIG_TIMEOUT_MS: u64 = 10_000;

pub struct WechatApi {
    http: reqwest::Client,
    base_url: String,
    app_id: String,
    client_version: String,
}

impl WechatApi {
    pub fn new(http: reqwest::Client, base_url: String, app_id: String, client_version: String) -> Self {
        Self {
            http,
            base_url,
            app_id,
            client_version,
        }
    }

    fn build_url(&self, endpoint: &str) -> String {
        format!("{}/{}", self.base_url.trim_end_matches('/'), endpoint)
    }

    async fn post_json<Req: serde::Serialize, Resp: DeserializeOwned>(
        &self,
        endpoint: &str,
        body: &Req,
        token: Option<&str>,
        timeout_ms: u64,
    ) -> Result<Resp, WechatApiError> {
        let serialized = serde_json::to_string(body)
            .map_err(|e| WechatApiError::Fatal(format!("serialize {endpoint}: {e}")))?;
        let headers = build_headers(HeaderInputs {
            app_id: &self.app_id,
            client_version: &self.client_version,
            bot_token: token,
            route_tag: None,
            body: &serialized,
        });
        let url = self.build_url(endpoint);

        let req = self
            .http
            .post(&url)
            .headers(headers)
            .body(serialized)
            .timeout(Duration::from_millis(timeout_ms));

        match req.send().await {
            Ok(r) if r.status().is_success() => {
                let raw = r
                    .text()
                    .await
                    .map_err(|e| WechatApiError::Transient(e.to_string()))?;
                serde_json::from_str(&raw)
                    .map_err(|e| WechatApiError::Fatal(format!("parse {endpoint}: {e}; raw={raw}")))
            }
            Ok(r) => {
                let status = r.status();
                let raw = r.text().await.unwrap_or_default();
                if status.is_client_error() {
                    Err(WechatApiError::Fatal(format!("{endpoint} HTTP {status}: {raw}")))
                } else {
                    Err(WechatApiError::Transient(format!("{endpoint} HTTP {status}")))
                }
            }
            Err(e) if e.is_timeout() => {
                // Long-poll / API timeout — caller decides whether transient
                // or wait. For getUpdates this is normal; we expose as Transient
                // and the runtime loop simply retries.
                Err(WechatApiError::Transient(format!("{endpoint} timeout")))
            }
            Err(e) => Err(WechatApiError::Transient(format!("{endpoint} send: {e}"))),
        }
    }

    /// Long-poll for inbound updates. Default timeout 35s (server may override
    /// in response). Returns the full response so caller can read `longpolling_timeout_ms`.
    pub async fn get_updates(
        &self,
        token: &str,
        get_updates_buf: &str,
        timeout: Duration,
    ) -> Result<GetUpdatesResp, WechatApiError> {
        let req = GetUpdatesReq {
            get_updates_buf: Some(get_updates_buf.to_string()),
            base_info: BaseInfo {
                channel_version: Some(self.client_version.clone()),
            },
        };
        let resp: GetUpdatesResp = self
            .post_json(endpoints::GET_UPDATES, &req, Some(token), timeout.as_millis() as u64)
            .await?;

        // Promote errcode = -14 to SessionExpired
        if let Some(c) = resp.errcode {
            if c == SESSION_EXPIRED_ERRCODE || resp.ret == Some(SESSION_EXPIRED_ERRCODE) {
                return Err(WechatApiError::SessionExpired {
                    errcode: c,
                    errmsg: resp.errmsg.unwrap_or_default(),
                });
            }
        }
        Ok(resp)
    }

    pub async fn send_message(
        &self,
        token: &str,
        to_user_id: &str,
        text: &str,
        context_token: Option<&str>,
    ) -> Result<(), WechatApiError> {
        let msg = WeixinMessage {
            to_user_id: Some(to_user_id.to_string()),
            context_token: context_token.map(String::from),
            item_list: Some(vec![super::types::MessageItem {
                r#type: Some(super::types::MessageItemType::Text),
                text_item: Some(super::types::TextItem {
                    text: Some(text.to_string()),
                }),
                ..Default::default()
            }]),
            ..Default::default()
        };
        let req = SendMessageReq {
            msg: Some(msg),
            base_info: BaseInfo {
                channel_version: Some(self.client_version.clone()),
            },
        };
        // sendMessage response is empty; ignore its body.
        let _: serde_json::Value = self
            .post_json(endpoints::SEND_MESSAGE, &req, Some(token), DEFAULT_API_TIMEOUT_MS)
            .await?;
        Ok(())
    }

    pub async fn get_config(
        &self,
        token: &str,
        ilink_user_id: &str,
        context_token: Option<&str>,
    ) -> Result<GetConfigResp, WechatApiError> {
        #[derive(serde::Serialize)]
        struct Req<'a> {
            ilink_user_id: &'a str,
            #[serde(skip_serializing_if = "Option::is_none")]
            context_token: Option<&'a str>,
            base_info: BaseInfo,
        }
        let req = Req {
            ilink_user_id,
            context_token,
            base_info: BaseInfo {
                channel_version: Some(self.client_version.clone()),
            },
        };
        self.post_json(endpoints::GET_CONFIG, &req, Some(token), DEFAULT_CONFIG_TIMEOUT_MS)
            .await
    }

    pub async fn send_typing(
        &self,
        token: &str,
        ilink_user_id: &str,
        typing_ticket: &str,
        status: i32,
    ) -> Result<(), WechatApiError> {
        let req = SendTypingReq {
            ilink_user_id: Some(ilink_user_id.to_string()),
            typing_ticket: Some(typing_ticket.to_string()),
            status: Some(status),
            base_info: BaseInfo {
                channel_version: Some(self.client_version.clone()),
            },
        };
        let _: serde_json::Value = self
            .post_json(endpoints::SEND_TYPING, &req, Some(token), DEFAULT_CONFIG_TIMEOUT_MS)
            .await?;
        Ok(())
    }

    pub async fn get_upload_url(
        &self,
        token: &str,
        req: GetUploadUrlReq,
    ) -> Result<GetUploadUrlResp, WechatApiError> {
        self.post_json(endpoints::GET_UPLOAD_URL, &req, Some(token), DEFAULT_API_TIMEOUT_MS)
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mockito::Server;

    fn mk(server_url: &str) -> WechatApi {
        WechatApi::new(
            reqwest::Client::new(),
            server_url.to_string(),
            "test-app-id".to_string(),
            "0.1.0".to_string(),
        )
    }

    #[tokio::test]
    async fn get_updates_normal_response() {
        let mut server = Server::new_async().await;
        let _m = server
            .mock("POST", "/ilink/bot/getupdates")
            .with_status(200)
            .with_body(
                r#"{"ret":0,"msgs":[],"get_updates_buf":"next","longpolling_timeout_ms":35000}"#,
            )
            .create_async()
            .await;
        let api = mk(&server.url());
        let resp = api
            .get_updates("tk", "", Duration::from_secs(5))
            .await
            .unwrap();
        assert_eq!(resp.get_updates_buf.as_deref(), Some("next"));
        assert_eq!(resp.longpolling_timeout_ms, Some(35000));
    }

    #[tokio::test]
    async fn get_updates_errcode_minus_14_maps_to_session_expired() {
        let mut server = Server::new_async().await;
        let _m = server
            .mock("POST", "/ilink/bot/getupdates")
            .with_status(200)
            .with_body(r#"{"ret":-14,"errcode":-14,"errmsg":"session timeout"}"#)
            .create_async()
            .await;
        let api = mk(&server.url());
        let err = api
            .get_updates("tk", "", Duration::from_secs(5))
            .await
            .unwrap_err();
        assert!(matches!(
            err,
            WechatApiError::SessionExpired { errcode: -14, .. }
        ));
    }

    #[tokio::test]
    async fn http_500_is_transient() {
        let mut server = Server::new_async().await;
        let _m = server
            .mock("POST", "/ilink/bot/getupdates")
            .with_status(500)
            .with_body("oops")
            .create_async()
            .await;
        let api = mk(&server.url());
        let err = api
            .get_updates("tk", "", Duration::from_secs(5))
            .await
            .unwrap_err();
        assert!(matches!(err, WechatApiError::Transient(_)));
    }

    #[tokio::test]
    async fn http_400_is_fatal() {
        let mut server = Server::new_async().await;
        let _m = server
            .mock("POST", "/ilink/bot/getupdates")
            .with_status(400)
            .with_body("bad request")
            .create_async()
            .await;
        let api = mk(&server.url());
        let err = api
            .get_updates("tk", "", Duration::from_secs(5))
            .await
            .unwrap_err();
        assert!(matches!(err, WechatApiError::Fatal(_)));
    }

    #[tokio::test]
    async fn send_message_ok() {
        let mut server = Server::new_async().await;
        let _m = server
            .mock("POST", "/ilink/bot/sendmessage")
            .with_status(200)
            .with_body("{}")
            .create_async()
            .await;
        let api = mk(&server.url());
        api.send_message("tk", "wxid_alice@im.wechat", "hello", Some("ctx-1"))
            .await
            .unwrap();
    }
}
```

- [ ] **Step P4.3.3: 测试通过**

Run: `cd src-tauri && cargo test --lib connector::im::wechat::api::tests`
Expected: 5/5 PASS。

- [ ] **Step P4.3.4: 提交**

```bash
git add src-tauri/src/connector/im/wechat/api.rs src-tauri/src/connector/im/wechat/mod.rs
git commit -m "feat(connector/im/wechat): api.rs — 7 endpoint HTTP wrappers with errcode=-14 → SessionExpired (Phase 5 PR4)"
```

---

## Task P4.4: `runtime.rs` —— getUpdates 长轮询 worker loop

**Files:**
- Create: `src-tauri/src/connector/im/wechat/runtime.rs`
- Modify: `src-tauri/src/connector/im/wechat/mod.rs`

照 spec §3.1 实现 `futures::stream::unfold` 长轮询循环。PR4 这一步**只接 ChannelMessage stream 的入站路径**——parser / context_token / allow_from 用 stub（PR5 实现）。

- [ ] **Step P4.4.1: 加 `pub mod runtime;`**

- [ ] **Step P4.4.2: 写失败的集成测试（mockito + stream 收两条消息）**

Create `src-tauri/src/connector/im/wechat/runtime.rs`:

```rust
//! Long-poll worker loop. See spec §3.1 — `futures::stream::unfold` based
//! to match Phase 0 conventions (no async-stream dependency).
//!
//! Responsibilities:
//!   1. Honor cancel_token (≤2s shutdown contract from trait_def)
//!   2. Pause / unpause via SessionGuard on errcode=-14
//!   3. Escalate to NeedsReauth (stream end) after MAX_PAUSE_BEFORE_REAUTH
//!   4. Adapt next-poll timeout from server's longpolling_timeout_ms
//!   5. Persist get_updates_buf every 10 messages + on cancel (state.json)
//!   6. allowFrom filter + context_token persist + parser normalize
//!      (PR5 wires the parser; PR4 uses a stub)

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use futures::stream::{self, BoxStream, StreamExt};
use tokio::sync::Mutex;
use tokio_util::sync::CancellationToken;

use crate::connector::im::shared::reconnect::ReconnectBackoff;
use crate::connector::im::trait_def::ConnectorError;
use crate::connector::im::types::ChannelMessage;

use super::api::{WechatApi, WechatApiError};
use super::session_guard::{SharedSessionGuard, DEFAULT_PAUSE_DURATION, MAX_PAUSE_BEFORE_REAUTH};

/// State that the worker loop persists across iterations.
pub struct WorkerState {
    pub account_id: String,
    pub bot_token: String,
    pub get_updates_buf: String,
    pub state_path: PathBuf,  // path to state.json
}

impl WorkerState {
    pub async fn load(account_dir: &std::path::Path, account_id: String, bot_token: String) -> Self {
        let state_path = account_dir.join("state.json");
        let buf = match tokio::fs::read_to_string(&state_path).await {
            Ok(raw) => match serde_json::from_str::<serde_json::Value>(&raw) {
                Ok(j) => j
                    .get("get_updates_buf")
                    .and_then(|v| v.as_str())
                    .map(String::from)
                    .unwrap_or_default(),
                Err(_) => String::new(),
            },
            Err(_) => String::new(),
        };
        Self {
            account_id,
            bot_token,
            get_updates_buf: buf,
            state_path,
        }
    }

    pub async fn flush(&self) -> std::io::Result<()> {
        if let Some(parent) = self.state_path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        let body = serde_json::json!({ "get_updates_buf": self.get_updates_buf });
        let tmp = self.state_path.with_extension("json.tmp");
        tokio::fs::write(&tmp, serde_json::to_string_pretty(&body).unwrap()).await?;
        tokio::fs::rename(&tmp, &self.state_path).await?;
        Ok(())
    }
}

/// PR4: minimal parser stub — just yields nothing for now. PR5 replaces with
/// the real parser + allowFrom filter + context_token persist.
fn pr4_parser_stub(_raw: &super::types::WeixinMessage) -> Option<ChannelMessage> {
    None
}

pub fn build_inbound_stream(
    api: Arc<WechatApi>,
    state: Arc<Mutex<WorkerState>>,
    session_guard: SharedSessionGuard,
    cancel: CancellationToken,
) -> BoxStream<'static, ChannelMessage> {
    let init: (ReconnectBackoff, u64, u64) = (
        ReconnectBackoff::default_schedule(),
        0,
        super::api::DEFAULT_LONGPOLL_TIMEOUT_MS,
    );

    let stream = stream::unfold(init, move |(mut backoff, mut since_last_flush, mut next_timeout_ms)| {
        let cancel = cancel.clone();
        let api = Arc::clone(&api);
        let state = Arc::clone(&state);
        let session_guard = Arc::clone(&session_guard);
        async move {
            loop {
                if cancel.is_cancelled() {
                    let s = state.lock().await;
                    let _ = s.flush().await;
                    return None;
                }

                // Pause check (§1.2)
                {
                    let s = state.lock().await;
                    if let Some(rem) = session_guard.remaining_pause(&s.account_id).await {
                        drop(s);
                        log::info!("[wechat] paused, sleeping {:?}", rem);
                        tokio::select! {
                            _ = cancel.cancelled() => {
                                let s = state.lock().await;
                                let _ = s.flush().await;
                                return None;
                            }
                            _ = tokio::time::sleep(rem) => continue,
                        }
                    }
                }

                let (token, buf, account_id) = {
                    let s = state.lock().await;
                    (s.bot_token.clone(), s.get_updates_buf.clone(), s.account_id.clone())
                };

                let result = tokio::select! {
                    _ = cancel.cancelled() => {
                        let s = state.lock().await;
                        let _ = s.flush().await;
                        return None;
                    }
                    r = api.get_updates(&token, &buf, Duration::from_millis(next_timeout_ms)) => r,
                };

                match result {
                    Ok(resp) => {
                        backoff.reset();
                        session_guard.reset_consecutive(&account_id).await;
                        if let Some(ms) = resp.longpolling_timeout_ms {
                            next_timeout_ms = ms;
                        }
                        let new_buf = resp.get_updates_buf.unwrap_or_default();
                        if !new_buf.is_empty() {
                            state.lock().await.get_updates_buf = new_buf;
                        }
                        let msgs = resp.msgs.unwrap_or_default();
                        for raw in msgs {
                            // PR5 replaces this stub with parser + filter + context_token persist
                            if let Some(msg) = pr4_parser_stub(&raw) {
                                since_last_flush += 1;
                                if since_last_flush >= 10 {
                                    let _ = state.lock().await.flush().await;
                                    since_last_flush = 0;
                                }
                                return Some((msg, (backoff, since_last_flush, next_timeout_ms)));
                            }
                        }
                        // no messages produced (or all filtered) → continue loop
                    }
                    Err(WechatApiError::SessionExpired { errcode, errmsg }) => {
                        log::warn!("[wechat] errcode={errcode} {errmsg}, pausing");
                        session_guard
                            .pause(&account_id, DEFAULT_PAUSE_DURATION)
                            .await;
                        let count = session_guard.consecutive_pause_count(&account_id).await;
                        if count >= MAX_PAUSE_BEFORE_REAUTH {
                            log::warn!(
                                "[wechat] {count} consecutive pauses, escalating to NeedsReauth"
                            );
                            let s = state.lock().await;
                            let _ = s.flush().await;
                            return None;
                        }
                    }
                    Err(WechatApiError::Transient(e)) => {
                        let delay = backoff.next_delay();
                        log::info!("[wechat] transient error, sleeping {:?}: {e}", delay);
                        tokio::select! {
                            _ = cancel.cancelled() => {
                                let s = state.lock().await;
                                let _ = s.flush().await;
                                return None;
                            }
                            _ = tokio::time::sleep(delay) => {}
                        }
                    }
                    Err(WechatApiError::Fatal(e)) => {
                        log::error!("[wechat] fatal: {e}");
                        let s = state.lock().await;
                        let _ = s.flush().await;
                        return None;
                    }
                }
            }
        }
    });

    Box::pin(stream)
}

/// Helper for `ConnectorError::SessionExpired` mapping (consumed by manager
/// when stream ends after MAX_PAUSE_BEFORE_REAUTH escalation).
pub fn translate_api_error(e: WechatApiError) -> ConnectorError {
    match e {
        WechatApiError::SessionExpired { errcode, errmsg } => {
            ConnectorError::SessionExpired { errcode, errmsg }
        }
        WechatApiError::Transient(s) => ConnectorError::Transient(s),
        WechatApiError::Fatal(s) => ConnectorError::Fatal(s),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mockito::Server;
    use tempfile::TempDir;

    #[tokio::test]
    async fn loop_exits_cleanly_on_cancel() {
        let mut server = Server::new_async().await;
        // get_updates returns wait forever (empty response)
        let _m = server
            .mock("POST", "/ilink/bot/getupdates")
            .with_status(200)
            .with_body(r#"{"ret":0,"msgs":[],"get_updates_buf":""}"#)
            .create_async()
            .await;

        let dir = tempfile::tempdir().unwrap();
        let api = Arc::new(WechatApi::new(
            reqwest::Client::new(),
            server.url(),
            "x".to_string(),
            "0.1.0".to_string(),
        ));
        let state = Arc::new(Mutex::new(
            WorkerState::load(dir.path(), "acc-1".to_string(), "tk".to_string()).await,
        ));
        let guard: SharedSessionGuard = Arc::new(super::super::session_guard::SessionGuard::new());
        let cancel = CancellationToken::new();
        let mut stream = build_inbound_stream(api, state, guard, cancel.clone());

        // Spawn the stream consumer; cancel after 200ms
        let task = tokio::spawn(async move {
            cancel.cancel();
        });
        // The stream should terminate within ~2s of cancel
        let next = tokio::time::timeout(Duration::from_secs(3), stream.next()).await;
        task.await.unwrap();
        assert!(next.is_ok());
        assert!(next.unwrap().is_none());
    }

    #[tokio::test]
    async fn errcode_minus_14_triggers_pause() {
        let mut server = Server::new_async().await;
        let _m = server
            .mock("POST", "/ilink/bot/getupdates")
            .with_status(200)
            .with_body(r#"{"ret":-14,"errcode":-14,"errmsg":"session timeout"}"#)
            .create_async()
            .await;

        let dir = tempfile::tempdir().unwrap();
        let api = Arc::new(WechatApi::new(
            reqwest::Client::new(),
            server.url(),
            "x".to_string(),
            "0.1.0".to_string(),
        ));
        let state = Arc::new(Mutex::new(
            WorkerState::load(dir.path(), "acc-2".to_string(), "tk".to_string()).await,
        ));
        let guard: SharedSessionGuard = Arc::new(super::super::session_guard::SessionGuard::new());
        let cancel = CancellationToken::new();
        let mut stream = build_inbound_stream(Arc::clone(&api), state, Arc::clone(&guard), cancel.clone());

        // Run for ~200ms to let the loop hit at least one -14 → pause
        let _ = tokio::time::timeout(Duration::from_millis(200), stream.next()).await;
        cancel.cancel();
        let _ = stream.next().await;

        // After 1 pause, consecutive count should be ≥ 1
        let count = guard.consecutive_pause_count("acc-2").await;
        assert!(count >= 1, "got count={count}");
    }
}
```

- [ ] **Step P4.4.3: 测试**

Run: `cd src-tauri && cargo test --lib connector::im::wechat::runtime::tests`
Expected: 2/2 PASS（耗时约 1-3s 因为 sleep）。

- [ ] **Step P4.4.4: 提交**

```bash
git add src-tauri/src/connector/im/wechat/runtime.rs src-tauri/src/connector/im/wechat/mod.rs
git commit -m "feat(connector/im/wechat): runtime.rs — long-poll loop with SessionGuard pause + NeedsReauth escalation (spec §3.1)"
```

---

## Task P4.5: WechatConnector::start 接 runtime + manager 设 NeedsReauth

**Files:**
- Modify: `src-tauri/src/connector/im/wechat/connector.rs`
- Modify: `src-tauri/src/connector/im/manager.rs`

- [ ] **Step P4.5.1: WechatConnector::start 调 build_inbound_stream**

修改 `connector.rs` 的 `start` 方法：

```rust
async fn start(
    &self,
    ctx: ConnectorContext,
) -> Result<BoxStream<'static, ChannelMessage>, ConnectorError> {
    let account_dir_base = self
        .account_dir_base
        .as_ref()
        .ok_or_else(|| ConnectorError::Fatal("wechat: account_dir_base not configured".into()))?;
    // Find the (single) registered bot_id by scanning the directory. For
    // PR4 we assume one account per WechatConnector instance; PR-future may
    // multiplex.
    let bot_id = discover_bot_id(account_dir_base).await
        .ok_or(ConnectorError::Fatal("wechat: no registered bot found".into()))?;
    let account_dir = account_dir_base.join(&bot_id);

    let ss = self.secure_storage.as_ref().ok_or_else(|| {
        ConnectorError::Fatal("wechat: SecureStorage required to load bot_token".into())
    })?;
    let key = format!("aijia-wechat-bot-token-{bot_id}");
    let bot_token = ss
        .get(&key)
        .await
        .map_err(|e| ConnectorError::Fatal(format!("SecureStorage read: {e}")))?
        .ok_or_else(|| ConnectorError::Fatal(format!("missing bot_token for {bot_id}")))?;
    let bot_token = String::from_utf8(bot_token)
        .map_err(|e| ConnectorError::Fatal(format!("bot_token not utf-8: {e}")))?;

    // Load effective baseurl from auth.json
    let auth_raw = tokio::fs::read_to_string(account_dir.join("auth.json"))
        .await
        .map_err(|e| ConnectorError::Fatal(format!("read auth.json: {e}")))?;
    let auth: serde_json::Value = serde_json::from_str(&auth_raw)
        .map_err(|e| ConnectorError::Fatal(format!("parse auth.json: {e}")))?;
    let effective_base_url = auth
        .get("baseurl")
        .and_then(|v| v.as_str())
        .unwrap_or(&self.base_url)
        .to_string();

    let api = std::sync::Arc::new(super::api::WechatApi::new(
        self.http.clone(),
        effective_base_url,
        self.app_id.clone(),
        self.client_version.clone(),
    ));
    let state = std::sync::Arc::new(tokio::sync::Mutex::new(
        super::runtime::WorkerState::load(&account_dir, bot_id.clone(), bot_token).await,
    ));
    // Lazy create a per-account guard. For simplicity store on the connector;
    // PR-future generalizes.
    let guard: super::session_guard::SharedSessionGuard =
        std::sync::Arc::new(super::session_guard::SessionGuard::new());

    let stream = super::runtime::build_inbound_stream(api, state, guard, ctx.cancel_token);
    Ok(stream)
}

async fn discover_bot_id(account_dir_base: &std::path::Path) -> Option<String> {
    let mut dir = tokio::fs::read_dir(account_dir_base).await.ok()?;
    while let Ok(Some(entry)) = dir.next_entry().await {
        if entry.file_type().await.ok()?.is_dir() {
            if entry.path().join("auth.json").exists() {
                return entry.file_name().to_str().map(String::from);
            }
        }
    }
    None
}
```

记得把 `discover_bot_id` 加到模块顶层（不是 trait impl 内）。

- [ ] **Step P4.5.2: manager.rs 加 wechat 启动 + NeedsReauth 设置**

参考飞书 (`set_feishu_connection_state` / 启动 worker) 在 `manager.rs` 加 wechat 分支。当 stream 自然终止（`None`）且当前 state 是 Connected，map 到 `NeedsReauth`（spec §1.2）：

Run: `grep -n "set_feishu_connection_state\|fn start_feishu_worker\|fn auto_connect" src-tauri/src/connector/im/manager.rs | head -10`

按飞书模式加 `set_wechat_connection_state` + `start_wechat_worker`，stream `None` 时设置 `ChannelConnectionState::NeedsReauth`。

- [ ] **Step P4.5.3: 测试 + 编译**

Run: `cd src-tauri && cargo test --lib connector::im::wechat && cargo build`
Expected: PASS。

- [ ] **Step P4.5.4: 提交**

```bash
git add src-tauri/src/connector/im/wechat/connector.rs src-tauri/src/connector/im/manager.rs
git commit -m "feat(connector/im/wechat): start() wires runtime loop; manager maps stream-end → NeedsReauth"
```

---

## Task P4.6: PR4 验收 + 描述

- [ ] **Step P4.6.1: 全套测试**

Run: `cd src-tauri && cargo test --lib connector::im::wechat && cargo test review_ --tests`
Expected: 全 PASS。

- [ ] **Step P4.6.2: PR4 描述**

```
Phase 5 PR4 — Wechat 长轮询 + IMConnector::start + NeedsReauth。

新增：
- session_guard.rs: SessionGuard with pause + assert_active + consecutive_count
- api.rs: 5 个 POST endpoint 封装 + errcode=-14 → SessionExpired
- runtime.rs: futures::stream::unfold worker loop（cancel ≤2s，
  longpolling_timeout_ms 自适应，pause N min 后升级 NeedsReauth）

WechatConnector::start 接通：discover_bot_id → load bot_token from SecureStorage
→ build WechatApi + WorkerState → build_inbound_stream。

manager.rs 加 wechat 分支：stream 结束 → ChannelConnectionState::NeedsReauth。

类型扩展：
- ConnectorError::SessionExpired { errcode, errmsg }
- ChannelConnectionState::NeedsReauth + 前端 TS 类型同步

注：parser/allow_from/context_token 仍是 PR5 stub；PR5 替换。
```

---

# PR5: sender + parser + 3 个 store + AiCard fallback + StreamingMarkdownFilter + allowFrom 过滤

> 因为 plan 已经偏长，PR5-PR7 部分采用更紧凑的"目标 + 文件清单 + 关键测试 + 注意陷阱"的形式，所有 step 仍然显式提供 TDD 检查点和代码示例的引用源（spec / openclaw 路径），实现者按 PR1-PR4 的模式照搬即可。

## Task P5.1: `markdown_filter.rs` —— 移植 openclaw StreamingMarkdownFilter

**Source**：`/Users/oayzz/Downloads/openclaw channel/openclaw-weixin-main/src/messaging/markdown-filter.ts`

**接口**：
```rust
pub struct StreamingMarkdownFilter { /* internal state */ }
impl StreamingMarkdownFilter {
    pub fn new() -> Self;
    pub fn feed(&mut self, chunk: &str) -> String;
    pub fn flush(&mut self) -> String;
    /// Convenience: feed + flush in one call (for non-streaming use).
    pub fn feed_and_flush(input: &str) -> String;
}
```

- [ ] **Step P5.1.1: 读 openclaw 实现细节**

Run: `cat "/Users/oayzz/Downloads/openclaw channel/openclaw-weixin-main/src/messaging/markdown-filter.ts"`

记下：① state-machine 状态（normal / saw_asterisk / saw_underscore / saw_backtick） ② 哪些字符要"剥离"（`*`, `_`, ` `` `, headers `#`） ③ 哪些保留（列表 `-` / `1.`, URL）

- [ ] **Step P5.1.2: 写圣经测试（用 openclaw 的 markdown-filter.test.ts 的 fixture）**

Run: `cat "/Users/oayzz/Downloads/openclaw channel/openclaw-weixin-main/src/messaging/markdown-filter.test.ts"`

把 fixture（输入 → 期望输出对）原样移植到 Rust 单测。这是 byte-for-byte compatibility 的圣经测试。

- [ ] **Step P5.1.3: 实现 + 测试通过 + 提交**

```bash
git commit -m "feat(connector/im/wechat): port StreamingMarkdownFilter from openclaw (Phase 5 PR5)"
```

---

## Task P5.2: 完善 `session.rs` —— WechatSessionStore + WechatContextTokenStore

**Files**: 在 PR3 已建的 `session.rs` 末尾加两个 store。

**WechatSessionStore**：`HashMap<session_id, ilink_user_id>` + 持久化 `sessions.json`。

**WechatContextTokenStore**：`HashMap<(account_id, ilink_user_id), context_token>` + 持久化 `context_tokens.json`，立即落盘（无 fsync 风暴风险，spec §3.4）。

**单测**：跟 AllowFromStore 同款 round-trip + per-account isolation。

```bash
git commit -m "feat(connector/im/wechat): WechatSessionStore + WechatContextTokenStore (spec §3.2 / §3.4)"
```

---

## Task P5.3: `parser.rs` —— WeixinMessage → ChannelMessage normalize

**Source**：`/Users/oayzz/Downloads/openclaw channel/openclaw-weixin-main/src/messaging/inbound.ts::bodyFromItemList`

**关键路径**：
- TEXT → text body
- IMAGE → 占位 + MediaPath（PR6 接 download）
- FILE → 占位 + MediaPath（PR6）
- VOICE 有 `voice_item.text` → 按 text body 走（spec §6 修正）
- VOICE 无 text / VIDEO → "[不支持的消息类型]" 占位
- `ref_msg` → "[引用: {prefix}]\n{body}" 前缀拼接

**ConversationType**：永远 `Private`（spec §2 Phase 5 仅私聊）。

**单测**：5 种 MessageItemType + ref_msg + VOICE 有/无 text 分支。

```bash
git commit -m "feat(connector/im/wechat): parser.rs — 5 message types → ChannelMessage (spec §3.2 / §6)"
```

---

## Task P5.4: `sender.rs` —— send_message + send_typing 入口

**Files**：把 PR4 `api.rs::send_message` 包成连接 session store / context_tokens / markdown filter 的 sender 高层入口。

```rust
pub async fn send_text(
    api: &WechatApi,
    sessions: &WechatSessionStore,
    context_tokens: &WechatContextTokenStore,
    session_guard: &SessionGuard,
    account_id: &str,
    session_id: &str,
    text: &str,
) -> Result<(), ConnectorError>;
```

完整流程：assert_active → sessions.get_user_id → context_tokens.get → StreamingMarkdownFilter::feed_and_flush → api.send_message。

**单测**：通过 mockito + 三个 store 的真实实例，覆盖正常 / 缺 session_id / 缺 context_token (log warn 不阻断) / pause 状态 4 个分支。

```bash
git commit -m "feat(connector/im/wechat): sender.rs — text send with session reverse-lookup + markdown filter (spec §3.2)"
```

---

## Task P5.5: AiCardChunk fallback + runtime.rs 接 parser / allow_from / context_token

**Files**: `connector.rs::send` 加 `ReplyContent::AiCardChunk` 分支用 `AiCardFallbackBuffer::new_no_placeholder()`（Phase 4 PR3 已加）。`runtime.rs` 的 `pr4_parser_stub` 替换为真实路径：

```rust
// 1. context_token persist
if let (Some(uid), Some(ct)) = (&raw.from_user_id, &raw.context_token) {
    context_tokens.set(account_id, uid, ct).await;
}
// 2. allow_from filter
if let Some(uid) = &raw.from_user_id {
    if !allow_from.is_allowed(uid).await {
        log::info!("[wechat] dropped from non-allowlisted {uid}");
        continue;
    }
}
// 3. parser normalize
if let Some(msg) = parser::normalize(&raw) {
    return Some(...);
}
```

`runtime::build_inbound_stream` 增加参数 `parser_fn`, `allow_from`, `context_tokens`。

**单测**：Mockito → mock get_updates 返回 (allowlisted_msg + not_allowed_msg) → 只 yield 前者；mock VOICE with text → yield as text。

```bash
git commit -m "feat(connector/im/wechat): wire parser + allow_from filter + context_token persist into runtime loop"
git commit -m "feat(connector/im/wechat): AiCardChunk → no_placeholder buffer (spec §3.3)"
```

---

## Task P5.6: `observe_session` trait 实现

`connector.rs::observe_session`：`self.sessions.observe(&session_id, conversation_key)`。

**Phase 1 PR0d** 已经在 manager 的 worker loop 里加了"router 建 session 后调 connector.observe_session" 的路径 —— 直接受益。

```bash
git commit -m "feat(connector/im/wechat): impl observe_session — populate WechatSessionStore"
```

---

## Task P5.7: PR5 验收

```
Phase 5 PR5 — Wechat 入站 normalize + 出站 + 3 个 store + AiCard fallback。

新增：
- markdown_filter.rs: StreamingMarkdownFilter (openclaw byte-for-byte port)
- session.rs 扩展：WechatSessionStore + WechatContextTokenStore
- parser.rs: WeixinMessage → ChannelMessage (5 types + ref_msg + VOICE 文本)
- sender.rs: send_text 走 session 反查 + context_token echo + markdown filter

WechatConnector::send 完整实现：Text/Markdown 走 sender；AiCardChunk 走
new_no_placeholder buffer。observe_session 写 WechatSessionStore。

runtime.rs 替换 pr4 stub：context_token persist + allow_from filter + 真实 parser。
```

---

# PR6: media —— getUploadUrl + 上传/下载 + crypto 接入

## Task P6.1: `media.rs`

**Source**: `/Users/oayzz/Downloads/openclaw channel/openclaw-weixin-main/src/cdn/upload.ts` / `pic-decrypt.ts` / `media-download.ts`

**核心 fn**：
```rust
pub async fn download_and_decrypt(
    http: &reqwest::Client,
    cdn_url: &str,
    aes_key_hex: &str,
) -> Result<Vec<u8>, MediaError>;

pub async fn encrypt_and_upload(
    http: &reqwest::Client,
    api: &WechatApi,
    token: &str,
    to_user_id: &str,
    file: &Path,
    media_type: UploadMediaType,
) -> Result<CdnMedia, MediaError>;
```

**关键陷阱**：
- inbound 优先用 `image_item.aeskey` (hex) 而非 `media.aes_key` (base64)
- 上传用 `upload_type_from_item_type` 显式转换避免两套枚举撞车
- 缩略图：IMAGE/VIDEO 必填，FILE/VOICE 不填

**单测**：圣经 fixture（PR2 已有）+ mockito 模拟 CDN URL + 上传 URL 流程。

```bash
git commit -m "feat(connector/im/wechat): media.rs — encrypted CDN download/upload + thumbnail (spec §4)"
```

---

## Task P6.2: parser.rs / sender.rs 接 media

inbound IMAGE/FILE/VOICE 媒体 → `media::download_and_decrypt` → 写本地临时文件 → ChannelMessage 带 `MediaPath`。

出站附件 → `media::encrypt_and_upload` → 拿到 `CdnMedia` → `sendMessage` 带 image_item/file_item。

```bash
git commit -m "feat(connector/im/wechat): wire media download/upload into parser + sender (spec §4)"
```

---

# PR7: 集成测试 + 前端 UI + allowFrom 管理 UI + NeedsReauth UI

## Task P7.1: `tests/im_wechat_integration.rs`

完整端到端：mock iLink server（mockito） → connector.start → 模拟 2 条私聊 inbound + 1 条 AiCardChunk fallback final + 1 条 allowFrom 过滤掉的陌生人消息 → 验证 context_token 持久化 + state.json 持久化 + mode 切换到 NeedsReauth。

```bash
git commit -m "test(connector/im/wechat): full integration with mocked iLink (spec §6)"
```

## Task P7.2: `review_im_layering.rs` 加 wechat

如果 PR1 Task 8 已经加过，本步骤跳过；否则补加。

## Task P7.3: 前端 ChannelConfig 加 wechat 分支

**Files**:
- Modify: `src/features/channel/ChannelConfig.tsx`
- Modify: `src/features/channel/ChannelConfig.test.tsx`

dingtalk 已通过 PR0 `RegistrationModal mode="url"` 接入。wechat 接 `mode="qr_url"`：

```tsx
function WechatRegistration({ onSaved, onClose }) {
  const [begin, setBegin] = useState<ChannelRegistrationBeginResult | null>(null)
  // ... 跟 dingtalk 同款 begin/poll 结构 ...
  return (
    <RegistrationModal
      mode="qr_url"
      title="添加个人微信账号"
      qrUrl={begin.verificationUriComplete}
      expireSeconds={begin.expiresInSeconds || 300}
      pollIntervalMs={(begin.intervalSeconds || 2) * 1000}
      pollState={pollState}
      onConfirmed={() => onSaved?.()}
      onCancel={() => {}}
    />
  )
}
```

入口（"添加频道"对话框里增加 wechat 选项）按现有 dingtalk 模式照搬。

```bash
git commit -m "feat(channel): wechat registration UI via RegistrationModal mode='qr_url' (Phase 5 PR7)"
```

## Task P7.4: `AllowFromManagement.tsx` 白名单 CRUD

**Files**:
- Create: `src/features/channel/wechat/AllowFromManagement.tsx`
- Create: `src/features/channel/wechat/AllowFromManagement.test.tsx`

UI: 列表 + "添加" 输入框 + 删除按钮。后端 commands：`channel_wechat_allow_from_list / add / remove`。

```bash
git commit -m "feat(channel): wechat allowFrom management UI + Tauri commands (spec §1.4)"
```

## Task P7.5: `NeedsReauthBanner.tsx` ⚠️ 提示

**Files**:
- Create: `src/features/channel/wechat/NeedsReauthBanner.tsx`

在频道页（或全局顶部）当 `ChannelConnectionState === 'needsReauth'` 时显示警告 + "重新扫码"按钮（触发 `beginRegistration('wechat')`）。dingtalk 也复用（device_code 过期同款场景）。

```bash
git commit -m "feat(channel): NeedsReauth banner with re-scan CTA (spec §1.2)"
```

## Task P7.6: 浏览器冒烟

`pnpm tauri:dev` → 跑：
- 添加微信账号 → 扫码（用真实账号）→ 收到一条消息 → AI 回复
- 删除账号
- 模拟 token 过期（手动改 SecureStorage 让 bot_token 失效）→ 看到 NeedsReauth banner → 点"重新扫码"恢复

记录 smoke 结果 amend 到最后一个 commit。

---

## §End — Phase 5 整体验收

- [ ] PR3-7 全部 PR 合入 main
- [ ] `cargo test --lib connector::im::wechat` 全 PASS
- [ ] `cargo test --test im_wechat_integration` 全 PASS
- [ ] `cargo test review_` 全 PASS
- [ ] `pnpm exec vitest run src/features/channel/wechat` 全 PASS
- [ ] `pnpm exec tsc --noEmit` 0 errors
- [ ] 浏览器冒烟通过：扫码登录、收发、allowFrom 过滤、NeedsReauth 流程
- [ ] iLink-App-Id 默认值来自 openclaw（暂不向 OSS 发布，spec §1.5）
- [ ] CLAUDE.md 加一行 "**个微（wechat / iLink）**" 简介，跟其他平台并列

完成。Phase 5 全部交付。
