//! 跨平台 token 缓存。每个平台实现 `PlatformTokenSource` 提供"如何拉一份新 token"，
//! `TokenCache<S>` 统一管"什么时候算过期、缓存写读、并发刷新"。
//!
//! 提前刷新阈值固定 5 分钟（300s），覆盖钉钉 / 飞书都符合的窗口。

use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::Result;
use async_trait::async_trait;
use tokio::sync::Mutex;

const REFRESH_AHEAD: Duration = Duration::from_secs(300);

#[async_trait]
pub trait PlatformTokenSource: Send + Sync {
    /// 调远端拿一份新 token + 该 token 的有效期（秒）。
    async fn fetch(&self) -> Result<(String, u64)>;
}

pub struct TokenCache<S: PlatformTokenSource> {
    source: Arc<S>,
    state: Mutex<Option<(String, Instant)>>,
}

impl<S: PlatformTokenSource> TokenCache<S> {
    pub fn new(source: Arc<S>) -> Self {
        Self {
            source,
            state: Mutex::new(None),
        }
    }

    /// 返回当前有效的 token；过期 / 临近过期则调 `source.fetch()` 刷新。
    pub async fn get(&self) -> Result<String> {
        let now = Instant::now();
        {
            let guard = self.state.lock().await;
            if let Some((tok, expires_at)) = guard.as_ref() {
                if *expires_at > now + REFRESH_AHEAD {
                    return Ok(tok.clone());
                }
            }
        }
        let mut guard = self.state.lock().await;
        if let Some((tok, expires_at)) = guard.as_ref() {
            if *expires_at > Instant::now() + REFRESH_AHEAD {
                return Ok(tok.clone());
            }
        }
        let (tok, expires_in_secs) = self.source.fetch().await?;
        let expires_at = Instant::now() + Duration::from_secs(expires_in_secs);
        *guard = Some((tok.clone(), expires_at));
        Ok(tok)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};

    struct CountingSource {
        calls: AtomicU64,
        next_secs: u64,
        fail_once: std::sync::Mutex<bool>,
    }

    #[async_trait]
    impl PlatformTokenSource for CountingSource {
        async fn fetch(&self) -> Result<(String, u64)> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            let mut fail = self.fail_once.lock().unwrap();
            if *fail {
                *fail = false;
                anyhow::bail!("forced failure");
            }
            Ok((
                format!("tok-{}", self.calls.load(Ordering::SeqCst)),
                self.next_secs,
            ))
        }
    }

    fn src(secs: u64) -> Arc<CountingSource> {
        Arc::new(CountingSource {
            calls: AtomicU64::new(0),
            next_secs: secs,
            fail_once: std::sync::Mutex::new(false),
        })
    }

    #[tokio::test]
    async fn first_call_fetches_token() {
        let s = src(7200);
        let cache = TokenCache::new(s.clone());
        let t = cache.get().await.unwrap();
        assert_eq!(t, "tok-1");
        assert_eq!(s.calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn second_call_hits_cache_when_far_from_expiry() {
        let s = src(7200);
        let cache = TokenCache::new(s.clone());
        let a = cache.get().await.unwrap();
        let b = cache.get().await.unwrap();
        assert_eq!(a, b);
        assert_eq!(s.calls.load(Ordering::SeqCst), 1, "must not refetch");
    }

    #[tokio::test]
    async fn near_expiry_triggers_refresh() {
        let s = src(60);
        let cache = TokenCache::new(s.clone());
        let a = cache.get().await.unwrap();
        let b = cache.get().await.unwrap();
        assert_eq!(a, "tok-1");
        assert_eq!(b, "tok-2");
        assert_eq!(s.calls.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn fetch_failure_is_propagated_and_cache_unchanged() {
        let s = Arc::new(CountingSource {
            calls: AtomicU64::new(0),
            next_secs: 7200,
            fail_once: std::sync::Mutex::new(true),
        });
        let cache = TokenCache::new(s.clone());
        assert!(cache.get().await.is_err());
        let t = cache.get().await.unwrap();
        assert_eq!(t, "tok-2");
    }
}
