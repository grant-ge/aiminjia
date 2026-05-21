//! WhatsApp config.json 元数据 read/write。spec v3 §3.1 + §3.10。
//!
//! 跟 wa-rs SqliteStore 解耦——这里只存 AIjia 自己用的元数据：
//! - jid：扫码后从 `Event::PairSuccess.id` 拿到，运维和 UI 用
//! - push_name：从 `Event::PairSuccess.push_name` 拿到，显示名
//! - paired_at：扫码完成 RFC3339 时间戳
//! - allow_from（v3 新增）：E.164 手机号 allowlist，PR4 入站过滤用
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
#[allow(dead_code)] // PR3 first consumer
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
#[allow(dead_code)] // PR3 first consumer
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
        assert!(
            !s.contains("allowFrom"),
            "None allowFrom must not serialize"
        );
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
        std::fs::write(
            &p,
            br#"{
            "schemaVersion": 1,
            "jid": "8613800138000@s.whatsapp.net",
            "pushName": "Alice",
            "pairedAt": "2026-05-20T10:30:00Z"
        }"#,
        )
        .unwrap();
        let got = read(&p).unwrap().unwrap();
        assert_eq!(got.allow_from, None);
    }
}
