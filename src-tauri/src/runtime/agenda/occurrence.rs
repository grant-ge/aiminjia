use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use super::item::AgendaItemId;
use crate::runtime::ids::{RunId, SessionId};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum OccurrenceStatus {
    Running,
    Succeeded,
    Failed,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TriggerSource {
    Scheduled,
    ManualRunNow,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Occurrence {
    pub id: String,
    pub agenda_item_id: AgendaItemId,
    pub fired_at: DateTime<Utc>,
    pub planned_fire_at: DateTime<Utc>,
    pub started_at: DateTime<Utc>,
    pub finished_at: Option<DateTime<Utc>>,
    #[serde(alias = "primaryPersonaId")]
    pub primary_employee_id: String,
    pub conversation_id: String,
    pub session_id: SessionId,
    pub run_id: RunId,
    pub status: OccurrenceStatus,
    pub error_summary: Option<String>,
    pub trigger_source: TriggerSource,
}

impl Occurrence {
    pub fn new_id() -> String {
        format!("occ-{}", uuid::Uuid::new_v4())
    }
}

#[cfg(test)]
mod occurrence_migration_tests {
    use super::*;

    #[test]
    fn legacy_primary_persona_id_deserializes_into_employee_id() {
        let raw = r#"{
            "id": "occ-x",
            "agendaItemId": "agenda-x",
            "firedAt": "2026-05-09T00:00:00Z",
            "plannedFireAt": "2026-05-09T00:00:00Z",
            "startedAt": "2026-05-09T00:00:00Z",
            "finishedAt": null,
            "primaryPersonaId": "default",
            "conversationId": "c1",
            "sessionId": "c1",
            "runId": "r1",
            "status": "running",
            "errorSummary": null,
            "triggerSource": "scheduled"
        }"#;
        let occ: Occurrence = serde_json::from_str(raw).expect("legacy occurrence must parse");
        assert_eq!(occ.primary_employee_id, "default");
    }

    #[test]
    fn new_primary_employee_id_round_trip() {
        use chrono::{TimeZone, Utc};
        let t = Utc.with_ymd_and_hms(2026, 5, 9, 0, 0, 0).unwrap();
        let occ = Occurrence {
            id: "occ-x".into(),
            agenda_item_id: AgendaItemId("agenda-x".into()),
            fired_at: t,
            planned_fire_at: t,
            started_at: t,
            finished_at: None,
            primary_employee_id: "emp-1".into(),
            conversation_id: "c1".into(),
            session_id: SessionId::from("c1"),
            run_id: RunId::from("r1"),
            status: OccurrenceStatus::Running,
            error_summary: None,
            trigger_source: TriggerSource::Scheduled,
        };
        let s = serde_json::to_string(&occ).unwrap();
        assert!(
            s.contains("\"primaryEmployeeId\":\"emp-1\""),
            "wire format = camelCase, got {s}"
        );
        assert!(
            !s.contains("primaryPersonaId"),
            "must not emit legacy field on write, got {s}"
        );
    }
}
