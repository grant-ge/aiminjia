use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(transparent)]
pub struct AgendaItemId(pub String);

impl AgendaItemId {
    pub fn new() -> Self {
        Self(format!("agenda-{}", uuid::Uuid::new_v4()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Default for AgendaItemId {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ItemStatus {
    Active,
    Paused,
    Completed,
    Orphaned,
    Cancelled,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Freq {
    Daily,
    Weekly,
    Monthly,
    Yearly,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum EndCondition {
    Never,
    Count { n: u32 },
    Until { at: DateTime<Utc> },
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Weekday {
    Mon,
    Tue,
    Wed,
    Thu,
    Fri,
    Sat,
    Sun,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Participant {
    #[serde(alias = "personaId")]
    pub employee_id: String,
    pub joined_at: DateTime<Utc>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RecurrenceRule {
    pub freq: Freq,
    pub interval: u32,
    pub end_condition: EndCondition,
    #[serde(default)]
    pub by_day: Vec<Weekday>,
    #[serde(default)]
    pub by_month_day: Vec<i8>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct OverrideRef {
    pub series_item_id: AgendaItemId,
    pub original_at: DateTime<Utc>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AgendaItem {
    pub id: AgendaItemId,
    pub title: String,
    pub prompt: String,
    #[serde(alias = "organizerPersonaId")]
    pub organizer_employee_id: String,
    pub participants: Vec<Participant>,
    pub start_at: DateTime<Utc>,
    pub timezone: String,
    pub rule: Option<RecurrenceRule>,
    #[serde(default)]
    pub skip_dates: Vec<DateTime<Utc>>,
    pub next_fire_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub occurrence_count: u32,
    pub status: ItemStatus,
    pub override_of: Option<OverrideRef>,
    /// 触发时绑定到新 conversation 的工作目录（可选）。None 表示不显式 authorize，
    /// 由 dispatcher 走应用全局当前 workspace 兜底。
    #[serde(default)]
    pub workspace_path: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[cfg(test)]
mod migration_tests {
    use super::*;

    #[test]
    fn legacy_organizer_persona_id_deserializes_into_employee_id() {
        let raw = r#"{
            "id": "agenda-x",
            "title": "t",
            "prompt": "p",
            "organizerPersonaId": "default",
            "participants": [{"personaId": "default", "joinedAt": "2026-05-09T00:00:00Z"}],
            "startAt": "2026-05-10T00:00:00Z",
            "timezone": "Asia/Shanghai",
            "rule": null,
            "skipDates": [],
            "nextFireAt": null,
            "occurrenceCount": 0,
            "status": "active",
            "overrideOf": null,
            "workspacePath": null,
            "createdAt": "2026-05-09T00:00:00Z",
            "updatedAt": "2026-05-09T00:00:00Z"
        }"#;
        let item: AgendaItem = serde_json::from_str(raw).expect("legacy json must parse");
        assert_eq!(item.organizer_employee_id, "default");
        assert_eq!(item.participants.len(), 1);
        assert_eq!(item.participants[0].employee_id, "default");
    }

    #[test]
    fn new_field_names_round_trip() {
        let item = AgendaItem {
            id: AgendaItemId("a".into()),
            title: "t".into(),
            prompt: "p".into(),
            organizer_employee_id: "emp-1".into(),
            participants: vec![Participant {
                employee_id: "emp-1".into(),
                joined_at: chrono::Utc::now(),
            }],
            start_at: chrono::Utc::now(),
            timezone: "Asia/Shanghai".into(),
            rule: None,
            skip_dates: vec![],
            next_fire_at: None,
            occurrence_count: 0,
            status: ItemStatus::Active,
            override_of: None,
            workspace_path: None,
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        };
        let s = serde_json::to_string(&item).unwrap();
        assert!(s.contains("\"organizerEmployeeId\":\"emp-1\""), "wire format = camelCase, got {s}");
        assert!(!s.contains("organizerPersonaId"), "must not emit legacy field on write, got {s}");
        assert!(s.contains("\"employeeId\":\"emp-1\""), "participant wire format = camelCase, got {s}");
        assert!(!s.contains("personaId"), "must not emit legacy participant field on write, got {s}");
    }
}
