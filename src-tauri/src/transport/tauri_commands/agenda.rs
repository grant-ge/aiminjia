use std::sync::Arc;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tauri::State;

use crate::runtime::agenda::{
    AgendaItem, AgendaItemId, AgendaStore, ItemStatus, Occurrence,
};
use crate::storage::UserScopedPathResolver;

fn store_for(
    resolver: &Arc<dyn UserScopedPathResolver>,
) -> Result<AgendaStore, String> {
    let paths = resolver.require_paths().map_err(|e| e.to_string())?;
    Ok(AgendaStore::new(paths.base_dir()))
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ItemFilter {
    pub status_in: Option<Vec<ItemStatus>>,
    pub persona_id: Option<String>,
    pub search: Option<String>,
}

#[tauri::command]
pub async fn list_agenda_items(
    filter: Option<ItemFilter>,
    resolver: State<'_, Arc<dyn UserScopedPathResolver>>,
) -> Result<Vec<AgendaItem>, String> {
    let store = store_for(&resolver)?;
    let mut items = store.list().map_err(|e| e.to_string())?;
    if let Some(filter) = filter {
        if let Some(statuses) = filter.status_in {
            items.retain(|i| statuses.contains(&i.status));
        }
        if let Some(persona) = filter.persona_id {
            items.retain(|i| i.organizer_persona_id == persona);
        }
        if let Some(search) = filter.search.filter(|s| !s.is_empty()) {
            let lower = search.to_lowercase();
            items.retain(|i| {
                i.title.to_lowercase().contains(&lower)
                    || i.prompt.to_lowercase().contains(&lower)
            });
        }
    }
    items.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
    Ok(items)
}

#[tauri::command]
pub async fn get_agenda_item(
    id: String,
    resolver: State<'_, Arc<dyn UserScopedPathResolver>>,
) -> Result<AgendaItem, String> {
    let store = store_for(&resolver)?;
    store.get(&AgendaItemId(id)).map_err(|e| e.to_string())
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateAgendaItemRequest {
    pub title: String,
    pub prompt: String,
    pub organizer_persona_id: String,
    pub start_at: DateTime<Utc>,
    pub timezone: Option<String>,
    pub rule: Option<crate::runtime::agenda::RecurrenceRule>,
}

#[tauri::command]
pub async fn create_agenda_item(
    request: CreateAgendaItemRequest,
    resolver: State<'_, Arc<dyn UserScopedPathResolver>>,
) -> Result<AgendaItem, String> {
    use crate::runtime::agenda::{Participant, ItemStatus};
    let store = store_for(&resolver)?;
    let now = Utc::now();
    let mut item = AgendaItem {
        id: AgendaItemId::new(),
        title: request.title,
        prompt: request.prompt,
        organizer_persona_id: request.organizer_persona_id.clone(),
        participants: vec![Participant {
            persona_id: request.organizer_persona_id,
            joined_at: now,
        }],
        start_at: request.start_at,
        timezone: request.timezone.unwrap_or_else(|| "Asia/Shanghai".into()),
        rule: request.rule,
        skip_dates: vec![],
        next_fire_at: None,
        occurrence_count: 0,
        status: ItemStatus::Active,
        override_of: None,
        created_at: now,
        updated_at: now,
    };
    item.next_fire_at =
        crate::runtime::agenda::compute_next_fire_at(&item, now);
    store.create(item).map_err(|e| e.to_string())
}
