use std::sync::Arc;

use anyhow::{Context, Result};
use async_trait::async_trait;
use serde::Deserialize;
use tokio::sync::Mutex;

use crate::connector::im::shared::token::{PlatformTokenSource, TokenCache as SharedTokenCache};

const DINGTALK_API: &str = "https://api.dingtalk.com";

/// 钉钉 access_token 拉取器。`fetch` 调用 `/v1.0/oauth2/accessToken`。
pub struct DingtalkTokenSource {
    app_key: String,
    app_secret: String,
    http: reqwest::Client,
}

impl DingtalkTokenSource {
    pub fn new(app_key: String, app_secret: String) -> Self {
        Self {
            app_key,
            app_secret,
            http: reqwest::Client::new(),
        }
    }
}

#[async_trait]
impl PlatformTokenSource for DingtalkTokenSource {
    async fn fetch(&self) -> Result<(String, u64)> {
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct TokenResp {
            access_token: String,
            expire_in: u64,
        }
        let resp = self
            .http
            .post(format!("{}/v1.0/oauth2/accessToken", DINGTALK_API))
            .json(&serde_json::json!({
                "appKey": self.app_key,
                "appSecret": self.app_secret,
            }))
            .send()
            .await
            .context("Failed to request DingTalk accessToken")?;
        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            anyhow::bail!("DingTalk token request failed: {} {}", status, body);
        }
        let data: TokenResp = resp
            .json()
            .await
            .context("Failed to parse token response")?;
        Ok((data.access_token, data.expire_in))
    }
}

/// 兼容老 API：钉钉模块原来的 `TokenCache` 现在是个空壳子，构造时不知道凭证，
/// `get_access_token(&cache, app_key, app_secret)` 仍然按老签名暴露；
/// 内部 lazy 初始化一个 `SharedTokenCache<DingtalkTokenSource>` per (app_key, app_secret)。
///
/// 保留一个可选的 per-instance override（仅供测试预灌 token 用），生产路径不会触发。
///
/// **不**派生 `Default`：`Default::default()` 会让 `test_override = None`，导致 `.set()`
/// 静默成为 no-op。强制走 `TokenCache::new()` 构造，保证 override slot 总是就位。
#[derive(Debug, Clone)]
pub struct TokenCache {
    test_override: Option<Arc<Mutex<Option<String>>>>,
}

impl TokenCache {
    pub fn new() -> Self {
        Self {
            test_override: Some(Arc::new(Mutex::new(None))),
        }
    }

    /// 测试用：直接预灌一个 token，`get_access_token` 会优先返回它，不走 HTTP。
    /// 生产路径不调用此方法。Override 视为始终有效，由测试控制生命周期；
    /// 如需测试过期语义，请改用 `SharedTokenCache<DingtalkTokenSource>` 直接构造。
    #[doc(hidden)]
    pub async fn set(&self, token: String) {
        if let Some(slot) = self.test_override.as_ref() {
            *slot.lock().await = Some(token);
        }
    }
}

pub async fn get_access_token(
    cache: &TokenCache,
    app_key: &str,
    app_secret: &str,
) -> Result<String> {
    use std::collections::HashMap;
    use std::sync::OnceLock;
    use tokio::sync::Mutex as TokioMutex;

    if let Some(slot) = cache.test_override.as_ref() {
        if let Some(tok) = slot.lock().await.clone() {
            return Ok(tok);
        }
    }

    type CacheMap = HashMap<String, Arc<SharedTokenCache<DingtalkTokenSource>>>;
    static REGISTRY: OnceLock<TokioMutex<CacheMap>> = OnceLock::new();
    let registry = REGISTRY.get_or_init(|| TokioMutex::new(HashMap::new()));

    let key = format!("{}::{}", app_key, app_secret);
    let cache = {
        let mut map = registry.lock().await;
        map.entry(key)
            .or_insert_with(|| {
                Arc::new(SharedTokenCache::new(Arc::new(DingtalkTokenSource::new(
                    app_key.to_string(),
                    app_secret.to_string(),
                ))))
            })
            .clone()
    };
    cache.get().await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn dingtalk_token_source_compiles_and_constructs() {
        let src = DingtalkTokenSource::new("ak".into(), "as".into());
        let _ = src.app_key;
        let _ = src.app_secret;
    }

    #[tokio::test]
    async fn legacy_token_cache_unit_constructs() {
        let _ = TokenCache::new();
    }

    #[tokio::test]
    async fn test_override_short_circuits_get_access_token() {
        let cache = TokenCache::new();
        cache.set("preseeded".into()).await;
        let tok = get_access_token(&cache, "ak", "as").await.unwrap();
        assert_eq!(tok, "preseeded");
    }
}
