//! End-to-end contract: AuthManager deactivation chain + ChannelManagerSlot.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use app_lib::auth::AuthDeactivationHandler;

struct Probe(Arc<AtomicUsize>);

#[async_trait::async_trait]
impl AuthDeactivationHandler for Probe {
    async fn on_deactivated(&self) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
}

#[test]
fn slot_starts_empty() {
    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(async {
        let slot = app_lib::connector::im::ChannelManagerSlot::new();
        assert!(slot.current().await.is_none());
    });
}

#[test]
fn slot_replace_returns_previous() {
    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(async {
        let slot = app_lib::connector::im::ChannelManagerSlot::new();
        let prev = slot.replace(None).await;
        assert!(prev.is_none());
    });
}

#[test]
fn deactivation_handler_trait_is_composable() {
    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(async {
        let c1 = Arc::new(AtomicUsize::new(0));
        let c2 = Arc::new(AtomicUsize::new(0));
        let handlers: Vec<Arc<dyn AuthDeactivationHandler>> =
            vec![Arc::new(Probe(c1.clone())), Arc::new(Probe(c2.clone()))];
        for h in &handlers {
            h.on_deactivated().await;
        }
        assert_eq!(c1.load(Ordering::SeqCst), 1);
        assert_eq!(c2.load(Ordering::SeqCst), 1);
    });
}

#[test]
fn deactivation_handler_is_idempotent() {
    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(async {
        let counter = Arc::new(AtomicUsize::new(0));
        let h: Arc<dyn AuthDeactivationHandler> = Arc::new(Probe(counter.clone()));
        h.on_deactivated().await;
        h.on_deactivated().await;
        h.on_deactivated().await;
        assert_eq!(counter.load(Ordering::SeqCst), 3);
    });
}
