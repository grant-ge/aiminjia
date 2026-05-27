//! Replaceable container for the active `ChannelManager`.
//!
//! Setup-time registration cannot satisfy "user not logged in at startup"
//! and "switch user without restart" simultaneously — `tauri::App::manage`
//! refuses to overwrite. The slot indirection lets us swap instances at
//! runtime while keeping a stable type registration in app state.

use std::sync::Arc;
use tokio::sync::Mutex;

use super::ChannelManager;

pub struct ChannelManagerSlot {
    inner: Mutex<Option<Arc<ChannelManager>>>,
}

impl ChannelManagerSlot {
    pub fn new() -> Self {
        Self { inner: Mutex::new(None) }
    }

    /// Read-only snapshot. Returns the current instance if any.
    pub async fn current(&self) -> Option<Arc<ChannelManager>> {
        self.inner.lock().await.clone()
    }

    /// Atomically replace the instance, returning the previous value so the
    /// caller can drive `shutdown()` on it.
    pub async fn replace(&self, new: Option<Arc<ChannelManager>>) -> Option<Arc<ChannelManager>> {
        let mut guard = self.inner.lock().await;
        std::mem::replace(&mut *guard, new)
    }
}

impl Default for ChannelManagerSlot {
    fn default() -> Self { Self::new() }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn new_slot_is_empty() {
        let slot = ChannelManagerSlot::new();
        assert!(slot.current().await.is_none());
    }

    #[tokio::test]
    async fn replace_returns_previous() {
        let slot = ChannelManagerSlot::new();
        // Can't easily instantiate a real ChannelManager here without all
        // its deps. Smoke-test the None -> None path; full coverage is in
        // the integration test (Task 9).
        let prev = slot.replace(None).await;
        assert!(prev.is_none());
    }
}
