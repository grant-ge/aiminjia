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
    pub persona_id: String,
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
    pub organizer_persona_id: String,
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
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}
