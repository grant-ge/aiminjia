//! 媒体上传 / 下载 / 解密。
//!
//! 上传协议：三步分片
//!   1) aibot_upload_media_init → { upload_id }
//!   2) aibot_upload_media_chunk × N（单分片 ≤512KB base64 之前）
//!   3) aibot_upload_media_finish → { media_id, ... }
//!
//! 下载：HTTP GET url（5 分钟有效）→ AES-256-CBC 解密（key 来自消息体 `aeskey`，
//! base64 decode 后 32 字节当 key，前 16 字节当 IV）。

use std::path::{Path, PathBuf};

use aes::cipher::{BlockDecryptMut, KeyIvInit};
use anyhow::{anyhow, Context, Result};
use base64::{engine::general_purpose::STANDARD as B64, Engine};
use sha2::{Digest, Sha256};
use tokio::io::AsyncWriteExt;

type Dec = cbc::Decryptor<aes::Aes256>;

/// 下载完成后写盘的产物。形状对齐 `feishu::download::FeishuDownloadedFile`，
/// manager 的 `downloaded_to_chat_attachment_wecom` helper 直接映射成
/// `ChatAttachmentRef` 喂给 LLM 路径。
#[derive(Debug, Clone)]
pub struct WecomDownloadedFile {
    pub path: PathBuf,
    pub file_name: String,
    pub size: u64,
    pub sha256: String,
    pub mime_type: Option<String>,
}

/// `wecom://{aeskey_b64}@{url}` → (aeskey_b64, url)
pub fn decode_download_code(code: &str) -> Result<(String, String)> {
    let stripped = code
        .strip_prefix("wecom://")
        .ok_or_else(|| anyhow!("not a wecom download code: {code}"))?;
    // aeskey 是 base64 编码（字符集 A-Za-z0-9+/=），不含 '@'，所以第一个 '@' 总是
    // key 与 url 的分隔符；即便 url 自带 '@'（userinfo@host），仍然正确。
    let (key, url) = stripped
        .split_once('@')
        .ok_or_else(|| anyhow!("missing @ separator in wecom download code"))?;
    Ok((key.to_string(), url.to_string()))
}

/// 把企微 `encodingAESKey`（43 字符 base64，缺尾部 padding）还原为 32 字节
/// AES 密钥。对应 `@wecom/aibot-node-sdk` 的 `decodeEncodingAESKey`：
/// 末尾 base64 padding 不足时补 `=` 到 4 的整数倍 → standard base64 decode →
/// 32 bytes（强制校验）。`trim + 补齐` 写法是为了对 已带 `=` / 缺 `=` 两种
/// 输入都鲁棒。
fn decode_encoding_aes_key(aeskey: &str) -> Result<Vec<u8>> {
    let trimmed = aeskey.trim_end_matches('=');
    let pad_count = (4 - trimmed.len() % 4) % 4;
    let mut padded = String::with_capacity(trimmed.len() + pad_count);
    padded.push_str(trimmed);
    for _ in 0..pad_count {
        padded.push('=');
    }
    let key = B64.decode(&padded).context("aeskey base64 decode")?;
    if key.len() != 32 {
        return Err(anyhow!("aeskey must decode to 32 bytes, got {}", key.len()));
    }
    Ok(key)
}

/// 用 aeskey（43 字符 encodingAESKey）AES-256-CBC 解密。注意企微媒体使用
/// **32 字节** PKCS#7 padding（不是 AES 默认的 16 字节块大小），所以这里
/// 用 `decrypt_padded_mut::<NoPadding>` 拿原始解密结果，再手工剥 PKCS#7（按
/// 末尾字节标记的 padding 长度截断）。对应 openclaw `webhook/media.ts:60` 的
/// `setAutoPadding(false)` + `pkcs7Unpad(decryptedPadded, 32)`。
pub fn decrypt_aeskey_cbc(ciphertext: &[u8], aeskey_encoded: &str) -> Result<Vec<u8>> {
    let key_bytes = decode_encoding_aes_key(aeskey_encoded)?;
    // 注意：aibot SDK 协议要求 IV = key[..16]，非随机 IV。这是协议固定行为，不要
    // "修复"成随机 IV — 与服务端 ciphertext 不兼容。参考 @wecom/aibot-node-sdk@1.0.7
    // `decryptFile` 实现。
    let iv: [u8; 16] = key_bytes[..16].try_into().unwrap();
    let dec = Dec::new_from_slices(&key_bytes, &iv).context("init cbc")?;
    let mut buf = ciphertext.to_vec();
    // 自定义 padding：先 raw decrypt，再按企微的 32 字节 PKCS#7 unpad。
    use aes::cipher::block_padding::NoPadding;
    let raw = dec
        .decrypt_padded_mut::<NoPadding>(&mut buf)
        .map_err(|e| anyhow!("decrypt failed: {e:?}"))?;
    pkcs7_unpad_32(raw)
}

/// 企微媒体专用的 PKCS#7 反填充：尾字节是 pad 长度（1..=32）。openclaw
/// 实现见 `@wecom/aibot-node-sdk::pkcs7Unpad`，传入 block size 32。
/// 该格式不强制 ciphertext 长度对齐 32（AES block 仍是 16），但 pad 字节
/// 仅取 1..=32。
fn pkcs7_unpad_32(data: &[u8]) -> Result<Vec<u8>> {
    let n = data.len();
    if n == 0 {
        return Err(anyhow!("empty plaintext after decrypt"));
    }
    let pad = data[n - 1] as usize;
    if pad == 0 || pad > 32 || pad > n {
        return Err(anyhow!("invalid pkcs7 pad length: {pad} (data_len={n})"));
    }
    Ok(data[..n - pad].to_vec())
}

/// HTTP GET + 解密。返回明文 buffer。
pub async fn download_and_decrypt(url: &str, aeskey_b64: &str) -> Result<Vec<u8>> {
    let client = reqwest::Client::builder()
        .connect_timeout(std::time::Duration::from_secs(10))
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .context("build http client")?;
    let resp = client.get(url).send().await.context("http get")?;
    if !resp.status().is_success() {
        return Err(anyhow!("download failed: status={}", resp.status()));
    }
    let bytes = resp.bytes().await.context("read body")?;
    decrypt_aeskey_cbc(&bytes, aeskey_b64)
}

/// 一站式下载落盘：HTTP GET → AES 解密 → 按 sha256 命名落到 `dest_dir`。
///
/// 落盘格式 `{sha256}.{ext}`（ext 从 `display_name` 推，没有就 `bin`），跟
/// feishu 的策略对齐 —— 同一张图重复发也只占一份磁盘。`mime_type` 优先
/// 走文件扩展名 → MIME 推断，aibot 解密后是明文 bytes，HTTP response
/// 头里的 Content-Type 是密文响应、不能信。
pub async fn download_and_save(
    download_code: &str,
    dest_dir: &Path,
    display_name: &str,
) -> Result<WecomDownloadedFile> {
    let (aeskey, url) = decode_download_code(download_code)?;
    let bytes = download_and_decrypt(&url, &aeskey).await?;
    tokio::fs::create_dir_all(dest_dir)
        .await
        .with_context(|| format!("create dest dir {}", dest_dir.display()))?;

    let mut hasher = Sha256::new();
    hasher.update(&bytes);
    let sha256 = format!("{:x}", hasher.finalize());
    let ext = extension_or_bin(display_name);
    let final_path = dest_dir.join(format!("{}.{}", sha256, ext));
    let size = bytes.len() as u64;
    let mime_type = mime_from_ext(&ext);

    if final_path.exists() {
        return Ok(WecomDownloadedFile {
            path: final_path,
            file_name: display_name.to_string(),
            size,
            sha256,
            mime_type,
        });
    }

    let tmp_path = dest_dir.join(format!(".tmp_{}", uuid::Uuid::new_v4()));
    let write_result = async {
        let mut file = tokio::fs::File::create(&tmp_path).await?;
        file.write_all(&bytes).await?;
        file.flush().await?;
        Ok::<(), std::io::Error>(())
    }
    .await;
    if let Err(e) = write_result {
        let _ = tokio::fs::remove_file(&tmp_path).await;
        return Err(anyhow!("write file: {e}"));
    }
    if let Err(e) = tokio::fs::rename(&tmp_path, &final_path).await {
        let _ = tokio::fs::remove_file(&tmp_path).await;
        return Err(anyhow!("rename: {e}"));
    }

    Ok(WecomDownloadedFile {
        path: final_path,
        file_name: display_name.to_string(),
        size,
        sha256,
        mime_type,
    })
}

/// 从文件名 / 路径里取扩展名，缺失或空时回落 "bin"。
pub fn extension_or_bin(file_name: &str) -> String {
    Path::new(file_name)
        .extension()
        .and_then(|v| v.to_str())
        .map(|v| v.to_ascii_lowercase())
        .filter(|v| !v.trim().is_empty())
        .unwrap_or_else(|| "bin".to_string())
}

/// 按扩展名手工映射 MIME。aibot 解密后是明文 bytes，HTTP response Content-Type
/// 来自加密响应，没法用——只能靠扩展名推。覆盖图片 / pdf / 常见办公文档；
/// 其余回落 `application/octet-stream`，下游 Anthropic vision / 通用文件查看
/// 都能处理。
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

// 媒体上传：本期最小可用。三步分片，分片大小 = 384KB（保留 base64 1.33x 膨胀后 < 512KB）。
pub const MEDIA_CHUNK_SIZE: usize = 384 * 1024;

// 完整 upload 流程留给 PR5 connector 接入 AibotClient 后实现；本 PR 提供的接口先到
// decrypt + decode 为止，PR5 在 connector.rs 用 AibotClient.respond/send_msg 拼装
// upload_init / chunk / finish 帧。

#[cfg(test)]
mod tests {
    use super::*;

    /// encodingAESKey 是 43 字符 base64（缺 `=` padding）。decoder 必须补回去。
    #[test]
    fn decode_encoding_aes_key_pads_43_char_input() {
        // 32 个 'A' 字节 → base64 编码原始长度是 44（含 1 个 padding `=`）。
        // 模拟企微的 43-char 输入（去掉末尾的 `=`）。
        let raw_32 = vec![b'A'; 32];
        let b64_44 = B64.encode(&raw_32); // "QUFBQU...=" len=44
        let stripped: &str = b64_44.trim_end_matches('=');
        assert_eq!(
            stripped.len(),
            43,
            "前置条件：32 字节 base64 末尾恰好 1 个 ="
        );
        let decoded = decode_encoding_aes_key(stripped).expect("decode 43-char aeskey");
        assert_eq!(decoded, raw_32);
    }

    /// 完整 44 字符（带 `=`）也应能解 —— trim_end_matches 兼容两种入参。
    #[test]
    fn decode_encoding_aes_key_accepts_padded_input() {
        let raw_32 = vec![0x5au8; 32];
        let b64_44 = B64.encode(&raw_32);
        let decoded = decode_encoding_aes_key(&b64_44).unwrap();
        assert_eq!(decoded, raw_32);
    }

    #[test]
    fn decode_encoding_aes_key_rejects_wrong_length() {
        // 24 字节 → base64 编码刚好 32 字符无 padding，但 decode 出来是 24 != 32，
        // 触发 size check（而不是 base64 padding 错误）。
        let b64 = B64.encode([0u8; 24].as_slice()); // 32 chars, no '='
        let err = decode_encoding_aes_key(&b64).unwrap_err();
        assert!(
            format!("{err:#}").contains("32 bytes"),
            "expected size error, got: {err:#}"
        );
    }

    #[test]
    fn pkcs7_unpad_32_strips_known_pad() {
        // 模拟 5 字节明文 + 27 字节 pad (每个字节是 0x1b=27)
        let plain = b"hello";
        let mut padded = plain.to_vec();
        padded.extend(std::iter::repeat(27u8).take(27));
        assert_eq!(padded.len(), 32);
        let out = pkcs7_unpad_32(&padded).unwrap();
        assert_eq!(out, plain);
    }

    #[test]
    fn pkcs7_unpad_32_rejects_zero_pad() {
        let bad = vec![0u8; 10];
        assert!(pkcs7_unpad_32(&bad).is_err());
    }

    #[test]
    fn pkcs7_unpad_32_rejects_oversize_pad() {
        // 末尾标 33 > 32 上限 → 拒
        let mut bad = vec![0u8; 40];
        *bad.last_mut().unwrap() = 33;
        assert!(pkcs7_unpad_32(&bad).is_err());
    }

    #[test]
    fn decode_download_code_splits_at_first_at_sign() {
        // url 自带 userinfo@host 时仍然按"第一个 @"分隔
        let code = "wecom://abc123key@https://host.com/path@foo";
        let (key, url) = decode_download_code(code).unwrap();
        assert_eq!(key, "abc123key");
        assert_eq!(url, "https://host.com/path@foo");
    }
}
