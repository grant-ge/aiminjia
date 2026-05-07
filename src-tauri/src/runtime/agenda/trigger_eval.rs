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

    if cursor <= now {
        cursor = advance_after_now(cursor, &rule.freq, interval, now)?;
    }

    let mut skip_steps: u32 = 0;
    while item.skip_dates.contains(&cursor) {
        cursor = advance_once(cursor, &rule.freq, interval)?;
        skip_steps += 1;
        if skip_steps > 10_000 {
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

fn advance_after_now(
    cursor: DateTime<Utc>,
    freq: &Freq,
    interval: i64,
    now: DateTime<Utc>,
) -> Option<DateTime<Utc>> {
    match freq {
        Freq::Daily => advance_by_fixed_days_after_now(cursor, interval, now),
        Freq::Weekly => advance_by_fixed_days_after_now(cursor, interval * 7, now),
        Freq::Monthly | Freq::Yearly => {
            let mut cursor = cursor;
            let mut steps_taken: u32 = 0;
            while cursor <= now {
                cursor = advance_once(cursor, freq, interval)?;
                steps_taken += 1;
                if steps_taken > 10_000 {
                    return None;
                }
            }
            Some(cursor)
        }
    }
}

fn advance_by_fixed_days_after_now(
    cursor: DateTime<Utc>,
    step_days: i64,
    now: DateTime<Utc>,
) -> Option<DateTime<Utc>> {
    let elapsed_days = (now - cursor).num_days();
    let steps = elapsed_days / step_days + 1;
    let days_to_add = step_days.checked_mul(steps)?;
    cursor.checked_add_signed(chrono::Duration::days(days_to_add))
}

fn advance_once(dt: DateTime<Utc>, freq: &Freq, interval: i64) -> Option<DateTime<Utc>> {
    match freq {
        Freq::Daily => dt.checked_add_signed(chrono::Duration::days(interval)),
        Freq::Weekly => dt.checked_add_signed(chrono::Duration::weeks(interval)),
        Freq::Monthly => add_months(dt, interval as u32),
        Freq::Yearly => add_years(dt, interval as u32),
    }
}

fn add_years(dt: DateTime<Utc>, years: u32) -> Option<DateTime<Utc>> {
    let years = i32::try_from(years.max(1)).ok()?;
    let mut target_year = dt.year().checked_add(years)?;
    let mut attempts: u32 = 0;

    loop {
        if let Some(next) = dt.with_year(target_year) {
            return Some(next);
        }

        attempts += 1;
        if attempts > 10_000 {
            return None;
        }
        target_year = target_year.checked_add(years)?;
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

    #[test]
    fn yearly_leap_day_skips_invalid_years() {
        let start = Utc.with_ymd_and_hms(2024, 2, 29, 9, 0, 0).unwrap();
        let now = Utc.with_ymd_and_hms(2024, 3, 1, 0, 0, 0).unwrap();
        let item = make_recurring(start, RecurrenceRule {
            freq: Freq::Yearly, interval: 1, end_condition: EndCondition::Never,
            by_day: vec![], by_month_day: vec![],
        }, 1);
        let expected = Utc.with_ymd_and_hms(2028, 2, 29, 9, 0, 0).unwrap();
        assert_eq!(compute_next_fire_at(&item, now), Some(expected));
    }

    #[test]
    fn daily_long_catch_up_returns_next_future_occurrence() {
        let start = Utc.with_ymd_and_hms(1990, 1, 1, 9, 0, 0).unwrap();
        let now = Utc.with_ymd_and_hms(2026, 5, 7, 12, 0, 0).unwrap();
        let item = make_recurring(start, RecurrenceRule {
            freq: Freq::Daily, interval: 1, end_condition: EndCondition::Never,
            by_day: vec![], by_month_day: vec![],
        }, 1);
        let expected = Utc.with_ymd_and_hms(2026, 5, 8, 9, 0, 0).unwrap();
        assert_eq!(compute_next_fire_at(&item, now), Some(expected));
    }

    #[test]
    fn count_returns_none_after_n_occurrences() {
        let start = Utc.with_ymd_and_hms(2026, 5, 7, 9, 0, 0).unwrap();
        let now = Utc.with_ymd_and_hms(2026, 5, 9, 0, 0, 0).unwrap();
        let item = make_recurring(start, RecurrenceRule {
            freq: Freq::Daily, interval: 1, end_condition: EndCondition::Count { n: 3 },
            by_day: vec![], by_month_day: vec![],
        }, 3);
        assert_eq!(compute_next_fire_at(&item, now), None);
    }

    #[test]
    fn count_returns_some_when_under_n() {
        let start = Utc.with_ymd_and_hms(2026, 5, 7, 9, 0, 0).unwrap();
        let now = Utc.with_ymd_and_hms(2026, 5, 8, 0, 0, 0).unwrap();
        let item = make_recurring(start, RecurrenceRule {
            freq: Freq::Daily, interval: 1, end_condition: EndCondition::Count { n: 3 },
            by_day: vec![], by_month_day: vec![],
        }, 1);
        let expected = Utc.with_ymd_and_hms(2026, 5, 8, 9, 0, 0).unwrap();
        assert_eq!(compute_next_fire_at(&item, now), Some(expected));
    }

    #[test]
    fn until_returns_none_after_until_at() {
        let start = Utc.with_ymd_and_hms(2026, 5, 7, 9, 0, 0).unwrap();
        let until = Utc.with_ymd_and_hms(2026, 5, 9, 0, 0, 0).unwrap();
        let now = Utc.with_ymd_and_hms(2026, 5, 9, 12, 0, 0).unwrap();
        let item = make_recurring(start, RecurrenceRule {
            freq: Freq::Daily, interval: 1, end_condition: EndCondition::Until { at: until },
            by_day: vec![], by_month_day: vec![],
        }, 2);
        assert_eq!(compute_next_fire_at(&item, now), None);
    }

    #[test]
    fn skip_dates_skips_to_next() {
        let start = Utc.with_ymd_and_hms(2026, 5, 7, 9, 0, 0).unwrap();
        let now = Utc.with_ymd_and_hms(2026, 5, 7, 12, 0, 0).unwrap();
        let skip = Utc.with_ymd_and_hms(2026, 5, 8, 9, 0, 0).unwrap();
        let mut item = make_recurring(start, RecurrenceRule {
            freq: Freq::Daily, interval: 1, end_condition: EndCondition::Never,
            by_day: vec![], by_month_day: vec![],
        }, 1);
        item.skip_dates.push(skip);
        let expected = Utc.with_ymd_and_hms(2026, 5, 9, 9, 0, 0).unwrap();
        assert_eq!(compute_next_fire_at(&item, now), Some(expected));
    }

    #[test]
    fn count_does_not_consume_missed_scheduled_slots() {
        let start = Utc.with_ymd_and_hms(2026, 5, 7, 9, 0, 0).unwrap();
        let now = Utc.with_ymd_and_hms(2026, 5, 10, 12, 0, 0).unwrap();
        let item = make_recurring(start, RecurrenceRule {
            freq: Freq::Daily, interval: 1, end_condition: EndCondition::Count { n: 3 },
            by_day: vec![], by_month_day: vec![],
        }, 1);
        let expected = Utc.with_ymd_and_hms(2026, 5, 11, 9, 0, 0).unwrap();
        assert_eq!(compute_next_fire_at(&item, now), Some(expected));
    }

}
