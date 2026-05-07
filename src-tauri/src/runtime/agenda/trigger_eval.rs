use super::item::AgendaItem;
use chrono::{DateTime, Utc};

pub fn compute_next_fire_at(_item: &AgendaItem, _now: DateTime<Utc>) -> Option<DateTime<Utc>> {
    None
}
