use std::fs;
use std::path::PathBuf;
use std::sync::Mutex;

use anyhow::{Context, Result};
use chrono::{DateTime, Local, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::runtime::employee::cron::{compute_next_cron_run, parse_cron_expression};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EmployeeLifecycle {
    /// Default after hiring; can be dispatched on-demand and via cron.
    Active,
    /// LEGACY (pre PR-6): user explicitly paused this employee.
    /// **The feature was removed 2026-05-15**: it duplicated `cron_enabled`
    /// for the cron-blocking case, and the "block on-demand dispatch" use
    /// case has no real-world scenario (users delete the employee instead).
    /// We keep the variant so older on-disk records still deserialize;
    /// all read paths treat `Paused` as `Active` via
    /// `EmployeeLifecycle::canonical()` (see below).
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

impl EmployeeLifecycle {
    /// Returns the lifecycle value to act on. Legacy `Paused` records collapse
    /// into `Active` so the rest of the runtime never has to branch on a
    /// retired state. Use this in any read path that selects behavior.
    pub fn canonical(self) -> Self {
        match self {
            Self::Paused => Self::Active,
            other => other,
        }
    }
}

#[derive(Debug, Clone, Copy, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum KnowledgeSourceStatus {
    Pending,
    Indexing,
    Done,
    Failed,
}

impl KnowledgeSourceStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Indexing => "indexing",
            Self::Done => "done",
            Self::Failed => "failed",
        }
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
    /// Additional skill ids available to this employee beyond the default.
    /// Listed in the dispatch prompt so the LLM knows it can load them.
    /// Empty = only the default skill (if any) is hinted.
    #[serde(default)]
    pub skill_ids: Vec<String>,
    /// Pointer to the template snapshot this instance was hired from. When
    /// present, the runtime should treat the snapshot at
    /// `<instance>/template/template.json` as the authoritative source for
    /// `tool_whitelist / system_prompt_extra / default_skill_id / role / ...`.
    /// Old records without this field are auto-populated on read by matching
    /// `template_id` against the embedded bootstrap registry. Records whose
    /// `template_id` doesn't match any known template stay `None` (custom
    /// hand-edited records).
    #[serde(default)]
    pub template_ref: Option<crate::runtime::employee::template_store::TemplateRef>,
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
    pub skill_ids: Option<Vec<String>>,
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
    pub skill_ids: Option<Vec<String>>,
}

#[derive(Debug, Clone)]
pub struct DueEmployee {
    pub record: EmployeeRecord,
    pub fire_at: DateTime<Utc>,
    /// Number of missed cron ticks since last_run_at (catchup count).
    pub missed_count: u32,
}

pub struct EmployeeStore {
    root: PathBuf,
    lock: Mutex<()>,
    /// AgentRegistry 同步钩子：employee lifecycle 变化时通知。
    /// `None` = 不通知（测试 / 早期 boot），生产路径在 lib.rs 中 wire 进去。
    /// 用 RwLock interior mutability 让 `set_sync(&self)` 不要求 mut，方便 Arc 共享。
    sync: std::sync::RwLock<
        Option<std::sync::Arc<dyn crate::runtime::agent::employee_projection::EmployeeAgentSync>>,
    >,
}

impl std::fmt::Debug for EmployeeStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EmployeeStore")
            .field("root", &self.root)
            .field(
                "has_sync",
                &self.sync.read().map(|g| g.is_some()).unwrap_or(false),
            )
            .finish()
    }
}

impl EmployeeStore {
    pub fn new(employees_dir: PathBuf) -> Self {
        Self {
            root: employees_dir,
            lock: Mutex::new(()),
            sync: std::sync::RwLock::new(None),
        }
    }

    /// 设置 lifecycle 变化时的同步钩子。lib.rs 启动后调用一次注入
    /// `AgentRegistrySync`；之后 hire / update / archive / purge 自动通知。
    pub fn set_sync(
        &self,
        sync: std::sync::Arc<dyn crate::runtime::agent::employee_projection::EmployeeAgentSync>,
    ) {
        let mut g = self.sync.write().expect("sync write poisoned");
        *g = Some(sync);
    }

    fn notify_active(&self, rec: &EmployeeRecord) {
        if let Some(sync) = self.sync.read().expect("sync read poisoned").as_ref() {
            sync.on_active(rec);
        }
    }

    fn notify_inactive(&self, id: &str) {
        if let Some(sync) = self.sync.read().expect("sync read poisoned").as_ref() {
            sync.on_inactive(id);
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
        write_atomic(&path, json.as_bytes())
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

        let mut record = EmployeeRecord {
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
            skill_ids: req.skill_ids.unwrap_or_default(),
            template_ref: None,
            created_at: Utc::now(),
            updated_at: Utc::now(),
            last_run_at: None,
            next_run_at,
        };

        self.write_record(&record)?;
        // Stamp the per-instance template/ snapshot if we recognise this
        // template_id. Failures here are non-fatal (snapshot is additive
        // metadata; PR4 will start *reading* from it).
        if let Some(ref_) = stamp_snapshot_for_record(&self.root, &record) {
            record.template_ref = Some(ref_);
            // Best-effort persist of the new template_ref. Ignore errors.
            let _ = self.write_record(&record);
        }
        if matches!(record.lifecycle, EmployeeLifecycle::Active) {
            self.notify_active(&record);
        }
        Ok(record)
    }

    pub fn get(&self, id: &str) -> Result<EmployeeRecord> {
        let _guard = self.lock.lock().unwrap();
        let path = self.record_path(id);
        let content =
            fs::read_to_string(&path).with_context(|| format!("employee not found: {id}"))?;
        let mut record: EmployeeRecord = serde_json::from_str(&content)?;
        // PR-6: collapse legacy `Paused` lifecycle into `Active`. The pause
        // feature was removed; on-disk records may still carry the value.
        let pre_canonical = record.lifecycle;
        record.lifecycle = record.lifecycle.canonical();
        let canonicalized = pre_canonical != record.lifecycle;
        let mut needs_write = canonicalized;
        if record.template_ref.is_none() {
            if let Some(ref_) = stamp_snapshot_for_record(&self.root, &record) {
                record.template_ref = Some(ref_);
                needs_write = true;
            }
        }
        if needs_write {
            let _ = self.write_record(&record);
        }
        Ok(record)
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
                Ok(mut r) => {
                    // PR-6: collapse legacy `Paused` lifecycle into `Active`
                    // and persist so subsequent reads don't keep converting.
                    let pre_canonical = r.lifecycle;
                    r.lifecycle = r.lifecycle.canonical();
                    let canonicalized = pre_canonical != r.lifecycle;
                    let mut needs_write = canonicalized;
                    if r.template_ref.is_none() {
                        if let Some(ref_) = stamp_snapshot_for_record(&self.root, &r) {
                            r.template_ref = Some(ref_);
                            needs_write = true;
                        }
                    }
                    if needs_write {
                        let _ = self.write_record(&r);
                    }
                    records.push(r)
                }
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
        if let Some(v) = req.skill_ids {
            record.skill_ids = v;
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
        match record.lifecycle.canonical() {
            EmployeeLifecycle::Active => self.notify_active(&record),
            EmployeeLifecycle::Archived => {
                self.notify_inactive(&record.id);
            }
            // `Paused` is collapsed to `Active` by `canonical()`; the match
            // arm above handles it. This explicit branch keeps the compiler
            // happy with an exhaustive match without a wildcard hiding bugs.
            EmployeeLifecycle::Paused => unreachable!("canonical() collapsed Paused → Active"),
        }
        Ok(record)
    }

    pub fn update_knowledge_source_status(
        &self,
        id: &str,
        path: &str,
        status: KnowledgeSourceStatus,
        sliced_count: u64,
        error: Option<String>,
    ) -> anyhow::Result<()> {
        let _guard = self.lock.lock().unwrap();
        let path_buf = self.record_path(id);
        let content = std::fs::read_to_string(&path_buf)?;
        let mut record: EmployeeRecord = serde_json::from_str(&content)?;

        let now = chrono::Utc::now().to_rfc3339();
        let sources = record
            .resource_config
            .get_mut("knowledgeSources")
            .and_then(|v| v.as_array_mut())
            .ok_or_else(|| anyhow::anyhow!("knowledgeSources field missing"))?;

        let entry = sources
            .iter_mut()
            .find(|s| s.get("path").and_then(|p| p.as_str()) == Some(path))
            .ok_or_else(|| anyhow::anyhow!("knowledge source path not found: {}", path))?;

        let obj = entry
            .as_object_mut()
            .ok_or_else(|| anyhow::anyhow!("knowledge source is not an object"))?;

        obj.insert(
            "status".into(),
            serde_json::Value::String(status.as_str().into()),
        );
        obj.insert("slicedCount".into(), serde_json::Value::from(sliced_count));
        match status {
            KnowledgeSourceStatus::Indexing => {
                obj.insert("startedAt".into(), serde_json::Value::String(now));
                obj.remove("error");
            }
            KnowledgeSourceStatus::Done => {
                obj.insert("completedAt".into(), serde_json::Value::String(now));
                obj.remove("error");
            }
            KnowledgeSourceStatus::Failed => {
                if let Some(err) = error {
                    obj.insert("error".into(), serde_json::Value::String(err));
                }
                obj.insert("completedAt".into(), serde_json::Value::String(now));
            }
            KnowledgeSourceStatus::Pending => {
                obj.remove("error");
                obj.remove("startedAt");
                obj.remove("completedAt");
            }
        }

        self.write_record(&record)
    }

    /// Hard-delete an employee directory. The escape hatch behind the soft-delete
    /// model: used by `purge_old_archived` (auto-cleanup after retention) and by
    /// the "永久删除" UI action. Normal user delete should set lifecycle=Archived
    /// via `update` instead, so the record can be recovered for 7 days.
    pub fn purge(&self, id: &str) -> Result<bool> {
        let _guard = self.lock.lock().unwrap();
        let dir = self.record_dir(id);
        if !dir.exists() {
            return Ok(false);
        }
        remove_dir_all_retry(&dir)?;
        self.notify_inactive(id);
        Ok(true)
    }

    /// Hard-delete an employee directory IFF it is still archived AND its
    /// updated_at is still older than `threshold`. Both checks happen under
    /// the same lock as the directory removal — protects against the race
    /// where a user calls `employee_restore` between the purge sweep's
    /// list() and the per-record purge() call.
    ///
    /// Returns Ok(true) if purged, Ok(false) if the precondition no longer
    /// holds (employee is now Active/Paused, or was just touched).
    pub fn purge_if_archived_older_than(
        &self,
        id: &str,
        threshold: chrono::Duration,
    ) -> Result<bool> {
        let _guard = self.lock.lock().unwrap();
        let path = self.record_path(id);
        if !path.exists() {
            return Ok(false);
        }
        let content =
            fs::read_to_string(&path).with_context(|| format!("employee not found: {id}"))?;
        let record: EmployeeRecord = serde_json::from_str(&content)?;

        if record.lifecycle != EmployeeLifecycle::Archived {
            return Ok(false);
        }
        if Utc::now().signed_duration_since(record.updated_at) <= threshold {
            return Ok(false);
        }

        let dir = self.record_dir(id);
        remove_dir_all_retry(&dir)?;
        self.notify_inactive(id);
        Ok(true)
    }

    /// Purge any employees that have been archived for longer than `threshold`.
    /// Returns the number of records hard-deleted.
    ///
    /// The per-record check is atomic (see `purge_if_archived_older_than`):
    /// if the user calls `employee_restore` between this method's `list()`
    /// snapshot and the per-record purge call, the restored record is
    /// preserved.
    ///
    /// Per-record errors are logged and skipped, not bubbled — Windows AV /
    /// Indexer can briefly hold handles to a directory we want to delete, and
    /// one stuck record should not abort the whole sweep.
    pub fn purge_old_archived(&self, threshold: chrono::Duration) -> Result<usize> {
        let mut purged = 0;
        let records = self.list()?;
        for record in records {
            if record.lifecycle != EmployeeLifecycle::Archived {
                continue;
            }
            match self.purge_if_archived_older_than(&record.id, threshold) {
                Ok(true) => purged += 1,
                Ok(false) => {}
                Err(e) => log::warn!(
                    "[EmployeeStore] purge of {} failed: {} — will retry next tick",
                    record.id,
                    e
                ),
            }
        }
        Ok(purged)
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
            write_atomic(&entry.path().join("employee.json"), json.as_bytes())?;

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
        write_atomic(&path, json.as_bytes())
    }

    /// Read-only clone of an Employee record by id.  Does not lock writes.
    /// Used by spawn_subagent (P1.3) to source a Teammate's profile without
    /// blocking on the write lock.
    ///
    /// Returns `None` if the employee does not exist or cannot be parsed.
    /// Use `get()` instead if you need an error on missing records.
    pub fn get_readonly(&self, id: &str) -> Option<EmployeeRecord> {
        self.get(id).ok()
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
            tool_whitelist: Some(vec!["WebSearch".to_string()]),
            cron: None,
            timezone: None,
            lifecycle: Some(EmployeeLifecycle::Active),
            cron_enabled: Some(true),
            resource_config: None,
            system_prompt_extra: None,
            default_skill_id: None,
            skill_ids: None,
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
            tool_whitelist: Some(vec!["WebSearch".to_string()]),
            cron: None,
            timezone: None,
            lifecycle: Some(EmployeeLifecycle::Active),
            cron_enabled: Some(true),
            resource_config: None,
            system_prompt_extra: None,
            default_skill_id: Some("competitive-intelligence".to_string()),
            skill_ids: None,
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

    fn make_template_snapshot(
        template_id: &str,
        version: &str,
    ) -> crate::runtime::employee::template_store::TemplateSnapshot {
        crate::runtime::employee::template_store::TemplateSnapshot {
            template_id: template_id.to_string(),
            version: version.to_string(),
            name: "X".to_string(),
            avatar: "".to_string(),
            role: "".to_string(),
            description: "".to_string(),
            badge: "".to_string(),
            display_i18n: serde_json::Value::Null,
            prompt_i18n: serde_json::Value::Null,
            schema_i18n: serde_json::Value::Null,
            system_prompt_extra: "".to_string(),
            tool_whitelist: vec![],
            cron: "".to_string(),
            default_skill_id: "".to_string(),
            skill_ids: vec![],
            requires_dingtalk: false,
            requires_attachment: serde_json::Value::Null,
            resource_config_schema: serde_json::Value::Null,
            resource_config_ui: serde_json::Value::Null,
        }
    }

    #[test]
    fn latest_snapshot_for_template_prefers_newer_cache_over_bootstrap() {
        let dir = TempDir::new().unwrap();
        let cache_dir = dir.path().join("cache");
        crate::runtime::employee::template_store::write_cache(
            &cache_dir,
            &make_template_snapshot("builtin:xiaoyuan", "9.9.9"),
        )
        .unwrap();

        let (snap, source) = latest_snapshot_for_template("builtin:xiaoyuan", &cache_dir).unwrap();

        assert_eq!(snap.version, "9.9.9");
        assert_eq!(source, "cache");
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
                    skill_ids: None,
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
                skill_ids: None,
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
                skill_ids: None,
            })
            .unwrap();

        assert!(store.purge(&created.id).unwrap());
        assert!(!store.purge(&created.id).unwrap());
    }

    #[test]
    fn purge_old_archived_removes_only_old_archived() {
        use chrono::Duration;
        let dir = TempDir::new().unwrap();
        let store = EmployeeStore::new(dir.path().to_path_buf());

        let recent_archived = store
            .create(CreateEmployeeRequest {
                name: "recent".into(),
                role: "r".into(),
                description: "d".into(),
                avatar: "🤖".into(),
                template_id: None,
                tool_whitelist: None,
                cron: None,
                timezone: None,
                lifecycle: Some(EmployeeLifecycle::Archived),
                cron_enabled: Some(true),
                resource_config: None,
                system_prompt_extra: None,
                default_skill_id: None,
                skill_ids: None,
            })
            .unwrap();

        let old_archived = store
            .create(CreateEmployeeRequest {
                name: "old".into(),
                role: "r".into(),
                description: "d".into(),
                avatar: "🤖".into(),
                template_id: None,
                tool_whitelist: None,
                cron: None,
                timezone: None,
                lifecycle: Some(EmployeeLifecycle::Archived),
                cron_enabled: Some(true),
                resource_config: None,
                system_prompt_extra: None,
                default_skill_id: None,
                skill_ids: None,
            })
            .unwrap();

        let active = store
            .create(CreateEmployeeRequest {
                name: "active".into(),
                role: "r".into(),
                description: "d".into(),
                avatar: "🤖".into(),
                template_id: None,
                tool_whitelist: None,
                cron: None,
                timezone: None,
                lifecycle: Some(EmployeeLifecycle::Active),
                cron_enabled: Some(true),
                resource_config: None,
                system_prompt_extra: None,
                default_skill_id: None,
                skill_ids: None,
            })
            .unwrap();

        // Backdate `old_archived.updated_at` to 8 days ago by writing the JSON directly.
        let path = dir.path().join(&old_archived.id).join("employee.json");
        let content = std::fs::read_to_string(&path).unwrap();
        let mut value: serde_json::Value = serde_json::from_str(&content).unwrap();
        value["updatedAt"] =
            serde_json::json!((chrono::Utc::now() - chrono::Duration::days(8)).to_rfc3339());
        std::fs::write(&path, serde_json::to_string_pretty(&value).unwrap()).unwrap();

        let purged = store.purge_old_archived(Duration::days(7)).unwrap();
        assert_eq!(purged, 1);

        let remaining = store.list().unwrap();
        let ids: Vec<&str> = remaining.iter().map(|r| r.id.as_str()).collect();
        assert!(ids.contains(&recent_archived.id.as_str()));
        assert!(ids.contains(&active.id.as_str()));
        assert!(!ids.contains(&old_archived.id.as_str()));
    }

    #[test]
    fn purge_if_archived_older_than_skips_active_record() {
        use chrono::Duration;
        let dir = TempDir::new().unwrap();
        let store = EmployeeStore::new(dir.path().to_path_buf());

        // Create an Active employee
        let active = store
            .create(CreateEmployeeRequest {
                name: "active".into(),
                role: "r".into(),
                description: "d".into(),
                avatar: "🤖".into(),
                template_id: None,
                tool_whitelist: None,
                cron: None,
                timezone: None,
                lifecycle: Some(EmployeeLifecycle::Active),
                cron_enabled: Some(true),
                resource_config: None,
                system_prompt_extra: None,
                default_skill_id: None,
                skill_ids: None,
            })
            .unwrap();

        // Even if we lie about it being old, the lifecycle check should reject
        let path = dir.path().join(&active.id).join("employee.json");
        let content = std::fs::read_to_string(&path).unwrap();
        let mut value: serde_json::Value = serde_json::from_str(&content).unwrap();
        value["updatedAt"] =
            serde_json::json!((chrono::Utc::now() - chrono::Duration::days(30)).to_rfc3339());
        std::fs::write(&path, serde_json::to_string_pretty(&value).unwrap()).unwrap();

        let purged = store
            .purge_if_archived_older_than(&active.id, Duration::days(7))
            .unwrap();
        assert!(
            !purged,
            "Active employee should never be purged regardless of age"
        );
        assert!(
            store.get(&active.id).is_ok(),
            "directory should still exist"
        );
    }

    #[test]
    fn purge_if_archived_older_than_skips_recently_archived() {
        use chrono::Duration;
        let dir = TempDir::new().unwrap();
        let store = EmployeeStore::new(dir.path().to_path_buf());

        let recent = store
            .create(CreateEmployeeRequest {
                name: "recent".into(),
                role: "r".into(),
                description: "d".into(),
                avatar: "🤖".into(),
                template_id: None,
                tool_whitelist: None,
                cron: None,
                timezone: None,
                lifecycle: Some(EmployeeLifecycle::Archived),
                cron_enabled: Some(true),
                resource_config: None,
                system_prompt_extra: None,
                default_skill_id: None,
                skill_ids: None,
            })
            .unwrap();

        let purged = store
            .purge_if_archived_older_than(&recent.id, Duration::days(7))
            .unwrap();
        assert!(!purged, "Recently archived employee should not be purged");
        assert!(store.get(&recent.id).is_ok());
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
                skill_ids: None,
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

/// Atomic write: tmp file + rename. On crash mid-write, the original file is
/// left intact instead of becoming a 0-byte stub. Especially important on
/// Windows where AV briefly holds open handles to written files; an
/// unfortunately-timed panic with `fs::write` can corrupt employee.json,
/// after which `list_unlocked` silently drops the record from `list()` /
/// `take_due` forever.
fn write_atomic(path: &std::path::Path, bytes: &[u8]) -> Result<()> {
    let tmp = path.with_extension("json.tmp");
    fs::write(&tmp, bytes).with_context(|| format!("write tmp: {}", tmp.display()))?;
    fs::rename(&tmp, path).with_context(|| format!("rename tmp → {}", path.display()))?;
    Ok(())
}

/// Pick the newest available snapshot for one template id.
///
/// Bootstrap is always available for built-ins, but a cached server copy with
/// a higher version must win so manual/automatic sync affects newly hired
/// employees immediately.
fn latest_snapshot_for_template(
    tid: &str,
    cache_dir: &std::path::Path,
) -> Option<(
    crate::runtime::employee::template_store::TemplateSnapshot,
    &'static str,
)> {
    use crate::runtime::employee::template_store as ts;

    let mut best: Option<(ts::TemplateSnapshot, &'static str)> = match ts::bootstrap_template(tid) {
        Ok(Some(s)) => Some((s, "bootstrap")),
        Ok(None) => None,
        Err(e) => {
            log::warn!("[EmployeeStore] bootstrap lookup failed for {tid}: {e}");
            None
        }
    };

    let tid_dir = cache_dir.join(tid);
    if let Ok(rd) = std::fs::read_dir(&tid_dir) {
        for entry in rd.flatten() {
            let p = entry.path();
            if p.extension().and_then(|e| e.to_str()) != Some("json") {
                continue;
            }
            let Ok(content) = std::fs::read_to_string(&p) else {
                continue;
            };
            let Ok(snap) = serde_json::from_str::<ts::TemplateSnapshot>(&content) else {
                continue;
            };
            match best.as_ref() {
                None => best = Some((snap, "cache")),
                Some((prev, _)) if snap.version > prev.version => best = Some((snap, "cache")),
                _ => {}
            }
        }
    }

    best
}

/// Stamp the per-instance `template/` snapshot dir based on `record.template_id`.
///
/// Lookup order:
///   1. Embedded bootstrap registry for offline first-run support.
///   2. Global cache `~/.renlijia/employee-templates-cache/{tid}/*.json`
///      populated by `employee_template_refresh`; newer cache versions win.
///
/// Returns the `TemplateRef` to store on the record, or `None` if neither
/// source has the template (e.g. custom hand-edited records, or a record
/// whose template was deleted from OPS and isn't cached locally).
///
/// All filesystem errors are swallowed and logged via `log::warn`; the
/// snapshot is additive metadata that PR5 will start *reading* from —
/// failing the whole hire/list flow over it would be too aggressive while
/// the feature is still landing.
fn stamp_snapshot_for_record(
    root: &std::path::Path,
    record: &EmployeeRecord,
) -> Option<crate::runtime::employee::template_store::TemplateRef> {
    use crate::runtime::employee::template_store as ts;
    let tid = record.template_id.as_deref()?;
    let cache_dir = crate::storage::AiJiaHome::from_home().employee_templates_cache_dir();
    let snap_and_source = latest_snapshot_for_template(tid, &cache_dir);

    let (snap, source) = snap_and_source?;
    let instance_dir = root.join(&record.id);
    match ts::ensure_instance_snapshot(&instance_dir, &snap, source) {
        Ok(r) => Some(r),
        Err(e) => {
            log::warn!(
                "[EmployeeStore] failed to stamp template snapshot for {}: {e}",
                record.id
            );
            None
        }
    }
}

/// `fs::remove_dir_all` with Windows-friendly retry. AV / Indexer / Explorer
/// can briefly hold handles to a directory we want to delete; a single retry
/// after a short backoff clears the vast majority of those cases. No-op delay
/// on Unix where the first call almost always succeeds.
fn remove_dir_all_retry(dir: &std::path::Path) -> Result<()> {
    let mut last_err = None;
    for attempt in 0..3 {
        match fs::remove_dir_all(dir) {
            Ok(()) => return Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(e) => {
                last_err = Some(e);
                if attempt < 2 {
                    std::thread::sleep(std::time::Duration::from_millis(150 * (attempt + 1)));
                }
            }
        }
    }
    Err(last_err.unwrap().into())
}

#[cfg(test)]
mod sync_hook_tests {
    use super::*;
    use crate::runtime::agent::employee_projection::EmployeeAgentSync;
    use std::sync::{Arc, Mutex};

    #[derive(Default)]
    struct CountingSync {
        active: Mutex<Vec<String>>,
        inactive: Mutex<Vec<String>>,
    }
    impl EmployeeAgentSync for CountingSync {
        fn on_active(&self, rec: &EmployeeRecord) {
            self.active.lock().unwrap().push(rec.id.clone());
        }
        fn on_inactive(&self, name: &str) {
            self.inactive.lock().unwrap().push(name.to_string());
        }
    }

    fn mk(req_name: &str) -> CreateEmployeeRequest {
        CreateEmployeeRequest {
            name: req_name.into(),
            role: "r".into(),
            description: "".into(),
            avatar: "".into(),
            template_id: None,
            tool_whitelist: None,
            cron: None,
            timezone: None,
            lifecycle: None,
            cron_enabled: None,
            resource_config: None,
            system_prompt_extra: None,
            default_skill_id: None,
            skill_ids: None,
        }
    }

    #[test]
    fn create_calls_on_active_when_active() {
        let tmp = tempfile::tempdir().unwrap();
        let store = EmployeeStore::new(tmp.path().to_path_buf());
        let sync = Arc::new(CountingSync::default());
        store.set_sync(sync.clone());
        let rec = store.create(mk("n1")).unwrap();
        let active = sync.active.lock().unwrap();
        assert_eq!(active.len(), 1);
        assert_eq!(active[0], rec.id);
    }

    #[test]
    fn legacy_paused_record_is_canonicalized_to_active_on_get() {
        // PR-6: a record on disk with lifecycle: "paused" must come back as
        // Active when read via `get` — the pause feature is retired.
        let tmp = tempfile::tempdir().unwrap();
        let store = EmployeeStore::new(tmp.path().to_path_buf());
        let rec = store.create(mk("legacy-paused")).unwrap();
        // Hand-mutate the on-disk record to simulate a pre-PR-6 paused state.
        let path = store.record_path(&rec.id);
        let raw = std::fs::read_to_string(&path).unwrap();
        let patched = raw.replace("\"lifecycle\": \"active\"", "\"lifecycle\": \"paused\"");
        std::fs::write(&path, patched).unwrap();

        let loaded = store.get(&rec.id).unwrap();
        assert_eq!(
            loaded.lifecycle,
            EmployeeLifecycle::Active,
            "legacy paused record must canonicalize to active"
        );
        // Subsequent read should see the file with Active baked in (persisted).
        let raw2 = std::fs::read_to_string(&path).unwrap();
        assert!(
            raw2.contains("\"lifecycle\": \"active\""),
            "canonical lifecycle must be persisted: {raw2}"
        );
    }

    #[test]
    fn update_to_paused_canonicalizes_to_active_and_fires_active_hook() {
        // PR-6: the `Paused` lifecycle was retired; canonical() collapses it
        // into Active. Submitting `Paused` via update must keep the employee
        // active (cron toggle is the real "pause" knob).
        let tmp = tempfile::tempdir().unwrap();
        let store = EmployeeStore::new(tmp.path().to_path_buf());
        let rec = store.create(mk("n3")).unwrap();
        let sync = Arc::new(CountingSync::default());
        store.set_sync(sync.clone());
        let updated = store
            .update(
                &rec.id,
                UpdateEmployeeRequest {
                    lifecycle: Some(EmployeeLifecycle::Paused),
                    ..Default::default()
                },
            )
            .unwrap();
        // Stored lifecycle stays as submitted (Paused) — but `update` calls
        // notify_active because the match arm uses `.canonical()`. The store
        // reader (`get` / `list_unlocked`) will collapse Paused → Active on
        // the next read.
        assert_eq!(updated.lifecycle, EmployeeLifecycle::Paused);
        let active = sync.active.lock().unwrap();
        assert_eq!(active[0], rec.id);
        let inactive = sync.inactive.lock().unwrap();
        assert!(
            inactive.is_empty(),
            "Paused must NOT trigger the inactive hook anymore"
        );
    }

    #[test]
    fn purge_calls_on_inactive() {
        let tmp = tempfile::tempdir().unwrap();
        let store = EmployeeStore::new(tmp.path().to_path_buf());
        let rec = store.create(mk("n2")).unwrap();
        // archive first (legitimate path)
        store
            .update(
                &rec.id,
                UpdateEmployeeRequest {
                    lifecycle: Some(EmployeeLifecycle::Archived),
                    ..Default::default()
                },
            )
            .unwrap();
        let sync = Arc::new(CountingSync::default());
        store.set_sync(sync.clone());
        store.purge(&rec.id).unwrap();
        let inactive = sync.inactive.lock().unwrap();
        assert!(inactive.iter().any(|n| n == &rec.id));
    }
}
