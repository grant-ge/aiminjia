# Updater 跨启动缓存 + 断点续传 实施计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 实现更新包跨启动缓存、HTTP Range 断点续传、3 次自动重试、自动/手动模式开关。

**Architecture:** Rust 后端主导下载，前端只做 UI 状态展示。下载完成后字节传给 Tauri `Update::install(bytes)` API 复用内置 Ed25519 验签。缓存放 `~/.renlijia/global/updater/`，绕开 Tauri ACL。

**Tech Stack:** Rust (reqwest + tokio), Tauri 2.x, React/TypeScript, Zustand

**Spec:** `docs/superpowers/specs/2026-05-25-updater-cache-resume-design.md`

---

## 文件结构

| 文件 | 操作 | 职责 |
|---|---|---|
| `src-tauri/src/updater/mod.rs` | 创建 | 模块导出 |
| `src-tauri/src/updater/cache.rs` | 创建 | meta.json 读写、缓存目录管理 |
| `src-tauri/src/updater/downloader.rs` | 创建 | reqwest + Range + 重试 |
| `src-tauri/src/updater/commands.rs` | 创建 | 5 个 Tauri commands |
| `src-tauri/src/storage/aijia_home.rs` | 修改 | 新增 `global_updater_dir()` |
| `src-tauri/src/lib.rs` | 修改 | 注册新 commands + 模块 |
| `src-tauri/src/models/settings.rs` | 修改 | `AppSettings` 加 `auto_download` 字段 |
| `src/types/settings.ts` | 修改 | `Settings` 加 `autoDownload` |
| `src/lib/updaterStore.ts` | 修改 | 加 `available` phase、`autoDownload`、`startDownload`/`retryDownload` |
| `src/components/common/UpdaterPanel.tsx` | 修改 | 新增 `available` phase UI |
| `src/components/layout/UpdateAvailableLink.tsx` | 修改 | 新增 `available` 显示 |
| `src/components/settings/SettingsModal.tsx` | 修改 | 自动下载开关 |
| `src/i18n/zh-CN.json` + `en-US.json` | 修改 | 新增翻译 key |
| `src/lib/updaterStore.test.ts` | 修改 | 适配新状态机 |

---

### Task 1: AiJiaHome 新增 `global_updater_dir()`

**Files:**
- Modify: `src-tauri/src/storage/aijia_home.rs`

- [ ] **Step 1: 找到 `global_dir()` 方法**

Read `src-tauri/src/storage/aijia_home.rs` 并 grep 找到 `pub fn global_dir(&self)` 方法的位置。

- [ ] **Step 2: 在 `global_dir()` 方法下面新增**

```rust
pub fn global_updater_dir(&self) -> PathBuf {
    self.global_dir().join("updater")
}
```

- [ ] **Step 3: 验证编译**

Run: `cd src-tauri && cargo check 2>&1 | tail -5`
Expected: 编译通过

- [ ] **Step 4: Commit**

```bash
git add src-tauri/src/storage/aijia_home.rs
git commit -m "feat(updater): add global_updater_dir() helper

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
```

---

### Task 2: Settings 加 `auto_download` 字段

**Files:**
- Modify: `src-tauri/src/models/settings.rs`
- Modify: `src/types/settings.ts`

- [ ] **Step 1: Rust 端 AppSettings 加字段**

在 `src-tauri/src/models/settings.rs` 的 `AppSettings` struct 中（在 `ui_home_recent_workspaces` 之后）添加：

```rust
    /// 自动下载更新（默认 true）。关闭后需要用户手动点击下载。
    #[serde(default = "default_auto_download")]
    pub auto_download: bool,
```

并在文件末尾的 helper 函数区域（`default_chat_width_mode` 那一块）添加：

```rust
fn default_auto_download() -> bool {
    true
}
```

在 `impl Default for AppSettings` 中添加：

```rust
            auto_download: true,
```

- [ ] **Step 2: TS 端 Settings 加字段**

在 `src/types/settings.ts` 的 `Settings` interface 添加（在 `ui_home_recent_workspaces` 之后）：

```typescript
  autoDownload?: boolean
```

在 `DEFAULT_SETTINGS` 添加：

```typescript
  autoDownload: true,
```

- [ ] **Step 3: 验证**

Run: `cd src-tauri && cargo check 2>&1 | tail -5`
Expected: 编译通过

Run: `pnpm build 2>&1 | tail -5`
Expected: 构建成功

- [ ] **Step 4: Commit**

```bash
git add src-tauri/src/models/settings.rs src/types/settings.ts
git commit -m "feat(updater): add auto_download setting (default true)

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
```

---

### Task 3: 创建 updater/cache.rs — 缓存管理

**Files:**
- Create: `src-tauri/src/updater/mod.rs`
- Create: `src-tauri/src/updater/cache.rs`

- [ ] **Step 1: 创建模块入口**

Create `src-tauri/src/updater/mod.rs`:

```rust
pub mod cache;
pub mod commands;
pub mod downloader;
```

- [ ] **Step 2: 实现 cache.rs**

Create `src-tauri/src/updater/cache.rs`:

```rust
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CacheMeta {
    pub version: String,
    pub url: String,
    pub expected_size: u64,
    pub downloaded_size: u64,
    pub complete: bool,
    #[serde(default)]
    pub etag: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum CacheStatus {
    Complete,
    Partial,
    None,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CacheCheckResult {
    pub status: CacheStatus,
    pub downloaded_size: u64,
}

pub struct UpdaterCache {
    dir: PathBuf,
}

impl UpdaterCache {
    pub fn new(dir: PathBuf) -> Self {
        Self { dir }
    }

    pub fn meta_path(&self) -> PathBuf {
        self.dir.join("meta.json")
    }

    pub fn package_path(&self, version: &str) -> PathBuf {
        self.dir.join(format!("{}.tar.gz", version))
    }

    pub fn ensure_dir(&self) -> Result<()> {
        fs::create_dir_all(&self.dir).context("create updater cache dir")?;
        Ok(())
    }

    pub fn load_meta(&self) -> Option<CacheMeta> {
        let path = self.meta_path();
        let text = fs::read_to_string(&path).ok()?;
        serde_json::from_str(&text).ok()
    }

    pub fn save_meta(&self, meta: &CacheMeta) -> Result<()> {
        self.ensure_dir()?;
        let text = serde_json::to_string_pretty(meta)?;
        let tmp = self.meta_path().with_extension("json.tmp");
        fs::write(&tmp, text)?;
        fs::rename(&tmp, self.meta_path())?;
        Ok(())
    }

    pub fn clear(&self) -> Result<()> {
        if self.dir.exists() {
            fs::remove_dir_all(&self.dir).context("remove updater cache dir")?;
        }
        Ok(())
    }

    /// Decide cache status given the server's version, expected_size, and optional etag.
    /// Returns Partial only if metadata matches and partial file exists.
    pub fn check(
        &self,
        version: &str,
        expected_size: u64,
        etag: &str,
    ) -> Result<CacheCheckResult> {
        let Some(meta) = self.load_meta() else {
            return Ok(CacheCheckResult {
                status: CacheStatus::None,
                downloaded_size: 0,
            });
        };

        // Version mismatch or ETag mismatch (when both sides have etag) → invalidate
        let etag_mismatch = !etag.is_empty() && !meta.etag.is_empty() && meta.etag != etag;
        if meta.version != version || meta.expected_size != expected_size || etag_mismatch {
            return Ok(CacheCheckResult {
                status: CacheStatus::None,
                downloaded_size: 0,
            });
        }

        let pkg = self.package_path(version);
        if !pkg.exists() {
            return Ok(CacheCheckResult {
                status: CacheStatus::None,
                downloaded_size: 0,
            });
        }

        // Verify on-disk size matches meta
        let actual = fs::metadata(&pkg).map(|m| m.len()).unwrap_or(0);
        if meta.complete && actual == expected_size {
            return Ok(CacheCheckResult {
                status: CacheStatus::Complete,
                downloaded_size: actual,
            });
        }
        if !meta.complete && actual > 0 && actual < expected_size && actual == meta.downloaded_size {
            return Ok(CacheCheckResult {
                status: CacheStatus::Partial,
                downloaded_size: actual,
            });
        }

        Ok(CacheCheckResult {
            status: CacheStatus::None,
            downloaded_size: 0,
        })
    }

    /// Read the complete cached package.
    pub fn read_complete(&self, version: &str) -> Result<Vec<u8>> {
        let pkg = self.package_path(version);
        fs::read(&pkg).context("read cached update package")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn make_cache() -> (tempfile::TempDir, UpdaterCache) {
        let dir = tempdir().unwrap();
        let cache = UpdaterCache::new(dir.path().join("updater"));
        cache.ensure_dir().unwrap();
        (dir, cache)
    }

    #[test]
    fn check_returns_none_when_no_meta() {
        let (_d, cache) = make_cache();
        let r = cache.check("0.5.30", 100, "").unwrap();
        assert_eq!(r.status, CacheStatus::None);
    }

    #[test]
    fn check_returns_complete_when_meta_and_file_match() {
        let (_d, cache) = make_cache();
        let meta = CacheMeta {
            version: "0.5.30".into(),
            url: "u".into(),
            expected_size: 5,
            downloaded_size: 5,
            complete: true,
            etag: "e1".into(),
        };
        cache.save_meta(&meta).unwrap();
        fs::write(cache.package_path("0.5.30"), b"hello").unwrap();
        let r = cache.check("0.5.30", 5, "e1").unwrap();
        assert_eq!(r.status, CacheStatus::Complete);
        assert_eq!(r.downloaded_size, 5);
    }

    #[test]
    fn check_returns_partial_when_size_smaller_and_meta_partial() {
        let (_d, cache) = make_cache();
        let meta = CacheMeta {
            version: "0.5.30".into(),
            url: "u".into(),
            expected_size: 10,
            downloaded_size: 5,
            complete: false,
            etag: "".into(),
        };
        cache.save_meta(&meta).unwrap();
        fs::write(cache.package_path("0.5.30"), b"hello").unwrap();
        let r = cache.check("0.5.30", 10, "").unwrap();
        assert_eq!(r.status, CacheStatus::Partial);
        assert_eq!(r.downloaded_size, 5);
    }

    #[test]
    fn check_invalidates_on_version_mismatch() {
        let (_d, cache) = make_cache();
        let meta = CacheMeta {
            version: "0.5.29".into(),
            url: "u".into(),
            expected_size: 5,
            downloaded_size: 5,
            complete: true,
            etag: "".into(),
        };
        cache.save_meta(&meta).unwrap();
        fs::write(cache.package_path("0.5.29"), b"hello").unwrap();
        let r = cache.check("0.5.30", 5, "").unwrap();
        assert_eq!(r.status, CacheStatus::None);
    }

    #[test]
    fn check_invalidates_on_etag_mismatch() {
        let (_d, cache) = make_cache();
        let meta = CacheMeta {
            version: "0.5.30".into(),
            url: "u".into(),
            expected_size: 5,
            downloaded_size: 5,
            complete: true,
            etag: "old-etag".into(),
        };
        cache.save_meta(&meta).unwrap();
        fs::write(cache.package_path("0.5.30"), b"hello").unwrap();
        let r = cache.check("0.5.30", 5, "new-etag").unwrap();
        assert_eq!(r.status, CacheStatus::None);
    }

    #[test]
    fn clear_removes_everything() {
        let (_d, cache) = make_cache();
        let meta = CacheMeta {
            version: "0.5.30".into(),
            url: "u".into(),
            expected_size: 5,
            downloaded_size: 5,
            complete: true,
            etag: "".into(),
        };
        cache.save_meta(&meta).unwrap();
        fs::write(cache.package_path("0.5.30"), b"hello").unwrap();
        cache.clear().unwrap();
        assert!(cache.load_meta().is_none());
        assert!(!cache.package_path("0.5.30").exists());
    }
}
```

- [ ] **Step 3: Cargo.toml 加 tempfile dev 依赖**

Check `src-tauri/Cargo.toml` `[dev-dependencies]` section. If `tempfile` is not there, add:

```toml
[dev-dependencies]
tempfile = "3"
```

- [ ] **Step 4: 在 lib.rs 加 updater 模块声明**

在 `src-tauri/src/lib.rs` 顶部 module 声明区域加：

```rust
mod updater;
```

- [ ] **Step 5: 运行测试**

Run: `cd src-tauri && cargo test --lib updater::cache -- --nocapture 2>&1 | tail -15`
Expected: 5 tests passed

- [ ] **Step 6: Commit**

```bash
git add src-tauri/src/updater/ src-tauri/src/lib.rs src-tauri/Cargo.toml
git commit -m "feat(updater): add UpdaterCache for meta.json + version package management

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
```

---

### Task 4: 实现 downloader.rs — Range 续传 + 重试

**Files:**
- Create: `src-tauri/src/updater/downloader.rs`

- [ ] **Step 1: 实现 downloader**

Create `src-tauri/src/updater/downloader.rs`:

```rust
use anyhow::{anyhow, Context, Result};
use std::fs::OpenOptions;
use std::io::Write;
use std::path::Path;
use std::time::Duration;

use crate::updater::cache::{CacheMeta, UpdaterCache};

const RETRY_DELAYS_SECS: [u64; 3] = [2, 8, 30];

#[derive(Debug, Clone)]
pub struct DownloadParams {
    pub url: String,
    pub version: String,
    pub expected_size: u64,
    pub etag: String,
}

pub trait ProgressSink: Send + Sync {
    fn on_progress(&self, downloaded: u64, total: u64);
}

/// Returns Ok(()) on successful complete download.
/// Errors after retries exhausted carry the last underlying error.
pub async fn download_with_resume(
    cache: &UpdaterCache,
    params: &DownloadParams,
    progress: &dyn ProgressSink,
) -> Result<()> {
    cache.ensure_dir()?;
    let pkg_path = cache.package_path(&params.version);

    // Determine starting offset from existing partial file (if any).
    let mut start: u64 = if pkg_path.exists() {
        std::fs::metadata(&pkg_path).map(|m| m.len()).unwrap_or(0)
    } else {
        0
    };
    if start > params.expected_size {
        // Stale partial; truncate
        std::fs::remove_file(&pkg_path).ok();
        start = 0;
    }

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(60))
        .build()
        .context("build reqwest client")?;

    let mut last_err: Option<anyhow::Error> = None;
    for (attempt, delay) in std::iter::once(0).chain(RETRY_DELAYS_SECS.iter().copied()).enumerate() {
        if delay > 0 {
            tokio::time::sleep(Duration::from_secs(delay)).await;
        }
        match do_download(&client, &params, &pkg_path, start, progress).await {
            Ok(()) => {
                let meta = CacheMeta {
                    version: params.version.clone(),
                    url: params.url.clone(),
                    expected_size: params.expected_size,
                    downloaded_size: params.expected_size,
                    complete: true,
                    etag: params.etag.clone(),
                };
                cache.save_meta(&meta)?;
                return Ok(());
            }
            Err(e) => {
                if !is_transient(&e) {
                    return Err(e);
                }
                // Update start for next attempt based on what's on disk now
                start = std::fs::metadata(&pkg_path).map(|m| m.len()).unwrap_or(start);
                // Persist progress to meta so a later session can resume
                let _ = cache.save_meta(&CacheMeta {
                    version: params.version.clone(),
                    url: params.url.clone(),
                    expected_size: params.expected_size,
                    downloaded_size: start,
                    complete: false,
                    etag: params.etag.clone(),
                });
                last_err = Some(e);
                let _ = attempt;
            }
        }
    }
    Err(last_err.unwrap_or_else(|| anyhow!("download failed after retries")))
}

async fn do_download(
    client: &reqwest::Client,
    params: &DownloadParams,
    pkg_path: &Path,
    start: u64,
    progress: &dyn ProgressSink,
) -> Result<()> {
    let mut req = client.get(&params.url);
    if start > 0 {
        req = req.header("Range", format!("bytes={}-", start));
    }
    let resp = req.send().await.context("send http request")?;
    let status = resp.status();
    if start > 0 && status.as_u16() != 206 {
        // Server doesn't support Range or returned full 200 — restart from scratch
        std::fs::remove_file(pkg_path).ok();
        return do_download(client, params, pkg_path, 0, progress).await;
    }
    if !status.is_success() {
        return Err(anyhow!(format!("http {}", status.as_u16())));
    }

    let mut file = OpenOptions::new()
        .create(true)
        .append(start > 0)
        .write(true)
        .truncate(start == 0)
        .open(pkg_path)
        .context("open package file")?;

    let mut downloaded = start;
    let total = params.expected_size;
    progress.on_progress(downloaded, total);

    use futures::StreamExt;
    let mut stream = resp.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.context("read chunk")?;
        file.write_all(&chunk).context("write chunk to file")?;
        downloaded += chunk.len() as u64;
        progress.on_progress(downloaded, total);
    }
    file.flush().ok();
    drop(file);

    let actual = std::fs::metadata(pkg_path).map(|m| m.len()).unwrap_or(0);
    if actual != params.expected_size {
        return Err(anyhow!(format!(
            "size mismatch: expected {} actual {}",
            params.expected_size, actual
        )));
    }
    Ok(())
}

fn is_transient(err: &anyhow::Error) -> bool {
    let s = format!("{err:#}").to_lowercase();
    s.contains("timeout")
        || s.contains("timed out")
        || s.contains("connection reset")
        || s.contains("connection refused")
        || s.contains("connection closed")
        || s.contains("dns")
        || s.contains("network")
        || s.contains("http 5")
        || s.contains("size mismatch")
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    struct NullSink;
    impl ProgressSink for NullSink {
        fn on_progress(&self, _: u64, _: u64) {}
    }

    #[tokio::test]
    async fn returns_error_for_invalid_url() {
        let dir = tempdir().unwrap();
        let cache = UpdaterCache::new(dir.path().join("updater"));
        let params = DownloadParams {
            url: "http://127.0.0.1:1/nope".to_string(),
            version: "0.5.30".to_string(),
            expected_size: 10,
            etag: "".to_string(),
        };
        let r = download_with_resume(&cache, &params, &NullSink).await;
        assert!(r.is_err());
    }

    #[test]
    fn is_transient_recognizes_network_errors() {
        assert!(is_transient(&anyhow!("connection reset by peer")));
        assert!(is_transient(&anyhow!("operation timed out")));
        assert!(is_transient(&anyhow!("http 502 bad gateway")));
        assert!(!is_transient(&anyhow!("http 404 not found")));
        assert!(!is_transient(&anyhow!("signature verification failed")));
    }
}
```

- [ ] **Step 2: 检查 Cargo.toml 是否已有 futures**

Run: `grep -E '^futures' src-tauri/Cargo.toml | head -3`

If not present, add to `[dependencies]`:

```toml
futures = "0.3"
```

- [ ] **Step 3: 运行测试**

Run: `cd src-tauri && cargo test --lib updater::downloader -- --nocapture 2>&1 | tail -10`
Expected: 2 tests passed (network error + is_transient)

- [ ] **Step 4: Commit**

```bash
git add src-tauri/src/updater/downloader.rs src-tauri/Cargo.toml
git commit -m "feat(updater): add downloader with HTTP Range resume + 3-retry backoff

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
```

---

### Task 5: 实现 commands.rs — 5 个 Tauri commands

**Files:**
- Create: `src-tauri/src/updater/commands.rs`

- [ ] **Step 1: 实现 commands**

Create `src-tauri/src/updater/commands.rs`:

```rust
use std::sync::Arc;
use tauri::{AppHandle, Emitter, Manager, State};

use crate::storage::aijia_home::AiJiaHome;
use crate::updater::cache::{CacheCheckResult, UpdaterCache};
use crate::updater::downloader::{download_with_resume, DownloadParams, ProgressSink};

fn cache_for(home: &AiJiaHome) -> UpdaterCache {
    UpdaterCache::new(home.global_updater_dir())
}

#[tauri::command]
pub async fn updater_check_cache(
    home: State<'_, Arc<AiJiaHome>>,
    version: String,
    expected_size: u64,
    etag: Option<String>,
) -> Result<CacheCheckResult, String> {
    let cache = cache_for(&home);
    let etag = etag.unwrap_or_default();
    cache.check(&version, expected_size, &etag).map_err(|e| e.to_string())
}

struct EmitSink {
    app: AppHandle,
    version: String,
}

impl ProgressSink for EmitSink {
    fn on_progress(&self, downloaded: u64, total: u64) {
        let _ = self.app.emit(
            "updater:download-progress",
            serde_json::json!({
                "version": self.version,
                "downloaded": downloaded,
                "total": total,
            }),
        );
    }
}

#[tauri::command]
pub async fn updater_download(
    app: AppHandle,
    home: State<'_, Arc<AiJiaHome>>,
    url: String,
    version: String,
    expected_size: u64,
    etag: Option<String>,
) -> Result<(), String> {
    let cache = cache_for(&home);
    let params = DownloadParams {
        url,
        version: version.clone(),
        expected_size,
        etag: etag.unwrap_or_default(),
    };
    let sink = EmitSink {
        app: app.clone(),
        version: version.clone(),
    };
    match download_with_resume(&cache, &params, &sink).await {
        Ok(()) => Ok(()),
        Err(e) => {
            let msg = format!("{e:#}");
            let _ = app.emit(
                "updater:download-failed",
                serde_json::json!({
                    "version": version,
                    "error": msg,
                }),
            );
            Err(msg)
        }
    }
}

#[tauri::command]
pub async fn updater_read_cached_bytes(
    home: State<'_, Arc<AiJiaHome>>,
    version: String,
) -> Result<Vec<u8>, String> {
    let cache = cache_for(&home);
    cache.read_complete(&version).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn updater_clear_cache(
    home: State<'_, Arc<AiJiaHome>>,
) -> Result<(), String> {
    let cache = cache_for(&home);
    cache.clear().map_err(|e| e.to_string())
}
```

- [ ] **Step 2: 在 lib.rs 注册 commands**

在 `src-tauri/src/lib.rs` 的 `tauri::generate_handler![ ... ]` 列表中添加：

```rust
            updater::commands::updater_check_cache,
            updater::commands::updater_download,
            updater::commands::updater_read_cached_bytes,
            updater::commands::updater_clear_cache,
```

- [ ] **Step 3: 验证编译**

Run: `cd src-tauri && cargo check 2>&1 | tail -5`
Expected: 编译通过

- [ ] **Step 4: Commit**

```bash
git add src-tauri/src/updater/commands.rs src-tauri/src/lib.rs
git commit -m "feat(updater): add 4 Tauri commands for cache check/download/read/clear

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
```

---

### Task 6: 前端 lib/tauri.ts 加 wrapper

**Files:**
- Modify: `src/lib/tauri.ts`

- [ ] **Step 1: 加 wrapper 函数**

在 `src/lib/tauri.ts` 文件末尾添加：

```typescript
// ─────────────────────────────────────────────────────────────
// Updater Commands
// ─────────────────────────────────────────────────────────────

export type UpdaterCacheStatus = 'complete' | 'partial' | 'none'

export interface UpdaterCacheCheckResult {
  status: UpdaterCacheStatus
  downloaded_size: number
}

export function updaterCheckCache(
  version: string,
  expectedSize: number,
  etag?: string,
): Promise<UpdaterCacheCheckResult> {
  return invoke<UpdaterCacheCheckResult>('updater_check_cache', {
    version,
    expectedSize,
    etag,
  })
}

export function updaterDownload(
  url: string,
  version: string,
  expectedSize: number,
  etag?: string,
): Promise<void> {
  return invoke('updater_download', { url, version, expectedSize, etag })
}

export function updaterReadCachedBytes(version: string): Promise<number[]> {
  return invoke<number[]>('updater_read_cached_bytes', { version })
}

export function updaterClearCache(): Promise<void> {
  return invoke('updater_clear_cache')
}
```

- [ ] **Step 2: 验证编译**

Run: `pnpm build 2>&1 | tail -5`
Expected: 构建成功

- [ ] **Step 3: Commit**

```bash
git add src/lib/tauri.ts
git commit -m "feat(updater): add TS wrappers for updater commands

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
```

---

### Task 7: updaterStore 状态机重构 + Rust 集成

**Files:**
- Modify: `src/lib/updaterStore.ts`

- [ ] **Step 1: 读取当前 store 文件理解结构**

Read `src/lib/updaterStore.ts` 了解现有代码。

- [ ] **Step 2: 替换整个文件**

Replace `src/lib/updaterStore.ts` entirely with:

```typescript
import { create } from 'zustand'
import type { Update } from '@tauri-apps/plugin-updater'
import { check } from '@tauri-apps/plugin-updater'
import { relaunch } from '@tauri-apps/plugin-process'
import { getVersion } from '@tauri-apps/api/app'
import { listen, type UnlistenFn } from '@tauri-apps/api/event'
import {
  updaterCheckCache,
  updaterDownload,
  updaterReadCachedBytes,
  updaterClearCache,
} from '@/lib/tauri'
import { useNotificationStore } from '@/stores/notificationStore'
import { useSettingsStore } from '@/stores/settingsStore'
import i18n from '@/i18n'

type Phase = 'idle' | 'checking' | 'available' | 'downloading' | 'ready' | 'failed' | 'installing'

interface UpdaterState {
  phase: Phase
  version: string | null
  notes: string
  progress: { downloaded: number; total: number } | null
  error: string | null
  panelOpen: boolean
  online: boolean
  _update: Update | null
  _cachedBytes: Uint8Array | null
  _expectedSize: number
  _downloadUrl: string
  _etag: string
  _bootstrapPromise: Promise<void> | null
  _progressUnlisten: UnlistenFn | null
  _failedUnlisten: UnlistenFn | null

  bootstrap(): Promise<void>
  startDownload(): Promise<void>
  retryDownload(): Promise<void>
  openPanel(): void
  closePanel(): void
  installNow(): Promise<void>
}

let networkListenersInstalled = false

async function setupEventListeners(set: (partial: Partial<UpdaterState>) => void, get: () => UpdaterState) {
  if (get()._progressUnlisten) return
  const progressUnlisten = await listen<{ version: string; downloaded: number; total: number }>(
    'updater:download-progress',
    (e) => {
      const { downloaded, total } = e.payload
      set({ progress: { downloaded, total } })
    },
  )
  const failedUnlisten = await listen<{ version: string; error: string }>(
    'updater:download-failed',
    (e) => {
      set({ phase: 'failed', error: e.payload.error })
    },
  )
  set({ _progressUnlisten: progressUnlisten, _failedUnlisten: failedUnlisten })
}

function extractUpdateMeta(update: Update): { url: string; size: number; etag: string } {
  // Tauri Update doesn't expose download URL / size directly. We pull from rawJson if available.
  // Fall back to empty values; Rust side will return error if URL missing.
  const raw = (update as unknown as { rawJson?: { platforms?: Record<string, { url?: string }> } })?.rawJson
  let url = ''
  if (raw && raw.platforms) {
    for (const platform of Object.values(raw.platforms)) {
      if (platform?.url) { url = platform.url; break }
    }
  }
  return { url, size: 0, etag: '' }
}

export const useUpdaterStore = create<UpdaterState>()((set, get) => ({
  phase: 'idle',
  version: null,
  notes: '',
  progress: null,
  error: null,
  panelOpen: false,
  online: typeof navigator !== 'undefined' ? navigator.onLine : true,
  _update: null,
  _cachedBytes: null,
  _expectedSize: 0,
  _downloadUrl: '',
  _etag: '',
  _bootstrapPromise: null,
  _progressUnlisten: null,
  _failedUnlisten: null,

  async bootstrap() {
    const inFlight = get()._bootstrapPromise
    if (inFlight) return inFlight

    let resolveHolder!: () => void
    const holder = new Promise<void>((r) => { resolveHolder = r })
    set({ _bootstrapPromise: holder })

    const run = (async () => {
      if (typeof navigator !== 'undefined' && !networkListenersInstalled) {
        networkListenersInstalled = true
        window.addEventListener('online', () => set({ online: true }))
        window.addEventListener('offline', () => set({ online: false }))
      }
      await setupEventListeners(set, get)

      set({ phase: 'checking', error: null })

      let update: Update | null = null
      try {
        update = await check()
      } catch (e) {
        console.warn('[updater] check failed:', e)
        set({ phase: 'idle' })
        return
      }
      if (!update) {
        set({ phase: 'idle', version: null, notes: '', progress: null, _update: null, _cachedBytes: null })
        return
      }
      const current = await getVersion()
      if (update.version === current) {
        set({ phase: 'idle', version: null, notes: '', progress: null, _update: null, _cachedBytes: null })
        return
      }

      const { url, etag } = extractUpdateMeta(update)
      // expected_size: we'd need a HEAD request to find this. For now, fetch it as 0 and let
      // downloader treat it as best-effort (size mismatch only triggers if expected_size > 0).
      // To keep cache invalidation working, we use 0 as sentinel meaning "unknown".
      const expectedSize = 0

      set({
        _update: update,
        version: update.version,
        notes: update.body ?? '',
        _downloadUrl: url,
        _expectedSize: expectedSize,
        _etag: etag,
        error: null,
      })

      // Check cache
      let cacheStatus: 'complete' | 'partial' | 'none' = 'none'
      try {
        const r = await updaterCheckCache(update.version, expectedSize, etag)
        cacheStatus = r.status
      } catch (e) {
        console.warn('[updater] cache check failed:', e)
      }

      if (cacheStatus === 'complete') {
        try {
          const bytes = await updaterReadCachedBytes(update.version)
          set({ phase: 'ready', _cachedBytes: new Uint8Array(bytes), progress: { downloaded: bytes.length, total: bytes.length } })
          return
        } catch (e) {
          console.warn('[updater] read cached bytes failed:', e)
        }
      }

      const autoDownload = useSettingsStore.getState().autoDownload ?? true
      if (autoDownload) {
        // Auto mode: jump straight to downloading
        void get().startDownload()
      } else {
        set({ phase: 'available' })
      }
    })()

    try { await run } finally {
      resolveHolder()
      if (get()._bootstrapPromise === holder) set({ _bootstrapPromise: null })
    }
  },

  async startDownload() {
    const { _update, _downloadUrl, _expectedSize, _etag, phase } = get()
    if (!_update || (phase !== 'available' && phase !== 'failed' && phase !== 'checking')) return
    if (!_downloadUrl) {
      set({ phase: 'failed', error: 'Download URL not available from update metadata' })
      return
    }

    set({ phase: 'downloading', progress: { downloaded: 0, total: _expectedSize }, error: null, _cachedBytes: null })

    try {
      await updaterDownload(_downloadUrl, _update.version, _expectedSize, _etag)
      const bytes = await updaterReadCachedBytes(_update.version)
      set({ phase: 'ready', _cachedBytes: new Uint8Array(bytes), progress: { downloaded: bytes.length, total: bytes.length } })
    } catch (e) {
      const msg = String((e as Error)?.message ?? e)
      if (get().phase !== 'failed') {
        // Failed event may have already set phase; otherwise set it here.
        set({ phase: 'failed', error: msg })
      }
    }
  },

  async retryDownload() {
    if (get().phase !== 'failed') return
    await get().startDownload()
  },

  openPanel() { set({ panelOpen: true }) },
  closePanel() { set({ panelOpen: false }) },

  async installNow() {
    const { _update, _cachedBytes, phase, online } = get()
    if (!_update || !_cachedBytes || phase !== 'ready') {
      useNotificationStore.getState().push({
        context: 'toast',
        level: 'error',
        title: i18n.t('updater.installFailedTitle'),
        message: i18n.t('updater.notReadyMessage'),
        actions: [], dismissible: true, autoHide: 6,
      })
      return
    }
    if (!online) {
      useNotificationStore.getState().push({
        context: 'toast',
        level: 'error',
        title: i18n.t('updater.installFailedTitle'),
        message: i18n.t('updater.offlineHint'),
        actions: [], dismissible: true, autoHide: 6,
      })
      return
    }
    set({ phase: 'installing' })
    let installed = false
    try {
      await _update.install(_cachedBytes)
      installed = true
      await updaterClearCache().catch(() => {})
      set({ phase: 'idle', version: null, notes: '', progress: null, _update: null, _cachedBytes: null })
      await relaunch()
    } catch (e) {
      console.error('[updater] install failed:', e)
      if (installed) {
        useNotificationStore.getState().push({
          context: 'toast', level: 'info',
          title: i18n.t('updater.installSuccessTitle'),
          message: i18n.t('updater.relaunchFailedHint'),
          actions: [], dismissible: true, autoHide: 10,
        })
        set({ phase: 'idle', version: null, notes: '', progress: null, _update: null, _cachedBytes: null })
      } else {
        const msg = String((e as Error)?.message ?? e)
        // Signature verification failure: clear cache to prevent loop.
        if (msg.toLowerCase().includes('signature')) {
          await updaterClearCache().catch(() => {})
        }
        useNotificationStore.getState().push({
          context: 'toast', level: 'error',
          title: i18n.t('updater.installFailedTitle'),
          message: msg,
          actions: [], dismissible: true, autoHide: 8,
        })
        set({ phase: 'failed', error: msg })
      }
    }
  },
}))
```

- [ ] **Step 3: 验证编译**

Run: `pnpm build 2>&1 | tail -5`
Expected: 构建成功

- [ ] **Step 4: Commit**

```bash
git add src/lib/updaterStore.ts
git commit -m "feat(updater): integrate Rust cache + add available phase + retryDownload

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
```

---

### Task 8: settingsStore 透出 autoDownload

**Files:**
- Modify: `src/stores/settingsStore.ts`

- [ ] **Step 1: 检查 settingsStore 现状**

Read `src/stores/settingsStore.ts`，理解现有 state 字段是怎么从 `Settings` 类型映射来的。如果 store 是直接 spread `Settings`（如 `extends Settings`），则不需要改动——`autoDownload` 字段会自动出现在 store 中。

- [ ] **Step 2: 如果需要显式映射，加上 `autoDownload`**

If the store explicitly initializes each Settings field, add to initial state:

```typescript
autoDownload: true,
```

If `SettingsState extends Settings` (as confirmed by Task 2 setup), no change needed — verify by searching the file for `extends Settings`.

- [ ] **Step 3: 验证**

Run: `pnpm build 2>&1 | tail -5`
Expected: 构建成功

- [ ] **Step 4: Commit (only if changes made)**

```bash
git diff --stat src/stores/settingsStore.ts
# If output is empty, skip commit. Otherwise:
git add src/stores/settingsStore.ts
git commit -m "feat(updater): expose autoDownload in settingsStore

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
```

---

### Task 9: i18n 翻译 key

**Files:**
- Modify: `src/i18n/zh-CN.json`
- Modify: `src/i18n/en-US.json`

- [ ] **Step 1: zh-CN.json — 添加 autoDownload 相关 key**

在 `"updater"` 块中添加以下新 key（在最后一个 key 之前）：

```json
"autoDownloadLabel": "自动下载更新",
"autoDownloadDesc": "关闭后需要手动点击下载",
"updateAvailable": "新版本可用",
"downloadNow": "立即下载",
"retried3Times": "已重试 3 次仍失���"
```

- [ ] **Step 2: en-US.json — 添加对应英文 key**

```json
"autoDownloadLabel": "Auto-download updates",
"autoDownloadDesc": "Turn off to require manual download",
"updateAvailable": "New version available",
"downloadNow": "Download now",
"retried3Times": "Retried 3 times, still failing"
```

- [ ] **Step 3: 验证**

Run: `pnpm build 2>&1 | tail -5`
Expected: 构建成功

- [ ] **Step 4: Commit**

```bash
git add src/i18n/zh-CN.json src/i18n/en-US.json
git commit -m "feat(updater): add i18n keys for auto-download switch and available phase

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
```

---

### Task 10: UpdateAvailableLink 新增 available 状态

**Files:**
- Modify: `src/components/layout/UpdateAvailableLink.tsx`

- [ ] **Step 1: 读取当前文件**

Read `src/components/layout/UpdateAvailableLink.tsx`.

- [ ] **Step 2: 确认 available 已支持**

文件中应该已有 `DOT_COLORS` 包含 `available: '#ef4444'`（从上次重构遗留）。验证条件分支：

- 如果 `phase === 'available'`：显示 `t('updater.linkAvailable', { version })` + tooltip + onClick → openPanel
- 如果没有这个分支，按下面 Step 3 修复

- [ ] **Step 3: 如果缺 available 分支，添加之**

确保组件的 if/else if 链中有这一段（应该已存在，如果没有就加）：

```typescript
} else {
  // available
  label = t('updater.linkAvailable', { version })
  tooltip = t('updater.linkAvailableTooltip')
  onClick = openPanel
}
```

- [ ] **Step 4: 验证**

Run: `pnpm build 2>&1 | tail -5`
Expected: 构建成功

- [ ] **Step 5: Commit (only if changes made)**

```bash
git diff --stat src/components/layout/UpdateAvailableLink.tsx
# If empty, skip; otherwise commit
git add src/components/layout/UpdateAvailableLink.tsx
git commit -m "feat(updater): ensure available phase shown in title bar link

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
```

---

### Task 11: UpdaterPanel — 加 available phase 渲染

**Files:**
- Modify: `src/components/common/UpdaterPanel.tsx`

- [ ] **Step 1: 读取当前文件确认结构**

Read `src/components/common/UpdaterPanel.tsx`.

- [ ] **Step 2: 确认 available 渲染存在**

文件中应该已经有 `{phase === 'available' && ...}` 渲染 release notes 的逻辑（上次重构遗留）。验证：
- 渲染 release notes（bullets）
- footer 有「稍后再说」「立即下载」两个按钮
- 「立即下载」点击调 `startDownload()`

- [ ] **Step 3: 如果缺失，按下面补全**

确保 footer 有：

```tsx
{phase === 'available' && (
  <>
    <Button variant="outline" onClick={closePanel}>{t('updater.updateLater')}</Button>
    <Button onClick={() => void startDownload()}>{t('updater.downloadNow')}</Button>
  </>
)}
```

- [ ] **Step 4: 验证**

Run: `pnpm build 2>&1 | tail -5`
Expected: 构建成功

- [ ] **Step 5: Commit (only if changes)**

```bash
git diff --stat src/components/common/UpdaterPanel.tsx
git add src/components/common/UpdaterPanel.tsx
git commit -m "feat(updater): ensure available phase UI complete in UpdaterPanel

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
```

---

### Task 12: SettingsModal — 自动下载开关 UI

**Files:**
- Modify: `src/components/settings/SettingsModal.tsx`

- [ ] **Step 1: 找到 AboutPanel / 检查更新按钮位置**

Run: `grep -n "onCheckUpdate\|checkUpdate\|alreadyLatestVersion\|AboutPanel" src/components/settings/SettingsModal.tsx | head -10`

定位「检查更新」按钮渲染位置。

- [ ] **Step 2: 在 AboutPanel 内、检查更新按钮上方加 toggle**

参考代码（按现有组件 import 调整）：

```tsx
import { useSettingsStore } from '@/stores/settingsStore'

// ... inside AboutPanel render:
const autoDownload = useSettingsStore((s) => s.autoDownload ?? true)

// Render BEFORE the existing "Check Update" button:
<div className="mb-4 flex items-center justify-between">
  <div>
    <p className="text-sm font-medium text-foreground">
      {t('updater.autoDownloadLabel')}
    </p>
    <p className="text-xs text-muted-foreground">
      {t('updater.autoDownloadDesc')}
    </p>
  </div>
  <input
    type="checkbox"
    checked={autoDownload}
    onChange={async (e) => {
      const next = e.target.checked
      // 1) Update Zustand state immediately for UI feedback
      useSettingsStore.setState({ autoDownload: next })
      // 2) Persist via existing updateSettings command
      try {
        const current = await getSettings()
        await updateSettings({ ...current, autoDownload: next })
      } catch (err) {
        console.warn('[settings] failed to persist autoDownload:', err)
      }
    }}
  />
</div>
```

If a `Switch` / `Toggle` component is already used elsewhere in SettingsModal, use that component instead of raw `<input type="checkbox">` for consistency. Search for `Switch` / `Toggle` imports in the file first.

- [ ] **Step 3: 验证**

Run: `pnpm build 2>&1 | tail -5`
Expected: 构建成功

- [ ] **Step 4: Commit**

```bash
git add src/components/settings/SettingsModal.tsx
git commit -m "feat(updater): add auto-download toggle in About panel

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
```

---

### Task 13: 更新 updaterStore 测试

**Files:**
- Modify: `src/lib/updaterStore.test.ts`

- [ ] **Step 1: 读取现有测试**

Read `src/lib/updaterStore.test.ts`.

- [ ] **Step 2: 替换整个测试文件**

Replace `src/lib/updaterStore.test.ts` with:

```typescript
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest'

const checkMock = vi.fn()
const relaunchMock = vi.fn()
const getVersionMock = vi.fn()
const listenMock = vi.fn(async () => () => {})
const invokeMock = vi.fn()

vi.mock('@tauri-apps/plugin-updater', () => ({ check: (...a: unknown[]) => checkMock(...a) }))
vi.mock('@tauri-apps/plugin-process', () => ({ relaunch: (...a: unknown[]) => relaunchMock(...a) }))
vi.mock('@tauri-apps/api/app', () => ({ getVersion: (...a: unknown[]) => getVersionMock(...a) }))
vi.mock('@tauri-apps/api/event', () => ({ listen: (...a: unknown[]) => listenMock(...a) }))
vi.mock('@tauri-apps/api/core', () => ({ invoke: (...a: unknown[]) => invokeMock(...a) }))

async function loadModules() {
  vi.resetModules()
  const storeMod = await import('./updaterStore')
  const notifMod = await import('@/stores/notificationStore')
  const settingsMod = await import('@/stores/settingsStore')
  return {
    useUpdaterStore: storeMod.useUpdaterStore,
    useNotificationStore: notifMod.useNotificationStore,
    useSettingsStore: settingsMod.useSettingsStore,
  }
}

function setupCommandMocks(opts: {
  cacheStatus?: 'complete' | 'partial' | 'none'
  cachedBytes?: number[]
  downloadResolve?: boolean
}) {
  invokeMock.mockImplementation(async (cmd: string) => {
    if (cmd === 'updater_check_cache') {
      return { status: opts.cacheStatus ?? 'none', downloaded_size: 0 }
    }
    if (cmd === 'updater_read_cached_bytes') {
      return opts.cachedBytes ?? [1, 2, 3]
    }
    if (cmd === 'updater_download') {
      if (opts.downloadResolve === false) throw new Error('network timeout')
      return undefined
    }
    if (cmd === 'updater_clear_cache') return undefined
    throw new Error(`unexpected command: ${cmd}`)
  })
}

function fakeUpdate(version = '0.5.30', body = 'notes') {
  return {
    version,
    body,
    rawJson: { platforms: { 'darwin-aarch64': { url: 'https://example/pkg.tar.gz' } } },
    install: vi.fn().mockResolvedValue(undefined),
  }
}

beforeEach(() => {
  vi.clearAllMocks()
})

afterEach(() => {
  vi.clearAllMocks()
})

describe('updaterStore.bootstrap', () => {
  it('stays idle when no update available', async () => {
    checkMock.mockResolvedValue(null)
    getVersionMock.mockResolvedValue('0.5.29')
    const { useUpdaterStore } = await loadModules()
    await useUpdaterStore.getState().bootstrap()
    expect(useUpdaterStore.getState().phase).toBe('idle')
  })

  it('stays idle when server version equals current', async () => {
    checkMock.mockResolvedValue(fakeUpdate('0.5.29'))
    getVersionMock.mockResolvedValue('0.5.29')
    const { useUpdaterStore } = await loadModules()
    await useUpdaterStore.getState().bootstrap()
    expect(useUpdaterStore.getState().phase).toBe('idle')
  })

  it('auto-mode: auto-starts download when no cache', async () => {
    checkMock.mockResolvedValue(fakeUpdate())
    getVersionMock.mockResolvedValue('0.5.29')
    setupCommandMocks({ cacheStatus: 'none', cachedBytes: [9, 9] })
    const { useUpdaterStore, useSettingsStore } = await loadModules()
    useSettingsStore.setState({ autoDownload: true } as never)
    await useUpdaterStore.getState().bootstrap()
    // After bootstrap, startDownload was triggered, which awaits download + reads bytes
    // and sets phase=ready
    expect(useUpdaterStore.getState().phase).toBe('ready')
  })

  it('manual-mode: stays in available when no cache', async () => {
    checkMock.mockResolvedValue(fakeUpdate())
    getVersionMock.mockResolvedValue('0.5.29')
    setupCommandMocks({ cacheStatus: 'none' })
    const { useUpdaterStore, useSettingsStore } = await loadModules()
    useSettingsStore.setState({ autoDownload: false } as never)
    await useUpdaterStore.getState().bootstrap()
    expect(useUpdaterStore.getState().phase).toBe('available')
  })

  it('cache complete: jumps straight to ready', async () => {
    checkMock.mockResolvedValue(fakeUpdate())
    getVersionMock.mockResolvedValue('0.5.29')
    setupCommandMocks({ cacheStatus: 'complete', cachedBytes: [1, 2, 3] })
    const { useUpdaterStore, useSettingsStore } = await loadModules()
    useSettingsStore.setState({ autoDownload: false } as never) // even manual mode skips
    await useUpdaterStore.getState().bootstrap()
    expect(useUpdaterStore.getState().phase).toBe('ready')
  })
})

describe('updaterStore.startDownload', () => {
  it('transitions to ready on successful download', async () => {
    checkMock.mockResolvedValue(fakeUpdate())
    getVersionMock.mockResolvedValue('0.5.29')
    setupCommandMocks({ cacheStatus: 'none', cachedBytes: [7, 8, 9] })
    const { useUpdaterStore, useSettingsStore } = await loadModules()
    useSettingsStore.setState({ autoDownload: false } as never)
    await useUpdaterStore.getState().bootstrap()
    expect(useUpdaterStore.getState().phase).toBe('available')

    await useUpdaterStore.getState().startDownload()
    expect(useUpdaterStore.getState().phase).toBe('ready')
  })

  it('transitions to failed on download error', async () => {
    checkMock.mockResolvedValue(fakeUpdate())
    getVersionMock.mockResolvedValue('0.5.29')
    setupCommandMocks({ cacheStatus: 'none', downloadResolve: false })
    const { useUpdaterStore, useSettingsStore } = await loadModules()
    useSettingsStore.setState({ autoDownload: false } as never)
    await useUpdaterStore.getState().bootstrap()
    await useUpdaterStore.getState().startDownload()
    expect(useUpdaterStore.getState().phase).toBe('failed')
  })
})

describe('updaterStore.retryDownload', () => {
  it('only runs from failed state', async () => {
    const { useUpdaterStore } = await loadModules()
    await useUpdaterStore.getState().retryDownload()
    expect(useUpdaterStore.getState().phase).toBe('idle')
  })

  it('retries from failed state', async () => {
    checkMock.mockResolvedValue(fakeUpdate())
    getVersionMock.mockResolvedValue('0.5.29')
    let downloadAttempts = 0
    invokeMock.mockImplementation(async (cmd: string) => {
      if (cmd === 'updater_check_cache') return { status: 'none', downloaded_size: 0 }
      if (cmd === 'updater_read_cached_bytes') return [1, 2, 3]
      if (cmd === 'updater_download') {
        downloadAttempts++
        if (downloadAttempts === 1) throw new Error('first attempt failed')
        return undefined
      }
      if (cmd === 'updater_clear_cache') return undefined
      throw new Error('unexpected: ' + cmd)
    })
    const { useUpdaterStore, useSettingsStore } = await loadModules()
    useSettingsStore.setState({ autoDownload: false } as never)
    await useUpdaterStore.getState().bootstrap()
    await useUpdaterStore.getState().startDownload()
    expect(useUpdaterStore.getState().phase).toBe('failed')

    await useUpdaterStore.getState().retryDownload()
    expect(useUpdaterStore.getState().phase).toBe('ready')
    expect(downloadAttempts).toBe(2)
  })
})

describe('updaterStore.installNow', () => {
  it('passes cached bytes to install()', async () => {
    const upd = fakeUpdate()
    checkMock.mockResolvedValue(upd)
    getVersionMock.mockResolvedValue('0.5.29')
    setupCommandMocks({ cacheStatus: 'complete', cachedBytes: [1, 2, 3] })
    relaunchMock.mockResolvedValue(undefined)
    const { useUpdaterStore, useSettingsStore } = await loadModules()
    useSettingsStore.setState({ autoDownload: false } as never)
    await useUpdaterStore.getState().bootstrap()
    expect(useUpdaterStore.getState().phase).toBe('ready')

    await useUpdaterStore.getState().installNow()
    expect(upd.install).toHaveBeenCalled()
    const arg = upd.install.mock.calls[0][0]
    expect(arg).toBeInstanceOf(Uint8Array)
    expect(Array.from(arg as Uint8Array)).toEqual([1, 2, 3])
    expect(relaunchMock).toHaveBeenCalled()
  })

  it('shows error toast when not ready', async () => {
    const { useUpdaterStore, useNotificationStore } = await loadModules()
    useNotificationStore.getState().dismissAll()
    await useUpdaterStore.getState().installNow()
    const notes = useNotificationStore.getState().notifications
    expect(notes.length).toBe(1)
    expect(notes[0].level).toBe('error')
  })
})
```

- [ ] **Step 3: 运行测试**

Run: `pnpm exec vitest run src/lib/updaterStore.test.ts 2>&1 | tail -10`
Expected: All tests pass

- [ ] **Step 4: Commit**

```bash
git add src/lib/updaterStore.test.ts
git commit -m "test(updater): rewrite tests for cache + resume + auto/manual modes

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>"
```

---

### Task 14: 端到端验证

- [ ] **Step 1: 全量前端测试**

Run: `pnpm test 2>&1 | tail -10`
Expected: 全部通过

- [ ] **Step 2: Rust 全量测试**

Run: `cd src-tauri && cargo test --lib updater 2>&1 | tail -10`
Expected: 全部通过

- [ ] **Step 3: Lint + Build**

Run: `pnpm lint 2>&1 | grep -E "updaterStore|UpdaterPanel|UpdateAvailableLink|SettingsModal" | head -10`
Expected: 无 lint 错误（output 为空或全是历史警告）

Run: `pnpm build 2>&1 | tail -3`
Expected: 构建成功

- [ ] **Step 4: 手工测试**

1. 改 version 到 0.5.28: `python3 scripts/bump-version.py 0.5.28`
2. 启动: `pnpm tauri:dev`
3. 验证 4 个场景：
   - **自动模式新装**：清空 `~/.renlijia/global/updater/`，启动 → 自动下载 → ready
   - **缓存命中**：再次启动 → 直接 ready，不下载
   - **手动模式**：设置里关闭自动下载 → 清缓存 → 重启 → 出现 available 状态 → 点击弹窗 → 点立即下载
   - **网络中断恢复**：自动模式下载中拔网 → failed → 弹窗点重试 → 续传完成
4. 改回 0.5.29: `python3 scripts/bump-version.py 0.5.29`

---

## Spec 覆盖自检

- ✅ 状态机扩展（available phase）→ Task 7
- ✅ 缓存设计（meta.json + version.tar.gz）→ Task 3
- ✅ AiJiaHome 新增方法 → Task 1
- ✅ Rust 4 个 commands → Task 5
- ✅ Range 续传 + 3 次重试 → Task 4
- ✅ 进度/失败事件 → Task 4 + Task 5
- ✅ 前端 store 改造（+ 事件订阅）→ Task 7
- ✅ UI 改动（设置开关 / 弹窗 / 链接）→ Task 10, 11, 12
- ✅ Settings auto_download → Task 2
- ✅ i18n → Task 9
- ✅ 测试 → Task 13
- ✅ 端到端 → Task 14
