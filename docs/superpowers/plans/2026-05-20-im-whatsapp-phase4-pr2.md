# Phase 4 WhatsApp PR2 — Bot 生命周期 + 凭证文件路径 + PairingState

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 把 PR1 的 `WhatsAppConnector` stub 升级为持有 Bot `JoinHandle` + `PairingState`，新建 `session.rs` 管理 session.db / .bak 路径，新建 `config.rs` 读写 `config.json` 元数据。**不**真起 Bot::run()（PR3 做），**不**写 begin/poll_registration（PR3 做）。

**Architecture:** 抄 [OpenClaw `extensions/whatsapp/src/`](https://github.com/openclaw/openclaw) 的形态——固定目录 `channels/whatsapp/`，三件套 `session.db / session.db.bak / config.json`，**无** `_pairing/` 临时路径，**无** rename，**无** race 防护（spec v3 §3）。`WhatsAppConnector` 持 `Arc<Mutex<Option<JoinHandle<()>>>>` + `Arc<Mutex<PairingState>>`；`stop()` 用 `JoinHandle::abort()`，wa-rs 没 graceful shutdown，这是唯一手段。

**Tech Stack:** `wa-rs = "0.2"`（PR1 已加依赖；PR2 不调它的 API，只准备脚手架）。`ChannelConfigStore::platform_dir(Platform::Whatsapp)` 是 channels/whatsapp 目录的现成入口（`src-tauri/src/connector/im/shared/config_store.rs:46`）。

**Spec 来源：** `docs/superpowers/specs/2026-05-18-im-whatsapp-phase4-design.md` §3（v3 OpenClaw-aligned）+ §10.2 PR2 行。

**与 spec v3 的偏离：**
- 无偏离。v3 spec 是这次 brainstorm 才写的，跟 plan 对齐。
- 但跟 v2 仍有差异（删了 `_pairing/`、删了 rename、4 状态 PairingState）——这些都已在 spec v3 §3.0 登记。

---

## File Structure（PR2 范围）

新建：
- `src-tauri/src/connector/im/whatsapp/session.rs` — 路径计算 + 备份/删除函数。单文件 ~120 行，4-5 个 pure 函数 + 6 个单测。
- `src-tauri/src/connector/im/whatsapp/config.rs` — `WhatsAppChannelConfig` struct + read/write JSON。单文件 ~80 行 + 5 个单测。

修改：
- `src-tauri/src/connector/im/whatsapp/types.rs` — 加 `PairingState` enum（4 变体）+ 一个测试。
- `src-tauri/src/connector/im/whatsapp/mod.rs` — 加 `pub mod session;` + `pub mod config;`。
- `src-tauri/src/connector/im/whatsapp/connector.rs` — `WhatsAppConnector` 加 `bot_handle` + `pairing_state` 字段；保留 `start()` / `send()` 仍返 `NotSupported`（PR3-PR5 才填），但 `stop()` 现在真做 `JoinHandle::abort()`。

不动（PR2 不碰）：
- `factory.rs`（build_whatsapp_connector 签名不变；fields 通过 `with_status_callback` 内部初始化）
- `mod.rs` 的 `im/mod.rs` 注册（PR1 已加）
- `manager.rs`（manager wiring 留到 PR3）
- 前端任何文件
- `Cargo.toml`（wa-rs 已在 PR1 加）

---

## Task 1: PairingState enum + 4 状态单测

**Files:**
- Modify: `src-tauri/src/connector/im/whatsapp/types.rs`

- [ ] **Step 1: 先写失败测试（TDD）**

Edit `src-tauri/src/connector/im/whatsapp/types.rs`：在文件末尾追加测试：

```rust

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pairing_state_idle_default() {
        let s = PairingState::Idle;
        assert!(matches!(s, PairingState::Idle));
    }

    #[test]
    fn pairing_state_qr_issued_holds_code_and_expiry() {
        use std::time::{Duration, Instant};
        let s = PairingState::QrIssued {
            code: "1@abc123def456".into(),
            expires_at: Instant::now() + Duration::from_secs(60),
        };
        match s {
            PairingState::QrIssued { ref code, .. } => assert_eq!(code, "1@abc123def456"),
            _ => panic!("expected QrIssued"),
        }
    }

    #[test]
    fn pairing_state_connected_holds_jid_and_push_name() {
        let s = PairingState::Connected {
            jid: "8613800138000@s.whatsapp.net".into(),
            push_name: "Alice".into(),
        };
        match s {
            PairingState::Connected { jid, push_name } => {
                assert!(jid.ends_with("@s.whatsapp.net"));
                assert_eq!(push_name, "Alice");
            }
            _ => panic!("expected Connected"),
        }
    }

    #[test]
    fn pairing_state_awaiting_qr_carries_start_time() {
        use std::time::Instant;
        let s = PairingState::AwaitingQr { started_at: Instant::now() };
        assert!(matches!(s, PairingState::AwaitingQr { .. }));
    }
}
```

- [ ] **Step 2: 跑测试验证它失���（编译失败：PairingState 不存在）**

Run: `cd src-tauri && cargo test --lib connector::im::whatsapp::types:: 2>&1 | tail -10`
Expected: `error[E0412]: cannot find type \`PairingState\` in this scope`

- [ ] **Step 3: 实现 PairingState**

Edit `src-tauri/src/connector/im/whatsapp/types.rs`：在已有的 `WhatsAppSessionTarget` 之后、`#[cfg(test)] mod tests` 之前，插入：

```rust

use std::time::Instant;

/// QR 扫码登录的 4 状态机。spec v3 §3.5。
///
/// v2 设计的 `AwaitingDeviceConfirm` / `Expired` / `Cancelled` / `Failed` 砍掉，
/// 从超时 / 错误 event 派生即可，不需要单独存。`Instant` 不实现 Serialize，
/// 该 enum 是 connector 内部状态、不直接 emit 给前端；poll_registration 把它
/// 映射到 `ChannelRegistrationPollState`。
#[allow(dead_code)] // PR3 begin/poll_registration 会用；PR2 只定义类型
#[derive(Debug, Clone)]
pub enum PairingState {
    /// 没开始扫码（manager 还没调 begin_registration，或上一次扫码已完成）
    Idle,
    /// bot.run() 起来但 `Event::PairingQrCode` 还没到
    AwaitingQr { started_at: Instant },
    /// QR 已下发；前端展示中。`expires_at` 来自 wa-rs `Event::PairingQrCode.timeout`
    QrIssued { code: String, expires_at: Instant },
    /// 扫码完成。`Event::PairSuccess` 提供 jid + push_name；`Event::Connected` 之后才到此态
    Connected { jid: String, push_name: String },
}

impl Default for PairingState {
    fn default() -> Self {
        PairingState::Idle
    }
}
```

- [ ] **Step 4: 跑测试验证通过**

Run: `cd src-tauri && cargo test --lib connector::im::whatsapp::types:: 2>&1 | tail -10`
Expected: `test result: ok. 4 passed; 0 failed`

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/connector/im/whatsapp/types.rs
git commit -m "$(cat <<'EOF'
feat(connector/im/whatsapp): PR2 加 PairingState 4 状态 enum

spec v3 §3.5。Idle / AwaitingQr / QrIssued / Connected 4 个变体，
携带必要的 Instant + jid + push_name。砍掉 v2 设计的另外 4 个状态
（AwaitingDeviceConfirm / Expired / Cancelled / Failed），从超时
和错误 event 派生即可。

PR3 begin/poll_registration 会用；PR2 只定义类型 + 4 个 unit test。

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 2: session.rs 路径计算 + 备份/删除

**Files:**
- Create: `src-tauri/src/connector/im/whatsapp/session.rs`
- Modify: `src-tauri/src/connector/im/whatsapp/mod.rs`（注册 submodule）

### Step 1: 先写 session.rs 起手（TDD test 块在末尾）

Create `src-tauri/src/connector/im/whatsapp/session.rs`:

```rust
//! WhatsApp 凭证文件路径计算 + 备份/删除。spec v3 §3.1-§3.3。
//!
//! 抄 OpenClaw 的 oauth/whatsapp/{accountId}/ + creds.json + creds.json.bak
//! 模式。单账号下连 `default/` 子目录也省，直接 channels/whatsapp/。
//!
//! ```text
//! channels/whatsapp/
//! ├── session.db          # wa-rs SqliteStore
//! ├── session.db.bak      # 启动前自动备份；wa-rs 启动失败时手动恢复
//! └── config.json         # AIjia 元数据：jid / push_name / paired_at
//! ```

use std::path::{Path, PathBuf};

/// 路径计算 helper。`base` 是 channels/whatsapp/ 目录（来自
/// `ChannelConfigStore::platform_dir(Platform::Whatsapp)`）。
#[derive(Debug, Clone)]
pub struct WhatsAppPaths {
    base: PathBuf,
}

impl WhatsAppPaths {
    pub fn new(base: impl Into<PathBuf>) -> Self {
        Self { base: base.into() }
    }

    /// `channels/whatsapp/`
    pub fn base(&self) -> &Path {
        &self.base
    }

    /// `channels/whatsapp/session.db` —— wa-rs SqliteStore 的文件
    pub fn session_db(&self) -> PathBuf {
        self.base.join("session.db")
    }

    /// `channels/whatsapp/session.db.bak` —— 启动前备份
    pub fn session_db_bak(&self) -> PathBuf {
        self.base.join("session.db.bak")
    }

    /// `channels/whatsapp/config.json` —— AIjia 元数据
    /// （也是 `ChannelConfigStore::platform_config_path` 的对应路径）
    pub fn config_path(&self) -> PathBuf {
        self.base.join("config.json")
    }

    /// 确保 base 目录存在。在 PR3 begin_registration 第一次写文件前调一次即可。
    pub fn ensure_base_dir(&self) -> std::io::Result<()> {
        std::fs::create_dir_all(&self.base)
    }
}

/// 启动前备份：如果 session.db 存在且非空，复制一份到 session.db.bak（覆盖旧 bak）。
///
/// spec v3 §3.3。**不**判断 wa-rs 启动是否能读 session.db；wa-rs 自己会报错，
/// 上层在 PR4 集成测试发现实际损坏概率后决定要不要做自动回滚。
///
/// 返回 `Ok(true)` 表示备份发生了，`Ok(false)` 表示无 session.db 或文件空 → 跳过。
pub fn backup_session_db_if_present(paths: &WhatsAppPaths) -> std::io::Result<bool> {
    let src = paths.session_db();
    if !src.exists() {
        return Ok(false);
    }
    let meta = std::fs::metadata(&src)?;
    if meta.len() == 0 {
        return Ok(false);
    }
    let dst = paths.session_db_bak();
    std::fs::copy(&src, &dst)?;
    Ok(true)
}

/// 重新扫码用：删 session.db + config.json，**保留** session.db.bak。
///
/// spec v3 §3.9。如果删除失败（文件本不存在），不返回错——重新扫码语义
/// 是"清掉登录态"，已不在的文件就当成功。
pub fn delete_for_reauth(paths: &WhatsAppPaths) -> std::io::Result<()> {
    for p in [paths.session_db(), paths.config_path()] {
        match std::fs::remove_file(&p) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(e),
        }
    }
    Ok(())
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

    #[test]
    fn path_helpers_compose_under_base() {
        let p = WhatsAppPaths::new("/tmp/foo");
        assert_eq!(p.session_db(), PathBuf::from("/tmp/foo/session.db"));
        assert_eq!(p.session_db_bak(), PathBuf::from("/tmp/foo/session.db.bak"));
        assert_eq!(p.config_path(), PathBuf::from("/tmp/foo/config.json"));
    }

    #[test]
    fn ensure_base_dir_creates_nested_dirs() {
        let dir = TempDir::new().unwrap();
        let base = dir.path().join("a").join("b").join("whatsapp");
        let paths = WhatsAppPaths::new(&base);
        paths.ensure_base_dir().unwrap();
        assert!(base.exists());
    }

    #[test]
    fn backup_skips_when_session_db_missing() {
        let (_dir, paths) = tmp_paths();
        let did = backup_session_db_if_present(&paths).unwrap();
        assert!(!did, "no session.db → no backup");
        assert!(!paths.session_db_bak().exists());
    }

    #[test]
    fn backup_skips_when_session_db_empty() {
        let (_dir, paths) = tmp_paths();
        std::fs::write(paths.session_db(), b"").unwrap();
        let did = backup_session_db_if_present(&paths).unwrap();
        assert!(!did, "empty session.db → no backup");
        assert!(!paths.session_db_bak().exists());
    }

    #[test]
    fn backup_copies_session_db_to_bak() {
        let (_dir, paths) = tmp_paths();
        std::fs::write(paths.session_db(), b"SQLite payload").unwrap();
        let did = backup_session_db_if_present(&paths).unwrap();
        assert!(did);
        assert_eq!(
            std::fs::read(paths.session_db_bak()).unwrap(),
            b"SQLite payload"
        );
    }

    #[test]
    fn backup_overwrites_existing_bak() {
        let (_dir, paths) = tmp_paths();
        std::fs::write(paths.session_db_bak(), b"OLD").unwrap();
        std::fs::write(paths.session_db(), b"NEW").unwrap();
        let did = backup_session_db_if_present(&paths).unwrap();
        assert!(did);
        assert_eq!(std::fs::read(paths.session_db_bak()).unwrap(), b"NEW");
    }

    #[test]
    fn delete_for_reauth_removes_db_and_config_keeps_bak() {
        let (_dir, paths) = tmp_paths();
        std::fs::write(paths.session_db(), b"db").unwrap();
        std::fs::write(paths.session_db_bak(), b"bak").unwrap();
        std::fs::write(paths.config_path(), b"{}").unwrap();

        delete_for_reauth(&paths).unwrap();

        assert!(!paths.session_db().exists());
        assert!(!paths.config_path().exists());
        assert!(paths.session_db_bak().exists(), "bak must be preserved as recovery anchor");
    }

    #[test]
    fn delete_for_reauth_idempotent_when_files_missing() {
        let (_dir, paths) = tmp_paths();
        // Files don't exist; should not error
        delete_for_reauth(&paths).unwrap();
        delete_for_reauth(&paths).unwrap();
    }
}
```

### Step 2: 注册 submodule

Edit `src-tauri/src/connector/im/whatsapp/mod.rs` —— 在已有的 `pub mod connector;` / `pub mod types;` 旁边加 `pub mod session;`：

```rust
pub mod connector;
pub mod config;    // Task 3 加
pub mod session;
pub mod types;

pub use connector::WhatsAppConnector;
```

⚠️ Task 3 还没写 `config.rs`，所以**这步先只加 `pub mod session;`**，`pub mod config;` 留到 Task 3 一起加。即本步真实改动：

```rust
pub mod connector;
pub mod session;
pub mod types;

pub use connector::WhatsAppConnector;
```

### Step 3: 跑 session 单测

Run: `cd src-tauri && cargo test --lib connector::im::whatsapp::session:: 2>&1 | tail -15`
Expected: 7 个 test 全过：
```
test result: ok. 7 passed; 0 failed
```

### Step 4: 跑全 lib 编译确认没破

Run: `cd src-tauri && cargo build --lib 2>&1 | tail -3`
Expected: `Finished` 无 error。

### Step 5: Commit

```bash
git add src-tauri/src/connector/im/whatsapp/mod.rs \
        src-tauri/src/connector/im/whatsapp/session.rs
git commit -m "$(cat <<'EOF'
feat(connector/im/whatsapp): PR2 加 session.rs 路径 + 备份/删除

spec v3 §3.1 + §3.3 + §3.9。

- WhatsAppPaths：从 base 派生 session.db / session.db.bak / config.json
- backup_session_db_if_present：启动前备份兜底（OpenClaw 同款思路）
- delete_for_reauth：删 session.db + config.json，保留 .bak

7 个单测覆盖：路径派生 / ensure dir / 备份的 3 个分支
（不存在/空文件/正常）/ bak 覆盖 / 重扫两条路径 / 幂等。

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 3: config.rs 元数据 read/write

**Files:**
- Create: `src-tauri/src/connector/im/whatsapp/config.rs`
- Modify: `src-tauri/src/connector/im/whatsapp/mod.rs`（加 `pub mod config;`）

### Step 1: 先写 config.rs

Create `src-tauri/src/connector/im/whatsapp/config.rs`:

```rust
//! WhatsApp config.json 元数据 read/write。spec v3 §3.1。
//!
//! 跟 wa-rs SqliteStore 解耦——这里只存 AIjia 自己用的元数据：
//! - jid：扫码后从 `Event::PairSuccess.id` 拿到，运维和 UI 用
//! - push_name：从 `Event::PairSuccess.push_name` 拿到，显示名
//! - paired_at：扫码完成 RFC3339 时间戳
//!
//! schema_version=1 锁定向后兼容；未来加字段必走 #[serde(default)]。

use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct WhatsAppChannelConfig {
    pub schema_version: u32,
    pub jid: String,
    pub push_name: String,
    /// RFC3339 时间戳，e.g. `"2026-05-20T10:30:00Z"`
    pub paired_at: String,
    /// E.164 手机号 allowlist。`None` 或空 vec = 接收所有入站（默认）。
    /// 见 spec v3 §3.10：只回复列表内的号码，降低风控 + 避免回陌生人。
    /// PR3 配置 UI 编辑，PR4 入站 worker 过滤时读这里。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub allow_from: Option<Vec<String>>,
}

/// 读 config.json。返回 `Ok(None)` 表示文件不存在或为空（未配对），
/// `Ok(Some(cfg))` 表示已配对，`Err` 表示文件存在但格式损坏。
pub fn read(path: &Path) -> std::io::Result<Option<WhatsAppChannelConfig>> {
    let bytes = match std::fs::read(path) {
        Ok(b) => b,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e),
    };
    if bytes.is_empty() {
        return Ok(None);
    }
    serde_json::from_slice(&bytes)
        .map(Some)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))
}

/// 写 config.json。覆盖式写入，**不**用 tmp+rename atomic 写
/// （wa-rs SqliteStore 的 WAL + synchronous=NORMAL 已是耐久性主防线，
/// config.json 即便写到一半坏了下次重扫也能恢复）。
pub fn write(path: &Path, cfg: &WhatsAppChannelConfig) -> std::io::Result<()> {
    let json = serde_json::to_vec_pretty(cfg)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, json)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn sample() -> WhatsAppChannelConfig {
        WhatsAppChannelConfig {
            schema_version: 1,
            jid: "8613800138000@s.whatsapp.net".into(),
            push_name: "Alice".into(),
            paired_at: "2026-05-20T10:30:00Z".into(),
            allow_from: None,
        }
    }

    #[test]
    fn read_returns_none_when_missing() {
        let dir = TempDir::new().unwrap();
        let p = dir.path().join("config.json");
        assert_eq!(read(&p).unwrap(), None);
    }

    #[test]
    fn read_returns_none_when_empty() {
        let dir = TempDir::new().unwrap();
        let p = dir.path().join("config.json");
        std::fs::write(&p, b"").unwrap();
        assert_eq!(read(&p).unwrap(), None);
    }

    #[test]
    fn write_then_read_roundtrip() {
        let dir = TempDir::new().unwrap();
        let p = dir.path().join("config.json");
        let cfg = sample();
        write(&p, &cfg).unwrap();
        let got = read(&p).unwrap().expect("should read back");
        assert_eq!(got, cfg);
    }

    #[test]
    fn write_creates_parent_dir() {
        let dir = TempDir::new().unwrap();
        let p = dir.path().join("a").join("b").join("config.json");
        write(&p, &sample()).unwrap();
        assert!(p.exists());
    }

    #[test]
    fn read_errors_on_invalid_json() {
        let dir = TempDir::new().unwrap();
        let p = dir.path().join("config.json");
        std::fs::write(&p, b"not json {").unwrap();
        let err = read(&p).unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::InvalidData);
    }

    #[test]
    fn serde_field_names_camel_case() {
        // 锁定字段名向后兼容性。如果 enum 的 rename_all 被改 / 字段被 rename，
        // 这个测试会 fail，逼实施者明确处理迁移。
        let cfg = sample();
        let s = serde_json::to_string(&cfg).unwrap();
        assert!(s.contains("\"schemaVersion\":1"));
        assert!(s.contains("\"jid\":"));
        assert!(s.contains("\"pushName\":"));
        assert!(s.contains("\"pairedAt\":"));
    }

    #[test]
    fn allow_from_optional_skipped_when_none() {
        // None → 不出现在序列化结果里（向后兼容，老 config.json 没有此字段）。
        let cfg = sample();
        let s = serde_json::to_string(&cfg).unwrap();
        assert!(!s.contains("allowFrom"), "None allowFrom must not serialize");
    }

    #[test]
    fn allow_from_roundtrip_preserves_phones() {
        // Some(vec) 正确 roundtrip。spec §3.10。
        let dir = TempDir::new().unwrap();
        let p = dir.path().join("config.json");
        let cfg = WhatsAppChannelConfig {
            allow_from: Some(vec!["+8613912345678".into(), "+8613987654321".into()]),
            ..sample()
        };
        write(&p, &cfg).unwrap();
        let got = read(&p).unwrap().unwrap();
        assert_eq!(got.allow_from, cfg.allow_from);
    }

    #[test]
    fn read_old_config_without_allow_from_field() {
        // serde(default) 兼容性：老 config.json 不带 allowFrom 字段也能读出。
        let dir = TempDir::new().unwrap();
        let p = dir.path().join("config.json");
        std::fs::write(&p, br#"{
            "schemaVersion": 1,
            "jid": "8613800138000@s.whatsapp.net",
            "pushName": "Alice",
            "pairedAt": "2026-05-20T10:30:00Z"
        }"#).unwrap();
        let got = read(&p).unwrap().unwrap();
        assert_eq!(got.allow_from, None);
    }
}
```

### Step 2: 注册 submodule

Edit `src-tauri/src/connector/im/whatsapp/mod.rs`：

```rust
pub mod config;
pub mod connector;
pub mod session;
pub mod types;

pub use connector::WhatsAppConnector;
```

按字母序插入 `pub mod config;` 在 `pub mod connector;` 之前。

### Step 3: 跑 config 单测

Run: `cd src-tauri && cargo test --lib connector::im::whatsapp::config:: 2>&1 | tail -15`
Expected: 6 个 test 全过。

### Step 4: 跑全 lib 编译

Run: `cd src-tauri && cargo build --lib 2>&1 | tail -3`
Expected: `Finished` 无 error。

### Step 5: Commit

```bash
git add src-tauri/src/connector/im/whatsapp/mod.rs \
        src-tauri/src/connector/im/whatsapp/config.rs
git commit -m "$(cat <<'EOF'
feat(connector/im/whatsapp): PR2 加 config.rs JSON 元数据读写

spec v3 §3.1。WhatsAppChannelConfig 4 字段（schemaVersion / jid /
pushName / pairedAt），serde camelCase 跟前端约定一致。

read 区分三态：文件缺失 → Ok(None)（未配对）/ 空文件 → Ok(None) /
损坏 JSON → Err(InvalidData)。write 覆盖式不走 tmp+rename
（耐久性主防线在 wa-rs WAL，config.json 坏了重扫即可恢复）。

6 个单测：缺失/空 read / roundtrip / 父目录自动创建 / 损坏报错 /
serde 字段名锁定（防 schema 漂移）。

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 4: connector.rs 升级——加 bot_handle + pairing_state，stop() 真做 abort

**Files:**
- Modify: `src-tauri/src/connector/im/whatsapp/connector.rs`

### Step 1: 检查现状

Run: `cat src-tauri/src/connector/im/whatsapp/connector.rs | head -50`

应看到 PR1 的 stub：`WhatsAppConnector { on_status }` + `with_status_callback` 构造器 + `IMConnector` 4 个方法（platform / capabilities / start NotSupported / send NotSupported）。

### Step 2: 升级 struct 字段 + 构造器 + stop 实现

完整覆盖 `src-tauri/src/connector/im/whatsapp/connector.rs`：

```rust
//! `WhatsAppConnector` —— PR2 升级版（持 Bot JoinHandle + PairingState）。
//!
//! 实施进度：
//! - PR1：stub，start/send 返 NotSupported
//! - **PR2（本 PR）**：加 bot_handle + pairing_state 字段；stop() 真做
//!   JoinHandle::abort()；start/send 仍 NotSupported
//! - PR3：begin/poll_registration 真做扫码（用 PairingState）
//! - PR4：start() 真起 bot.run() 入站 worker
//! - PR5：send() 真做出站
//! - PR6：AI Card edit 路径
//! - PR7：媒体下载
//! - PR8：集成测试 + UI banner

use std::sync::Arc;

use async_trait::async_trait;
use futures::stream::BoxStream;
use tokio::sync::Mutex;
use tokio::task::JoinHandle;

use crate::connector::im::trait_def::{
    AuthFlow, ConnectorCapabilities, ConnectorContext, ConnectorError, IMConnector, InboundModel,
    ReplyContent, ReplyTarget,
};
use crate::connector::im::types::{ChannelConnectionState, ChannelMessage, Platform};

use super::types::PairingState;

pub struct WhatsAppConnector {
    /// 状态回调。PR1 持有但不调用；PR2 仍不主动调（manager 会在 stop 时
    /// 收到对应的 Disconnected/Connected 状态变化由 PR3+ 驱动）。
    #[allow(dead_code)]
    on_status: Arc<dyn Fn(ChannelConnectionState, Option<String>) + Send + Sync + 'static>,

    /// bot.run() 起的 task join handle。spec v3 §3.4：wa-rs 无 graceful stop，
    /// 关闭靠 abort()。PR2 在 stop() 里调用；PR3 真起 Bot 后会 set 它。
    #[allow(dead_code)] // PR3 才赋值
    bot_handle: Arc<Mutex<Option<JoinHandle<()>>>>,

    /// 扫码状态机。PR3 begin_registration 改这里，poll_registration 读这里。
    /// PR2 只声明字段并默认 Idle，让 PR3 实施时无须改 struct 形状。
    #[allow(dead_code)] // PR3 才驱动
    pairing_state: Arc<Mutex<PairingState>>,
}

impl WhatsAppConnector {
    pub fn new() -> Self {
        Self::with_status_callback(Arc::new(|_state, _err| {}))
    }

    pub fn with_status_callback(
        on_status: Arc<dyn Fn(ChannelConnectionState, Option<String>) + Send + Sync + 'static>,
    ) -> Self {
        Self {
            on_status,
            bot_handle: Arc::new(Mutex::new(None)),
            pairing_state: Arc::new(Mutex::new(PairingState::default())),
        }
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
        // spec §1 capability 表逐字对齐。PR2 不动 capability 值。
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
            "whatsapp::start — PR4 入站 worker 未实现",
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

    /// spec v3 §3.4：wa-rs Bot 没 graceful shutdown，唯一手段 JoinHandle::abort()。
    /// 不 await handle —— abort 后 await 会返 `Err(JoinError::Cancelled)`，
    /// 这正是预期，不需等"任务完整跑完"。
    ///
    /// SqliteStore 的 Arc 在 connector 被 drop 时由 r2d2 自动回收 connection pool；
    /// in-flight 的 spawn_blocking 写入可能丢，由 session.db.bak 兜底
    /// （spec v3 §3.3）。
    async fn stop(&self) -> Result<(), ConnectorError> {
        if let Some(handle) = self.bot_handle.lock().await.take() {
            handle.abort();
            log::info!("[whatsapp] bot task aborted");
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capabilities_match_phase4_spec() {
        let c = WhatsAppConnector::new();
        let caps = c.capabilities();
        assert_eq!(caps.inbound, InboundModel::Stream);
        assert!(!caps.outbound_aicard);
        assert!(caps.outbound_text_streaming);
        assert!(!caps.outbound_markdown);
        assert!(caps.supports_attachments);
        assert!(!caps.supports_group_chat);
        assert!(caps.supports_private_chat);
        assert_eq!(caps.auth_flow, AuthFlow::QRCode);
    }

    #[test]
    fn platform_is_whatsapp() {
        let c = WhatsAppConnector::new();
        assert_eq!(c.platform(), Platform::Whatsapp);
    }

    #[tokio::test]
    async fn start_still_returns_not_supported_in_pr2() {
        // PR2 不实现 start；PR4 才动。
        let c = WhatsAppConnector::new();
        let ctx = test_ctx();
        let err = c.start(ctx).await.unwrap_err();
        match err {
            ConnectorError::NotSupported(msg) => assert!(msg.contains("PR4")),
            other => panic!("expected NotSupported, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn send_still_returns_not_supported_in_pr2() {
        let c = WhatsAppConnector::new();
        let err = c
            .send(
                ReplyTarget {
                    session_id: "sess".into(),
                    external_conversation_key: "8613800138000@s.whatsapp.net".into(),
                },
                ReplyContent::Text("hi".into()),
            )
            .await
            .unwrap_err();
        match err {
            ConnectorError::NotSupported(msg) => assert!(msg.contains("PR5")),
            other => panic!("expected NotSupported, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn stop_is_noop_when_no_bot_handle() {
        // PR2：没起过 Bot，stop() 应该 silently Ok。
        let c = WhatsAppConnector::new();
        c.stop().await.expect("stop should be Ok when no bot handle");
    }

    #[tokio::test]
    async fn stop_aborts_the_bot_handle_when_present() {
        // PR2 直接对 connector 的 bot_handle 注入一个 join handle，验证 stop 会 abort 它。
        let c = WhatsAppConnector::new();
        let task: JoinHandle<()> = tokio::spawn(async {
            // 跑一个永远不结束的 task，模拟 bot.run()。
            std::future::pending::<()>().await;
        });
        // 直接通过同名字段在 test scope 内 inject（field 是 pub(crate) 的，
        // tests 在同 module 所以能拿到）。
        *c.bot_handle.lock().await = Some(task);

        c.stop().await.expect("stop should abort bot handle");

        // 验证 handle 被取走（再调一次 stop 是 noop）
        assert!(c.bot_handle.lock().await.is_none());
    }

    fn test_ctx() -> ConnectorContext {
        // 复用 PR1 的测试 fixture 逻辑。注意：PR1 实施时实际用的 import 路径是：
        //   ChannelConfigStore — crate::connector::im::shared::config_store::ChannelConfigStore
        //   PendingQueueManager — crate::runtime::pending::PendingQueueManager
        //   ConvDirResolver — crate::runtime::pending::queue_manager::ConvDirResolver
        //   PendingConfig — crate::runtime::pending::types::PendingConfig
        //   RuntimeRunRegistry / RuntimeEventBus — 实际路径见 PR1 implementer 在
        //   commit 085527fa 里写的 test_ctx。
        // 直接复用 PR1 已有的 test_ctx 实现——本 step 不重写，只确保该 helper 还在。
        crate::connector::im::whatsapp::connector::tests::__pr1_test_ctx()
    }
}
```

⚠️ **重要 caveat**：上面的 `test_ctx` 调用了 `__pr1_test_ctx()`——这个 helper **PR1 已经存在**但名字未必叫 `__pr1_test_ctx`，可能就叫 `test_ctx`。**实施步骤**：

1. 先 `grep -n 'fn test_ctx' src-tauri/src/connector/im/whatsapp/connector.rs` 看 PR1 原始 helper 函数名
2. 在 PR2 重写 `connector.rs` 时**保留**该 helper 原封不动（它在 `#[cfg(test)] mod tests` 内，rewrite 时整段 tests block 都保留 PR1 已有的 helper 函数）
3. 上面 PR2 的 5 个新 test 复用同一个 helper，不要重定义

为避免歧义：实施时**先**用 `cargo test --lib connector::im::whatsapp` 跑一遍 PR1 baseline 看 4 个 test 现在叫什么名字 + helper 函数怎么命名，**然后**在新 rewrite 文件中保留它们。

### Step 3: 跑 connector 单测

Run: `cd src-tauri && cargo test --lib connector::im::whatsapp::connector:: 2>&1 | tail -20`

Expected: PR1 的 4 个 test 还过，PR2 新增 2 个 test（`stop_is_noop_when_no_bot_handle` + `stop_aborts_the_bot_handle_when_present`）也过，共 6 个 test。

注意：原 PR1 的 `start_returns_not_supported_in_pr1` test 断言 message contains "PR2"。**本 PR 把 start 的 err message 改成了 "PR4 入站 worker 未实现"**（因为 PR2 没真起 worker，PR4 才做）——所以那个 test 现在的断言也得改：原断言 `msg.contains("PR2")`，新断言 `msg.contains("PR4")`，对应改成新的 test 名 `start_still_returns_not_supported_in_pr2`。

上面我已经把 5 个 test 都按 PR2 语义重写，包含 `start_still_returns_not_supported_in_pr2` —— 实施时把 PR1 的 4 个 test 整体替换为 PR2 的 5 个 test。

### Step 4: 跑全 lib 编译 + 全 IM 测试

Run: `cd src-tauri && cargo build --lib 2>&1 | tail -3`
Expected: `Finished`。

Run: `cd src-tauri && cargo test --lib connector::im:: 2>&1 | grep -E '^test result' | tail -3`
Expected: `0 failed`（除了 8 个 pre-existing download 失败保持不变）。

### Step 5: 跑架构约束测试，确认 review_im_layering 不破

Run: `cd src-tauri && cargo test --test review_im_layering 2>&1 | tail -5`
Expected: 3 个 layering test 全过。

### Step 6: Commit

```bash
git add src-tauri/src/connector/im/whatsapp/connector.rs
git commit -m "$(cat <<'EOF'
feat(connector/im/whatsapp): PR2 升级 connector 持 bot_handle + pairing_state

spec v3 §3.4 + §3.5。

- WhatsAppConnector 加两个字段：
  * bot_handle: Arc<Mutex<Option<JoinHandle<()>>>> —— PR3 真起 Bot 后赋值
  * pairing_state: Arc<Mutex<PairingState>> —— PR3 扫码状态机驱动
- IMConnector::stop() 真做 JoinHandle::abort()（wa-rs 无 graceful
  shutdown，spec §3.4 已登记 in-flight 写入丢失由 session.db.bak 兜底）
- start/send 仍返 NotSupported，但 message 更新到对应的 PR 编号
  （start → PR4 入站，send → PR5 出站；之前都说 PR2）

新 6 个 unit test 覆盖：4 个 PR1 既有契约（capabilities / platform /
start NotSupported / send NotSupported）+ 2 个 PR2 新增（stop noop /
stop aborts）。

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 5: 收尾校验

**Files:** （无修改）

### Step 1: 跑全 PR2 测试

Run: `cd src-tauri && cargo test --lib connector::im::whatsapp:: 2>&1 | tail -20`

Expected: 4 个 modules 测试全过：
- `connector::im::whatsapp::config::tests::*` — 9 个（含 3 个 allow_from 测试）
- `connector::im::whatsapp::session::tests::*` — 7 个
- `connector::im::whatsapp::types::tests::*` — 4 个
- `connector::im::whatsapp::connector::tests::*` — 6 个

总共 **26 个 test pass**（PR1 是 4 个，PR2 新增 22 个）。

### Step 2: 跑架构约束 + 全 IM 测试

Run: `cd src-tauri && cargo test --test review_im_layering 2>&1 | tail -5`
Expected: 3 passed。

Run: `cd src-tauri && cargo test --lib connector::im 2>&1 | grep -E '^test result' | tail -3`
Expected: passed 数 >= PR1 baseline，failed 数等于 PR1 baseline 的 8（pre-existing download tests）。**不能**有任何**新**失败。

### Step 3: 跑 clippy 在 PR2-touched 文件

Run: `cd src-tauri && cargo clippy --lib --message-format=short 2>&1 | grep -E 'src/connector/im/whatsapp/' | head -20`

Expected: PR2 创建/修改的 4 个文件（connector.rs / config.rs / session.rs / types.rs）没有任何 warning。Pre-existing warnings 在其他文件忽略。

如果**有**PR2 文件的 warning：fix it。典型：`#[allow(dead_code)]` 漏加（实施 PR3 才用的字段需要 `#[allow(dead_code)] // PR3 ...`）/ unused imports / `let _ = ...` 漏（异步 Result 没 use 应该 await 或显式 ignore）。

### Step 4: 跑 cargo fmt

Run: `cd src-tauri && cargo fmt -- --check src/connector/im/whatsapp/ 2>&1`

Expected: 无 diff 输出。如有 diff：跑 `cargo fmt -- src/connector/im/whatsapp/` 修，然后单独 commit：

```bash
git add src-tauri/src/connector/im/whatsapp/
git commit -m "$(cat <<'EOF'
style(connector/im/whatsapp): PR2 cargo fmt

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

### Step 5: 前端检查（PR2 不动前端，应该 0 影响）

Run: `pnpm exec tsc --noEmit 2>&1 | tail -3`
Expected: 跟 PR1 完成时的 baseline 一致（无新 error）。

Run: `pnpm lint 2>&1 | tail -3`
Expected: 跟 PR1 完成时的 baseline 一致（无新 error）。

### Step 6: 最终 git status

Run: `git status; git log --oneline 5c90d077..HEAD | head -10`

Expected: working tree clean（除了 plan/spec 的 untracked / staged 文件可能还在）。PR2 应有 4 个 commit：
1. PairingState
2. session.rs
3. config.rs
4. connector.rs 升级

外加一个可能的 fmt commit。如果 commit 数对不上，自查哪步漏了。

---

## Self-Review

### 1. Spec 覆盖（v3 §3 + §10.2 PR2 范围）

| spec v3 子段 | 本 plan task | 状态 |
|---|---|---|
| §3.1 凭证路径（session.db / .bak / config.json） | Task 2 session.rs + Task 3 config.rs | ✅ |
| §3.2 不加密 | 隐式（不动 SecureStorage） | ✅ |
| §3.3 启动备份策略 | Task 2 `backup_session_db_if_present` | ✅ |
| §3.4 关闭 Bot（JoinHandle::abort） | Task 4 connector.stop | ✅ |
| §3.5 PairingState 4 状态 | Task 1 types.rs | ✅ |
| §3.6 扫码流程 | **不在 PR2 范围**（PR3 做） | N/A |
| §3.7 复用 Tauri 命令 | **不在 PR2 范围**（PR3 做） | N/A |
| §3.8 复用 RegistrationModal | **不在 PR2 范围**（PR3 做） | N/A |
| §3.9 重新扫码 | Task 2 `delete_for_reauth` | ✅（PR3 拼装 flow） |

### 2. Placeholder 扫描

搜 plan 全文：
- ✅ 无 "TBD" / "TODO" / "implement later"
- ✅ 无 "Add appropriate error handling"（每个 helper 函数明确写错误处理逻辑）
- ✅ 无 "Write tests for the above" 而无实际代码
- ⚠️ Task 4 Step 2 末尾的 ⚠️ caveat **不是 placeholder**——它是 "PR1 实际 helper 函数名我没事先 grep 出来" 的真实模糊点。实施者按指示先 grep 再 rewrite 即可。

### 3. 类型一致性

- `WhatsAppPaths` 字段方法名：`session_db()` / `session_db_bak()` / `config_path()` / `ensure_base_dir()` —— 整个 plan 一致
- `WhatsAppChannelConfig` 字段名：`schema_version` / `jid` / `push_name` / `paired_at`（snake_case in Rust, camelCase via `#[serde(rename_all = "camelCase")]`）—— Task 3 内部一致
- `PairingState` 4 变体名：`Idle` / `AwaitingQr` / `QrIssued` / `Connected` —— Task 1 内部 + Task 4 connector.stop 都一致
- `bot_handle` 字段类型：`Arc<Mutex<Option<JoinHandle<()>>>>` —— Task 4 内部统一
- `pairing_state` 字段类型：`Arc<Mutex<PairingState>>` —— Task 1 + Task 4 衔接一致

### 4. 验证 Task 4 Step 2 的关键 caveat

`__pr1_test_ctx()` 那段 placeholder-like 写法实际是给实施者一个"先查再写"的指令，**不是**真的让他们引用一个不存在的函数名。读这个 plan 的实施者会理解：grep → 看 PR1 实际命名 → 在 PR2 重写中复用同名 helper。

但是为了减少模糊性，**自我修正**：把 Task 4 Step 2 末尾的 caveat 块改写成"前置探查指令"（在 Step 2 之前加一步"先查 PR1 既有 test helper 命名"）。

**修正后 Task 4 增加 Step 1.5：**

```
- [ ] **Step 1.5 (added by self-review): 先 grep PR1 既有 test helper 命名**

Run: `grep -n 'fn test_ctx\|fn build_test\|fn make_test' src-tauri/src/connector/im/whatsapp/connector.rs`

Read the existing helper. In Step 2 rewrite of connector.rs, **保留** that helper 函数原封不动（连函数名+签名+import）。然后让新增 5 个 test 调用它。
```

实施者应该在 Step 1.5 拿到函数名（可能就叫 `test_ctx`），然后在 Step 2 rewrite 时把它整段保留即可。

---

## Execution Handoff

Plan 完成并保存到 `docs/superpowers/plans/2026-05-20-im-whatsapp-phase4-pr2.md`。

两种执行方式：

**1. Subagent-Driven（推荐，跟 PR1 一样的模式）** —— 5 个 task 派 fresh subagent，每 task 之间我 review + 两段 review（spec + code quality）

**2. Inline Execution** —— 当前 session 顺序跑

哪种？
