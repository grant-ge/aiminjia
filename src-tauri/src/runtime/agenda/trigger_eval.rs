use super::item::AgendaItem;
use chrono::{DateTime, Utc};

pub fn compute_next_fire_at(item: &AgendaItem, now: DateTime<Utc>) -> Option<DateTime<Utc>> {
    if item.rule.is_none() {
        return one_shot_next(item, now);
    }
    None // 循环分支后续任务实现
}

fn one_shot_next(item: &AgendaItem, now: DateTime<Utc>) -> Option<DateTime<Utc>> {
    if item.occurrence_count == 0 && item.start_at > now {
        Some(item.start_at)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::super::item::*;
    use super::*;
    use chrono::TimeZone;

    fn make_one_shot(start_at: DateTime<Utc>, occurrence_count: u32) -> AgendaItem {
        AgendaItem {
            id: AgendaItemId::new(),
            title: "T".into(),
            prompt: "P".into(),
            organizer_persona_id: "p1".into(),
            participants: vec![Participant {
                persona_id: "p1".into(),
                joined_at: Utc::now(),
            }],
            start_at,
            timezone: "UTC".into(),
            rule: None,
            skip_dates: vec![],
            next_fire_at: None,
            occurrence_count,
            status: ItemStatus::Active,
            override_of: None,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }
    }

    #[test]
    fn one_shot_future_returns_start_at() {
        let now = Utc.with_ymd_and_hms(2026, 5, 7, 8, 0, 0).unwrap();
        let start_at = Utc.with_ymd_and_hms(2026, 5, 7, 9, 0, 0).unwrap();
        let item = make_one_shot(start_at, 0);
        assert_eq!(compute_next_fire_at(&item, now), Some(start_at));
    }

    #[test]
    fn one_shot_past_returns_none() {
        let now = Utc.with_ymd_and_hms(2026, 5, 7, 10, 0, 0).unwrap();
        let start_at = Utc.with_ymd_and_hms(2026, 5, 7, 9, 0, 0).unwrap();
        let item = make_one_shot(start_at, 0);
        assert_eq!(compute_next_fire_at(&item, now), None);
    }

    #[test]
    fn one_shot_already_fired_returns_none() {
        let now = Utc.with_ymd_and_hms(2026, 5, 7, 8, 0, 0).unwrap();
        let start_at = Utc.with_ymd_and_hms(2026, 5, 7, 9, 0, 0).unwrap();
        let item = make_one_shot(start_at, 1);
        assert_eq!(compute_next_fire_at(&item, now), None);
    }
}
