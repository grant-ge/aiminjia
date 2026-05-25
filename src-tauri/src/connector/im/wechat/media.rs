//! 微信 iLink CDN 媒体下载 + 解密。镜像 `wecom::media`，但密钥/加密参数不同：
//!
//! - 算法：AES-128-ECB + PKCS#7 padding（块大小 16，不是 wecom 那种 32）
//! - 密钥编码（两种，[`parse_aes_key`] 都兼容）：
//!   1. 16 字节裸 key → base64(16 字节)，base64 解开就直接是 key
//!   2. 16 字节 hex 字符串 → base64(32 ASCII hex chars)，base64 解开是 32 字符再
//!      hex-decode 一次得到 16 字节
//!   文件路径用编码 2（`file_item.media.aes_key`），图片同时下发裸 hex
//!   (`image_item.aeskey`) 和编码 2 (`image_item.media.aes_key`)。
//! - 下载：直接 GET `media.full_url`（已经带好 `encrypted_query_param` query
//!   string），响应 body 是密文。
//!
//! 参考实现：openclaw-weixin-main `src/cdn/{aes-ecb.ts,pic-decrypt.ts}`。

use std::path::{Path, PathBuf};

use aes::cipher::{block_padding::Pkcs7, BlockDecryptMut, KeyInit};
use anyhow::{anyhow, Context, Result};
use base64::{engine::general_purpose::STANDARD as B64, Engine};
use sha2::{Digest, Sha256};
use tokio::io::AsyncWriteExt;

type EcbDec = ecb::Decryptor<aes::Aes128>;

/// 下载完成后写盘的产物。形状对齐 `wecom::media::WecomDownloadedFile`，
/// `manager::downloaded_to_chat_attachment_wechat` 直接映射成
/// `ChatAttachmentRef`。
#[derive(Debug, Clone)]
pub struct WechatDownloadedFile {
    pub path: PathBuf,
    pub file_name: String,
    pub size: u64,
    pub sha256: String,
    pub mime_type: Option<String>,
}

/// `wechat://{aeskey_b64}@{url}` → (aeskey_b64, url)。跟 wecom 同形式但前缀
/// 不同，避免误调到错平台。aeskey 永远 base64 编码后传，不直接传 hex——所以
/// `extract_attachments_from_item_list` 拿到 `image_item.aeskey`（hex）时要
/// 先用 hex→bytes→base64 转一次再塞进 download_code。
pub fn decode_download_code(code: &str) -> Result<(String, String)> {
    let stripped = code
        .strip_prefix("wechat://")
        .ok_or_else(|| anyhow!("not a wechat download code: {code}"))?;
    let (key, url) = stripped
        .split_once('@')
        .ok_or_else(|| anyhow!("missing @ separator in wechat download code"))?;
    Ok((key.to_string(), url.to_string()))
}

/// 解析 aeskey 字符串到 16 字节 AES 密钥。
///
/// 两种合法编码（openclaw 注释里叫"in the wild seen"）：
/// - `base64(16 raw bytes)` —— 部分图片 media 走这条路径
/// - `base64(32 ASCII hex chars)` —— 文件 / 语音 / 视频 + 部分图片
pub(crate) fn parse_aes_key(aes_key_b64: &str) -> Result<[u8; 16]> {
    let decoded = B64
        .decode(aes_key_b64)
        .with_context(|| format!("aes_key base64 decode failed; input={aes_key_b64}"))?;
    match decoded.len() {
        16 => {
            let mut out = [0u8; 16];
            out.copy_from_slice(&decoded);
            Ok(out)
        }
        32 if decoded.iter().all(|b| b.is_ascii_hexdigit()) => {
            // base64 解开是 32 字符的 ASCII hex，再 hex-decode 拿到真 16 字节。
            let s = std::str::from_utf8(&decoded).context("aes_key hex utf8")?;
            hex16_decode(s)
        }
        n => Err(anyhow!(
            "aes_key must decode to 16 raw bytes or 32-char hex (got {n} bytes); input={aes_key_b64}"
        )),
    }
}

/// 32 字符 hex → 16 字节。手写避免新增 `hex` 依赖。
pub(crate) fn hex16_decode(s: &str) -> Result<[u8; 16]> {
    if s.len() != 32 {
        return Err(anyhow!("hex16 expects 32 chars, got {}", s.len()));
    }
    let mut out = [0u8; 16];
    for (i, chunk) in s.as_bytes().chunks(2).enumerate() {
        let hi = hex_nibble(chunk[0])?;
        let lo = hex_nibble(chunk[1])?;
        out[i] = (hi << 4) | lo;
    }
    Ok(out)
}

fn hex_nibble(c: u8) -> Result<u8> {
    match c {
        b'0'..=b'9' => Ok(c - b'0'),
        b'a'..=b'f' => Ok(c - b'a' + 10),
        b'A'..=b'F' => Ok(c - b'A' + 10),
        other => Err(anyhow!("non-hex char: {:?}", other as char)),
    }
}

/// 16 字节 → 32 字符 hex（小写）。
pub(crate) fn hex16_encode(bytes: &[u8; 16]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(32);
    for b in bytes {
        out.push(HEX[(b >> 4) as usize] as char);
        out.push(HEX[(b & 0x0f) as usize] as char);
    }
    out
}

/// 把 `image_item.aeskey`（裸 hex 32 字符）转成 `wechat://` download_code 用的
/// base64(hex)。直接复用 [`parse_aes_key`] 接受的"base64(hex)"编码，避免
/// download_code 字符串里冒出 hex 跟 base64 两种格式。
pub fn hex_aeskey_to_base64(hex: &str) -> Result<String> {
    let bytes = hex16_decode(hex)?;
    // 再把 16 字节 hex-encode 回字符串、把 32 字符 ASCII 当 base64 输入二次编码。
    let hex_string = hex16_encode(&bytes);
    Ok(B64.encode(hex_string.as_bytes()))
}

/// AES-128-ECB + PKCS#7 解密。
pub fn decrypt_aes_ecb(ciphertext: &[u8], key: &[u8; 16]) -> Result<Vec<u8>> {
    let dec = EcbDec::new_from_slice(key).context("init ecb")?;
    let mut buf = ciphertext.to_vec();
    let plain = dec
        .decrypt_padded_mut::<Pkcs7>(&mut buf)
        .map_err(|e| anyhow!("ecb decrypt failed: {e:?}"))?;
    Ok(plain.to_vec())
}

/// HTTP GET full_url + AES-ECB 解密。返回明文 buffer。
pub async fn download_and_decrypt(url: &str, aes_key_b64: &str) -> Result<Vec<u8>> {
    let key = parse_aes_key(aes_key_b64)?;
    let client = reqwest::Client::builder()
        .connect_timeout(std::time::Duration::from_secs(10))
        .timeout(std::time::Duration::from_secs(60))
        .build()
        .context("build http client")?;
    let resp = client
        .get(url)
        .send()
        .await
        .with_context(|| format!("wechat CDN http get failed url={url}"))?;
    if !resp.status().is_success() {
        return Err(anyhow!(
            "wechat CDN download failed: status={}",
            resp.status()
        ));
    }
    let bytes = resp.bytes().await.context("read CDN body")?;
    decrypt_aes_ecb(&bytes, &key)
}

/// 一站式下载落盘：HTTP GET → AES 解密 → 按 sha256 命名落到 `dest_dir`。
///
/// 落盘格式 `{sha256}.{ext}`（ext 从 `display_name` 推，没有就 `bin`）。
/// 跟 wecom / feishu 策略对齐：同一张图重发也只占一份磁盘。
pub async fn download_and_save(
    download_code: &str,
    dest_dir: &Path,
    display_name: &str,
) -> Result<WechatDownloadedFile> {
    let (aeskey_b64, url) = decode_download_code(download_code)?;
    let bytes = download_and_decrypt(&url, &aeskey_b64).await?;
    tokio::fs::create_dir_all(dest_dir)
        .await
        .with_context(|| format!("create dest dir {}", dest_dir.display()))?;

    let mut hasher = Sha256::new();
    hasher.update(&bytes);
    let sha256 = format!("{:x}", hasher.finalize());
    let ext = extension_or_bin(display_name);
    let final_path = dest_dir.join(format!("{sha256}.{ext}"));
    let size = bytes.len() as u64;
    let mime_type = mime_from_ext(&ext);

    if final_path.exists() {
        return Ok(WechatDownloadedFile {
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

    Ok(WechatDownloadedFile {
        path: final_path,
        file_name: display_name.to_string(),
        size,
        sha256,
        mime_type,
    })
}

// wecom 已经维护了完全相同的扩展名 → MIME 表，直接 re-export 它的 pub 函数
// 避免重复。`mime_from_ext` 在 wecom 是私有的，单独 inline 一份镜像，避免
// 仅为本期收口去改 wecom 公开 API 边界。两边将来如有分歧（比如 wechat 想
// 加 audio/silk 单独路径），保留在这里也方便独立演进。
pub use super::super::wecom::media::extension_or_bin;

/// 按扩展名手工映射 MIME。aibot 解密后是明文 bytes，HTTP response Content-Type
/// 来自加密响应、没法用——只能靠扩展名推。覆盖图片 / pdf / 常见办公文档；
/// 其余回落 `None`，下游按 octet-stream 处理。
pub(crate) fn mime_from_ext(ext: &str) -> Option<String> {
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
    use aes::cipher::BlockEncryptMut;

    type EcbEnc = ecb::Encryptor<aes::Aes128>;

    /// base64(16 raw bytes) 路径：解出来 16 字节直接当 key。
    #[test]
    fn parse_aes_key_accepts_raw_16_bytes_base64() {
        let raw: [u8; 16] = [0x11; 16];
        let b64 = B64.encode(raw);
        let parsed = parse_aes_key(&b64).unwrap();
        assert_eq!(parsed, raw);
    }

    /// base64(32 ascii hex) 路径：base64 解开是 32 个 ASCII hex 字符 → 再 hex 解 → 16 字节。
    /// 这是从实测抓的格式：file_item.media.aes_key="Y2MwYjIzODNiYzgwNjVjZWQ0YWRjNWFmNDI0ZmEwMmE="
    /// → base64 解开 "cc0b2383bc8065ced4adc5af424fa02a"（32 字符 hex）→ 16 字节。
    #[test]
    fn parse_aes_key_accepts_base64_hex_path() {
        let real_world = "Y2MwYjIzODNiYzgwNjVjZWQ0YWRjNWFmNDI0ZmEwMmE=";
        let parsed = parse_aes_key(real_world).unwrap();
        assert_eq!(hex16_encode(&parsed), "cc0b2383bc8065ced4adc5af424fa02a");
    }

    #[test]
    fn parse_aes_key_rejects_wrong_length() {
        // base64("abc") → 3 字节，既不是 16 也不是 32 → 报错。
        let b64 = B64.encode(b"abc");
        let err = parse_aes_key(&b64).unwrap_err();
        assert!(format!("{err:#}").contains("16 raw bytes or 32-char hex"));
    }

    #[test]
    fn parse_aes_key_rejects_32_bytes_non_hex() {
        // 32 字节里掺非 hex 字符 → 报错（确保不会被误识别为 hex 路径）
        let mut bytes = vec![b'0'; 32];
        bytes[5] = b'Z'; // 非 hex
        let b64 = B64.encode(&bytes);
        let err = parse_aes_key(&b64).unwrap_err();
        assert!(format!("{err:#}").contains("16 raw bytes or 32-char hex"));
    }

    #[test]
    fn hex16_decode_roundtrips_with_encode() {
        let raw: [u8; 16] = [
            0xde, 0xad, 0xbe, 0xef, 0x12, 0x34, 0x56, 0x78, 0x9a, 0xbc, 0xde, 0xf0, 0x11, 0x22,
            0x33, 0x44,
        ];
        let encoded = hex16_encode(&raw);
        assert_eq!(encoded, "deadbeef123456789abcdef011223344");
        let decoded = hex16_decode(&encoded).unwrap();
        assert_eq!(decoded, raw);
    }

    #[test]
    fn hex_aeskey_to_base64_produces_path_consumable_by_parse() {
        // 这是 image_item.aeskey 真实形态，从 raw body 抓的：
        let hex = "7716ac836c2fc1faae223956cff3dbf7";
        let b64 = hex_aeskey_to_base64(hex).unwrap();
        // parse_aes_key 应该能用这个 base64 还原 16 字节
        let parsed = parse_aes_key(&b64).unwrap();
        assert_eq!(hex16_encode(&parsed), hex);
    }

    /// 已知向量：自己用 AES-128-ECB+PKCS7 加密一段 → decrypt_aes_ecb 应能还原。
    /// 用 EcbEnc 自洽加密，保证密钥/padding 跟解密侧匹配。
    #[test]
    fn decrypt_aes_ecb_roundtrip() {
        let key: [u8; 16] = [0x42; 16];
        let plaintext = b"hello wechat phase 5 cdn attachment";
        // 加密
        let enc = EcbEnc::new_from_slice(&key).unwrap();
        let mut buf = vec![0u8; plaintext.len() + 16];
        buf[..plaintext.len()].copy_from_slice(plaintext);
        let ct_len = enc
            .encrypt_padded_mut::<Pkcs7>(&mut buf, plaintext.len())
            .unwrap()
            .len();
        let ciphertext = buf[..ct_len].to_vec();
        // 解密
        let decrypted = decrypt_aes_ecb(&ciphertext, &key).unwrap();
        assert_eq!(decrypted, plaintext);
    }

    #[test]
    fn decode_download_code_splits_at_first_at_sign() {
        let code = "wechat://abc123key@https://cdn.weixin.qq.com/path?a=1@b";
        let (key, url) = decode_download_code(code).unwrap();
        assert_eq!(key, "abc123key");
        assert_eq!(url, "https://cdn.weixin.qq.com/path?a=1@b");
    }

    #[test]
    fn decode_download_code_rejects_wrong_prefix() {
        let err = decode_download_code("wecom://abc@http://x").unwrap_err();
        assert!(format!("{err:#}").contains("wechat download code"));
    }
}
