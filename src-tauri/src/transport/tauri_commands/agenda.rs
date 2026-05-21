use std::sync::Arc;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use tauri::State;

use crate::runtime::agenda::{
    AgendaItem, AgendaItemId, AgendaStore, ItemStatus, Occurrence, Participant,
};
use crate::storage::UserScopedPathResolver;

fn store_for(resolver: &Arc<dyn UserScopedPathResolver>) -> Result<AgendaStore, String> {
    let paths = resolver.require_paths().map_err(|e| e.to_string())?;
    Ok(AgendaStore::new(paths.base_dir()))
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ItemFilter {
    pub status_in: Option<Vec<ItemStatus>>,
    #[serde(alias = "personaId")]
    pub employee_id: Option<String>,
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
        if let Some(employee_id) = filter.employee_id {
            items.retain(|i| i.organizer_employee_id == employee_id);
        }
        if let Some(search) = filter.search.filter(|s| !s.is_empty()) {
            let lower = search.to_lowercase();
            items.retain(|i| {
                i.title.to_lowercase().contains(&lower) || i.prompt.to_lowercase().contains(&lower)
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
    #[serde(alias = "organizerPersonaId")]
    pub organizer_employee_id: String,
    pub start_at: DateTime<Utc>,
    pub timezone: Option<String>,
    pub rule: Option<crate::runtime::agenda::RecurrenceRule>,
    #[serde(default)]
    pub workspace_path: Option<String>,
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

    let organizer_employee_id = request.organizer_employee_id.trim().to_string();
    if organizer_employee_id.is_empty() {
        return Err("organizer_employee_id is required".into());
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

    let workspace_path = request
        .workspace_path
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string);

    let mut item = AgendaItem {
        id: AgendaItemId::new(),
        title,
        prompt,
        organizer_employee_id: organizer_employee_id.clone(),
        participants: vec![Participant {
            employee_id: organizer_employee_id,
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
        workspace_path,
        created_at: now,
        updated_at: now,
    };
    item.next_fire_at = crate::runtime::agenda::compute_next_fire_at(&item, now);
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

fn deserialize_nullable_rule<'de, D>(
    deserializer: D,
) -> Result<Option<Option<crate::runtime::agenda::RecurrenceRule>>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = Option::<serde_json::Value>::deserialize(deserializer)?;
    match value {
        None => Ok(Some(None)),
        Some(value) => serde_json::from_value(value)
            .map(Some)
            .map_err(serde::de::Error::custom),
    }
}

fn deserialize_nullable_string<'de, D>(deserializer: D) -> Result<Option<Option<String>>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = Option::<String>::deserialize(deserializer)?;
    Ok(Some(value))
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct UpdateAgendaItemRequest {
    pub title: Option<String>,
    pub prompt: Option<String>,
    pub start_at: Option<DateTime<Utc>>,
    pub timezone: Option<String>,
    #[serde(default, deserialize_with = "deserialize_nullable_rule")]
    pub rule: Option<Option<crate::runtime::agenda::RecurrenceRule>>,
    pub status: Option<ItemStatus>,
    #[serde(default, deserialize_with = "deserialize_nullable_string")]
    pub workspace_path: Option<Option<String>>,
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
    if let Some(wp) = request.workspace_path {
        item.workspace_path = wp
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string);
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

#[tauri::command]
pub async fn cancel_agenda_item(
    id: String,
    resolver: State<'_, Arc<dyn UserScopedPathResolver>>,
) -> Result<AgendaItem, String> {
    let store = store_for(&resolver)?;
    store
        .cancel(&AgendaItemId(id), Utc::now())
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn restore_agenda_item(
    id: String,
    resolver: State<'_, Arc<dyn UserScopedPathResolver>>,
) -> Result<AgendaItem, String> {
    let store = store_for(&resolver)?;
    store
        .restore(&AgendaItemId(id), Utc::now())
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn run_agenda_item_now(
    id: String,
    resolver: State<'_, Arc<dyn UserScopedPathResolver>>,
    dispatcher: State<'_, Arc<crate::transport::tauri_commands::chat::TauriChatCommandAdapter>>,
) -> Result<String, String> {
    use crate::runtime::agenda::{AgendaRunDispatcher, TriggerSource};
    let store = store_for(&resolver)?;
    let item = store.get(&AgendaItemId(id)).map_err(|e| e.to_string())?;
    let now = Utc::now();
    dispatcher
        .dispatch(item, now, TriggerSource::ManualRunNow, now)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn list_agenda_occurrences(
    item_id: String,
    limit: Option<usize>,
    resolver: State<'_, Arc<dyn UserScopedPathResolver>>,
) -> Result<Vec<Occurrence>, String> {
    let store = store_for(&resolver)?;
    store
        .list_occurrences(&AgendaItemId(item_id), limit.unwrap_or(50))
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn skip_occurrence(
    id: String,
    at: DateTime<Utc>,
    resolver: State<'_, Arc<dyn UserScopedPathResolver>>,
) -> Result<AgendaItem, String> {
    let store = store_for(&resolver)?;
    store
        .set_skip(&AgendaItemId(id), at)
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn unskip_occurrence(
    id: String,
    at: DateTime<Utc>,
    resolver: State<'_, Arc<dyn UserScopedPathResolver>>,
) -> Result<AgendaItem, String> {
    let store = store_for(&resolver)?;
    store
        .unset_skip(&AgendaItemId(id), at)
        .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn create_request(
        title: &str,
        prompt: &str,
        organizer_employee_id: &str,
        timezone: Option<&str>,
    ) -> CreateAgendaItemRequest {
        CreateAgendaItemRequest {
            title: title.into(),
            prompt: prompt.into(),
            organizer_employee_id: organizer_employee_id.into(),
            start_at: Utc.with_ymd_and_hms(2026, 5, 7, 9, 0, 0).unwrap(),
            timezone: timezone.map(str::to_string),
            rule: None,
            workspace_path: None,
        }
    }

    #[test]
    fn build_create_item_trims_required_fields_and_defaults_blank_timezone() {
        let now = Utc.with_ymd_and_hms(2026, 5, 7, 8, 0, 0).unwrap();
        let item = build_agenda_item_from_create_request(
            create_request(
                "  Standup  ",
                "  Discuss blockers  ",
                " persona-1 ",
                Some("   "),
            ),
            now,
        )
        .unwrap();

        assert_eq!(item.title, "Standup");
        assert_eq!(item.prompt, "Discuss blockers");
        assert_eq!(item.organizer_employee_id, "persona-1");
        assert_eq!(item.participants[0].employee_id, "persona-1");
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
        assert_eq!(err, "organizer_employee_id is required");
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
            organizer_employee_id: "p1".into(),
            participants: vec![Participant {
                employee_id: "p1".into(),
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
            workspace_path: None,
            created_at: now,
            updated_at: now,
        }
    }

    #[test]
    fn apply_update_trims_fields_and_recomputes_next_fire() {
        let now = Utc.with_ymd_and_hms(2026, 5, 7, 8, 0, 0).unwrap();
        let mut item = make_item_for_update(now);
        let original_organizer = item.organizer_employee_id.clone();
        let updated = apply_update_agenda_item_request(
            &mut item,
            UpdateAgendaItemRequest {
                title: Some("  New title  ".into()),
                prompt: Some("  New prompt  ".into()),
                start_at: Some(Utc.with_ymd_and_hms(2026, 5, 7, 10, 0, 0).unwrap()),
                timezone: Some("  UTC  ".into()),
                rule: Some(None),
                status: Some(ItemStatus::Paused),
                workspace_path: None,
            },
            now,
        )
        .unwrap();

        assert_eq!(updated.title, "New title");
        assert_eq!(updated.prompt, "New prompt");
        assert_eq!(updated.timezone, "UTC");
        assert_eq!(updated.status, ItemStatus::Paused);
        assert_eq!(updated.organizer_employee_id, original_organizer);
        assert_eq!(updated.participants[0].employee_id, original_organizer);
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

    #[test]
    fn update_request_json_rule_null_means_clear_rule() {
        let request: UpdateAgendaItemRequest = serde_json::from_value(serde_json::json!({
            "rule": null
        }))
        .unwrap();
        assert!(matches!(request.rule, Some(None)));
    }

    #[test]
    fn update_request_json_missing_rule_means_leave_unchanged() {
        let request: UpdateAgendaItemRequest = serde_json::from_value(serde_json::json!({
            "title": "T"
        }))
        .unwrap();
        assert!(matches!(request.rule, None));
    }
}
