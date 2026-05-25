//! iLink-App-Id source (spec §1.5).
//!
//! Decision: MVP reuses the openclaw plugin's appid ("bot", from its
//! package.json). Override at runtime by setting
//! `~/.renlijia/config.json::wechat.ilink_app_id`. Once a tencent-issued
//! AIjia-specific appid lands, ship that as the new default and treat the
//! "bot" fallback as a dev-only knob.

use std::path::Path;

use serde::Deserialize;

/// openclaw plugin's `ilink_appid`. Replace once Tencent allocates an
/// AIjia-specific appid (spec §1.5 and PR1 follow-up).
pub const DEFAULT_OPENCLAW_APP_ID: &str = "bot";

#[derive(Debug, Clone, Default, Deserialize)]
struct WechatConfigSection {
    #[serde(default)]
    ilink_app_id: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
struct AijiaConfig {
    #[serde(default)]
    wechat: Option<WechatConfigSection>,
}

/// Read `~/.renlijia/config.json` if it exists. Missing file / parse errors /
/// missing field all collapse to `None` — caller falls back to the compile-time
/// default.
pub fn read_configured_app_id(config_path: &Path) -> Option<String> {
    let raw = std::fs::read_to_string(config_path).ok()?;
    let cfg: AijiaConfig = serde_json::from_str(&raw).ok()?;
    let id = cfg.wechat.and_then(|w| w.ilink_app_id)?;
    let trimmed = id.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

/// Final resolved app id. Logs at info when a custom override wins.
pub fn resolve_app_id(config_path: &Path) -> String {
    match read_configured_app_id(config_path) {
        Some(custom) => {
            log::info!(
                "[wechat] iLink-App-Id from config override (len={})",
                custom.len()
            );
            custom
        }
        None => {
            log::debug!("[wechat] iLink-App-Id falls back to compile-time default");
            DEFAULT_OPENCLAW_APP_ID.to_string()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    #[test]
    fn default_used_when_config_missing() {
        let nope = Path::new("/nonexistent/path/aijia-config.json");
        let id = resolve_app_id(nope);
        assert_eq!(id, DEFAULT_OPENCLAW_APP_ID);
    }

    #[test]
    fn config_value_overrides_default() {
        let mut f = NamedTempFile::new().unwrap();
        write!(f, r#"{{"wechat":{{"ilink_app_id":"AIJIA-CUSTOM-123"}}}}"#).unwrap();
        let id = resolve_app_id(f.path());
        assert_eq!(id, "AIJIA-CUSTOM-123");
    }

    #[test]
    fn empty_string_falls_back_to_default() {
        let mut f = NamedTempFile::new().unwrap();
        write!(f, r#"{{"wechat":{{"ilink_app_id":""}}}}"#).unwrap();
        let id = resolve_app_id(f.path());
        assert_eq!(id, DEFAULT_OPENCLAW_APP_ID);
    }

    #[test]
    fn malformed_config_falls_back_to_default() {
        let mut f = NamedTempFile::new().unwrap();
        write!(f, "this is not json {{").unwrap();
        let id = resolve_app_id(f.path());
        assert_eq!(id, DEFAULT_OPENCLAW_APP_ID);
    }
}
