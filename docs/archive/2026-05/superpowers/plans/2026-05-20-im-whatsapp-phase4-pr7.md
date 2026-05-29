# Phase 4 WhatsApp PR7 — 入站 IMAGE / FILE 媒体下载 + 25h 老化清理

> Subagent-driven execution. 4 decisions confirmed by user.

---

## Context

PR4 parser 把 `attachments: vec![]` 留空；IMAGE/DOCUMENT 的 caption 直接进 text。
PR7 让 AI 真能看到用户发的图片和文件 —— 下载 + 写 tmp + 填 ChannelAttachmentSpec
让 manager worker 走 chat_attachments 链路。

Spec：`docs/superpowers/specs/2026-05-18-im-whatsapp-phase4-design.md` §7。

## 4 decisions (user-confirmed)

1. **下载时机**：runtime closure 在 Event::Message handler 里调 `client.download(&img)`
   拿 bytes，写 tmp，然后才把 ChannelMessage push 进 inbound_tx。意味着 parser 改成
   `async fn normalize_async(msg, info, cfg, downloader: &WhatsAppMediaDownloader) -> Option<ChannelMessage>`。
   下载失败 → text 里加 `[附件下载失败]` 占位、attachments 留空，让对话继续。
2. **25h 老化清理**：加 `spawn_whatsapp_attachment_gc(dir)` 在 manager 启动期起一个
   tokio task 循环：sleep 1h → walk dir → mtime + 25h < now 则 fs::remove_file → repeat。
   单平台 cron，不抽 shared（其他平台无）。
3. **PR7 范围**：只下载 IMAGE / DOCUMENT 两类。VOICE/VIDEO/STICKER/Location/Contact
   保持 PR4 的占位文案行为不变。
4. **Size 检查**：不做。常量 `IMAGE_SIZE_LIMIT_BYTES = 5 * 1024 * 1024` /
   `FILE_SIZE_LIMIT_BYTES = 100 * 1024 * 1024` 加到 `download.rs`，给未来出站留位置。

---

## File structure

新建：
- `src-tauri/src/connector/im/whatsapp/download.rs` — `WhatsAppMediaDownloader { client, dest_dir }`
  + `download_image(img: &wa::message::ImageMessage, msg_id: &str)` +
  `download_document(doc: &wa::message::DocumentMessage, msg_id: &str)` + sha256 dedup +
  atomic write + mime helpers + size 常量。~250 行 + ~5 单测（纯 helper 测试，不真 download）。
- `src-tauri/src/connector/im/whatsapp/gc.rs` — `pub async fn run_attachment_gc(dir: PathBuf)` 循环
  sleep 1h + walk dir + mtime+25h<now → remove。+ `pub async fn sweep_once(dir: &Path, ttl: Duration)` 提取
  让 spawn 和单测都能调用。~80 行 + 3 单测（tempdir + 假 mtime 验证）。

修改：
- `mod.rs` — 加 `pub mod download;` + `pub mod gc;`
- `parser.rs` — 把 `pub fn normalize(...)` 改成 `pub async fn normalize_async(msg, info, cfg, downloader: &WhatsAppMediaDownloader) -> Option<ChannelMessage>`。
  IMAGE/DOCUMENT 分支真调下载；下载成功填 `ChannelAttachmentSpec { kind, download_code: local_path_str, file_name }`；失败时 text 加占位。
  10 单测里**新增** 3 个：image-with-download-success / image-with-download-fail / document-success。
  原 7 测里用 mock downloader（trait 抽象）—— 或者用 `Option<&Downloader>`，None 时跳过下载（保持 PR4 测试形态不变）。
  **决策**：用 `Option<&WhatsAppMediaDownloader>`；PR4 已有 10 个测试全部传 None；新加 3 测试构造真 downloader 但 mock client（如果 mock client 难，就 _ignored 标真 IO 测试，逻辑路径走"if downloader_is_some && msg_has_image"分支验证存在性即可）。
- `runtime.rs::start_bot` 新增 `downloader: Arc<WhatsAppMediaDownloader>` 参数（在 spawn 之前 build）；closure capture。`handle_event::Event::Message` 改调 `parser::normalize_async(..., Some(&downloader))`。
- `connector.rs::start_pairing_session` build downloader（拿 client 拿 dest_dir）传给 `runtime::start_bot`。**问题**：build downloader 需要 client，但 `start_pairing_session` 是在 client 还没起来时调的。**解决**：downloader 内部持 `Arc<Mutex<Option<Arc<Client>>>>`（**复用 connector.bot_client 字段**），下载时读取；start_bot 把 `Arc::clone(&self.bot_client)` 也给 downloader 用。
  - 或更简单：downloader 直接 `Arc::clone(&connector.bot_client)`，自己内部 lock 拿 client。
  - PR7 决策：**downloader 持 `bot_client: Arc<Mutex<Option<Arc<Client>>>>` clone**，下载时 `bot_client.lock().await.clone()` None → error。复用 PR5 字段。
- `manager.rs` — 启动期 spawn `spawn_whatsapp_attachment_gc()` 任务：用 `AiJiaHome::tmp_whatsapp_downloads_dir()` 路径，task 内 loop sleep 1h → sweep。
- `aijia_home.rs` — 加 `pub fn tmp_whatsapp_downloads_dir(&self) -> PathBuf { self.tmp_dir().join("whatsapp_downloads") }`。

不动：connector field shape / sender / aicard / mod.rs 之外 / manager worker logic（PR7 不需要 build_pending_item_from_telegram 之外的改动 —— attachments 路径已经在 ChannelMessage.attachments 字段；worker 现在跳过空 attachments，PR7 填上后自动走 chat_attachments 链路；**但**worker 没有 download_specs_for_turn_whatsapp 函数 —— 实际上 parser 已经把 local_path 写进 download_code 了，worker 不需要再 download；spec 设计把"local 路径直接给上层"。这跟 telegram 的"download_code = telegram file_id 后由 worker download"不一样，但更简单）。

⚠️ 关键 design：跟 telegram 的"延迟下载"对比，PR7 是"早期下载"。理由 = wa-rs 协议 protobuf 内的 media_key 一会就过期（spec 里 media_key_timestamp 字段提示），延后下载有风险；早期下载让 ChannelAttachmentSpec.download_code 直接是本地路径，worker 一行 `Path::new(spec.download_code)` 就能 `ChatAttachmentRef`。

- `manager.rs` — 在 worker 内**用一个新 helper** `make_chat_attachments_whatsapp(specs)` 把 `Vec<ChannelAttachmentSpec>` 直接转 `Vec<ChatAttachmentRef>`（不下载，只读本地路径）。或者 worker 直接 inline 转换。

---

## Task 1 — download.rs + gc.rs + mod.rs + aijia_home.rs

新建 `src-tauri/src/connector/im/whatsapp/download.rs`：

```rust
//! WhatsApp 媒体下载（仅入站 IMAGE / DOCUMENT）。spec v3 §7。
//!
//! 跟 telegram_downloader 同 shape：dest_dir + sha256-prefix dedup + atomic write。
//! 区别：wa-rs `client.download(&Downloadable)` 直接走 protobuf media struct 拿
//! 解密 bytes，**不**经过 file_id 中转。`downloader` 共享 connector 的 bot_client
//! 句柄，lock 拿当前 Arc<Client>。

use std::path::{Path, PathBuf};
use std::sync::Arc;

use sha2::{Digest, Sha256};
use tokio::io::AsyncWriteExt;
use wa_rs::client::Client;
use wa_rs::wa_rs_proto::whatsapp as wa;

#[allow(dead_code)]  // PR7 不做 size 检查，仅常量预留
pub const IMAGE_SIZE_LIMIT_BYTES: u64 = 5 * 1024 * 1024;
#[allow(dead_code)]
pub const FILE_SIZE_LIMIT_BYTES: u64 = 100 * 1024 * 1024;

#[derive(Debug, thiserror::Error)]
pub enum WhatsAppDownloadError {
    #[error("bot not running")]
    BotNotRunning,
    #[error("download failed: {0:#}")]
    Download(anyhow::Error),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
}

#[derive(Debug, Clone)]
pub struct WhatsAppDownloadedFile {
    pub path: PathBuf,
    pub file_name: String,
    pub size: u64,
    pub sha256: String,
    pub mime_type: Option<String>,
}

#[derive(Clone)]
pub struct WhatsAppMediaDownloader {
    bot_client: Arc<tokio::sync::Mutex<Option<Arc<Client>>>>,
    dest_dir: PathBuf,
}

impl WhatsAppMediaDownloader {
    pub fn new(
        bot_client: Arc<tokio::sync::Mutex<Option<Arc<Client>>>>,
        dest_dir: PathBuf,
    ) -> Self {
        Self { bot_client, dest_dir }
    }

    async fn current_client(&self) -> Result<Arc<Client>, WhatsAppDownloadError> {
        self.bot_client.lock().await.clone().ok_or(WhatsAppDownloadError::BotNotRunning)
    }

    /// 下载 ImageMessage 到 dest_dir，落盘 `<sha256[..16]>-image-<msg_id>.<ext>`。
    pub async fn download_image(
        &self,
        img: &wa::message::ImageMessage,
        msg_id: &str,
    ) -> Result<WhatsAppDownloadedFile, WhatsAppDownloadError> {
        let client = self.current_client().await?;
        let bytes = client.download(img).await.map_err(WhatsAppDownloadError::Download)?;
        let mime = img.mimetype.clone();
        let ext = ext_from_mime(mime.as_deref()).unwrap_or("jpg");
        let requested_name = format!("image-{msg_id}.{ext}");
        self.write_bytes(&bytes, &requested_name, mime).await
    }

    /// 下载 DocumentMessage。文件名优先用 DocumentMessage.file_name，否则用
    /// `document-{msg_id}.bin`。
    pub async fn download_document(
        &self,
        doc: &wa::message::DocumentMessage,
        msg_id: &str,
    ) -> Result<WhatsAppDownloadedFile, WhatsAppDownloadError> {
        let client = self.current_client().await?;
        let bytes = client.download(doc).await.map_err(WhatsAppDownloadError::Download)?;
        let mime = doc.mimetype.clone();
        let requested_name = doc.file_name.clone()
            .filter(|s| !s.trim().is_empty())
            .unwrap_or_else(|| format!("document-{msg_id}.bin"));
        self.write_bytes(&bytes, &requested_name, mime).await
    }

    async fn write_bytes(
        &self,
        bytes: &[u8],
        requested_name: &str,
        mime: Option<String>,
    ) -> Result<WhatsAppDownloadedFile, WhatsAppDownloadError> {
        let sha256 = hex_encode(Sha256::digest(bytes).as_slice());
        let size = bytes.len() as u64;
        tokio::fs::create_dir_all(&self.dest_dir).await?;
        let safe_name = sanitize_filename(requested_name);
        let final_name = format!("{}-{}", &sha256[..16], safe_name);
        let final_path = self.dest_dir.join(&final_name);

        if final_path.exists() {
            return Ok(WhatsAppDownloadedFile {
                path: final_path,
                file_name: safe_name,
                size,
                sha256,
                mime_type: mime,
            });
        }

        let tmp_path = self.dest_dir.join(format!(".{}.tmp", &final_name));
        {
            let mut f = tokio::fs::File::create(&tmp_path).await?;
            f.write_all(bytes).await?;
            f.flush().await?;
        }
        tokio::fs::rename(&tmp_path, &final_path).await?;

        Ok(WhatsAppDownloadedFile {
            path: final_path,
            file_name: safe_name,
            size,
            sha256,
            mime_type: mime,
        })
    }
}

fn hex_encode(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        use std::fmt::Write;
        let _ = write!(s, "{b:02x}");
    }
    s
}

fn ext_from_mime(mime: Option<&str>) -> Option<&'static str> {
    match mime? {
        "image/jpeg" => Some("jpg"),
        "image/png" => Some("png"),
        "image/gif" => Some("gif"),
        "image/webp" => Some("webp"),
        "image/heic" => Some("heic"),
        _ => None,
    }
}

/// 简化的文件名 sanitize：去掉路径分隔符 / 不可见字符 / 太长尾巴。
fn sanitize_filename(name: &str) -> String {
    let cleaned: String = name.chars()
        .filter(|c| !matches!(c, '/' | '\\' | '\0'))
        .collect();
    let cleaned = cleaned.trim();
    if cleaned.is_empty() {
        return "unnamed".into();
    }
    if cleaned.chars().count() <= 100 {
        return cleaned.to_string();
    }
    cleaned.chars().take(100).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test] fn hex_encode_basic() { assert_eq!(hex_encode(&[0x12, 0xab]), "12ab"); }
    #[test] fn ext_from_mime_jpeg() { assert_eq!(ext_from_mime(Some("image/jpeg")), Some("jpg")); }
    #[test] fn ext_from_mime_unknown_none() { assert_eq!(ext_from_mime(Some("application/unknown")), None); }
    #[test] fn sanitize_strips_path_separators() { assert_eq!(sanitize_filename("evil/../path.pdf"), "evil...path.pdf"); }
    #[test] fn sanitize_empty_fallback() { assert_eq!(sanitize_filename(""), "unnamed"); }
    #[test] fn sanitize_truncates_long() {
        let long = "a".repeat(200);
        assert_eq!(sanitize_filename(&long).chars().count(), 100);
    }
    #[test] fn current_client_returns_bot_not_running_when_none() {
        let bc = Arc::new(tokio::sync::Mutex::new(None));
        let dl = WhatsAppMediaDownloader::new(bc, std::path::PathBuf::from("/tmp"));
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
        rt.block_on(async {
            let err = dl.current_client().await.unwrap_err();
            assert!(matches!(err, WhatsAppDownloadError::BotNotRunning));
        });
    }
}
```

新建 `src-tauri/src/connector/im/whatsapp/gc.rs`：

```rust
//! 25h 附件老化清理。spec v3 §7.2。
//!
//! WhatsApp tmp 目录单平台 cron，不抽 shared（其他平台无）。

use std::path::{Path, PathBuf};
use std::time::Duration;

const SWEEP_INTERVAL: Duration = Duration::from_secs(60 * 60);  // 1h
const TTL: Duration = Duration::from_secs(25 * 60 * 60);        // 25h

/// 启动循环：每 1h 调一次 sweep_once。manager 启动期 spawn。
/// 该函数无限循环，调用方应在 tokio::spawn 里跑。
pub async fn run_attachment_gc(dir: PathBuf) {
    loop {
        tokio::time::sleep(SWEEP_INTERVAL).await;
        if let Err(e) = sweep_once(&dir, TTL).await {
            log::warn!("[whatsapp gc] sweep_once failed: {e:#}");
        }
    }
}

/// 单次扫描：dir 下所有 entry，mtime + ttl < now 则 remove_file。
pub async fn sweep_once(dir: &Path, ttl: Duration) -> std::io::Result<()> {
    if !dir.exists() {
        return Ok(());
    }
    let now = std::time::SystemTime::now();
    let mut read = tokio::fs::read_dir(dir).await?;
    while let Some(entry) = read.next_entry().await? {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let metadata = match entry.metadata().await {
            Ok(m) => m,
            Err(_) => continue,
        };
        let mtime = match metadata.modified() {
            Ok(t) => t,
            Err(_) => continue,
        };
        if let Ok(age) = now.duration_since(mtime) {
            if age > ttl {
                if let Err(e) = tokio::fs::remove_file(&path).await {
                    log::warn!("[whatsapp gc] remove {} failed: {e}", path.display());
                } else {
                    log::info!("[whatsapp gc] removed expired attachment {}", path.display());
                }
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::SystemTime;

    #[tokio::test]
    async fn sweep_skips_nonexistent_dir() {
        // 不存在的目录直接 Ok（启动期可能 tmp dir 还没建）
        let res = sweep_once(Path::new("/tmp/nonexistent_whatsapp_gc_test_xyz"), TTL).await;
        assert!(res.is_ok());
    }

    #[tokio::test]
    async fn sweep_removes_old_file() {
        let dir = tempfile::tempdir().unwrap();
        let f = dir.path().join("old.txt");
        tokio::fs::write(&f, b"hello").await.unwrap();
        // 用 filetime crate 改 mtime 是最干净的，但仓库可能没这 dep。
        // 简化：调用 sweep_once 时把 ttl 改成 Duration::ZERO；
        // 任何文件 age > 0 都被删。
        sweep_once(dir.path(), Duration::from_millis(1)).await.unwrap();
        // sleep 一下让 age 真>1ms
        tokio::time::sleep(Duration::from_millis(10)).await;
        sweep_once(dir.path(), Duration::from_millis(1)).await.unwrap();
        assert!(!f.exists(), "old file should be deleted");
    }

    #[tokio::test]
    async fn sweep_keeps_fresh_file() {
        let dir = tempfile::tempdir().unwrap();
        let f = dir.path().join("fresh.txt");
        tokio::fs::write(&f, b"hi").await.unwrap();
        // ttl=24h，age 一定 < ttl
        sweep_once(dir.path(), Duration::from_secs(24 * 3600)).await.unwrap();
        assert!(f.exists(), "fresh file must NOT be deleted");
    }
}
```

修改 `mod.rs`：加 `pub mod download;` + `pub mod gc;` (alphabetical: `aicard`, `config`, `connector`, `download`, `gc`, `markdown`, `parser`, `runtime`, `sender`, `session`, `types`).

修改 `src-tauri/src/storage/aijia_home.rs`：在 `tmp_telegram_downloads_dir` 旁加：
```rust
/// WhatsApp 附件下载目录 `~/.renlijia/tmp/whatsapp_downloads/`。
/// 镜像 `tmp_telegram_downloads_dir`。父目录在首次写文件时由
/// `WhatsAppMediaDownloader::download_*` 通过 `tokio::fs::create_dir_all` 按需创建。
pub fn tmp_whatsapp_downloads_dir(&self) -> PathBuf {
    self.tmp_dir().join("whatsapp_downloads")
}
```

### Verification

```bash
cd src-tauri && cargo build --lib 2>&1 | tail -10
cd src-tauri && cargo test --lib connector::im::whatsapp::download:: 2>&1 | tail -10  # 7 pass
cd src-tauri && cargo test --lib connector::im::whatsapp::gc:: 2>&1 | tail -10        # 3 pass
cd src-tauri && cargo clippy --lib --message-format=short 2>&1 | grep -E 'whatsapp/(download|gc)\.rs' | head -5
cd src-tauri && cargo fmt -- --check 2>&1 | grep -E 'whatsapp/(download|gc)|storage/aijia_home' || echo OK
```

### Commit

```bash
git add src-tauri/src/connector/im/whatsapp/download.rs \
        src-tauri/src/connector/im/whatsapp/gc.rs \
        src-tauri/src/connector/im/whatsapp/mod.rs \
        src-tauri/src/storage/aijia_home.rs
git commit -m "$(cat <<'EOF'
feat(connector/im/whatsapp): PR7 加 download.rs + gc.rs（媒体下载 + 老化清理）

spec v3 §7。

download.rs：
- WhatsAppMediaDownloader { bot_client: Arc<Mutex<Option<Arc<Client>>>>, dest_dir }
  共享 connector.bot_client（PR5 已存），不重复持有 client
- download_image(&ImageMessage, msg_id) / download_document(&DocumentMessage, msg_id)
  调 client.download(&downloadable) 拿解密 bytes
- sha256-prefix 文件名 dedup + 原子写（tmp + rename）+ sanitize_filename
- ext_from_mime helper for image/jpeg/png/gif/webp/heic
- size 常量 IMAGE_SIZE_LIMIT_BYTES=5MB / FILE_SIZE_LIMIT_BYTES=100MB 仅预留
  （spec §7.3 不做 size 检查；常量给未来出站留位置）
- 7 个 unit test 覆盖 hex_encode / ext / sanitize / bot-not-running

gc.rs：
- run_attachment_gc(dir) 启动期 spawn 的无限循环：每 1h sweep 一次
- sweep_once(dir, ttl) 提取出来让单测能调；mtime + ttl < now 则 remove_file
- 不存在目录直接 Ok（启动期 tmp 可能还没建）
- 3 个 unit test：不存在目录 / 老文件被删 / 新文件保留

aijia_home.rs：加 tmp_whatsapp_downloads_dir() 返
`~/.renlijia/tmp/whatsapp_downloads/`。

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 2 — parser.rs 改 async + 真下载

修改 `src-tauri/src/connector/im/whatsapp/parser.rs`：

1. **签名改 async**：把 `pub fn normalize(...)` 改成
   `pub async fn normalize_async(msg, info, cfg, downloader: Option<&super::download::WhatsAppMediaDownloader>) -> Option<ChannelMessage>`。

2. **IMAGE 分支**：在 `extract_body_text` 调用之后、return Some(ChannelMessage) 之前，检查
   `msg.image_message.is_some() && downloader.is_some()`，调 `downloader.unwrap().download_image(img, &info.id).await`：
   - Ok → 把 path str 写进 `ChannelAttachmentSpec { kind: AttachmentKind::Picture, download_code: path.to_string_lossy().to_string(), file_name }`
   - Err → 在 text 前面 prefix `[附件下载失败]\n`（或在原有 caption 后面 append）

3. **DOCUMENT 分支**：同上但用 `download_document` + `AttachmentKind::File`。

4. **测试改造**：现有 10 个 PR4 测试调 `normalize(&msg, &info, cfg)` 全部改成
   `normalize_async(&msg, &info, cfg, None).await` —— `None` downloader 保持 PR4 测试行为（不下载），现有断言不变。

   新加 1 个测试：`normalize_async_no_downloader_skips_attachments_keeps_text` 显式验证 None 路径不影响 IMAGE caption text。

   真下载路径（IMAGE + downloader 成功 / 失败）的 unit 测难做 —— 用 PR8 集成测试或真账号 canary 覆盖。

5. **`is_private_chat`** 保持 `pub fn`（同步）。

### Verification

```bash
cd src-tauri && cargo build --lib 2>&1 | tail -10
cd src-tauri && cargo test --lib connector::im::whatsapp::parser:: 2>&1 | tail -15  # 10 + 1 = 11 pass
cd src-tauri && cargo clippy --lib --message-format=short 2>&1 | grep whatsapp/parser.rs | head -5
cd src-tauri && cargo fmt -- --check 2>&1 | grep whatsapp/parser || echo OK
```

⚠️ 此 task **会破坏**所有 `parser::normalize` 调用方（runtime.rs）—— task 2 单独 commit 后 build fail；task 3 一起补。**决策**：task 2 + task 3 合并 commit（同 PR3/PR5 合并 commit 模式）。

提示 implementer：task 2 完成后**不要 commit**，进 task 3。

---

## Task 3 — runtime.rs 接 downloader

修改 `src-tauri/src/connector/im/whatsapp/runtime.rs`：

1. **`start_bot` 签名加 `downloader: Arc<download::WhatsAppMediaDownloader>` 参数**（作为最后一个参数）。closure capture downloader。
2. **`handle_event::Event::Message` 分支**：
   - 原：`parser::normalize(&msg, &info, cfg.as_ref())`
   - 新：`parser::normalize_async(&msg, &info, cfg.as_ref(), Some(&downloader)).await`
3. **`connector.rs::start_pairing_session`** build downloader 传给 start_bot：
   ```rust
   let downloader = Arc::new(super::download::WhatsAppMediaDownloader::new(
       Arc::clone(&self.bot_client),
       paths.parent_dir().to_path_buf().join("downloads"),  // OR use AiJiaHome — see below
   ));
   ```
   
   ⚠️ **dest_dir 来源**：`paths` 是 `WhatsAppPaths { base: ~/.renlijia/users/{scope}/channels/whatsapp/ }`，**不**是 tmp/whatsapp_downloads。我们要的是 `AiJiaHome::tmp_whatsapp_downloads_dir()`。
   
   **方案**：`WhatsAppConnector` 加字段 `attachments_dir: PathBuf`（在 `with_status_callback` / `new` 时不传，**改**成 `new_with_attachments_dir(callback, attachments_dir)` 或者新加一个 setter；最简化：connector 加字段 `attachments_dir: RwLock<Option<PathBuf>>`，manager 在 register 时 set；start_pairing_session 时 read）。
   
   **更简化**：`WhatsAppMediaDownloader::new` 接受 `dest_dir`，而 dest_dir 由 **manager** 在 register_whatsapp_connector 或 connect_whatsapp_from_store 时通过 `AiJiaHome` 解析后**传进 connector**。`WhatsAppConnector::set_attachments_dir(&self, dir)` inherent 方法。然后 `start_pairing_session` read 它（lock None 时回退到 `paths.base.parent().join("downloads")`）。
   
   **最最简化**：`AiJiaHome` 是 process-wide singleton（manager 已读它），把 `aijia_home_handle: OnceCell<Arc<AiJiaHome>>` 设进去 —— 太重。
   
   **plan 决策**：在 `factory.rs::build_whatsapp_connector` 签名加一个 `attachments_dir: PathBuf` 参数；manager 在 `register_whatsapp_connector` 时通过 `AiJiaHome::tmp_whatsapp_downloads_dir()` 解析后传进去。connector struct 加 `attachments_dir: PathBuf` 字段。简单直接。

   `manager::register_whatsapp_connector` 已经在 PR3 落地了，需要改签名 + 调用点（grep 一下，只有 2 个调用点）。

4. **`stop()`** 不需要改，downloader 跟 bot_client 共享 lifecycle。

### Verification

```bash
cd src-tauri && cargo build --lib 2>&1 | tail -10
cd src-tauri && cargo test --lib connector::im::whatsapp 2>&1 | tail -10
cd src-tauri && cargo clippy --lib --message-format=short 2>&1 | grep -E 'whatsapp/(parser|runtime|connector|factory)\.rs' | head -10
cd src-tauri && cargo fmt -- --check 2>&1 | grep whatsapp/ || echo OK
```

### Commit (Task 2 + Task 3 合并)

```bash
git add src-tauri/src/connector/im/whatsapp/parser.rs \
        src-tauri/src/connector/im/whatsapp/runtime.rs \
        src-tauri/src/connector/im/whatsapp/connector.rs \
        src-tauri/src/connector/im/whatsapp/factory.rs
git commit -m "$(cat <<'EOF'
feat(connector/im/whatsapp): PR7 parser/runtime 接媒体下载

spec v3 §7.1 + §7.2。

parser:
- normalize 改 async fn normalize_async(msg, info, cfg, downloader)
- IMAGE 分支：downloader.is_some() 时 download_image，成功填
  ChannelAttachmentSpec { Picture, download_code=local_path, file_name }；
  失败 text 前 prefix [附件下载失败]
- DOCUMENT 分支：同上但 AttachmentKind::File
- VOICE / VIDEO / STICKER / Location / Contact 保持 PR4 占位文案不变
- 10 个 PR4 测试 .await + None downloader 保持行为；加 1 新测覆盖 None
  路径不影响 IMAGE caption

runtime:
- start_bot 签名加 downloader: Arc<WhatsAppMediaDownloader>
- handle_event::Event::Message 用 normalize_async + Some(&downloader)

connector + factory:
- WhatsAppConnector 加 attachments_dir: PathBuf 字段
- factory::build_whatsapp_connector 签名加 attachments_dir 参数
- start_pairing_session build downloader (bot_client + attachments_dir)
  传给 runtime::start_bot

manager (register_whatsapp_connector):
- 调用方传 AiJiaHome::tmp_whatsapp_downloads_dir() 作为 attachments_dir

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 4 — manager: factory 签名调用 + spawn GC + worker 接 spec.path

修改 `src-tauri/src/connector/im/manager.rs`：

1. **`register_whatsapp_connector`**：把 `super::factory::build_whatsapp_connector(on_status)` 改成
   `super::factory::build_whatsapp_connector(on_status, attachments_dir)`，其中
   ```rust
   let attachments_dir = self.aijia_home()
       .map(|h| h.tmp_whatsapp_downloads_dir())
       .unwrap_or_else(|| self.chat_adapter.workspace_path().join("whatsapp_downloads"));
   ```
   grep `fn aijia_home\b\|aijia_home()` 确认方法存在；feishu_downloads_dir 函数（manager.rs:1597）已经用同款 fallback 模式。

2. **加 GC spawn**：在 `connect_from_store_all` 或类似启动期入口（或在 `register_whatsapp_connector` 第一次被调时通过 OnceCell guard）加：
   ```rust
   let gc_dir = ... (same as attachments_dir);
   tokio::spawn(super::whatsapp::gc::run_attachment_gc(gc_dir));
   ```
   ⚠️ 不能在 `register_whatsapp_connector` 每次都 spawn（重启 connector 时会重 spawn → 多个 GC 跑）。**用 OnceCell 字段**：`whatsapp_gc_spawned: tokio::sync::OnceCell<()>`；第一次 register 时 get_or_init spawn。

3. **worker 内** spec.path 转 ChatAttachmentRef：因为 PR7 parser 把 `download_code = local_path_str`，worker 不需要再下载。修改 `spawn_whatsapp_inbound_worker` 内当前 attachment 处理：
   - 当前 PR4 是 `build_pending_item_from_telegram(..., vec![], &Vec::new())`，attachments 永远空
   - PR7 改成：把 `msg.attachments` 转 `Vec<ChatAttachmentRef>` —— 用一个新 inline helper：
     ```rust
     fn whatsapp_specs_to_chat_attachments(specs: &[ChannelAttachmentSpec]) -> Vec<ChatAttachmentRef> {
         specs.iter().filter_map(|spec| {
             let path = std::path::Path::new(&spec.download_code);
             if !path.exists() { return None; }
             let kind = match spec.kind {
                 AttachmentKind::Picture => ChatAttachmentKind::Image,
                 AttachmentKind::File => ChatAttachmentKind::File,
             };
             let size = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);
             Some(ChatAttachmentRef {
                 path: path.to_path_buf(),
                 file_name: spec.file_name.clone(),
                 size,
                 mime_type: None,   // parser 没保存 mime；后续从 ext 推
                 kind,
                 sha256: None,
             })
         }).collect()
     }
     ```
     
     ⚠️ grep `pub struct ChatAttachmentRef\|pub enum ChatAttachmentKind` 看实际字段定义，按真实 shape 写。telegram pattern (`downloaded_to_chat_attachment_telegram`) 是模板。

   - worker 调 `build_pending_item_from_telegram` / `build_channel_chat_request` 时把 `vec![]` 改成 `whatsapp_specs_to_chat_attachments(&msg.attachments)`。

### Verification

```bash
cd src-tauri && cargo build --lib 2>&1 | tail -10
cd src-tauri && cargo test --lib connector::im::whatsapp 2>&1 | tail -10  # 81 + 11 PR7 = 92 pass
cd src-tauri && cargo test --lib connector::im 2>&1 | grep -E '^test result' | tail -3
cd src-tauri && cargo test --test review_im_layering 2>&1 | tail -3
cd src-tauri && cargo clippy --lib --message-format=short 2>&1 | grep manager.rs | head -10
cd src-tauri && cargo fmt -- --check 2>&1 | grep manager.rs || echo OK
```

### Commit

```bash
git add src-tauri/src/connector/im/manager.rs
git commit -m "$(cat <<'EOF'
feat(connector/im/whatsapp): PR7 manager 接附件下载 + GC + worker chat_attachments

spec v3 §7。

register_whatsapp_connector 传 attachments_dir = AiJiaHome::tmp_whatsapp_downloads_dir()
(fallback: workspace/whatsapp_downloads) 给 factory::build_whatsapp_connector。

OnceCell whatsapp_gc_spawned 保证 GC task 整 process 只 spawn 一次：
tokio::spawn(gc::run_attachment_gc(dir)) → 每 1h sweep, mtime+25h<now 则删。

spawn_whatsapp_inbound_worker 内：parser 现在把 download_code 填本地路径，
新加 whatsapp_specs_to_chat_attachments helper 把 ChannelAttachmentSpec 转
ChatAttachmentRef（exists + metadata size + Picture/File → Image/File）。
worker 把转换结果传给 build_pending_item_from_telegram 和
build_channel_chat_request 替代 PR4/PR6 的 vec![]。

不下载失败处理（path missing）→ filter_map skip，让对话继续；spec §7.1
"占位文案 + attachments 空" 由 parser 端 [附件下载失败] prefix 负责。

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 5 — 收尾

```bash
cd src-tauri && cargo test --lib connector::im::whatsapp 2>&1 | tail -10
cd src-tauri && cargo test --lib connector::im 2>&1 | grep -E '^test result' | tail -3
cd src-tauri && cargo test --test review_im_layering 2>&1 | tail -3
cd src-tauri && cargo clippy --lib --message-format=short 2>&1 | grep whatsapp/ | head -10
cd src-tauri && cargo fmt -- --check 2>&1 | head -5
cd .. && pnpm exec tsc --noEmit 2>&1 | tail -3
```

Expected: 81 + 11 = 92 whatsapp tests; 3 review_im_layering; 0 new clippy/tsc.

更新 memory PR7 行 + PR8 续接指引。

---

## Self-Review

| spec | task |
|---|---|
| §7.1 IMAGE / DOCUMENT 下载 | Task 1 download.rs + Task 2 parser async |
| §7.2 25h 老化清理 | Task 1 gc.rs + Task 4 manager OnceCell spawn |
| §7.3 size 常量 不做检查 | Task 1 IMAGE/FILE_SIZE_LIMIT_BYTES const + `#[allow(dead_code)]` |
| §7.4 VOICE/VIDEO/STICKER/Location/Contact 占位 | PR4 已落实，PR7 不动 |

无 unimplemented!/TODO。

执行：task 1 独立 → task 2+3 合并 commit → task 4 → task 5 collat。

可并行点：Task 1 (download.rs + gc.rs) 没有依赖，完全独立。但 Task 2-4 串行。Task 1 跟 Task 2 也可并行（Task 2 import download.rs 但只在 cargo build 时验证）。**决策**：Task 1 单独派；Task 2+3 一起；Task 4 收尾。3 个串行 step。
