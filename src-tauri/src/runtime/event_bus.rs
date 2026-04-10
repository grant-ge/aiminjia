use std::sync::{Arc, Mutex};

use anyhow::Result;
use async_trait::async_trait;

use crate::runtime::events::RuntimeEvent;

#[async_trait]
pub trait RuntimeEventSubscriber: Send + Sync {
    async fn on_event(&self, event: &RuntimeEvent) -> Result<()>;
}

#[derive(Clone, Default)]
pub struct RuntimeEventBus {
    subscribers: Arc<Mutex<Vec<Arc<dyn RuntimeEventSubscriber>>>>,
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
        self.subscribers.lock().unwrap().push(subscriber);
    }

    pub async fn emit(&self, event: RuntimeEvent) -> Result<()> {
        self.recorded.lock().unwrap().push(event.clone());
        let subscribers = self.subscribers.lock().unwrap().clone();
        for subscriber in subscribers {
            subscriber.on_event(&event).await?;
        }
        Ok(())
    }

    pub fn recorded(&self) -> Vec<RuntimeEvent> {
        self.recorded.lock().unwrap().clone()
    }
}
