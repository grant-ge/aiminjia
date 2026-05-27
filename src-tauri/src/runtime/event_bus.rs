use std::sync::{Arc, Mutex, Weak};

use anyhow::Result;
use async_trait::async_trait;

use crate::runtime::events::RuntimeEvent;

#[async_trait]
pub trait RuntimeEventSubscriber: Send + Sync {
    async fn on_event(&self, event: &RuntimeEvent) -> Result<()>;
}

#[derive(Clone, Default)]
pub struct RuntimeEventBus {
    subscribers: Arc<Mutex<Vec<Weak<dyn RuntimeEventSubscriber>>>>,
    recorded: Arc<Mutex<Vec<RuntimeEvent>>>,
}

impl RuntimeEventBus {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_subscriber(subscriber: Arc<dyn RuntimeEventSubscriber>) -> Self {
        let bus = Self::new();
        bus.subscribe(subscriber);
        bus
    }

    pub fn subscribe(&self, subscriber: Arc<dyn RuntimeEventSubscriber>) {
        self.subscribers.lock().unwrap().push(Arc::downgrade(&subscriber));
    }

    pub async fn emit(&self, event: RuntimeEvent) -> Result<()> {
        self.recorded.lock().unwrap().push(event.clone());
        let snapshot: Vec<Arc<dyn RuntimeEventSubscriber>> = {
            let mut subs = self.subscribers.lock().unwrap();
            // Prune dead weak refs while collecting live ones.
            let mut live = Vec::with_capacity(subs.len());
            subs.retain(|w| {
                if let Some(arc) = w.upgrade() {
                    live.push(arc);
                    true
                } else {
                    false
                }
            });
            live
        };
        for subscriber in snapshot {
            subscriber.on_event(&event).await?;
        }
        Ok(())
    }

    /// Record a runtime event locally without notifying subscribers.
    ///
    /// This is used during transitional migrations where the runtime must retain
    /// ownership markers in `recorded_events()` while avoiding duplicate legacy
    /// frontend events from existing executor-owned transport paths.
    pub fn record_only(&self, event: RuntimeEvent) {
        self.recorded.lock().unwrap().push(event);
    }

    pub fn recorded(&self) -> Vec<RuntimeEvent> {
        self.recorded.lock().unwrap().clone()
    }
}
