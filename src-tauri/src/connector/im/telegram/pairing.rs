//! PairingCodeStore：in-flight 配对码（5 min TTL）+ PR4 落盘持久化。
//!
//! Code 字符集去掉歧义字符 `O/0/I/1/l`，base32 风格 8 个字符。Code 全局唯一性
//! 由 set 去重保证；重复生成时直接重抽。
//!
//! 协议（spec §2.5）：
//! 1. `begin` 生成 code，返回 deep_link
//! 2. bot 收到 /start <code> 时 `attempt_attach(code, pairer)` 把 pairer 写进
//!    pending entry；幂等：同 user_id 重复 attach 不报错
//! 3. 桌面端 `approve(code)` → 把 pairer 写进 config.json allowlist，从 store 删
//! 4. `reject(code)` → 仅删除，不写盘
//! 5. 5 min 后未 approve 的 code 由 list_pending 中的 expire sweep 删除
//!
//! ## PR4 落盘（spec §6.1）
//!
//! `begin / attempt_attach / take / drop` 四个写操作在释放 write guard 之后
//! 调 `persist()` 把当前未过期 entry 原子写到 `pending-pairings.json`。
//! 启动时通过 `load_from_disk` 重建内存 store，过期 entry 自动过滤。

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::Result;
use rand::{thread_rng, Rng};
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;

const CODE_LEN: usize = 8;
const CODE_ALPHABET: &[u8] = b"ABCDEFGHJKMNPQRSTUVWXYZ23456789"; // 去掉 O/0/I/1/L
pub const PAIRING_CODE_TTL: Duration = Duration::from_secs(300);

#[derive(Debug, Clone)]
pub struct PairerInfo {
    pub user_id: i64,
    pub first_name: String,
    pub username: Option<String>,
    pub chat_id: i64,
    pub attached_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone)]
pub struct PendingPairing {
    pub code: String,
    pub created_at: Instant,
    pub expires_at: Instant,
    pub pairer: Option<PairerInfo>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AttachOutcome {
    /// 第一次 attach，已写入 pairer
    Attached,
    /// 同一个 user 重复 attach（幂等成功）
    AlreadyAttached,
    /// code 已被另一个 user 占用
    Conflict,
    /// code 不存在或过期
    NotFound,
}

// ── 落盘格式（PR4）──────────────────────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PersistedPairer {
    user_id: i64,
    first_name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    username: Option<String>,
    chat_id: i64,
    /// RFC 3339
    attached_at: String,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PersistedPairing {
    code: String,
    created_at_unix_millis: i64,
    expires_at_unix_millis: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    attached_user: Option<PersistedPairer>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PendingPairingsFile {
    schema_version: u32,
    pending_pairings: Vec<PersistedPairing>,
}

// ── Store ────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct PairingCodeStore {
    inner: Arc<RwLock<HashMap<String, PendingPairing>>>,
    save_path: Option<Arc<std::path::PathBuf>>,
}

impl Default for PairingCodeStore {
    fn default() -> Self {
        Self::new()
    }
}

impl PairingCodeStore {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(RwLock::new(HashMap::new())),
            save_path: None,
        }
    }

    /// Builder：启用落盘到指定路径。
    pub fn with_save_path(mut self, path: std::path::PathBuf) -> Self {
        self.save_path = Some(Arc::new(path));
        self
    }

    /// 从磁盘加载（PR4）。文件不存在 / 解析失败 → 空 store，log warn，不 panic。
    pub async fn load_from_disk(path: &Path) -> Self {
        let store = Self::new();
        if !path.exists() {
            return store;
        }
        let raw = match tokio::fs::read_to_string(path).await {
            Ok(s) => s,
            Err(e) => {
                log::warn!("[telegram-pairing] load_from_disk read error: {e:?}");
                return store;
            }
        };
        let file: PendingPairingsFile = match serde_json::from_str(&raw) {
            Ok(f) => f,
            Err(e) => {
                log::warn!("[telegram-pairing] load_from_disk parse error: {e:?}");
                return store;
            }
        };

        let now_instant = Instant::now();
        let now_ms = chrono::Utc::now().timestamp_millis();

        let mut guard = store.inner.write().await;
        for p in file.pending_pairings {
            // 过滤过期
            if p.expires_at_unix_millis <= now_ms {
                continue;
            }
            let remaining_ms = (p.expires_at_unix_millis - now_ms).max(0) as u64;
            let expires_at = now_instant + Duration::from_millis(remaining_ms);

            let created_elapsed_ms = (now_ms - p.created_at_unix_millis).max(0) as u64;
            // created_at 在 Instant 上尽量还原（可能略偏，但仅用于 list_pending 排序）
            let created_at = now_instant
                .checked_sub(Duration::from_millis(created_elapsed_ms))
                .unwrap_or(now_instant);

            let pairer = p.attached_user.map(|a| PairerInfo {
                user_id: a.user_id,
                first_name: a.first_name,
                username: a.username,
                chat_id: a.chat_id,
                attached_at: chrono::DateTime::parse_from_rfc3339(&a.attached_at)
                    .map(|dt| dt.with_timezone(&chrono::Utc))
                    .unwrap_or_else(|_| chrono::Utc::now()),
            });

            guard.insert(
                p.code.clone(),
                PendingPairing {
                    code: p.code,
                    created_at,
                    expires_at,
                    pairer,
                },
            );
        }
        drop(guard);
        store
    }

    /// 写当前未过期 entry 到磁盘（原子写）。
    pub async fn save_to_disk(&self, path: &Path) -> Result<()> {
        let now_ms = chrono::Utc::now().timestamp_millis();
        let now_instant = Instant::now();

        let guard = self.inner.read().await;
        let pairings: Vec<PersistedPairing> = guard
            .values()
            .filter(|e| e.expires_at > now_instant)
            .map(|e| {
                // 将 Instant 转换回 unix millis（近似值）。
                let expires_remaining = e
                    .expires_at
                    .checked_duration_since(now_instant)
                    .unwrap_or_default();
                let expires_at_unix_millis = now_ms + expires_remaining.as_millis() as i64;

                let created_elapsed = now_instant
                    .checked_duration_since(e.created_at)
                    .unwrap_or_default();
                let created_at_unix_millis = now_ms - created_elapsed.as_millis() as i64;

                let attached_user = e.pairer.as_ref().map(|p| PersistedPairer {
                    user_id: p.user_id,
                    first_name: p.first_name.clone(),
                    username: p.username.clone(),
                    chat_id: p.chat_id,
                    attached_at: p.attached_at.to_rfc3339(),
                });

                PersistedPairing {
                    code: e.code.clone(),
                    created_at_unix_millis,
                    expires_at_unix_millis,
                    attached_user,
                }
            })
            .collect();
        drop(guard);

        let file = PendingPairingsFile {
            schema_version: 1,
            pending_pairings: pairings,
        };
        let json = serde_json::to_string_pretty(&file)?;

        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }

        // 原子写：先写 .tmp，再 rename
        let tmp_path = path.with_extension("json.tmp");
        tokio::fs::write(&tmp_path, json.as_bytes()).await?;
        tokio::fs::rename(&tmp_path, path).await?;
        Ok(())
    }

    /// 内部：若有 save_path 则 fire-and-forget 写盘（guard 已 drop 后调用）。
    async fn persist(&self) {
        let Some(path) = &self.save_path else {
            return;
        };
        if let Err(e) = self.save_to_disk(path).await {
            log::warn!("[telegram-pairing] save_to_disk failed: {e:?}");
        }
    }

    /// 生成新 code 并放入 store。
    pub async fn begin(&self) -> Result<PendingPairing> {
        use std::collections::hash_map::Entry;
        let entry_out;
        {
            let mut guard = self.inner.write().await;
            // 80 次尝试足够稀疏地避免冲突（31^8 = 8.5e11）。
            let mut found = None;
            for _ in 0..80 {
                let code = random_code();
                if let Entry::Vacant(slot) = guard.entry(code.clone()) {
                    let now = Instant::now();
                    let entry = PendingPairing {
                        code: code.clone(),
                        created_at: now,
                        expires_at: now + PAIRING_CODE_TTL,
                        pairer: None,
                    };
                    slot.insert(entry.clone());
                    found = Some(entry);
                    break;
                }
            }
            entry_out = found;
        } // guard dropped here
        self.persist().await;
        entry_out.ok_or_else(|| {
            anyhow::anyhow!("failed to generate unique pairing code after 80 attempts")
        })
    }

    /// bot 收到 /start <code> 时调。
    pub async fn attempt_attach(&self, code: &str, pairer: PairerInfo) -> AttachOutcome {
        let outcome;
        {
            let mut guard = self.inner.write().await;
            let entry = match guard.get_mut(code) {
                Some(e) => e,
                None => return AttachOutcome::NotFound,
            };
            if entry.expires_at < Instant::now() {
                guard.remove(code);
                return AttachOutcome::NotFound;
            }
            outcome = match &entry.pairer {
                None => {
                    entry.pairer = Some(pairer);
                    AttachOutcome::Attached
                }
                Some(existing) if existing.user_id == pairer.user_id => {
                    AttachOutcome::AlreadyAttached
                }
                Some(_) => AttachOutcome::Conflict,
            };
        } // guard dropped here
        self.persist().await;
        outcome
    }

    /// 桌面端 approve 取走 entry（移除 + 返回）。
    pub async fn take(&self, code: &str) -> Option<PendingPairing> {
        let result;
        {
            let mut guard = self.inner.write().await;
            let entry = guard.remove(code)?;
            result = if entry.expires_at < Instant::now() {
                None
            } else {
                Some(entry)
            };
        } // guard dropped here
        self.persist().await;
        result
    }

    /// 列出所有已被扫码的 pending pairing（pairer.is_some()），按 attached_at 降序。
    /// 同时顺手清理过期 entry。
    pub async fn list_pending(&self) -> Vec<PendingPairing> {
        let now = Instant::now();
        let mut out: Vec<PendingPairing> = {
            let mut guard = self.inner.write().await;
            guard.retain(|_, e| e.expires_at > now);
            guard
                .values()
                .filter(|e| e.pairer.is_some())
                .cloned()
                .collect()
        };
        out.sort_by(|a, b| {
            b.pairer
                .as_ref()
                .map(|p| p.attached_at)
                .cmp(&a.pairer.as_ref().map(|p| p.attached_at))
        });
        out
    }

    /// 桌面端 reject。
    pub async fn drop(&self, code: &str) {
        {
            let mut guard = self.inner.write().await;
            guard.remove(code);
        } // guard dropped here
        self.persist().await;
    }
}

/// 在 telegram 平台目录下拼出 pending-pairings.json 完整路径。
/// `telegram_dir` 必须是当前 user scope 下的 `channels/telegram/` 目录
/// （由 `ChannelConfigStore::platform_dir(Platform::Telegram)` 计算）。
pub fn pending_path_in(telegram_dir: &std::path::Path) -> std::path::PathBuf {
    telegram_dir.join("pending-pairings.json")
}

fn random_code() -> String {
    let mut rng = thread_rng();
    let mut out = String::with_capacity(CODE_LEN);
    for _ in 0..CODE_LEN {
        let idx = rng.gen_range(0..CODE_ALPHABET.len());
        out.push(CODE_ALPHABET[idx] as char);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pairer(uid: i64) -> PairerInfo {
        PairerInfo {
            user_id: uid,
            first_name: format!("u{uid}"),
            username: None,
            chat_id: uid,
            attached_at: chrono::Utc::now(),
        }
    }

    #[tokio::test]
    async fn begin_returns_8char_uppercase_code() {
        let s = PairingCodeStore::new();
        let p = s.begin().await.unwrap();
        assert_eq!(p.code.len(), CODE_LEN);
        assert!(p.code.chars().all(|c| CODE_ALPHABET.contains(&(c as u8))));
        assert!(p.pairer.is_none());
    }

    #[tokio::test]
    async fn attempt_attach_first_succeeds_and_second_same_user_is_idempotent() {
        let s = PairingCodeStore::new();
        let p = s.begin().await.unwrap();
        assert_eq!(
            s.attempt_attach(&p.code, pairer(42)).await,
            AttachOutcome::Attached
        );
        assert_eq!(
            s.attempt_attach(&p.code, pairer(42)).await,
            AttachOutcome::AlreadyAttached
        );
    }

    #[tokio::test]
    async fn attempt_attach_with_different_user_returns_conflict() {
        let s = PairingCodeStore::new();
        let p = s.begin().await.unwrap();
        s.attempt_attach(&p.code, pairer(42)).await;
        assert_eq!(
            s.attempt_attach(&p.code, pairer(43)).await,
            AttachOutcome::Conflict
        );
    }

    #[tokio::test]
    async fn unknown_code_returns_not_found() {
        let s = PairingCodeStore::new();
        assert_eq!(
            s.attempt_attach("ZZZZZZZZ", pairer(1)).await,
            AttachOutcome::NotFound
        );
    }

    #[tokio::test]
    async fn list_pending_only_returns_attached_entries() {
        let s = PairingCodeStore::new();
        let p1 = s.begin().await.unwrap();
        let p2 = s.begin().await.unwrap();
        s.attempt_attach(&p1.code, pairer(42)).await;
        // p2 未 attach
        let _ = p2;
        let list = s.list_pending().await;
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].code, p1.code);
    }

    #[tokio::test]
    async fn take_removes_entry() {
        let s = PairingCodeStore::new();
        let p = s.begin().await.unwrap();
        s.attempt_attach(&p.code, pairer(42)).await;
        assert!(s.take(&p.code).await.is_some());
        assert!(s.take(&p.code).await.is_none());
    }

    // ── PR4 落盘测试 ──────────────────────────────────────────────────────────

    #[tokio::test]
    async fn save_and_load_roundtrip() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("pending-pairings.json");

        // 创建 store 写盘
        let store = PairingCodeStore::new().with_save_path(path.clone());
        let pending = store.begin().await.unwrap();
        store.attempt_attach(&pending.code, pairer(99)).await;

        // 从磁盘重新加载
        let store2 = PairingCodeStore::load_from_disk(&path).await;
        let list = store2.list_pending().await;
        assert_eq!(
            list.len(),
            1,
            "reloaded store should have 1 pending pairing"
        );
        assert_eq!(list[0].code, pending.code);
        assert_eq!(list[0].pairer.as_ref().unwrap().user_id, 99);
    }

    #[tokio::test]
    async fn nonexistent_file_returns_empty_store() {
        let path = std::path::PathBuf::from("/nonexistent/path/pending-pairings.json");
        let store = PairingCodeStore::load_from_disk(&path).await;
        let list = store.list_pending().await;
        assert!(list.is_empty());
    }

    #[tokio::test]
    async fn corrupt_file_returns_empty_store() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("pending-pairings.json");
        tokio::fs::write(&path, b"not valid json!!!").await.unwrap();
        let store = PairingCodeStore::load_from_disk(&path).await;
        let list = store.list_pending().await;
        assert!(list.is_empty(), "corrupt file should yield empty store");
    }

    #[tokio::test]
    async fn expired_entries_filtered_on_load() {
        let dir = tempfile::TempDir::new().unwrap();
        let path = dir.path().join("pending-pairings.json");

        // 写一个已经过期的 entry（expires_at = 1ms 前）
        let now_ms = chrono::Utc::now().timestamp_millis();
        let expired_file = serde_json::json!({
            "schemaVersion": 1,
            "pendingPairings": [
                {
                    "code": "EXPIRED1",
                    "createdAtUnixMillis": now_ms - 400_000,
                    "expiresAtUnixMillis": now_ms - 1,
                    "attachedUser": null
                }
            ]
        });
        tokio::fs::write(&path, expired_file.to_string().as_bytes())
            .await
            .unwrap();

        let store = PairingCodeStore::load_from_disk(&path).await;
        // 过期 entry 应该被过滤掉，inner HashMap 为空
        let list = store.list_pending().await;
        assert!(
            list.is_empty(),
            "expired entries should be filtered on load"
        );
    }
}
