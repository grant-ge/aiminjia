use std::collections::BTreeSet;
use std::fs;
use std::path::PathBuf;
use std::sync::Mutex;

use anyhow::{anyhow, Context, Result};
use chrono::{DateTime, Datelike, Duration, Local, Timelike, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScheduleStatus {
    Enabled,
    Disabled,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScheduleRecord {
    pub id: String,
    pub title: String,
    pub prompt: String,
    pub cron: String,
    pub human_schedule: String,
    pub status: ScheduleStatus,
    pub next_run_at: Option<DateTime<Utc>>,
    pub timezone: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateScheduleRequest {
    pub title: String,
    pub prompt: String,
    pub cron: String,
    pub timezone: Option<String>,
    pub enabled: Option<bool>,
}

#[derive(Clone, Debug)]
pub struct DueSchedule {
    pub record: ScheduleRecord,
    pub fire_at: DateTime<Utc>,
}

#[derive(Debug)]
pub struct ScheduleStore {
    root: PathBuf,
    lock: Mutex<()>,
}

impl ScheduleStore {
    pub fn new(aijia_home: PathBuf) -> Self {
        Self {
            root: aijia_home.join("schedules"),
            lock: Mutex::new(()),
        }
    }

    pub fn create(&self, request: CreateScheduleRequest) -> Result<ScheduleRecord> {
        let _guard = self.lock.lock().unwrap();
        fs::create_dir_all(&self.root)?;

        let cron = request.cron.trim().to_string();
        let fields =
            parse_cron_expression(&cron).ok_or_else(|| anyhow!("invalid cron: {}", cron))?;
        let now = Utc::now();
        let id = format!("sched-{}", Uuid::new_v4());
        let enabled = request.enabled.unwrap_or(true);
        let next_run_at = if enabled {
            compute_next_cron_run(&fields, Local::now()).map(|d| d.with_timezone(&Utc))
        } else {
            None
        };
        let record = ScheduleRecord {
            id,
            title: request.title.trim().to_string(),
            prompt: request.prompt.trim().to_string(),
            cron: cron.clone(),
            human_schedule: cron_to_human(&cron),
            status: if enabled {
                ScheduleStatus::Enabled
            } else {
                ScheduleStatus::Disabled
            },
            next_run_at,
            timezone: request.timezone.unwrap_or_else(|| "local".to_string()),
            created_at: now,
            updated_at: now,
        };
        self.write_record(&record)?;
        Ok(record)
    }

    pub fn list(&self) -> Result<Vec<ScheduleRecord>> {
        let _guard = self.lock.lock().unwrap();
        if !self.root.exists() {
            return Ok(Vec::new());
        }
        let mut records = Vec::new();
        for entry in fs::read_dir(&self.root)? {
            let path = entry?.path();
            if path.extension().and_then(|s| s.to_str()) != Some("json") {
                continue;
            }
            let content = fs::read_to_string(&path)?;
            records.push(serde_json::from_str::<ScheduleRecord>(&content)?);
        }
        records.sort_by(|a, b| {
            a.created_at
                .cmp(&b.created_at)
                .then_with(|| a.id.cmp(&b.id))
        });
        Ok(records)
    }

    pub fn delete(&self, id: &str) -> Result<bool> {
        let _guard = self.lock.lock().unwrap();
        let path = self.record_path(id);
        if !path.exists() {
            return Ok(false);
        }
        fs::remove_file(path)?;
        Ok(true)
    }

    pub fn take_due(&self, now: DateTime<Utc>) -> Result<Vec<DueSchedule>> {
        let _guard = self.lock.lock().unwrap();
        if !self.root.exists() {
            return Ok(Vec::new());
        }

        let mut due = Vec::new();
        for entry in fs::read_dir(&self.root)? {
            let path = entry?.path();
            if path.extension().and_then(|s| s.to_str()) != Some("json") {
                continue;
            }
            let content = fs::read_to_string(&path)?;
            let mut record = serde_json::from_str::<ScheduleRecord>(&content)?;
            if record.status != ScheduleStatus::Enabled {
                continue;
            }
            let Some(next_run_at) = record.next_run_at else {
                continue;
            };
            if next_run_at > now {
                continue;
            }

            due.push(DueSchedule {
                record: record.clone(),
                fire_at: next_run_at,
            });

            if let Some(fields) = parse_cron_expression(&record.cron) {
                let from_local = now.with_timezone(&Local);
                record.next_run_at =
                    compute_next_cron_run(&fields, from_local).map(|next| next.with_timezone(&Utc));
                record.updated_at = now;
                self.write_record(&record)?;
            }
        }

        due.sort_by(|a, b| {
            a.fire_at
                .cmp(&b.fire_at)
                .then_with(|| a.record.id.cmp(&b.record.id))
        });
        Ok(due)
    }

    fn write_record(&self, record: &ScheduleRecord) -> Result<()> {
        let path = self.record_path(&record.id);
        let tmp = path.with_extension("json.tmp");
        let bytes = serde_json::to_vec_pretty(record)?;
        fs::write(&tmp, bytes).with_context(|| format!("write temp schedule {:?}", tmp))?;
        fs::rename(&tmp, &path).with_context(|| format!("rename schedule {:?}", path))?;
        Ok(())
    }

    fn record_path(&self, id: &str) -> PathBuf {
        self.root.join(format!("{}.json", sanitize_id(id)))
    }
}

fn sanitize_id(id: &str) -> String {
    id.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '-'
            }
        })
        .collect()
}

#[derive(Clone, Debug)]
struct CronFields {
    minute: BTreeSet<u32>,
    hour: BTreeSet<u32>,
    day_of_month: BTreeSet<u32>,
    month: BTreeSet<u32>,
    day_of_week: BTreeSet<u32>,
}

fn parse_cron_expression(expr: &str) -> Option<CronFields> {
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

fn compute_next_cron_run(fields: &CronFields, from: DateTime<Local>) -> Option<DateTime<Local>> {
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

fn cron_to_human(cron: &str) -> String {
    let parts: Vec<&str> = cron.split_whitespace().collect();
    if parts.len() != 5 {
        return cron.to_string();
    }
    let minute = parts[0].parse::<u32>().ok();
    let hour = parts[1].parse::<u32>().ok();
    match (minute, hour, parts[2], parts[3], parts[4]) {
        (Some(m), Some(h), "*", "*", "*") => format!("每天 {:02}:{:02}", h, m),
        (Some(m), Some(h), "*", "*", dow) if dow.parse::<u32>().is_ok() => {
            format!(
                "每周{} {:02}:{:02}",
                weekday_name(dow.parse().unwrap()),
                h,
                m
            )
        }
        _ => cron.to_string(),
    }
}

fn weekday_name(day: u32) -> &'static str {
    match day {
        0 | 7 => "日",
        1 => "一",
        2 => "二",
        3 => "三",
        4 => "四",
        5 => "五",
        6 => "六",
        _ => "?",
    }
}
