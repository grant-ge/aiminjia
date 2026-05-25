//! Telegram inbound attachment downloader. 走 Telegram Bot API 两步：
//!   1. `getFile(file_id)` 拿 `file_path`
//!   2. GET `https://api.telegram.org/file/bot<token>/<file_path>` 取字节
//!
//! 落盘到 `~/.renlijia/tmp/telegram_downloads/`，按 sha256 去重命名，避免重复
//! 占用磁盘。每个 attachment 写一次：先临时文件 + 原子 rename，防止 partial。
//!
//! 与 feishu / wecom 的 download 模块对齐字段语义；将来可统一到 `im/shared/download.rs`，
//! 当前阶段维持各平台独立 module 形态。
//!
//! Bot API `getFile` 对结果有 20MB 上限：超大文件返回 400 `file is too big`，
//! 这里把 400 透传给上层（worker 会把 failure 记到 `download_failures` 让 LLM 看到提示）。

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use sha2::{Digest, Sha256};
use tokio::io::AsyncWriteExt;

use super::api::{TelegramApi, TelegramApiError};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TelegramDownloadedFile {
    pub path: PathBuf,
    pub file_name: String,
    pub size: u64,
    pub sha256: String,
    pub mime_type: Option<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum TelegramDownloadError {
    #[error("api: {0}")]
    Api(TelegramApiError),
    #[error("io: {0}")]
    Io(std::io::Error),
    #[error("file_path missing in getFile response")]
    NoFilePath,
}

#[derive(Clone)]
pub struct TelegramFileDownloader {
    api: Arc<TelegramApi>,
    dest_dir: PathBuf,
}

impl TelegramFileDownloader {
    pub fn new(api: Arc<TelegramApi>, dest_dir: PathBuf) -> Self {
        Self { api, dest_dir }
    }

    /// 用 `file_id` 下载附件到 `dest_dir`。落盘文件名格式：
    /// `<sha256>-<requested_file_name>`（前缀 sha256 保证同内容 dedup；
    /// 同时保留原 file_name 让用户在 file picker 里能认出来）。
    /// `requested_file_name` 是 parser 在 `ChannelAttachmentSpec.file_name` 里
    /// 填好的（telegram document 有 file_name；photo 没有，parser 合成
    /// `photo-{msg_id}.jpg`）。
    /// `mime_hint` 来自上游（document 的 mime_type 字段）；缺失时按文件扩展
    /// 名推断（与 wecom::media::mime_from_ext 同模式），让下游 vision 路径
    /// 能识别 image/jpeg 等。
    pub async fn download(
        &self,
        file_id: &str,
        requested_file_name: &str,
        mime_hint: Option<String>,
    ) -> Result<TelegramDownloadedFile, TelegramDownloadError> {
        let info = self
            .api
            .get_file(file_id)
            .await
            .map_err(TelegramDownloadError::Api)?;
        let file_path = info.file_path.ok_or(TelegramDownloadError::NoFilePath)?;

        // 重试 3 次，简单 backoff（与 feishu/dingtalk 模式一致）
        let bytes = retry_download(&self.api, &file_path, 3, Duration::from_millis(500))
            .await
            .map_err(TelegramDownloadError::Api)?;

        let sha256 = {
            let mut hasher = Sha256::new();
            hasher.update(&bytes);
            hex_encode(hasher.finalize().as_slice())
        };
        let size = bytes.len() as u64;

        tokio::fs::create_dir_all(&self.dest_dir)
            .await
            .map_err(TelegramDownloadError::Io)?;

        // 文件名：sha256 前缀（确保 dedup） + "-" + 安全化文件名
        let safe_name = sanitize_filename(requested_file_name);
        let final_name = format!("{}-{}", &sha256[..16], safe_name);
        let final_path = self.dest_dir.join(&final_name);

        // mime 解析：上游提示优先；缺失则按 ext 推断；都没有 None 透出，
        // 下游 vision 路径会按 file_type 退化处理。
        let mime_type = mime_hint
            .filter(|s| !s.trim().is_empty())
            .or_else(|| mime_from_ext(&extension_or_bin(&safe_name)));

        // dedup：相同 sha256 已存在则直接返回，不再写
        if final_path.exists() {
            return Ok(TelegramDownloadedFile {
                path: final_path,
                file_name: safe_name,
                size,
                sha256,
                mime_type,
            });
        }

        // 原子写：临时文件 + rename
        let tmp_path = self.dest_dir.join(format!(".{}.tmp", &final_name));
        {
            let mut f = tokio::fs::File::create(&tmp_path)
                .await
                .map_err(TelegramDownloadError::Io)?;
            f.write_all(&bytes)
                .await
                .map_err(TelegramDownloadError::Io)?;
            f.sync_all().await.map_err(TelegramDownloadError::Io)?;
        }
        tokio::fs::rename(&tmp_path, &final_path)
            .await
            .map_err(TelegramDownloadError::Io)?;

        Ok(TelegramDownloadedFile {
            path: final_path,
            file_name: safe_name,
            size,
            sha256,
            mime_type,
        })
    }
}

async fn retry_download(
    api: &TelegramApi,
    file_path: &str,
    attempts: u32,
    backoff: Duration,
) -> Result<Vec<u8>, TelegramApiError> {
    let mut last_err: Option<TelegramApiError> = None;
    for i in 0..attempts {
        match api.download_file(file_path).await {
            Ok(bytes) => return Ok(bytes),
            Err(e) => {
                // 401/403/400 等终态错误：不重试，直接返回
                if matches!(
                    e,
                    TelegramApiError::Unauthorized(_)
                        | TelegramApiError::Forbidden(_)
                        | TelegramApiError::BadRequest(_)
                ) {
                    return Err(e);
                }
                last_err = Some(e);
                if i + 1 < attempts {
                    tokio::time::sleep(backoff).await;
                }
            }
        }
    }
    Err(last_err.unwrap_or_else(|| TelegramApiError::TransportConnected("retry exhausted".into())))
}

fn hex_encode(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push_str(&format!("{:02x}", b));
    }
    s
}

/// 文件名安全化：把路径分隔符 + 控制字符替换为 `_`，限制长度。
/// 不重新发明轮子——Telegram 自己已经给出"安全"的 file_name（不含 `/`），
/// 但用户可控字段，保守起见还是 sanitize 一遍。
fn sanitize_filename(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    for c in name.chars() {
        if c.is_control() || c == '/' || c == '\\' || c == '\0' {
            out.push('_');
        } else {
            out.push(c);
        }
    }
    // 限长 200（避免某些文件系统 ENAMETOOLONG）
    if out.chars().count() > 200 {
        out = out.chars().take(200).collect();
    }
    if out.trim().is_empty() {
        out = "file.bin".to_string();
    }
    out
}

/// `feishu::download::extension_or_bin` 的等价物——从路径取后缀，缺失返回 "bin"。
pub fn extension_or_bin(path: &str) -> String {
    Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .map(|s| s.to_ascii_lowercase())
        .unwrap_or_else(|| "bin".to_string())
}

/// 按文件扩展名推断 MIME。镜像 `wecom::media::mime_from_ext`，避免跨模块
/// 引用——两边将来都可以收编到 `im/shared/`。
fn mime_from_ext(ext: &str) -> Option<String> {
    let m = match ext.to_ascii_lowercase().as_str() {
        "jpg" | "jpeg" => "image/jpeg",
        "png" => "image/png",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "bmp" => "image/bmp",
        "svg" => "image/svg+xml",
        "pdf" => "application/pdf",
        "doc" => "application/msword",
        "docx" => "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        "xls" => "application/vnd.ms-excel",
        "xlsx" => "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
        "ppt" => "application/vnd.ms-powerpoint",
        "pptx" => "application/vnd.openxmlformats-officedocument.presentationml.presentation",
        "txt" | "log" | "md" => "text/plain",
        "csv" => "text/csv",
        "json" => "application/json",
        "zip" => "application/zip",
        _ => return None,
    };
    Some(m.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_keeps_safe_chars() {
        assert_eq!(sanitize_filename("report.pdf"), "report.pdf");
        assert_eq!(sanitize_filename("项目-文档 v1.docx"), "项目-文档 v1.docx");
    }

    #[test]
    fn sanitize_replaces_path_separators() {
        assert_eq!(sanitize_filename("../etc/passwd"), ".._etc_passwd");
        assert_eq!(sanitize_filename("foo\\bar.txt"), "foo_bar.txt");
    }

    #[test]
    fn sanitize_falls_back_for_empty() {
        assert_eq!(sanitize_filename(""), "file.bin");
        assert_eq!(sanitize_filename("   "), "file.bin");
    }

    #[test]
    fn extension_or_bin_basic() {
        assert_eq!(extension_or_bin("a/b/c.PDF"), "pdf");
        assert_eq!(extension_or_bin("noext"), "bin");
        assert_eq!(extension_or_bin("photo-1.JPG"), "jpg");
    }

    #[test]
    fn hex_encode_basic() {
        assert_eq!(hex_encode(&[0xde, 0xad, 0xbe, 0xef]), "deadbeef");
        assert_eq!(hex_encode(&[0x00, 0xff]), "00ff");
    }
}
