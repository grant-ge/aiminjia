use super::item::{AgendaItem, EndCondition, Freq, RecurrenceRule};
use chrono::{DateTime, Datelike, Months, Utc};

pub fn compute_next_fire_at(item: &AgendaItem, now: DateTime<Utc>) -> Option<DateTime<Utc>> {
    match &item.rule {
        None => one_shot_next(item, now),
        Some(rule) => recurring_next(item, rule, now),
    }
}

fn one_shot_next(item: &AgendaItem, now: DateTime<Utc>) -> Option<DateTime<Utc>> {
    if item.occurrence_count == 0 && item.start_at >= now {
        Some(item.start_at)
    } else {
        None
    }
}

fn recurring_next(
    item: &AgendaItem,
    rule: &RecurrenceRule,
    now: DateTime<Utc>,
) -> Option<DateTime<Utc>> {
    let interval = rule.interval.max(1) as i64;
    let mut cursor = item.start_at;
    let mut steps_taken: u32 = 0;

    while cursor <= now || item.skip_dates.contains(&cursor) {
        cursor = match rule.freq {
            Freq::Daily => cursor + chrono::Duration::days(interval),
            Freq::Weekly => cursor + chrono::Duration::weeks(interval),
            Freq::Monthly => add_months(cursor, interval as u32)?,
            Freq::Yearly => add_years(cursor, interval as u32)?,
        };
        steps_taken += 1;
        if steps_taken > 10_000 {
            return None;
        }
    }

    let total_occurrences = item.occurrence_count + 1;
    match &rule.end_condition {
        EndCondition::Never => Some(cursor),
        EndCondition::Count { n } => {
            if total_occurrences > *n {
                None
            } else {
                Some(cursor)
            }
        }
        EndCondition::Until { at } => {
            if cursor > *at {
                None
            } else {
                Some(cursor)
            }
        }
    }
}

fn add_months(dt: DateTime<Utc>, months: u32) -> Option<DateTime<Utc>> {
    dt.checked_add_months(Months::new(months))
}

fn add_years(dt: DateTime<Utc>, years: u32) -> Option<DateTime<Utc>> {
    dt.with_year(dt.year() + years as i32)
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
    fn one_shot_equal_now_returns_start_at() {
        let now = Utc.with_ymd_and_hms(2026, 5, 7, 9, 0, 0).unwrap();
        let item = make_one_shot(now, 0);
        assert_eq!(compute_next_fire_at(&item, now), Some(now));
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

    fn make_recurring(
        start_at: DateTime<Utc>,
        rule: RecurrenceRule,
        occurrence_count: u32,
    ) -> AgendaItem {
        AgendaItem {
            id: AgendaItemId::new(),
            title: "T".into(),
            prompt: "P".into(),
            organizer_persona_id: "p1".into(),
            participants: vec![Participant { persona_id: "p1".into(), joined_at: Utc::now() }],
            start_at,
            timezone: "UTC".into(),
            rule: Some(rule),
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
    fn daily_returns_first_future_occurrence() {
        let start = Utc.with_ymd_and_hms(2026, 5, 7, 9, 0, 0).unwrap();
        let now = Utc.with_ymd_and_hms(2026, 5, 8, 12, 0, 0).unwrap();
        let item = make_recurring(start, RecurrenceRule {
            freq: Freq::Daily, interval: 1, end_condition: EndCondition::Never,
            by_day: vec![], by_month_day: vec![],
        }, 1);
        let expected = Utc.with_ymd_and_hms(2026, 5, 9, 9, 0, 0).unwrap();
        assert_eq!(compute_next_fire_at(&item, now), Some(expected));
    }

    #[test]
    fn daily_interval_2_skips_every_other_day() {
        let start = Utc.with_ymd_and_hms(2026, 5, 7, 9, 0, 0).unwrap();
        let now = Utc.with_ymd_and_hms(2026, 5, 8, 0, 0, 0).unwrap();
        let item = make_recurring(start, RecurrenceRule {
            freq: Freq::Daily, interval: 2, end_condition: EndCondition::Never,
            by_day: vec![], by_month_day: vec![],
        }, 1);
        let expected = Utc.with_ymd_and_hms(2026, 5, 9, 9, 0, 0).unwrap();
        assert_eq!(compute_next_fire_at(&item, now), Some(expected));
    }

    #[test]
    fn weekly_steps_seven_days() {
        let start = Utc.with_ymd_and_hms(2026, 5, 7, 9, 0, 0).unwrap();
        let now = Utc.with_ymd_and_hms(2026, 5, 8, 0, 0, 0).unwrap();
        let item = make_recurring(start, RecurrenceRule {
            freq: Freq::Weekly, interval: 1, end_condition: EndCondition::Never,
            by_day: vec![], by_month_day: vec![],
        }, 1);
        let expected = Utc.with_ymd_and_hms(2026, 5, 14, 9, 0, 0).unwrap();
        assert_eq!(compute_next_fire_at(&item, now), Some(expected));
    }

    #[test]
    fn monthly_steps_one_month() {
        let start = Utc.with_ymd_and_hms(2026, 5, 7, 9, 0, 0).unwrap();
        let now = Utc.with_ymd_and_hms(2026, 5, 8, 0, 0, 0).unwrap();
        let item = make_recurring(start, RecurrenceRule {
            freq: Freq::Monthly, interval: 1, end_condition: EndCondition::Never,
            by_day: vec![], by_month_day: vec![],
        }, 1);
        let expected = Utc.with_ymd_and_hms(2026, 6, 7, 9, 0, 0).unwrap();
        assert_eq!(compute_next_fire_at(&item, now), Some(expected));
    }

    #[test]
    fn yearly_steps_one_year() {
        let start = Utc.with_ymd_and_hms(2026, 5, 7, 9, 0, 0).unwrap();
        let now = Utc.with_ymd_and_hms(2026, 5, 8, 0, 0, 0).unwrap();
        let item = make_recurring(start, RecurrenceRule {
            freq: Freq::Yearly, interval: 1, end_condition: EndCondition::Never,
            by_day: vec![], by_month_day: vec![],
        }, 1);
        let expected = Utc.with_ymd_and_hms(2027, 5, 7, 9, 0, 0).unwrap();
        assert_eq!(compute_next_fire_at(&item, now), Some(expected));
    }
}
