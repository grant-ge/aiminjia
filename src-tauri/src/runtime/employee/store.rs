use std::fs;
use std::path::PathBuf;
use std::sync::Mutex;

use anyhow::{Context, Result, anyhow};
use chrono::{DateTime, Local, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::runtime::schedule::{compute_next_cron_run, parse_cron_expression};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EmployeeLifecycle {
    /// Default after hiring; can be dispatched on-demand and via cron.
    Active,
    /// User explicitly paused this employee. on-demand dispatch blocked,
    /// cron is automatically blocked too. UI shows ⏸ with "恢复" action.
    Paused,
    /// Soft-deleted; hidden from main grid but recoverable for 7 days.
    /// scheduler ignores; on-demand dispatch returns error.
    Archived,
}

impl Default for EmployeeLifecycle {
    fn default() -> Self {
        Self::Active
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EmployeeRecord {
    pub id: String,
    pub name: String,
    pub role: String,
    pub description: String,
    pub avatar: String,
    pub template_id: Option<String>,
    /// Tool names allowed for this employee (empty = all tools allowed).
    pub tool_whitelist: Vec<String>,
    /// Standard cron expression (5 fields). None = on-demand only.
    pub cron: Option<String>,
    pub timezone: String,
    /// Employment lifecycle. Replaces the legacy boolean `enabled` field.
    /// Old records without this field deserialize to `Active` (see migration test).
    #[serde(default)]
    pub lifecycle: EmployeeLifecycle,
    /// Whether the cron schedule (if any) fires. Independent of lifecycle so
    /// users can pause cron without pausing the whole employee.
    ///
    /// `serde(alias = "enabled")` lets legacy employee.json files (which
    /// only have the old `enabled` field) deserialize transparently. If a
    /// JSON object happens to contain BOTH `cron_enabled` and `enabled`,
    /// serde's behavior is last-one-wins — undefined for our purposes.
    /// New writers must emit only `cron_enabled`.
    #[serde(default = "default_true", alias = "enabled")]
    pub cron_enabled: bool,
    /// Employee-specific resource config (monitoring URLs, table IDs, field mappings, etc.)
    pub resource_config: serde_json::Value,
    /// Prepended to the system prompt to establish the employee's identity.
    pub system_prompt_extra: Option<String>,
    /// Skill id (matching `~/.renlijia/skills/<id>/SKILL.md`) the LLM should
    /// `load_skill` as the first action when the employee is dispatched.
    /// `None` means no skill hint is injected.
    pub default_skill_id: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub last_run_at: Option<DateTime<Utc>>,
    pub next_run_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateEmployeeRequest {
    pub name: String,
    pub role: String,
    pub description: String,
    pub avatar: String,
    pub template_id: Option<String>,
    pub tool_whitelist: Option<Vec<String>>,
    pub cron: Option<String>,
    pub timezone: Option<String>,
    pub lifecycle: Option<EmployeeLifecycle>,
    pub cron_enabled: Option<bool>,
    pub resource_config: Option<serde_json::Value>,
    pub system_prompt_extra: Option<String>,
    pub default_skill_id: Option<String>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateEmployeeRequest {
    pub name: Option<String>,
    pub role: Option<String>,
    pub description: Option<String>,
    pub avatar: Option<String>,
    pub tool_whitelist: Option<Vec<String>>,
    pub cron: Option<Option<String>>,
    pub timezone: Option<String>,
    pub lifecycle: Option<EmployeeLifecycle>,
    pub cron_enabled: Option<bool>,
    pub resource_config: Option<serde_json::Value>,
    pub system_prompt_extra: Option<Option<String>>,
    pub default_skill_id: Option<Option<String>>,
}

#[derive(Debug, Clone)]
pub struct DueEmployee {
    pub record: EmployeeRecord,
    pub fire_at: DateTime<Utc>,
    /// Number of missed cron ticks since last_run_at (catchup count).
    pub missed_count: u32,
}

#[derive(Debug)]
pub struct EmployeeStore {
    root: PathBuf,
    lock: Mutex<()>,
}

impl EmployeeStore {
    pub fn new(employees_dir: PathBuf) -> Self {
        Self {
            root: employees_dir,
            lock: Mutex::new(()),
        }
    }

    fn record_dir(&self, id: &str) -> PathBuf {
        self.root.join(id)
    }

    fn record_path(&self, id: &str) -> PathBuf {
        self.record_dir(id).join("employee.json")
    }

    fn write_record(&self, record: &EmployeeRecord) -> Result<()> {
        let dir = self.record_dir(&record.id);
        fs::create_dir_all(&dir)?;
        let path = dir.join("employee.json");
        let json = serde_json::to_string_pretty(record)?;
        fs::write(path, json)?;
        Ok(())
    }

    pub fn create(&self, req: CreateEmployeeRequest) -> Result<EmployeeRecord> {
        let _guard = self.lock.lock().unwrap();
        fs::create_dir_all(&self.root)?;

        let lifecycle = req.lifecycle.unwrap_or(EmployeeLifecycle::Active);
        let cron_enabled = req.cron_enabled.unwrap_or(true);
        let timezone = req.timezone.unwrap_or_else(|| "Asia/Shanghai".to_string());

        let next_run_at = if lifecycle == EmployeeLifecycle::Active && cron_enabled {
            req.cron.as_deref().and_then(|cron| {
                let fields = parse_cron_expression(cron)?;
                compute_next_cron_run(&fields, Local::now()).map(|d| d.with_timezone(&Utc))
            })
        } else {
            None
        };

        let record = EmployeeRecord {
            id: format!("emp-{}", Uuid::new_v4()),
            name: req.name.trim().to_string(),
            role: req.role.trim().to_string(),
            description: req.description.trim().to_string(),
            avatar: req.avatar,
            template_id: req.template_id,
            tool_whitelist: req.tool_whitelist.unwrap_or_default(),
            cron: req.cron,
            timezone,
            lifecycle,
            cron_enabled,
            resource_config: req
                .resource_config
                .unwrap_or(serde_json::Value::Object(Default::default())),
            system_prompt_extra: req.system_prompt_extra,
            default_skill_id: req.default_skill_id,
            created_at: Utc::now(),
            updated_at: Utc::now(),
            last_run_at: None,
            next_run_at,
        };

        self.write_record(&record)?;
        Ok(record)
    }

    pub fn get(&self, id: &str) -> Result<EmployeeRecord> {
        let _guard = self.lock.lock().unwrap();
        let path = self.record_path(id);
        let content =
            fs::read_to_string(&path).with_context(|| format!("employee not found: {id}"))?;
        Ok(serde_json::from_str(&content)?)
    }

    pub fn list(&self) -> Result<Vec<EmployeeRecord>> {
        let _guard = self.lock.lock().unwrap();
        self.list_unlocked()
    }

    fn list_unlocked(&self) -> Result<Vec<EmployeeRecord>> {
        if !self.root.exists() {
            return Ok(Vec::new());
        }
        let mut records = Vec::new();
        for entry in fs::read_dir(&self.root)? {
            let entry = entry?;
            let path = entry.path().join("employee.json");
            if !path.exists() {
                continue;
            }
            let content = fs::read_to_string(&path)?;
            match serde_json::from_str::<EmployeeRecord>(&content) {
                Ok(r) => records.push(r),
                Err(e) => log::warn!("[EmployeeStore] failed to parse {}: {e}", path.display()),
            }
        }
        records.sort_by(|a, b| {
            a.created_at
                .cmp(&b.created_at)
                .then_with(|| a.id.cmp(&b.id))
        });
        Ok(records)
    }

    pub fn update(&self, id: &str, req: UpdateEmployeeRequest) -> Result<EmployeeRecord> {
        let _guard = self.lock.lock().unwrap();
        let path = self.record_path(id);
        let content =
            fs::read_to_string(&path).with_context(|| format!("employee not found: {id}"))?;
        let mut record: EmployeeRecord = serde_json::from_str(&content)?;

        if let Some(v) = req.name {
            record.name = v.trim().to_string();
        }
        if let Some(v) = req.role {
            record.role = v.trim().to_string();
        }
        if let Some(v) = req.description {
            record.description = v.trim().to_string();
        }
        if let Some(v) = req.avatar {
            record.avatar = v;
        }
        if let Some(v) = req.tool_whitelist {
            record.tool_whitelist = v;
        }
        if let Some(v) = req.cron {
            record.cron = v;
        }
        if let Some(v) = req.timezone {
            record.timezone = v;
        }
        if let Some(v) = req.lifecycle {
            record.lifecycle = v;
        }
        if let Some(v) = req.cron_enabled {
            record.cron_enabled = v;
        }
        if let Some(v) = req.resource_config {
            record.resource_config = v;
        }
        if let Some(v) = req.system_prompt_extra {
            record.system_prompt_extra = v;
        }
        if let Some(v) = req.default_skill_id {
            record.default_skill_id = v;
        }

        // Recompute next_run_at based on updated cron/lifecycle/cron_enabled
        record.next_run_at = if record.lifecycle == EmployeeLifecycle::Active && record.cron_enabled
        {
            record.cron.as_deref().and_then(|cron| {
                let fields = parse_cron_expression(cron)?;
                compute_next_cron_run(&fields, Local::now()).map(|d| d.with_timezone(&Utc))
            })
        } else {
            None
        };

        record.updated_at = Utc::now();
        self.write_record(&record)?;
        Ok(record)
    }

    pub fn delete(&self, id: &str) -> Result<bool> {
        let _guard = self.lock.lock().unwrap();
        let dir = self.record_dir(id);
        if !dir.exists() {
            return Ok(false);
        }
        fs::remove_dir_all(dir)?;
        Ok(true)
    }

    /// Returns employees whose cron is due at or before `now`, and advances next_run_at.
    /// Implements catch-up: if last_run_at is old, records missed_count but only fires once.
    pub fn take_due(&self, now: DateTime<Utc>) -> Result<Vec<DueEmployee>> {
        let _guard = self.lock.lock().unwrap();
        if !self.root.exists() {
            return Ok(Vec::new());
        }

        let mut due = Vec::new();
        for entry in fs::read_dir(&self.root)? {
            let entry = entry?;
            let path = entry.path().join("employee.json");
            if !path.exists() {
                continue;
            }
            let content = fs::read_to_string(&path)?;
            let mut record = match serde_json::from_str::<EmployeeRecord>(&content) {
                Ok(r) => r,
                Err(e) => {
                    log::warn!("[EmployeeStore] failed to parse {}: {e}", path.display());
                    continue;
                }
            };

            if record.lifecycle != EmployeeLifecycle::Active || !record.cron_enabled {
                continue;
            }
            let cron = match record.cron.as_deref() {
                Some(c) => c,
                None => continue,
            };
            let fields = match parse_cron_expression(cron) {
                Some(f) => f,
                None => continue,
            };

            let Some(next_run_at) = record.next_run_at else {
                continue;
            };
            if next_run_at > now {
                continue;
            }

            // Count missed ticks for logging; only fire once (catch-up semantics)
            let fire_at = next_run_at;
            let mut missed_count = 1u32;
            let mut cursor = next_run_at.with_timezone(&Local);
            loop {
                let Some(next) = compute_next_cron_run(&fields, cursor) else {
                    break;
                };
                if next.with_timezone(&Utc) > now {
                    break;
                }
                missed_count += 1;
                cursor = next;
            }

            // Advance next_run_at past `now`
            let new_next = compute_next_cron_run(&fields, now.with_timezone(&Local))
                .map(|d| d.with_timezone(&Utc));
            record.next_run_at = new_next;
            record.updated_at = Utc::now();

            let json = serde_json::to_string_pretty(&record)?;
            fs::write(entry.path().join("employee.json"), json)?;

            due.push(DueEmployee {
                record,
                fire_at,
                missed_count,
            });
        }
        Ok(due)
    }

    /// Called after a successful run to update last_run_at.
    pub fn record_run(&self, id: &str, ran_at: DateTime<Utc>) -> Result<()> {
        let _guard = self.lock.lock().unwrap();
        let path = self.record_path(id);
        let content =
            fs::read_to_string(&path).with_context(|| format!("employee not found: {id}"))?;
        let mut record: EmployeeRecord = serde_json::from_str(&content)?;
        record.last_run_at = Some(ran_at);
        record.updated_at = Utc::now();
        let json = serde_json::to_string_pretty(&record)?;
        fs::write(path, json)?;
        Ok(())
    }

    /// Returns the directory for an employee's reports.
    pub fn reports_dir(&self, id: &str) -> PathBuf {
        self.record_dir(id).join("reports")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn create_and_get() {
        let dir = TempDir::new().unwrap();
        let store = EmployeeStore::new(dir.path().to_path_buf());

        let req = CreateEmployeeRequest {
            name: "小研".to_string(),
            role: "竞品调研员".to_string(),
            description: "每周汇总竞品动态".to_string(),
            avatar: "🔍".to_string(),
            template_id: Some("builtin:xiaoyuan".to_string()),
            tool_whitelist: Some(vec!["web_search".to_string()]),
            cron: None,
            timezone: None,
            lifecycle: Some(EmployeeLifecycle::Active),
            cron_enabled: Some(true),
            resource_config: None,
            system_prompt_extra: None,
            default_skill_id: None,
        };
        let created = store.create(req).unwrap();
        assert!(created.id.starts_with("emp-"));
        assert_eq!(created.name, "小研");

        let fetched = store.get(&created.id).unwrap();
        assert_eq!(fetched.id, created.id);
    }

    #[test]
    fn create_and_get_with_default_skill_id() {
        let dir = TempDir::new().unwrap();
        let store = EmployeeStore::new(dir.path().to_path_buf());

        let req = CreateEmployeeRequest {
            name: "小研".to_string(),
            role: "竞品调研员".to_string(),
            description: "每周汇总竞品动态".to_string(),
            avatar: "🔍".to_string(),
            template_id: Some("builtin:xiaoyuan".to_string()),
            tool_whitelist: Some(vec!["web_search".to_string()]),
            cron: None,
            timezone: None,
            lifecycle: Some(EmployeeLifecycle::Active),
            cron_enabled: Some(true),
            resource_config: None,
            system_prompt_extra: None,
            default_skill_id: Some("competitive-intelligence".to_string()),
        };
        let created = store.create(req).unwrap();
        assert_eq!(
            created.default_skill_id.as_deref(),
            Some("competitive-intelligence")
        );

        let fetched = store.get(&created.id).unwrap();
        assert_eq!(
            fetched.default_skill_id.as_deref(),
            Some("competitive-intelligence")
        );
    }

    #[test]
    fn list_returns_all() {
        let dir = TempDir::new().unwrap();
        let store = EmployeeStore::new(dir.path().to_path_buf());

        for name in ["小研", "小法", "小算"] {
            store
                .create(CreateEmployeeRequest {
                    name: name.to_string(),
                    role: "test".to_string(),
                    description: "desc".to_string(),
                    avatar: "🤖".to_string(),
                    template_id: None,
                    tool_whitelist: None,
                    cron: None,
                    timezone: None,
                    lifecycle: Some(EmployeeLifecycle::Active),
                    cron_enabled: Some(true),
                    resource_config: None,
                    system_prompt_extra: None,
                    default_skill_id: None,
                })
                .unwrap();
        }
        assert_eq!(store.list().unwrap().len(), 3);
    }

    #[test]
    fn update_cron_enabled_false_clears_next_run() {
        let dir = TempDir::new().unwrap();
        let store = EmployeeStore::new(dir.path().to_path_buf());

        let created = store
            .create(CreateEmployeeRequest {
                name: "小钉".to_string(),
                role: "钉办助理".to_string(),
                description: "每天早上汇总".to_string(),
                avatar: "📌".to_string(),
                template_id: None,
                tool_whitelist: None,
                cron: Some("30 9 * * 1-5".to_string()),
                timezone: None,
                lifecycle: Some(EmployeeLifecycle::Active),
                cron_enabled: Some(true),
                resource_config: None,
                system_prompt_extra: None,
                default_skill_id: None,
            })
            .unwrap();
        assert!(created.next_run_at.is_some());

        let updated = store
            .update(
                &created.id,
                UpdateEmployeeRequest {
                    cron_enabled: Some(false),
                    ..Default::default()
                },
            )
            .unwrap();
        assert!(updated.next_run_at.is_none());
    }

    #[test]
    fn delete_removes_directory() {
        let dir = TempDir::new().unwrap();
        let store = EmployeeStore::new(dir.path().to_path_buf());

        let created = store
            .create(CreateEmployeeRequest {
                name: "小法".to_string(),
                role: "合同审阅员".to_string(),
                description: "审阅合同".to_string(),
                avatar: "⚖️".to_string(),
                template_id: None,
                tool_whitelist: None,
                cron: None,
                timezone: None,
                lifecycle: Some(EmployeeLifecycle::Active),
                cron_enabled: Some(true),
                resource_config: None,
                system_prompt_extra: None,
                default_skill_id: None,
            })
            .unwrap();

        assert!(store.delete(&created.id).unwrap());
        assert!(!store.delete(&created.id).unwrap());
    }

    #[test]
    fn take_due_advances_next_run() {
        let dir = TempDir::new().unwrap();
        let store = EmployeeStore::new(dir.path().to_path_buf());

        let created = store
            .create(CreateEmployeeRequest {
                name: "小销".to_string(),
                role: "客户跟进员".to_string(),
                description: "跟进客户".to_string(),
                avatar: "💼".to_string(),
                template_id: None,
                tool_whitelist: None,
                cron: Some("* * * * *".to_string()),
                timezone: None,
                lifecycle: Some(EmployeeLifecycle::Active),
                cron_enabled: Some(true),
                resource_config: None,
                system_prompt_extra: None,
                default_skill_id: None,
            })
            .unwrap();

        let future = Utc::now() + chrono::Duration::minutes(2);
        let due = store.take_due(future).unwrap();
        assert_eq!(due.len(), 1);
        assert_eq!(due[0].record.id, created.id);

        let listed = store.list().unwrap();
        assert!(listed[0].next_run_at.unwrap() > future);
    }

    #[test]
    fn legacy_employee_json_migrates_enabled_to_lifecycle() {
        use std::fs;
        let dir = TempDir::new().unwrap();
        let store = EmployeeStore::new(dir.path().to_path_buf());

        let emp_dir = dir.path().join("emp-legacy");
        fs::create_dir_all(&emp_dir).unwrap();
        let legacy = serde_json::json!({
            "id": "emp-legacy",
            "name": "L",
            "role": "r",
            "description": "d",
            "avatar": "ð¤",
            "templateId": null,
            "toolWhitelist": [],
            "cron": "0 9 * * 1",
            "timezone": "Asia/Shanghai",
            "enabled": true,
            "resourceConfig": {},
            "systemPromptExtra": null,
            "defaultSkillId": null,
            "createdAt": "2026-01-01T00:00:00Z",
            "updatedAt": "2026-01-01T00:00:00Z",
            "lastRunAt": null,
            "nextRunAt": null,
        });
        fs::write(
            emp_dir.join("employee.json"),
            serde_json::to_string_pretty(&legacy).unwrap(),
        )
        .unwrap();

        let record = store.get("emp-legacy").unwrap();
        assert_eq!(record.lifecycle, EmployeeLifecycle::Active);
        assert!(
            record.cron_enabled,
            "legacy enabled=true â cron_enabled=true"
        );
    }

    #[test]
    fn legacy_disabled_employee_migrates_to_active_with_cron_off() {
        use std::fs;
        let dir = TempDir::new().unwrap();
        let store = EmployeeStore::new(dir.path().to_path_buf());

        let emp_dir = dir.path().join("emp-paused");
        fs::create_dir_all(&emp_dir).unwrap();
        let legacy = serde_json::json!({
            "id": "emp-paused", "name": "P", "role": "r", "description": "d",
            "avatar": "ð¤", "templateId": null, "toolWhitelist": [],
            "cron": "0 9 * * 1", "timezone": "Asia/Shanghai",
            "enabled": false,
            "resourceConfig": {}, "systemPromptExtra": null,
            "defaultSkillId": null,
            "createdAt": "2026-01-01T00:00:00Z", "updatedAt": "2026-01-01T00:00:00Z",
            "lastRunAt": null, "nextRunAt": null,
        });
        fs::write(
            emp_dir.join("employee.json"),
            serde_json::to_string_pretty(&legacy).unwrap(),
        )
        .unwrap();

        let record = store.get("emp-paused").unwrap();
        assert_eq!(record.lifecycle, EmployeeLifecycle::Active);
        assert!(
            !record.cron_enabled,
            "legacy enabled=false â cron_enabled=false"
        );
    }
}

fn default_true() -> bool {
    true
}
