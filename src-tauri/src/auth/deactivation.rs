//! Auth deactivation hook — services that hold user-scoped runtime state
//! register a handler so `AuthManager` can fan out a single signal whenever
//! the active user is invalidated (manual logout, password change, server-
//! initiated refresh-token revocation).
//!
//! Handlers MUST be idempotent — they may be called even when the user was
//! already deactivated (e.g. logout after a 401 has already cleared state).
//! Handlers MUST NOT panic; errors should be logged and swallowed so one
//! misbehaving handler does not break the chain.

use async_trait::async_trait;

#[async_trait]
pub trait AuthDeactivationHandler: Send + Sync {
    /// Called after `AuthManager` has cleared in-memory + persisted auth
    /// state. The handler runs OUTSIDE any `AuthManager` lock, so it is
    /// safe to call back into the app handle / storage.
    async fn on_deactivated(&self);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    struct Counter(Arc<AtomicUsize>);

    #[async_trait]
    impl AuthDeactivationHandler for Counter {
        async fn on_deactivated(&self) {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
    }

    #[tokio::test]
    async fn handler_increments_counter() {
        let counter = Arc::new(AtomicUsize::new(0));
        let h: Arc<dyn AuthDeactivationHandler> = Arc::new(Counter(counter.clone()));
        h.on_deactivated().await;
        h.on_deactivated().await;
        assert_eq!(counter.load(Ordering::SeqCst), 2);
    }
}
