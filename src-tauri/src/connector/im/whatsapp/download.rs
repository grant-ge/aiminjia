//! WhatsApp 媒体下载（仅入站 IMAGE / DOCUMENT）。spec v3 §7。
//!
//! 跟 telegram_downloader 同 shape：dest_dir + sha256-prefix dedup + atomic write。
//! 区别：wa-rs `client.download(&Downloadable)` 直接走 protobuf media struct 拿
//! 解密 bytes，**不**经过 file_id 中转。`downloader` 共享 connector 的 bot_client
//! 句柄，lock 拿当前 Arc<Client>。

use std::path::PathBuf;
use std::sync::Arc;

use sha2::{Digest, Sha256};
use tokio::io::AsyncWriteExt;
use wa_rs::client::Client;
use wa_rs::wa_rs_proto::whatsapp as wa;

#[allow(dead_code)] // PR7 不做 size 检查，仅常量预留
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
        Self {
            bot_client,
            dest_dir,
        }
    }

    async fn current_client(&self) -> Result<Arc<Client>, WhatsAppDownloadError> {
        self.bot_client
            .lock()
            .await
            .clone()
            .ok_or(WhatsAppDownloadError::BotNotRunning)
    }

    /// 下载 ImageMessage 到 dest_dir，落盘 `<sha256[..16]>-image-<msg_id>.<ext>`。
    pub async fn download_image(
        &self,
        img: &wa::message::ImageMessage,
        msg_id: &str,
    ) -> Result<WhatsAppDownloadedFile, WhatsAppDownloadError> {
        let client = self.current_client().await?;
        let bytes = client
            .download(img)
            .await
            .map_err(WhatsAppDownloadError::Download)?;
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
        let bytes = client
            .download(doc)
            .await
            .map_err(WhatsAppDownloadError::Download)?;
        let mime = doc.mimetype.clone();
        let requested_name = doc
            .file_name
            .clone()
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
    let cleaned: String = name
        .chars()
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

    #[test]
    fn hex_encode_basic() {
        assert_eq!(hex_encode(&[0x12, 0xab]), "12ab");
    }

    #[test]
    fn ext_from_mime_jpeg() {
        assert_eq!(ext_from_mime(Some("image/jpeg")), Some("jpg"));
    }

    #[test]
    fn ext_from_mime_unknown_none() {
        assert_eq!(ext_from_mime(Some("application/unknown")), None);
    }

    #[test]
    fn sanitize_strips_path_separators() {
        assert_eq!(sanitize_filename("evil/../path.pdf"), "evil..path.pdf");
    }

    #[test]
    fn sanitize_empty_fallback() {
        assert_eq!(sanitize_filename(""), "unnamed");
    }

    #[test]
    fn sanitize_truncates_long() {
        let long = "a".repeat(200);
        assert_eq!(sanitize_filename(&long).chars().count(), 100);
    }

    #[test]
    fn current_client_returns_bot_not_running_when_none() {
        let bc = Arc::new(tokio::sync::Mutex::new(None));
        let dl = WhatsAppMediaDownloader::new(bc, PathBuf::from("/tmp"));
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        rt.block_on(async {
            match dl.current_client().await {
                Err(WhatsAppDownloadError::BotNotRunning) => {}
                other => panic!("expected BotNotRunning, got: {:?}", other.err()),
            }
        });
    }
}
