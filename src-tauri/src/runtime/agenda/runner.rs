use super::dispatcher::AgendaRunDispatcher;
use super::store::AgendaStore;
use crate::storage::UserScopedPathResolver;
use chrono::{DateTime, Utc};
use std::sync::Arc;

pub fn spawn_agenda_runner(
    _path_resolver: Arc<dyn UserScopedPathResolver>,
    _dispatcher: Arc<dyn AgendaRunDispatcher>,
) {
}

pub async fn run_due_once(
    _store: &AgendaStore,
    _dispatcher: &dyn AgendaRunDispatcher,
    _now: DateTime<Utc>,
) -> anyhow::Result<()> {
    Ok(())
}
