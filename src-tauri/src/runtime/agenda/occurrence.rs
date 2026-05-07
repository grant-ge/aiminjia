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
    pub primary_persona_id: String,
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
