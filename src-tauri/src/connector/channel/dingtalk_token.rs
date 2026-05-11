use std::sync::Arc;
use tokio::sync::Mutex;
use anyhow::Context;

const DINGTALK_API: &str = "https://api.dingtalk.com";

#[derive(Debug, Clone)]
pub struct TokenCache {
    inner: Arc<Mutex<CacheInner>>,
}

#[derive(Debug, Default)]
struct CacheInner {
    token: Option<String>,
    expires_at_ms: u64,
}

impl TokenCache {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(CacheInner::default())),
        }
    }

    /// 返回缓存的 token（如果还有 60s 以上有效期），否则返回 None
    pub async fn get_if_valid(&self) -> Option<String> {
        let now_ms = now_ms();
        let inner = self.inner.lock().await;
        if let Some(ref token) = inner.token {
            if inner.expires_at_ms > now_ms + 60_000 {
                return Some(token.clone());
            }
        }
        None
    }

    /// 更新缓存
    pub async fn set(&self, token: String, expires_in_secs: u64) {
        let mut inner = self.inner.lock().await;
        inner.token = Some(token);
        inner.expires_at_ms = now_ms() + expires_in_secs * 1000;
    }
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

/// 获取 Access Token，优先从缓存取；缓存过期时调用钉钉 API 刷新。
pub async fn get_access_token(
    cache: &TokenCache,
    app_key: &str,
    app_secret: &str,
) -> anyhow::Result<String> {
    if let Some(token) = cache.get_if_valid().await {
        return Ok(token);
    }

    let client = reqwest::Client::new();
    let resp = client
        .post(format!("{}/v1.0/oauth2/accessToken", DINGTALK_API))
        .json(&serde_json::json!({
            "appKey": app_key,
            "appSecret": app_secret,
        }))
        .send()
        .await
        .context("Failed to request DingTalk accessToken")?;

    if !resp.status().is_success() {
        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();
        anyhow::bail!("DingTalk token request failed: {} {}", status, body);
    }

    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct TokenResp {
        access_token: String,
        expire_in: u64,
    }

    let data: TokenResp = resp.json().await.context("Failed to parse token response")?;
    cache.set(data.access_token.clone(), data.expire_in).await;
    Ok(data.access_token)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn cache_returns_none_when_empty() {
        let cache = TokenCache::new();
        assert!(cache.get_if_valid().await.is_none());
    }

    #[tokio::test]
    async fn cache_returns_token_when_valid() {
        let cache = TokenCache::new();
        cache.set("tok123".into(), 7200).await;
        assert_eq!(cache.get_if_valid().await.unwrap(), "tok123");
    }

    #[tokio::test]
    async fn cache_returns_none_when_near_expiry() {
        let cache = TokenCache::new();
        // 30s 后过期，低于 60s 阈值，应该返回 None
        cache.set("tok_expiring".into(), 30).await;
        assert!(cache.get_if_valid().await.is_none());
    }
}
