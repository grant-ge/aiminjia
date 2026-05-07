use anyhow::Result;
use async_trait::async_trait;

use super::item::AgendaItem;
use super::occurrence::Occurrence;

#[async_trait]
pub trait AgendaRunDispatcher: Send + Sync {
    async fn dispatch(&self, item: AgendaItem, occurrence: Occurrence) -> Result<()>;
}
