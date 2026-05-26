# 断网检测与提示 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 后端常驻 30s 探测 lotus 网关，状态变化推前端；UI 顶栏红点 + 发送时 toast 提示"网络不通"，技术细节只落日志不进 UI。

**Architecture:** Rust 后端 `runtime/network/` 模块用独立 `reqwest::Client` HEAD `https://ai-tenant.renlijia.com`，结果分三态（Online / Offline / ServerDegraded）。状态变化时通过 `RuntimeHost::emit_legacy_event` 直接发 `network:status` legacy event（不经 `RuntimeEventBus`，因为 `RuntimeEvent` 强制带 SessionId/RunId 而网络状态是全局的）。前端 `useNetworkStatus` 订阅事件写入 `networkStore`，`NetworkStatusIndicator` 渲染顶栏角标，`useChat` 发送前检查 store 离线则 toast。

**Tech Stack:** Rust（tokio interval + reqwest）、Tauri 2 command/event、React + TypeScript + Zustand、lucide-react、Radix Popover、react-i18next、Vitest、wiremock。

**Spec:** `docs/superpowers/specs/2026-05-26-network-detection-design.md`

**与 spec 的微小偏离**：spec §3.1 说"状态变化通过 `RuntimeEventBus::publish` 发出"。实际 `RuntimeEvent` 结构强制要求 `session_id: SessionId` / `run_id: RunId`（`src-tauri/src/runtime/events.rs:218-224`），网络状态是全局事件，不属于任何会话。本 plan 改为：`NetworkProbe` 直接持有 `Arc<dyn RuntimeHost>` 引用，状态变化时调 `host.emit_legacy_event("network:status", payload)`，绕过 `RuntimeEvent` 包装。架构约束（`runtime/network/` 不 `use tauri::*`）仍然成立——通过 `RuntimeHost` trait 注入。

---

## File Structure

**Rust 新建：**
- `src-tauri/src/runtime/network/mod.rs` — 对外 re-export
- `src-tauri/src/runtime/network/state.rs` — `NetworkStatus` / `NetworkErrorKind` 枚举 + `NetworkSnapshot`
- `src-tauri/src/runtime/network/probe.rs` — `NetworkProbe`（探测逻辑 + classify）
- `src-tauri/src/transport/tauri_commands/network.rs` — `network_get_status` / `network_force_probe` Tauri commands
- `src-tauri/tests/network_probe_integration_test.rs` — wiremock 集成测试
- `src-tauri/tests/review_network_module.rs` — 架构回归测试（守 "no use tauri::*"）

**Rust 修改：**
- `src-tauri/src/runtime/mod.rs` — `pub mod network;`
- `src-tauri/src/transport/tauri_commands/mod.rs` — `pub mod network;` + invoke handler 注册
- `src-tauri/src/lib.rs` — `setup()` 内启动 `NetworkProbe` task + manage 共享状态

**前端新建：**
- `src/stores/networkStore.ts` — Zustand store
- `src/stores/networkStore.test.ts`
- `src/hooks/useNetworkStatus.ts` — 订阅 `network:status` event
- `src/hooks/useOfflineSendWarning.ts` — 发送前检查并 push toast
- `src/hooks/useOfflineSendWarning.test.tsx`
- `src/components/shell/NetworkStatusIndicator.tsx` — 角标 + popover
- `src/components/shell/__tests__/NetworkStatusIndicator.test.tsx`

**前端修改：**
- `src/lib/tauri.ts` — `TAURI_EVENTS.NETWORK_STATUS` 常量 + payload 类型 + `networkGetStatus` / `networkForceProbe` invoke 包装
- `src/i18n/zh-CN.json` / `src/i18n/en-US.json` — 新增 `network.*` 命名空间
- `src/App.tsx` 顶层 — `useNetworkStatus()` 挂载
- `src/components/shell/ChatTopBar.tsx` / `PageTopBar.tsx` — 在右侧渲染 `<NetworkStatusIndicator />`
- `src/hooks/useChat.ts:405` 前 — 调 `useOfflineSendWarning` hook 获取 warn 函数并在发送前调用

---

## Task 1: NetworkStatus 与 NetworkErrorKind 枚举

**Files:**
- Create: `src-tauri/src/runtime/network/state.rs`
- Create: `src-tauri/src/runtime/network/mod.rs`
- Modify: `src-tauri/src/runtime/mod.rs`

- [ ] **Step 1: 写失败测试**

Create `src-tauri/src/runtime/network/state.rs`:

```rust
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum NetworkStatus {
    Online,
    Offline,
    ServerDegraded,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum NetworkErrorKind {
    Timeout,
    Dns,
    ConnectRefused,
    Tls,
    Other,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct NetworkSnapshot {
    pub status: NetworkStatus,
    pub last_check_at_ms: i64,
    pub latency_ms: Option<u32>,
    pub error_kind: Option<NetworkErrorKind>,
}

impl NetworkSnapshot {
    pub fn unknown() -> Self {
        Self {
            status: NetworkStatus::Online, // see test_unknown_initial - placeholder
            last_check_at_ms: 0,
            latency_ms: None,
            error_kind: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_serializes_kebab_case() {
        let json = serde_json::to_string(&NetworkStatus::ServerDegraded).unwrap();
        assert_eq!(json, "\"server-degraded\"");
    }

    #[test]
    fn test_error_kind_serializes_snake_case() {
        let json = serde_json::to_string(&NetworkErrorKind::ConnectRefused).unwrap();
        assert_eq!(json, "\"connect_refused\"");
    }

    #[test]
    fn test_snapshot_camel_case_keys() {
        let snap = NetworkSnapshot {
            status: NetworkStatus::Offline,
            last_check_at_ms: 1234,
            latency_ms: None,
            error_kind: Some(NetworkErrorKind::Dns),
        };
        let json = serde_json::to_value(&snap).unwrap();
        assert!(json.get("lastCheckAtMs").is_some());
        assert!(json.get("errorKind").is_some());
    }
}
```

Create `src-tauri/src/runtime/network/mod.rs`:

```rust
pub mod state;

pub use state::{NetworkErrorKind, NetworkSnapshot, NetworkStatus};
```

Modify `src-tauri/src/runtime/mod.rs`, add after `pub mod messaging;`:

```rust
pub mod network;
```

- [ ] **Step 2: 运行测试验证失败**

Run: `cd src-tauri && cargo test --lib runtime::network::state::tests -- --nocapture`
Expected: 编译通过、测试 PASS（这一步本身没有"失败"的待实现项，只是验证序列化正确）。

- [ ] **Step 3: 提交**

```bash
git add src-tauri/src/runtime/mod.rs src-tauri/src/runtime/network/
git commit -m "feat(network): add NetworkStatus/ErrorKind/Snapshot types

Spec §5.1 — kebab-case status enum, snake_case error kind,
camelCase snapshot fields for direct legacy-event serialization."
```

---

## Task 2: NetworkProbe 单元测试（classify 函数）

**Files:**
- Modify: `src-tauri/src/runtime/network/probe.rs`（创建）

- [ ] **Step 1: 写 probe.rs 骨架 + 失败测试**

Create `src-tauri/src/runtime/network/probe.rs`:

```rust
use std::time::Duration;

use reqwest::StatusCode;

use crate::runtime::network::state::{NetworkErrorKind, NetworkStatus};

/// 把一次 HEAD 请求的结果（reqwest::Result<reqwest::Response>）映射为三态。
pub(crate) fn classify_response(
    result: &Result<reqwest::Response, reqwest::Error>,
) -> (NetworkStatus, Option<NetworkErrorKind>) {
    match result {
        Ok(resp) => {
            let status = resp.status();
            if status.is_server_error() {
                (NetworkStatus::ServerDegraded, None)
            } else {
                // 2xx / 3xx / 4xx including 401/403 — TCP+TLS+HTTP shook hands.
                (NetworkStatus::Online, None)
            }
        }
        Err(err) => {
            let kind = classify_error(err);
            (NetworkStatus::Offline, Some(kind))
        }
    }
}

pub(crate) fn classify_error(err: &reqwest::Error) -> NetworkErrorKind {
    if err.is_timeout() {
        return NetworkErrorKind::Timeout;
    }
    if err.is_connect() {
        // connect errors include DNS failures + TCP refused — drill into source.
        let msg = err.to_string().to_lowercase();
        if msg.contains("dns") || msg.contains("name resolution") || msg.contains("lookup") {
            return NetworkErrorKind::Dns;
        }
        if msg.contains("refused") {
            return NetworkErrorKind::ConnectRefused;
        }
        return NetworkErrorKind::Other;
    }
    let msg = err.to_string().to_lowercase();
    if msg.contains("certificate") || msg.contains("tls") || msg.contains("ssl") {
        return NetworkErrorKind::Tls;
    }
    NetworkErrorKind::Other
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ok_response(status: StatusCode) -> Result<reqwest::Response, reqwest::Error> {
        Ok(reqwest::Response::from(
            http::Response::builder()
                .status(status)
                .body("")
                .unwrap(),
        ))
    }

    #[test]
    fn test_200_is_online() {
        let (status, kind) = classify_response(&ok_response(StatusCode::OK));
        assert_eq!(status, NetworkStatus::Online);
        assert_eq!(kind, None);
    }

    #[test]
    fn test_401_is_online() {
        let (status, kind) = classify_response(&ok_response(StatusCode::UNAUTHORIZED));
        assert_eq!(status, NetworkStatus::Online);
        assert_eq!(kind, None);
    }

    #[test]
    fn test_500_is_server_degraded() {
        let (status, _) = classify_response(&ok_response(StatusCode::INTERNAL_SERVER_ERROR));
        assert_eq!(status, NetworkStatus::ServerDegraded);
    }

    #[test]
    fn test_502_is_server_degraded() {
        let (status, _) = classify_response(&ok_response(StatusCode::BAD_GATEWAY));
        assert_eq!(status, NetworkStatus::ServerDegraded);
    }
}
```

Add to `src-tauri/src/runtime/network/mod.rs`:

```rust
pub mod probe;
```

Add to `src-tauri/Cargo.toml` `[dev-dependencies]` (verify `http` crate present; reqwest re-exports it):

```toml
# http crate is already pulled in by reqwest. No additional dev-dependency needed.
```

- [ ] **Step 2: 运行测试**

Run: `cd src-tauri && cargo test --lib runtime::network::probe::tests -- --nocapture`
Expected: PASS。如果 `http::Response::builder()` 不可用，改用 `reqwest::Response::from(http::Response::new(""))` + builder 模式。

- [ ] **Step 3: 提交**

```bash
git add src-tauri/src/runtime/network/mod.rs src-tauri/src/runtime/network/probe.rs
git commit -m "feat(network): classify_response — map reqwest result to NetworkStatus

Spec §4.2 — 2xx/3xx/4xx → Online (incl 401/403, since TCP+TLS+HTTP
handshake succeeded), 5xx → ServerDegraded, transport errors → Offline."
```

---

## Task 3: NetworkProbe 探测循环骨架

**Files:**
- Modify: `src-tauri/src/runtime/network/probe.rs`

- [ ] **Step 1: 写探测循环**

Append to `src-tauri/src/runtime/network/probe.rs`:

```rust
use std::sync::{Arc, Mutex};
use std::time::Instant;

use chrono::Utc;
use serde_json::json;
use tokio::sync::mpsc;
use tokio::time::{interval, MissedTickBehavior};
use tracing::{info, warn};

use crate::runtime::network::state::NetworkSnapshot;
use crate::transport::runtime_host::RuntimeHost;

const PROBE_URL: &str = "https://ai-tenant.renlijia.com";
const ONLINE_INTERVAL_SECS: u64 = 30;
const OFFLINE_INTERVAL_SECS: u64 = 10;
const RECOVERY_SUCCESS_THRESHOLD: u32 = 3;
const HEAD_TIMEOUT_SECS: u64 = 5;
const FORCE_PROBE_THROTTLE_MS: u64 = 1000;

pub struct NetworkProbe {
    client: reqwest::Client,
    host: Arc<dyn RuntimeHost>,
    snapshot: Arc<Mutex<Option<NetworkSnapshot>>>,
    force_tx: mpsc::Sender<()>,
    force_rx: Arc<Mutex<Option<mpsc::Receiver<()>>>>,
    last_force_at_ms: Arc<Mutex<i64>>,
}

impl NetworkProbe {
    pub fn new(host: Arc<dyn RuntimeHost>) -> Self {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(HEAD_TIMEOUT_SECS))
            .build()
            .expect("network probe reqwest client");
        let (force_tx, force_rx) = mpsc::channel(4);
        Self {
            client,
            host,
            snapshot: Arc::new(Mutex::new(None)),
            force_tx,
            force_rx: Arc::new(Mutex::new(Some(force_rx))),
            last_force_at_ms: Arc::new(Mutex::new(0)),
        }
    }

    pub fn snapshot(&self) -> Option<NetworkSnapshot> {
        self.snapshot.lock().unwrap().clone()
    }

    /// Best-effort throttled force probe. Returns true if a probe was triggered,
    /// false if throttled.
    pub fn request_force_probe(&self) -> bool {
        let now_ms = Utc::now().timestamp_millis();
        let mut last = self.last_force_at_ms.lock().unwrap();
        if now_ms - *last < FORCE_PROBE_THROTTLE_MS as i64 {
            return false;
        }
        *last = now_ms;
        let _ = self.force_tx.try_send(());
        true
    }

    /// Spawn the long-running probe task. Returns immediately.
    pub fn spawn(self: Arc<Self>) {
        let task_self = self.clone();
        tokio::spawn(async move {
            task_self.run_loop().await;
        });
    }

    async fn run_loop(self: Arc<Self>) {
        // Take ownership of the rx (only one loop per probe).
        let mut force_rx = match self.force_rx.lock().unwrap().take() {
            Some(rx) => rx,
            None => {
                warn!("network probe: run_loop called twice, ignoring");
                return;
            }
        };

        let mut current_interval = interval(Duration::from_secs(ONLINE_INTERVAL_SECS));
        current_interval.set_missed_tick_behavior(MissedTickBehavior::Skip);
        let mut current_period = ONLINE_INTERVAL_SECS;
        let mut consecutive_success = 0u32;

        // Initial probe immediately.
        self.probe_once_and_emit().await;

        loop {
            tokio::select! {
                _ = current_interval.tick() => {
                    self.probe_once_and_emit().await;
                }
                _ = force_rx.recv() => {
                    self.probe_once_and_emit().await;
                }
            }

            // Decide next interval period based on current snapshot.
            let snap = self.snapshot.lock().unwrap().clone();
            let desired_period = match snap.as_ref().map(|s| s.status) {
                Some(NetworkStatus::Offline) => {
                    consecutive_success = 0;
                    OFFLINE_INTERVAL_SECS
                }
                Some(NetworkStatus::Online) | Some(NetworkStatus::ServerDegraded) => {
                    consecutive_success = consecutive_success.saturating_add(1);
                    if current_period == OFFLINE_INTERVAL_SECS
                        && consecutive_success >= RECOVERY_SUCCESS_THRESHOLD
                    {
                        ONLINE_INTERVAL_SECS
                    } else {
                        current_period
                    }
                }
                None => current_period,
            };
            if desired_period != current_period {
                current_period = desired_period;
                current_interval = interval(Duration::from_secs(current_period));
                current_interval.set_missed_tick_behavior(MissedTickBehavior::Skip);
            }
        }
    }

    async fn probe_once_and_emit(&self) {
        let started = Instant::now();
        let result = self.client.head(PROBE_URL).send().await;
        let elapsed_ms = started.elapsed().as_millis() as u32;
        let (status, error_kind) = classify_response(&result);

        match (&result, status) {
            (Ok(_), NetworkStatus::Online) => {
                info!(
                    "network probe ok: status=online latency_ms={}",
                    elapsed_ms
                );
            }
            (Ok(resp), NetworkStatus::ServerDegraded) => {
                warn!(
                    "network probe degraded: http_status={} elapsed_ms={}",
                    resp.status(),
                    elapsed_ms
                );
            }
            (Err(err), _) => {
                warn!(
                    "network probe failed: kind={:?} error=\"{}\" elapsed_ms={}",
                    error_kind, err, elapsed_ms
                );
            }
            _ => {}
        }

        let latency_ms = if matches!(status, NetworkStatus::Online | NetworkStatus::ServerDegraded)
        {
            Some(elapsed_ms)
        } else {
            None
        };
        let snapshot = NetworkSnapshot {
            status,
            last_check_at_ms: Utc::now().timestamp_millis(),
            latency_ms,
            error_kind,
        };

        let changed = {
            let mut guard = self.snapshot.lock().unwrap();
            let changed = match guard.as_ref() {
                Some(prev) => prev.status != snapshot.status,
                None => true, // first probe always emits
            };
            *guard = Some(snapshot.clone());
            changed
        };

        if changed {
            let prev_status_str = match snapshot.status {
                NetworkStatus::Online => "online",
                NetworkStatus::Offline => "offline",
                NetworkStatus::ServerDegraded => "server_degraded",
            };
            info!("network status changed -> {}", prev_status_str);
            let payload = serde_json::to_value(&snapshot).unwrap_or(json!({}));
            if let Err(e) = self.host.emit_legacy_event("network:status", payload) {
                warn!("emit network:status failed: {}", e);
            }
        }
    }
}
```

- [ ] **Step 2: 编译验证**

Run: `cd src-tauri && cargo build --lib 2>&1 | tail -40`
Expected: 编译通过。可能要补 `use crate::runtime::network::state::NetworkStatus;` 导入。修到编译过为止。

- [ ] **Step 3: 提交**

```bash
git add src-tauri/src/runtime/network/probe.rs
git commit -m "feat(network): NetworkProbe loop with backoff + force probe channel

Spec §4.3 — 30s default interval; 10s on Offline; recover to 30s after
3 consecutive non-Offline probes. MissedTickBehavior::Skip avoids macOS
sleep-wake tick avalanche. Force-probe throttled to 1/sec."
```

---

## Task 4: NetworkProbe wiremock 集成测试

**Files:**
- Create: `src-tauri/tests/network_probe_integration_test.rs`

- [ ] **Step 1: 确认 wiremock 在 dev-dependencies**

Run: `grep -n "wiremock" src-tauri/Cargo.toml`
Expected: 已存在（仓库其他 test 用到）。如果没有，加 `wiremock = "0.6"` 到 `[dev-dependencies]`。

- [ ] **Step 2: 写集成测试**

Create `src-tauri/tests/network_probe_integration_test.rs`:

```rust
//! 用 wiremock 起本地 server，直接调 NetworkProbe 私有 helper 验证状态分类。
//!
//! 这个测试不去 spawn run_loop（避免无限循环），而是直接构造单次 probe 场景。

use std::sync::Arc;

use serde_json::Value;
use wiremock::matchers::method;
use wiremock::{Mock, MockServer, ResponseTemplate};

mod common {
    use std::sync::{Arc, Mutex};

    use aijia::transport::runtime_host::RuntimeHost;
    use anyhow::Result;
    use aijia::runtime::agent::{
        AgentNameRegistry, CancellationRegistry, InboxRegistry, LeadIdleSupervisor, TeamRegistry,
    };

    /// 测试用 host：收集所有 emit_legacy_event 调用。
    pub struct CapturingHost {
        pub events: Mutex<Vec<(String, serde_json::Value)>>,
    }

    impl CapturingHost {
        pub fn new() -> Arc<Self> {
            Arc::new(Self { events: Mutex::new(Vec::new()) })
        }
    }

    impl RuntimeHost for CapturingHost {
        fn emit_legacy_event(&self, name: &str, payload: serde_json::Value) -> Result<()> {
            self.events.lock().unwrap().push((name.to_string(), payload));
            Ok(())
        }
        fn team_registry(&self) -> Arc<TeamRegistry> {
            Arc::new(TeamRegistry::new())
        }
        fn agent_names(&self) -> Arc<AgentNameRegistry> {
            Arc::new(AgentNameRegistry::new())
        }
        fn inbox_registry(&self) -> Arc<InboxRegistry> {
            Arc::new(InboxRegistry::new())
        }
        fn lead_idle_supervisor(&self) -> Arc<LeadIdleSupervisor> {
            Arc::new(LeadIdleSupervisor::new())
        }
        fn cancellation_registry(&self) -> Arc<CancellationRegistry> {
            Arc::new(CancellationRegistry::new())
        }
    }
}

use common::CapturingHost;

#[tokio::test]
async fn probe_200_emits_online_status_changed() {
    let mock_server = MockServer::start().await;
    Mock::given(method("HEAD"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&mock_server)
        .await;

    let host = CapturingHost::new();
    // 直接构造一个绑到 mock_server.uri() 的 probe（私有字段，所以走测试-only constructor）
    let probe = aijia::runtime::network::probe::NetworkProbe::new_for_test(
        host.clone(),
        mock_server.uri(),
    );
    probe.probe_once_for_test().await;

    let events = host.events.lock().unwrap();
    assert_eq!(events.len(), 1, "first probe emits one event");
    assert_eq!(events[0].0, "network:status");
    let status = events[0].1.get("status").and_then(Value::as_str).unwrap();
    assert_eq!(status, "online");
}

#[tokio::test]
async fn probe_500_emits_server_degraded() {
    let mock_server = MockServer::start().await;
    Mock::given(method("HEAD"))
        .respond_with(ResponseTemplate::new(503))
        .mount(&mock_server)
        .await;

    let host = CapturingHost::new();
    let probe = aijia::runtime::network::probe::NetworkProbe::new_for_test(
        host.clone(),
        mock_server.uri(),
    );
    probe.probe_once_for_test().await;

    let events = host.events.lock().unwrap();
    assert_eq!(events[0].1.get("status").and_then(Value::as_str).unwrap(), "server-degraded");
}

#[tokio::test]
async fn probe_connect_refused_emits_offline() {
    // 不启 mock server，直接打一个肯定没人监听的端口。
    let host = CapturingHost::new();
    let probe = aijia::runtime::network::probe::NetworkProbe::new_for_test(
        host.clone(),
        "http://127.0.0.1:1".to_string(),
    );
    probe.probe_once_for_test().await;

    let events = host.events.lock().unwrap();
    let status = events[0].1.get("status").and_then(Value::as_str).unwrap();
    assert_eq!(status, "offline");
    assert!(events[0].1.get("errorKind").is_some());
}

#[tokio::test]
async fn probe_dedups_unchanged_status() {
    let mock_server = MockServer::start().await;
    Mock::given(method("HEAD"))
        .respond_with(ResponseTemplate::new(200))
        .mount(&mock_server)
        .await;

    let host = CapturingHost::new();
    let probe = aijia::runtime::network::probe::NetworkProbe::new_for_test(
        host.clone(),
        mock_server.uri(),
    );
    probe.probe_once_for_test().await;
    probe.probe_once_for_test().await;
    probe.probe_once_for_test().await;

    let events = host.events.lock().unwrap();
    assert_eq!(events.len(), 1, "unchanged status must not re-emit");
}
```

- [ ] **Step 3: 在 NetworkProbe 加测试 constructor**

Modify `src-tauri/src/runtime/network/probe.rs`，append at end of `impl NetworkProbe`:

```rust
    #[cfg(any(test, feature = "test-support"))]
    pub fn new_for_test(host: Arc<dyn RuntimeHost>, probe_url: String) -> Arc<Self> {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(HEAD_TIMEOUT_SECS))
            .build()
            .expect("network probe reqwest client");
        let (force_tx, force_rx) = mpsc::channel(4);
        Arc::new(Self {
            client,
            host,
            snapshot: Arc::new(Mutex::new(None)),
            force_tx,
            force_rx: Arc::new(Mutex::new(Some(force_rx))),
            last_force_at_ms: Arc::new(Mutex::new(0)),
            probe_url_override: Some(probe_url),
        })
    }

    #[cfg(any(test, feature = "test-support"))]
    pub async fn probe_once_for_test(&self) {
        self.probe_once_and_emit().await;
    }
```

Then modify the struct to add `probe_url_override: Option<String>` field and adjust `probe_once_and_emit` to use:

```rust
let url = self.probe_url_override.as_deref().unwrap_or(PROBE_URL);
let result = self.client.head(url).send().await;
```

And update `NetworkProbe::new`：

```rust
pub fn new(host: Arc<dyn RuntimeHost>) -> Arc<Self> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(HEAD_TIMEOUT_SECS))
        .build()
        .expect("network probe reqwest client");
    let (force_tx, force_rx) = mpsc::channel(4);
    Arc::new(Self {
        client,
        host,
        snapshot: Arc::new(Mutex::new(None)),
        force_tx,
        force_rx: Arc::new(Mutex::new(Some(force_rx))),
        last_force_at_ms: Arc::new(Mutex::new(0)),
        probe_url_override: None,
    })
}
```

Update `spawn(self: Arc<Self>)` signature stays the same (already takes `Arc<Self>`).

- [ ] **Step 4: 运行集成测试**

Run: `cd src-tauri && cargo test --test network_probe_integration_test -- --nocapture`
Expected: 4 tests PASS。如果 `aijia` crate name 不对，用 `cargo pkgid` 确认，改第一行 `use aijia::...` 为正确 crate name（很可能是 `aijia`，从 `Cargo.toml` `[package].name` 看）。

- [ ] **Step 5: 提交**

```bash
git add src-tauri/tests/network_probe_integration_test.rs src-tauri/src/runtime/network/probe.rs
git commit -m "test(network): wiremock integration tests for NetworkProbe

Covers Spec §9 — 200/503/connect-refused → online/degraded/offline,
state dedup (unchanged status emits zero additional events)."
```

---

## Task 5: 架构回归测试（守 no use tauri::*）

**Files:**
- Create: `src-tauri/tests/review_network_module.rs`

- [ ] **Step 1: 写架构断言测试**

Create `src-tauri/tests/review_network_module.rs`:

```rust
//! 守护 CLAUDE.md 决策 #4：runtime/network/ 不得 use tauri::*。

use std::fs;
use std::path::PathBuf;

#[test]
fn network_module_does_not_use_tauri() {
    let crate_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let dir = crate_root.join("src/runtime/network");
    assert!(dir.exists(), "runtime/network module should exist");

    let mut bad = Vec::new();
    for entry in fs::read_dir(&dir).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().and_then(|s| s.to_str()) != Some("rs") {
            continue;
        }
        let content = fs::read_to_string(&path).unwrap();
        // Strip block comments (very crude — sufficient for our convention).
        // Then check for `use tauri::` or `use tauri ;`.
        for (i, line) in content.lines().enumerate() {
            let trimmed = line.trim_start();
            if trimmed.starts_with("//") {
                continue;
            }
            if trimmed.contains("use tauri::") || trimmed.contains("use tauri;") {
                bad.push(format!("{}:{}: {}", path.display(), i + 1, line));
            }
        }
    }

    assert!(
        bad.is_empty(),
        "runtime/network/ must not import tauri (CLAUDE.md #4):\n{}",
        bad.join("\n")
    );
}
```

- [ ] **Step 2: 运行**

Run: `cd src-tauri && cargo test --test review_network_module -- --nocapture`
Expected: PASS。

- [ ] **Step 3: 提交**

```bash
git add src-tauri/tests/review_network_module.rs
git commit -m "test(review): runtime/network must not depend on tauri

Guards CLAUDE.md decision #4 — runtime layer is transport-neutral."
```

---

## Task 6: Tauri commands — network_get_status / network_force_probe

**Files:**
- Create: `src-tauri/src/transport/tauri_commands/network.rs`
- Modify: `src-tauri/src/transport/tauri_commands/mod.rs`

- [ ] **Step 1: 写 commands**

Create `src-tauri/src/transport/tauri_commands/network.rs`:

```rust
use std::sync::Arc;

use serde_json::json;
use tauri::State;

use crate::runtime::network::probe::NetworkProbe;
use crate::runtime::network::state::NetworkSnapshot;

/// Return the latest cached snapshot, or null if no probe has completed yet.
#[tauri::command]
pub async fn network_get_status(
    probe: State<'_, Arc<NetworkProbe>>,
) -> Result<Option<NetworkSnapshot>, String> {
    Ok(probe.snapshot())
}

/// Trigger an immediate probe. Returns true if a probe was queued, false if
/// throttled (called within 1 second of the previous force probe).
#[tauri::command]
pub async fn network_force_probe(
    probe: State<'_, Arc<NetworkProbe>>,
) -> Result<serde_json::Value, String> {
    let triggered = probe.request_force_probe();
    Ok(json!({ "triggered": triggered }))
}
```

Modify `src-tauri/src/transport/tauri_commands/mod.rs`, add to the `pub mod` list (alphabetical):

```rust
pub mod network;
```

- [ ] **Step 2: 注册 invoke handler 和 manage state**

Modify `src-tauri/src/lib.rs`. Find the `setup()` block where other `app.manage(...)` calls live (around line 138-710). Add at the end of setup (after `runtime_manager` is managed):

```rust
// --- Network probe (spec docs/superpowers/specs/2026-05-26-network-detection-design.md) ---
let probe_host: Arc<dyn crate::transport::runtime_host::RuntimeHost> =
    /* whatever existing host instance is in scope — likely available via
       a previously-created RuntimeHost. If not yet created, use the same
       TauriRuntimeHost adapter that handles emit_legacy_event elsewhere. */
    runtime_host.clone();
let network_probe = Arc::new(crate::runtime::network::probe::NetworkProbe::new(probe_host));
app.manage(network_probe.clone());
let probe_for_spawn = network_probe.clone();
tokio::spawn(async move {
    probe_for_spawn.spawn();
});
```

> **执行注意**：上面的 `runtime_host` 实际变量名要在执行时通过 `grep -n "RuntimeHost\|emit_legacy_event" src-tauri/src/lib.rs` 找到。如果 lib.rs 里没有现成的 `Arc<dyn RuntimeHost>` 引用，需要先看 `transport/tauri_event_adapter.rs` 是怎么 manage 它的；通常存在一个 `TauriRuntimeHost { app_handle }` 实现，被 `app.manage(host.clone())`。直接复用即可。如果没有现成宿主：在本 task 内先新建一个最小宿主 `TauriRuntimeHost { app_handle: AppHandle }` 只实现 `emit_legacy_event`（其它 RuntimeHost 方法保留 default impl 或 panic—— 仅探针专用）。

Also add the commands to the `tauri::generate_handler![...]` macro invocation (search for it in `lib.rs`, look for `chat_*` and `runtime_*` commands already listed):

```rust
crate::transport::tauri_commands::network::network_get_status,
crate::transport::tauri_commands::network::network_force_probe,
```

- [ ] **Step 3: 编译**

Run: `cd src-tauri && cargo build --lib 2>&1 | tail -30`
Expected: 编译通过。如果 RuntimeHost 实例化卡住，参照 `transport/tauri_event_adapter.rs` 已有的 host 注册位置抄一份。

- [ ] **Step 4: 提交**

```bash
git add src-tauri/src/transport/tauri_commands/network.rs \
        src-tauri/src/transport/tauri_commands/mod.rs \
        src-tauri/src/lib.rs
git commit -m "feat(network): expose network_get_status / network_force_probe commands

Spec §5.3 — frontend can fetch initial snapshot on boot and trigger
manual retry from the popover. Setup() spawns the long-running probe."
```

---

## Task 7: 前端 networkStore

**Files:**
- Create: `src/stores/networkStore.ts`
- Create: `src/stores/networkStore.test.ts`

- [ ] **Step 1: 写失败测试**

Create `src/stores/networkStore.test.ts`:

```ts
import { beforeEach, describe, expect, it } from 'vitest'

import { useNetworkStore } from './networkStore'

describe('networkStore', () => {
  beforeEach(() => {
    useNetworkStore.setState({
      status: 'unknown',
      lastOnlineAt: null,
      lastCheckAt: null,
      latencyMs: null,
      errorKind: null,
    })
  })

  it('starts in unknown state', () => {
    expect(useNetworkStore.getState().status).toBe('unknown')
  })

  it('applies online event', () => {
    useNetworkStore.getState().applyEvent({
      status: 'online',
      lastCheckAtMs: 1000,
      latencyMs: 42,
      errorKind: null,
    })
    const s = useNetworkStore.getState()
    expect(s.status).toBe('online')
    expect(s.lastOnlineAt).toBe(1000)
    expect(s.lastCheckAt).toBe(1000)
    expect(s.latencyMs).toBe(42)
    expect(s.errorKind).toBeNull()
  })

  it('applying offline does not overwrite lastOnlineAt', () => {
    useNetworkStore.getState().applyEvent({
      status: 'online',
      lastCheckAtMs: 1000,
      latencyMs: 42,
      errorKind: null,
    })
    useNetworkStore.getState().applyEvent({
      status: 'offline',
      lastCheckAtMs: 2000,
      latencyMs: null,
      errorKind: 'dns',
    })
    const s = useNetworkStore.getState()
    expect(s.status).toBe('offline')
    expect(s.lastOnlineAt).toBe(1000) // preserved
    expect(s.lastCheckAt).toBe(2000)
    expect(s.errorKind).toBe('dns')
  })

  it('server-degraded preserves lastOnlineAt', () => {
    useNetworkStore.getState().applyEvent({
      status: 'online',
      lastCheckAtMs: 1000,
      latencyMs: 42,
      errorKind: null,
    })
    useNetworkStore.getState().applyEvent({
      status: 'server-degraded',
      lastCheckAtMs: 2000,
      latencyMs: 88,
      errorKind: null,
    })
    expect(useNetworkStore.getState().lastOnlineAt).toBe(1000)
  })
})
```

- [ ] **Step 2: 运行测试看失败**

Run: `pnpm exec vitest run src/stores/networkStore.test.ts`
Expected: FAIL — file does not exist.

- [ ] **Step 3: 写 store**

Create `src/stores/networkStore.ts`:

```ts
import { create } from 'zustand'

import { networkForceProbe } from '@/lib/tauri'
import type { NetworkErrorKind, NetworkStatus, NetworkStatusPayload } from '@/lib/tauri'

interface NetworkState {
  status: NetworkStatus | 'unknown'
  lastOnlineAt: number | null
  lastCheckAt: number | null
  latencyMs: number | null
  errorKind: NetworkErrorKind | null

  applyEvent: (payload: NetworkStatusPayload) => void
  forceProbe: () => Promise<void>
}

export const useNetworkStore = create<NetworkState>((set, get) => ({
  status: 'unknown',
  lastOnlineAt: null,
  lastCheckAt: null,
  latencyMs: null,
  errorKind: null,

  applyEvent: (payload) => {
    const prevOnlineAt = get().lastOnlineAt
    set({
      status: payload.status,
      lastCheckAt: payload.lastCheckAtMs,
      latencyMs: payload.latencyMs,
      errorKind: payload.errorKind,
      lastOnlineAt:
        payload.status === 'online' ? payload.lastCheckAtMs : prevOnlineAt,
    })
    if (payload.errorKind) {
      console.debug(
        '[networkStore] offline errorKind=%s lastCheckAtMs=%d',
        payload.errorKind,
        payload.lastCheckAtMs,
      )
    }
  },

  forceProbe: async () => {
    await networkForceProbe()
  },
}))
```

- [ ] **Step 4: 运行测试**

Run: `pnpm exec vitest run src/stores/networkStore.test.ts`
Expected: 失败 — `@/lib/tauri` 还没导出这些类型/函数。下一 task 处理。

- [ ] **Step 5: 提交（带未通过的测试也提交，以便后续修复对齐）**

不在这步提交。先做 Task 8。

---

## Task 8: 在 src/lib/tauri.ts 增加 network event 类型与 invoke 包装

**Files:**
- Modify: `src/lib/tauri.ts`

- [ ] **Step 1: 加常量和类型**

Modify `src/lib/tauri.ts`. 在 `TAURI_EVENTS` 对象内（约 line 70 前）增加：

```ts
  NETWORK_STATUS: 'network:status',
```

在「Event Payload Types」段（约 line 76 后）加：

```ts
export type NetworkStatus = 'online' | 'offline' | 'server-degraded'
export type NetworkErrorKind = 'timeout' | 'dns' | 'connect_refused' | 'tls' | 'other'

export interface NetworkStatusPayload {
  status: NetworkStatus
  lastCheckAtMs: number
  latencyMs: number | null
  errorKind: NetworkErrorKind | null
}
```

在文件适合的导出区（参照已有 `sendMessage` 位置）加：

```ts
export async function networkGetStatus(): Promise<NetworkStatusPayload | null> {
  return invoke<NetworkStatusPayload | null>('network_get_status')
}

export async function networkForceProbe(): Promise<{ triggered: boolean }> {
  return invoke<{ triggered: boolean }>('network_force_probe')
}
```

- [ ] **Step 2: 跑 networkStore 测试**

Run: `pnpm exec vitest run src/stores/networkStore.test.ts`
Expected: 4 tests PASS。

- [ ] **Step 3: 提交 Task 7 + 8 一起**

```bash
git add src/lib/tauri.ts src/stores/networkStore.ts src/stores/networkStore.test.ts
git commit -m "feat(network): networkStore + tauri.ts NETWORK_STATUS event types

Spec §5.2 / §6.1 — TS types mirror Rust NetworkSnapshot camelCase
serialization; store preserves lastOnlineAt across state transitions."
```

---

## Task 9: useNetworkStatus hook（订阅事件）

**Files:**
- Create: `src/hooks/useNetworkStatus.ts`
- Modify: `src/App.tsx`

- [ ] **Step 1: 写 hook**

Create `src/hooks/useNetworkStatus.ts`:

```ts
import { useEffect } from 'react'
import { listen } from '@tauri-apps/api/event'

import { TAURI_EVENTS, networkGetStatus } from '@/lib/tauri'
import type { NetworkStatusPayload } from '@/lib/tauri'
import { useNetworkStore } from '@/stores/networkStore'

/**
 * 挂载一次（App 顶层）。拉取启动初值 + 订阅 network:status event。
 */
export function useNetworkStatus() {
  useEffect(() => {
    let cancelled = false
    let unlisten: (() => void) | null = null

    void (async () => {
      try {
        const initial = await networkGetStatus()
        if (!cancelled && initial) {
          useNetworkStore.getState().applyEvent(initial)
        }
      } catch (err) {
        console.warn('[useNetworkStatus] initial fetch failed:', err)
      }

      try {
        const handle = await listen<NetworkStatusPayload>(
          TAURI_EVENTS.NETWORK_STATUS,
          (event) => {
            useNetworkStore.getState().applyEvent(event.payload)
          },
        )
        if (cancelled) {
          handle()
        } else {
          unlisten = handle
        }
      } catch (err) {
        console.warn('[useNetworkStatus] listen failed:', err)
      }
    })()

    return () => {
      cancelled = true
      if (unlisten) unlisten()
    }
  }, [])
}
```

- [ ] **Step 2: 在 App.tsx 顶层调用**

Modify `src/App.tsx`. 先看一下文件顶层：

```bash
grep -n "function App\|export.*App" src/App.tsx | head -5
```

在 `App` 组件函数体最顶部加：

```tsx
import { useNetworkStatus } from '@/hooks/useNetworkStatus'

function App() {
  useNetworkStatus()
  // ... existing body
}
```

如果 App.tsx 已经有一堆 hook 顺序，加在第一个 useEffect 之前。

- [ ] **Step 3: 手测**

Run: `pnpm tauri:dev`，看 webview console 是否出现 `[networkStore]` debug 或没有任何 listen 报错。
Expected: 没有 listener 报错，devtools network 不出现额外异常请求。

- [ ] **Step 4: 提交**

```bash
git add src/hooks/useNetworkStatus.ts src/App.tsx
git commit -m "feat(network): useNetworkStatus hook subscribes to network:status

Spec §6.2 — mount once at App root, fetch initial via networkGetStatus,
then listen() for live updates. Unknown→never-rendered guard handled
by NetworkStatusIndicator (next task)."
```

---

## Task 10: i18n 文案

**Files:**
- Modify: `src/i18n/zh-CN.json`
- Modify: `src/i18n/en-US.json`

- [ ] **Step 1: zh-CN**

Modify `src/i18n/zh-CN.json`, 在顶层对象中加入 `network` 命名空间（参考已有 key 的缩进风格）：

```json
"network": {
  "offlineBadge": "网络不通",
  "degradedBadge": "AI 服务暂时不可用",
  "popoverOfflineTitle": "当前无法连接到网络",
  "popoverOfflineDesc": "请检查 WiFi、有线网络或 VPN 是否正常，然后点击「重试」。",
  "popoverDegradedTitle": "AI 服务暂时无法访问",
  "popoverDegradedDesc": "网络已连通，但 AI 服务端暂时异常，请稍后重试。",
  "lastOnline": "上次连接成功：{{time}}",
  "retryNow": "重试",
  "sendWhileOfflineTitle": "网络不通",
  "sendWhileOfflineDesc": "消息可能发送失败，请检查网络后重试。"
}
```

- [ ] **Step 2: en-US**

Modify `src/i18n/en-US.json`:

```json
"network": {
  "offlineBadge": "No network",
  "degradedBadge": "AI service unavailable",
  "popoverOfflineTitle": "Can't connect to the network",
  "popoverOfflineDesc": "Check your Wi-Fi, wired connection, or VPN, then click Retry.",
  "popoverDegradedTitle": "AI service is temporarily unavailable",
  "popoverDegradedDesc": "Your network is fine, but the AI service is having issues. Please retry shortly.",
  "lastOnline": "Last online: {{time}}",
  "retryNow": "Retry",
  "sendWhileOfflineTitle": "No network",
  "sendWhileOfflineDesc": "Sending may fail. Please check your network and retry."
}
```

- [ ] **Step 3: 验证 JSON 有效**

Run: `pnpm exec tsc --noEmit && node -e "require('./src/i18n/zh-CN.json'); require('./src/i18n/en-US.json'); console.log('ok')"`
Expected: `ok`，无 SyntaxError。

- [ ] **Step 4: 提交**

```bash
git add src/i18n/zh-CN.json src/i18n/en-US.json
git commit -m "i18n(network): add user-facing copy (zh-CN + en-US)

Spec §7 — \"网络不通\" / \"No network\" plain-language phrasing;
technical errorKind (dns/tls/timeout) stays in logs only."
```

---

## Task 11: NetworkStatusIndicator 组件 + 测试

**Files:**
- Create: `src/components/shell/NetworkStatusIndicator.tsx`
- Create: `src/components/shell/__tests__/NetworkStatusIndicator.test.tsx`

- [ ] **Step 1: 写失败测试**

先看 popover 是否有现成组件：

```bash
ls src/components/ui/popover* 2>/dev/null || ls src/components/common/AppDropdown* 2>/dev/null
```

Create `src/components/shell/__tests__/NetworkStatusIndicator.test.tsx`:

```tsx
import { render, screen, fireEvent } from '@testing-library/react'
import { beforeEach, describe, expect, it, vi } from 'vitest'

import { useNetworkStore } from '@/stores/networkStore'

import { NetworkStatusIndicator } from '../NetworkStatusIndicator'

vi.mock('react-i18next', () => ({
  useTranslation: () => ({
    t: (key: string) => key,
  }),
}))

describe('NetworkStatusIndicator', () => {
  beforeEach(() => {
    useNetworkStore.setState({
      status: 'unknown',
      lastOnlineAt: null,
      lastCheckAt: null,
      latencyMs: null,
      errorKind: null,
    })
  })

  it('renders nothing when status is unknown', () => {
    const { container } = render(<NetworkStatusIndicator />)
    expect(container.firstChild).toBeNull()
  })

  it('renders nothing when online', () => {
    useNetworkStore.setState({ status: 'online' })
    const { container } = render(<NetworkStatusIndicator />)
    expect(container.firstChild).toBeNull()
  })

  it('renders offline indicator when offline', () => {
    useNetworkStore.setState({ status: 'offline', errorKind: 'dns' })
    render(<NetworkStatusIndicator />)
    expect(screen.getByRole('button', { name: /network\.offlineBadge/i })).toBeInTheDocument()
  })

  it('renders degraded indicator when server-degraded', () => {
    useNetworkStore.setState({ status: 'server-degraded' })
    render(<NetworkStatusIndicator />)
    expect(screen.getByRole('button', { name: /network\.degradedBadge/i })).toBeInTheDocument()
  })

  it('calls forceProbe when retry button clicked', async () => {
    const forceProbe = vi.fn().mockResolvedValue(undefined)
    useNetworkStore.setState({ status: 'offline', forceProbe })
    render(<NetworkStatusIndicator />)
    fireEvent.click(screen.getByRole('button', { name: /network\.offlineBadge/i }))
    const retry = await screen.findByRole('button', { name: /network\.retryNow/i })
    fireEvent.click(retry)
    expect(forceProbe).toHaveBeenCalledTimes(1)
  })
})
```

- [ ] **Step 2: 运行测试看失败**

Run: `pnpm exec vitest run src/components/shell/__tests__/NetworkStatusIndicator.test.tsx`
Expected: FAIL — component does not exist.

- [ ] **Step 3: 写组件**

Create `src/components/shell/NetworkStatusIndicator.tsx`:

```tsx
import { CloudOff, WifiOff } from 'lucide-react'
import { useState } from 'react'
import { useTranslation } from 'react-i18next'

import { Button } from '@/components/ui/button'
import {
  Popover,
  PopoverContent,
  PopoverTrigger,
} from '@/components/ui/popover'
import { useNetworkStore } from '@/stores/networkStore'

export function NetworkStatusIndicator() {
  const { t } = useTranslation()
  const status = useNetworkStore((s) => s.status)
  const lastOnlineAt = useNetworkStore((s) => s.lastOnlineAt)
  const forceProbe = useNetworkStore((s) => s.forceProbe)
  const [open, setOpen] = useState(false)

  if (status === 'unknown' || status === 'online') {
    return null
  }

  const isOffline = status === 'offline'
  const Icon = isOffline ? WifiOff : CloudOff
  const wrapperCls = isOffline
    ? 'bg-destructive/12 text-destructive'
    : 'bg-muted text-muted-foreground'
  const badgeLabel = isOffline ? t('network.offlineBadge') : t('network.degradedBadge')
  const popTitle = isOffline
    ? t('network.popoverOfflineTitle')
    : t('network.popoverDegradedTitle')
  const popDesc = isOffline
    ? t('network.popoverOfflineDesc')
    : t('network.popoverDegradedDesc')

  return (
    <Popover open={open} onOpenChange={setOpen}>
      <PopoverTrigger asChild>
        <button
          type="button"
          aria-label={badgeLabel}
          className={`inline-flex items-center justify-center rounded-full p-[6px] ${wrapperCls}`}
        >
          <Icon className="h-3.5 w-3.5" />
        </button>
      </PopoverTrigger>
      <PopoverContent
        align="end"
        className="w-72 space-y-3 shadow-[var(--shadow-popover)]"
      >
        <div className="space-y-1">
          <div className="text-sm font-medium text-foreground">{popTitle}</div>
          <div className="text-xs text-muted-foreground">{popDesc}</div>
        </div>
        {lastOnlineAt ? (
          <div className="text-xs text-muted-foreground">
            {t('network.lastOnline', {
              time: new Date(lastOnlineAt).toLocaleString(),
            })}
          </div>
        ) : null}
        <div className="flex justify-end">
          <Button
            type="button"
            size="sm"
            variant="secondary"
            onClick={() => {
              void forceProbe()
            }}
          >
            {t('network.retryNow')}
          </Button>
        </div>
      </PopoverContent>
    </Popover>
  )
}
```

> **执行注意**：如果 `@/components/ui/popover` 不存在，先确认仓库是否用 `AppDropdown` 之类的替代——执行时 grep `Radix.*Popover` / `radix-ui/react-popover` 找现成包装。如有缺失，沿用 shadcn 风格在 `src/components/ui/popover.tsx` 创建一个最小包装（依赖应已存在 `@radix-ui/react-popover`，看 `package.json` 验证）。如果连 `@radix-ui/react-popover` 都没有，**用 `AppDropdown` 或临时降级为内联 `<details>`**，并在 commit message 注明。

- [ ] **Step 4: 运行测试**

Run: `pnpm exec vitest run src/components/shell/__tests__/NetworkStatusIndicator.test.tsx`
Expected: 5 tests PASS。

- [ ] **Step 5: 提交**

```bash
git add src/components/shell/NetworkStatusIndicator.tsx \
        src/components/shell/__tests__/NetworkStatusIndicator.test.tsx
git commit -m "feat(network): NetworkStatusIndicator (badge + retry popover)

Spec §6.3 — lucide WifiOff/CloudOff icons driven by currentColor and
theme variables (text-destructive / text-muted-foreground); never renders
in online/unknown to avoid cold-start flicker."
```

---

## Task 12: 把 Indicator 挂到 ChatTopBar / PageTopBar

**Files:**
- Modify: `src/components/shell/ChatTopBar.tsx`
- Modify: `src/components/shell/PageTopBar.tsx`

- [ ] **Step 1: 在 ChatTopBar 右侧渲染**

先看 ChatTopBar 当前结构（line 55-130 区间）。找右侧操作区（通常是 `flex justify-between` 的右边那块 div 或 `ml-auto` 容器）。

Modify `src/components/shell/ChatTopBar.tsx`，import 顶部加：

```tsx
import { NetworkStatusIndicator } from './NetworkStatusIndicator'
```

在 JSX 右侧操作区（最靠近窗口控制按钮的位置）插入：

```tsx
<NetworkStatusIndicator />
```

- [ ] **Step 2: 在 PageTopBar 右侧渲染**

同样操作 `src/components/shell/PageTopBar.tsx`：

```tsx
import { NetworkStatusIndicator } from './NetworkStatusIndicator'
// 在右侧 actions 区插入 <NetworkStatusIndicator />
```

- [ ] **Step 3: 跑现有 ChatTopBar test 看是否破坏**

Run: `pnpm exec vitest run src/components/shell/ChatTopBar.test.tsx`
Expected: PASS（Indicator 在 status=unknown 时 render null，不影响 snapshot）。

- [ ] **Step 4: 手测**

Run: `pnpm tauri:dev`，断网（关 WiFi）→ 等 ≤ 30s → 顶栏右侧应出现红色 `WifiOff` 角标。点击展开 popover → 看到「当前无法连接到网络」+ 重试按钮。
Expected: 视觉如预期；重试按钮触发后 webview console 看到一次 `[networkStore]` debug。

- [ ] **Step 5: 提交**

```bash
git add src/components/shell/ChatTopBar.tsx src/components/shell/PageTopBar.tsx
git commit -m "feat(network): mount NetworkStatusIndicator in ChatTopBar/PageTopBar"
```

---

## Task 13: useOfflineSendWarning hook + 测试

**Files:**
- Create: `src/hooks/useOfflineSendWarning.ts`
- Create: `src/hooks/useOfflineSendWarning.test.tsx`

- [ ] **Step 1: 写失败测试**

Create `src/hooks/useOfflineSendWarning.test.tsx`:

```tsx
import { act, renderHook } from '@testing-library/react'
import { beforeEach, describe, expect, it, vi } from 'vitest'

import { useNetworkStore } from '@/stores/networkStore'
import { useNotificationStore } from '@/stores/notificationStore'

import { useOfflineSendWarning } from './useOfflineSendWarning'

vi.mock('react-i18next', () => ({
  useTranslation: () => ({
    t: (key: string) => key,
  }),
}))

describe('useOfflineSendWarning', () => {
  beforeEach(() => {
    useNetworkStore.setState({
      status: 'unknown',
      lastOnlineAt: null,
      lastCheckAt: null,
      latencyMs: null,
      errorKind: null,
    })
    useNotificationStore.setState({ notifications: [] })
  })

  it('does not push toast when online', () => {
    useNetworkStore.setState({ status: 'online' })
    const { result } = renderHook(() => useOfflineSendWarning())
    act(() => {
      result.current.warnIfOffline()
    })
    expect(useNotificationStore.getState().notifications).toHaveLength(0)
  })

  it('does not push toast when server-degraded (LLM error path handles it)', () => {
    useNetworkStore.setState({ status: 'server-degraded' })
    const { result } = renderHook(() => useOfflineSendWarning())
    act(() => {
      result.current.warnIfOffline()
    })
    expect(useNotificationStore.getState().notifications).toHaveLength(0)
  })

  it('pushes toast when offline', () => {
    useNetworkStore.setState({ status: 'offline' })
    const { result } = renderHook(() => useOfflineSendWarning())
    act(() => {
      result.current.warnIfOffline()
    })
    const notifs = useNotificationStore.getState().notifications
    expect(notifs).toHaveLength(1)
    expect(notifs[0].level).toBe('warning')
    expect(notifs[0].title).toBe('network.sendWhileOfflineTitle')
  })

  it('pushes once per call (caller decides cadence)', () => {
    useNetworkStore.setState({ status: 'offline' })
    const { result } = renderHook(() => useOfflineSendWarning())
    act(() => {
      result.current.warnIfOffline()
      result.current.warnIfOffline()
    })
    expect(useNotificationStore.getState().notifications).toHaveLength(2)
  })
})
```

- [ ] **Step 2: 运行测试看失败**

Run: `pnpm exec vitest run src/hooks/useOfflineSendWarning.test.tsx`
Expected: FAIL — module not found.

- [ ] **Step 3: 写 hook**

Create `src/hooks/useOfflineSendWarning.ts`:

```ts
import { useCallback } from 'react'
import { useTranslation } from 'react-i18next'

import { useNetworkStore } from '@/stores/networkStore'
import { useNotificationStore } from '@/stores/notificationStore'

export function useOfflineSendWarning() {
  const { t } = useTranslation()

  const warnIfOffline = useCallback(() => {
    if (useNetworkStore.getState().status !== 'offline') return
    useNotificationStore.getState().push({
      context: 'toast',
      level: 'warning',
      title: t('network.sendWhileOfflineTitle'),
      message: t('network.sendWhileOfflineDesc'),
      actions: [],
      dismissible: true,
      autoHide: 6,
    })
  }, [t])

  return { warnIfOffline }
}
```

> 注意 `autoHide: 6`——`notificationStore` 的注释 `// seconds`，所以传 6 而不是 6000。

- [ ] **Step 4: 运行测试**

Run: `pnpm exec vitest run src/hooks/useOfflineSendWarning.test.tsx`
Expected: 4 tests PASS。

- [ ] **Step 5: 提交**

```bash
git add src/hooks/useOfflineSendWarning.ts src/hooks/useOfflineSendWarning.test.tsx
git commit -m "feat(network): useOfflineSendWarning — toast on send while offline

Spec §6.4 — fires only when status === 'offline'; server-degraded is
left to the existing classify_llm_error toast path to avoid double-talk."
```

---

## Task 14: 接入 useChat 发送路径

**Files:**
- Modify: `src/hooks/useChat.ts`

- [ ] **Step 1: 在 useChat 顶部消费 hook**

Modify `src/hooks/useChat.ts`. 在 import 区加：

```ts
import { useOfflineSendWarning } from './useOfflineSendWarning'
```

在 `useChat` 函数体（约 line 20 后已有 `sendMessage` import 的下方）找到 hook 初始化区域，加：

```ts
const { warnIfOffline } = useOfflineSendWarning()
```

在 line 405 的 `await sendMessage(...)` 之前一行加：

```ts
warnIfOffline()
```

最终 line 403-407 区域应类似：

```ts
console.log('[useChat] Calling sendMessage IPC, attachments:', files, 'willBeQueued:', willBeQueued)
warnIfOffline()
await sendMessage(conversationId, text, files, null, messageId, skillCommand)
console.log('[useChat] sendMessage IPC returned OK')
```

- [ ] **Step 2: 跑 useChat 测试**

Run: `pnpm exec vitest run src/hooks/useChat.test.ts`
Expected: PASS — 测试里 `useNetworkStore.status` 默认 `unknown`，`warnIfOffline` 不会 push 任何东西。

- [ ] **Step 3: 手测**

Run: `pnpm tauri:dev`，关 WiFi 等 ≤ 30s 出现红点，**离线状态下**输入消息按发送 → 应弹出黄色 toast「网络不通，消息可能发送失败」。
Expected: toast 出现一次；消息照常进入发送流程（后端 LLM 失败后会另外弹 classify_llm_error 的 toast，符合 spec §8）。

- [ ] **Step 4: 提交**

```bash
git add src/hooks/useChat.ts
git commit -m "feat(network): warn user when sending while offline

Spec §6.4 — non-blocking advisory toast; does not gate send_message."
```

---

## Task 15: 端到端手测 + 提交回归脚本

**Files:**
- Modify: `docs/superpowers/specs/2026-05-26-network-detection-design.md`（标记 Status=Done）

- [ ] **Step 1: 跑 spec §9.1 6 项手测**

1. `pnpm tauri:dev` 起来，断 WiFi → 30s 内顶栏出现红点 → ✅
2. 红点出现期间发消息 → 看到 toast「网络不通，消息可能发送失败」→ ✅
3. 恢复 WiFi → 红点消失（≤ 30s）→ ✅
4. 模拟 5xx：临时 `/etc/hosts` 加 `127.0.0.1 ai-tenant.renlijia.com`，本地起一个 `python3 -m http.server 80` 当 503（或用 wiremock CLI）→ 灰色 degraded 角标 → 发消息**不**弹 toast → ✅
5. `caffeinate -s` 或物理盖盖等 5 分钟唤醒 → 30s 内重新探测，不雪崩 → ✅
6. popover 重试按钮连点 5 次 → 节流：webview console 应能看到 `{ triggered: true }` + `{ triggered: false }` 的混合 → ✅

- [ ] **Step 2: 跑全套自动化测试**

Run（并发执行）：

```bash
pnpm exec vitest run src/stores/networkStore.test.ts src/hooks/useOfflineSendWarning.test.tsx src/components/shell/__tests__/NetworkStatusIndicator.test.tsx
cd src-tauri && cargo test --lib runtime::network -- --nocapture && cargo test --test network_probe_integration_test --test review_network_module -- --nocapture
```

Expected: 全部 PASS。

- [ ] **Step 3: 更新 spec 状态**

Modify `docs/superpowers/specs/2026-05-26-network-detection-design.md`，把首行 `**状态**：Draft（待评审）` 改成 `**状态**：Done`。

- [ ] **Step 4: 提交**

```bash
git add docs/superpowers/specs/2026-05-26-network-detection-design.md
git commit -m "docs(spec): mark network detection design as done

All 15 tasks implemented; auto tests pass; manual scripts §9.1 verified."
```

---

## Self-Review

Spec 与 plan 比对（fresh eyes）：

- ✅ §3.1/3.2 模块结构：Task 1-6 覆盖 Rust 模块，Task 7-14 覆盖前端，全部映射明确
- ✅ §4.1 reqwest 独立 client：Task 3 内 `reqwest::Client::builder().timeout(5s).build()`
- ✅ §4.2 三态分类：Task 2 的 classify_response 单元测试覆盖 200/401/500/502，Task 4 的集成测试覆盖 200/503/connect-refused
- ✅ §4.3 退避节奏：Task 3 `OFFLINE_INTERVAL_SECS=10` / `RECOVERY_SUCCESS_THRESHOLD=3` / `MissedTickBehavior::Skip`
- ✅ §4.5 退出竞态：tokio task 通过 `tokio::spawn` 启动，进程结束随 runtime drop（spec 已允许）
- ✅ §5 事件协议：Task 1（序列化）+ Task 8（TS 类型）+ Task 6（Tauri command）三处对齐
- ✅ §6.1 unknown 初值不渲染：Task 11 component 测试 + Task 13 hook 测试都验证
- ✅ §7 文案分层：Task 10 i18n key 用白话；Task 3 中 `tracing::warn!`/`info!` 用技术原文
- ✅ §8 协同：Task 13 测试明确 server-degraded 不弹 toast
- ✅ §9.1 手测脚本：Task 15 完整覆盖 6 项
- ✅ §10 风险缓解全部对应到具体实现

**类型一致性扫描**：`NetworkStatus` / `NetworkErrorKind` / `NetworkSnapshot` / `NetworkStatusPayload` 在 Task 1（Rust）和 Task 8（TS）字段一一对应；snake_case ↔ camelCase 由 `#[serde(rename_all = "camelCase")]` 处理；`applyEvent` 在 Task 7（store）/ Task 9（hook 调用方）/ Task 11（component 间接消费）/ Task 13（hook 消费）使用名一致。

**Placeholder 扫描**：无 TBD/TODO；Task 6 Step 2 关于 `runtime_host` 变量名有「执行注意」备注（明确说怎么定位），不是 TBD 而是 well-defined fallback；Task 11 popover 依赖兼容性有「执行注意」（已给三档降级路径）。

**Scope check**：Plan 总 15 tasks，每个 task 2-5 min 体量，DRY/TDD/frequent commit 都到位。
