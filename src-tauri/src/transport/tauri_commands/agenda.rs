use std::sync::Arc;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tauri::State;

use crate::runtime::agenda::{
    AgendaItem, AgendaItemId, AgendaStore, ItemStatus, Participant,
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

fn build_agenda_item_from_create_request(
    request: CreateAgendaItemRequest,
    now: DateTime<Utc>,
) -> Result<AgendaItem, String> {
    let title = request.title.trim().to_string();
    if title.is_empty() {
        return Err("title is required".into());
    }

    let prompt = request.prompt.trim().to_string();
    if prompt.is_empty() {
        return Err("prompt is required".into());
    }

    let organizer_persona_id = request.organizer_persona_id.trim().to_string();
    if organizer_persona_id.is_empty() {
        return Err("organizer_persona_id is required".into());
    }

    let timezone = request
        .timezone
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .unwrap_or("Asia/Shanghai")
        .to_string();
    if timezone.parse::<chrono_tz::Tz>().is_err() {
        return Err("timezone must be a valid IANA timezone".into());
    }

    let mut item = AgendaItem {
        id: AgendaItemId::new(),
        title,
        prompt,
        organizer_persona_id: organizer_persona_id.clone(),
        participants: vec![Participant {
            persona_id: organizer_persona_id,
            joined_at: now,
        }],
        start_at: request.start_at,
        timezone,
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
    Ok(item)
}

#[tauri::command]
pub async fn create_agenda_item(
    request: CreateAgendaItemRequest,
    resolver: State<'_, Arc<dyn UserScopedPathResolver>>,
) -> Result<AgendaItem, String> {
    let store = store_for(&resolver)?;
    let item = build_agenda_item_from_create_request(request, Utc::now())?;
    store.create(item).map_err(|e| e.to_string())
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct UpdateAgendaItemRequest {
    pub title: Option<String>,
    pub prompt: Option<String>,
    pub start_at: Option<DateTime<Utc>>,
    pub timezone: Option<String>,
    pub rule: Option<Option<crate::runtime::agenda::RecurrenceRule>>,
    pub status: Option<ItemStatus>,
}

fn apply_update_agenda_item_request(
    item: &mut AgendaItem,
    request: UpdateAgendaItemRequest,
    now: DateTime<Utc>,
) -> Result<AgendaItem, String> {
    if let Some(t) = request.title {
        let title = t.trim().to_string();
        if title.is_empty() {
            return Err("title is required".into());
        }
        item.title = title;
    }
    if let Some(p) = request.prompt {
        let prompt = p.trim().to_string();
        if prompt.is_empty() {
            return Err("prompt is required".into());
        }
        item.prompt = prompt;
    }
    if let Some(s) = request.start_at {
        item.start_at = s;
    }
    if let Some(tz) = request.timezone {
        let timezone = tz.trim().to_string();
        if timezone.is_empty() {
            return Err("timezone is required".into());
        }
        if timezone.parse::<chrono_tz::Tz>().is_err() {
            return Err("timezone must be a valid IANA timezone".into());
        }
        item.timezone = timezone;
    }
    if let Some(r) = request.rule {
        item.rule = r;
    }
    if let Some(st) = request.status {
        item.status = st;
    }
    item.updated_at = now;
    item.next_fire_at = crate::runtime::agenda::compute_next_fire_at(item, now);
    Ok(item.clone())
}

#[tauri::command]
pub async fn update_agenda_item(
    id: String,
    request: UpdateAgendaItemRequest,
    resolver: State<'_, Arc<dyn UserScopedPathResolver>>,
) -> Result<AgendaItem, String> {
    let store = store_for(&resolver)?;
    let item_id = AgendaItemId(id);
    let mut item = store.get(&item_id).map_err(|e| e.to_string())?;
    let item = apply_update_agenda_item_request(&mut item, request, Utc::now())?;
    store.update(item).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn delete_agenda_item(
    id: String,
    resolver: State<'_, Arc<dyn UserScopedPathResolver>>,
) -> Result<bool, String> {
    let store = store_for(&resolver)?;
    store.delete(&AgendaItemId(id)).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn create_request(
        title: &str,
        prompt: &str,
        organizer_persona_id: &str,
        timezone: Option<&str>,
    ) -> CreateAgendaItemRequest {
        CreateAgendaItemRequest {
            title: title.into(),
            prompt: prompt.into(),
            organizer_persona_id: organizer_persona_id.into(),
            start_at: Utc.with_ymd_and_hms(2026, 5, 7, 9, 0, 0).unwrap(),
            timezone: timezone.map(str::to_string),
            rule: None,
        }
    }

    #[test]
    fn build_create_item_trims_required_fields_and_defaults_blank_timezone() {
        let now = Utc.with_ymd_and_hms(2026, 5, 7, 8, 0, 0).unwrap();
        let item = build_agenda_item_from_create_request(
            create_request("  Standup  ", "  Discuss blockers  ", " persona-1 ", Some("   ")),
            now,
        )
        .unwrap();

        assert_eq!(item.title, "Standup");
        assert_eq!(item.prompt, "Discuss blockers");
        assert_eq!(item.organizer_persona_id, "persona-1");
        assert_eq!(item.participants[0].persona_id, "persona-1");
        assert_eq!(item.timezone, "Asia/Shanghai");
    }

    #[test]
    fn build_create_item_rejects_blank_title() {
        let now = Utc.with_ymd_and_hms(2026, 5, 7, 8, 0, 0).unwrap();
        let err =
            build_agenda_item_from_create_request(create_request("   ", "Prompt", "p1", None), now)
                .unwrap_err();
        assert_eq!(err, "title is required");
    }

    #[test]
    fn build_create_item_rejects_blank_prompt() {
        let now = Utc.with_ymd_and_hms(2026, 5, 7, 8, 0, 0).unwrap();
        let err =
            build_agenda_item_from_create_request(create_request("Title", "   ", "p1", None), now)
                .unwrap_err();
        assert_eq!(err, "prompt is required");
    }

    #[test]
    fn build_create_item_rejects_blank_organizer() {
        let now = Utc.with_ymd_and_hms(2026, 5, 7, 8, 0, 0).unwrap();
        let err = build_agenda_item_from_create_request(
            create_request("Title", "Prompt", "   ", None),
            now,
        )
        .unwrap_err();
        assert_eq!(err, "organizer_persona_id is required");
    }

    #[test]
    fn build_create_item_rejects_invalid_timezone() {
        let now = Utc.with_ymd_and_hms(2026, 5, 7, 8, 0, 0).unwrap();
        let err = build_agenda_item_from_create_request(
            create_request("Title", "Prompt", "p1", Some("Not/AZone")),
            now,
        )
        .unwrap_err();
        assert_eq!(err, "timezone must be a valid IANA timezone");
    }

    fn make_item_for_update(now: DateTime<Utc>) -> AgendaItem {
        AgendaItem {
            id: AgendaItemId("agenda-update-test".into()),
            title: "Old".into(),
            prompt: "Old prompt".into(),
            organizer_persona_id: "p1".into(),
            participants: vec![Participant {
                persona_id: "p1".into(),
                joined_at: now,
            }],
            start_at: Utc.with_ymd_and_hms(2026, 5, 7, 9, 0, 0).unwrap(),
            timezone: "Asia/Shanghai".into(),
            rule: None,
            skip_dates: vec![],
            next_fire_at: None,
            occurrence_count: 0,
            status: ItemStatus::Active,
            override_of: None,
            created_at: now,
            updated_at: now,
        }
    }

    #[test]
    fn apply_update_trims_fields_and_recomputes_next_fire() {
        let now = Utc.with_ymd_and_hms(2026, 5, 7, 8, 0, 0).unwrap();
        let mut item = make_item_for_update(now);
        let original_organizer = item.organizer_persona_id.clone();
        let updated = apply_update_agenda_item_request(
            &mut item,
            UpdateAgendaItemRequest {
                title: Some("  New title  ".into()),
                prompt: Some("  New prompt  ".into()),
                start_at: Some(Utc.with_ymd_and_hms(2026, 5, 7, 10, 0, 0).unwrap()),
                timezone: Some("  UTC  ".into()),
                rule: Some(None),
                status: Some(ItemStatus::Paused),
            },
            now,
        )
        .unwrap();

        assert_eq!(updated.title, "New title");
        assert_eq!(updated.prompt, "New prompt");
        assert_eq!(updated.timezone, "UTC");
        assert_eq!(updated.status, ItemStatus::Paused);
        assert_eq!(updated.organizer_persona_id, original_organizer);
        assert_eq!(updated.participants[0].persona_id, original_organizer);
        assert_eq!(updated.updated_at, now);
        assert_eq!(updated.next_fire_at, Some(updated.start_at));
    }

    #[test]
    fn apply_update_rejects_blank_title() {
        let now = Utc.with_ymd_and_hms(2026, 5, 7, 8, 0, 0).unwrap();
        let mut item = make_item_for_update(now);
        let err = apply_update_agenda_item_request(
            &mut item,
            UpdateAgendaItemRequest {
                title: Some("   ".into()),
                ..Default::default()
            },
            now,
        )
        .unwrap_err();
        assert_eq!(err, "title is required");
    }

    #[test]
    fn apply_update_rejects_blank_prompt() {
        let now = Utc.with_ymd_and_hms(2026, 5, 7, 8, 0, 0).unwrap();
        let mut item = make_item_for_update(now);
        let err = apply_update_agenda_item_request(
            &mut item,
            UpdateAgendaItemRequest {
                prompt: Some("   ".into()),
                ..Default::default()
            },
            now,
        )
        .unwrap_err();
        assert_eq!(err, "prompt is required");
    }

    #[test]
    fn apply_update_rejects_blank_timezone() {
        let now = Utc.with_ymd_and_hms(2026, 5, 7, 8, 0, 0).unwrap();
        let mut item = make_item_for_update(now);
        let err = apply_update_agenda_item_request(
            &mut item,
            UpdateAgendaItemRequest {
                timezone: Some("   ".into()),
                ..Default::default()
            },
            now,
        )
        .unwrap_err();
        assert_eq!(err, "timezone is required");
    }

    #[test]
    fn apply_update_rejects_invalid_timezone() {
        let now = Utc.with_ymd_and_hms(2026, 5, 7, 8, 0, 0).unwrap();
        let mut item = make_item_for_update(now);
        let err = apply_update_agenda_item_request(
            &mut item,
            UpdateAgendaItemRequest {
                timezone: Some("Not/AZone".into()),
                ..Default::default()
            },
            now,
        )
        .unwrap_err();
        assert_eq!(err, "timezone must be a valid IANA timezone");
    }
}
