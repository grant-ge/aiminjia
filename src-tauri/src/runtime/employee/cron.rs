//! Cron parser + next-run computation, salvaged from the deleted
//! `runtime::schedule` module (PR-4 task 57). Employee scheduler is the
//! sole consumer; agenda runtime uses chrono-tz directly via trigger_eval.

use std::collections::BTreeSet;

use chrono::{DateTime, Datelike, Duration, Local, Timelike};

#[derive(Clone, Debug)]
pub(crate) struct CronFields {
    pub(crate) minute: BTreeSet<u32>,
    pub(crate) hour: BTreeSet<u32>,
    pub(crate) day_of_month: BTreeSet<u32>,
    pub(crate) month: BTreeSet<u32>,
    pub(crate) day_of_week: BTreeSet<u32>,
}

pub(crate) fn parse_cron_expression(expr: &str) -> Option<CronFields> {
    let parts: Vec<&str> = expr.split_whitespace().collect();
    if parts.len() != 5 {
        return None;
    }
    Some(CronFields {
        minute: expand_field(parts[0], 0, 59, false)?,
        hour: expand_field(parts[1], 0, 23, false)?,
        day_of_month: expand_field(parts[2], 1, 31, false)?,
        month: expand_field(parts[3], 1, 12, false)?,
        day_of_week: expand_field(parts[4], 0, 6, true)?,
    })
}

fn expand_field(field: &str, min: u32, max: u32, dow: bool) -> Option<BTreeSet<u32>> {
    let mut out = BTreeSet::new();
    for part in field.split(',') {
        if part.is_empty() {
            return None;
        }
        if let Some(step_part) = part.strip_prefix("*/") {
            let step = step_part.parse::<u32>().ok()?;
            if step == 0 {
                return None;
            }
            let mut value = min;
            while value <= max {
                out.insert(value);
                value += step;
            }
            continue;
        }
        if part == "*" {
            for value in min..=max {
                out.insert(value);
            }
            continue;
        }
        if let Some((lo, hi)) = part.split_once('-') {
            let lo = normalize_value(lo.parse().ok()?, dow)?;
            let hi_raw = hi.parse::<u32>().ok()?;
            let hi = normalize_value(hi_raw, dow)?;
            if lo > hi || lo < min || hi > max {
                return None;
            }
            for value in lo..=hi {
                out.insert(value);
            }
            continue;
        }
        let value = normalize_value(part.parse().ok()?, dow)?;
        if value < min || value > max {
            return None;
        }
        out.insert(value);
    }
    (!out.is_empty()).then_some(out)
}

fn normalize_value(value: u32, dow: bool) -> Option<u32> {
    if dow && value == 7 {
        Some(0)
    } else {
        Some(value)
    }
}

pub(crate) fn compute_next_cron_run(
    fields: &CronFields,
    from: DateTime<Local>,
) -> Option<DateTime<Local>> {
    let mut t = from + Duration::minutes(1);
    t = t.with_second(0)?.with_nanosecond(0)?;
    let dom_wild = fields.day_of_month.len() == 31;
    let dow_wild = fields.day_of_week.len() == 7;
    for _ in 0..(366 * 24 * 60) {
        let dow = t.weekday().num_days_from_sunday();
        let day_matches = if dom_wild && dow_wild {
            true
        } else if dom_wild {
            fields.day_of_week.contains(&dow)
        } else if dow_wild {
            fields.day_of_month.contains(&t.day())
        } else {
            fields.day_of_month.contains(&t.day()) || fields.day_of_week.contains(&dow)
        };
        if fields.month.contains(&t.month())
            && day_matches
            && fields.hour.contains(&t.hour())
            && fields.minute.contains(&t.minute())
        {
            return Some(t);
        }
        t += Duration::minutes(1);
    }
    None
}
