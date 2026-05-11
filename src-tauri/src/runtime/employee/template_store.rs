//! Digital-employee template registry and per-instance snapshots.
//!
//! Architectural intent (see `lotus/docs/superpowers/specs/2026-05-10-employee-templates-as-a-service.md`):
//!
//! - Templates are versioned, immutable JSON documents. The authoritative
//!   catalog lives on lotus ops-portal (table `employee_templates`, OSS path
//!   `ops/employee-templates/{template_id}/{version}.json`).
//! - The desktop client carries an embedded **bootstrap** copy of every
//!   template at version `1.0.0` so first-run / offline hire flows still
//!   work.
//! - Each employee instance freezes the exact template snapshot it was hired
//!   from into `<employees>/<id>/template/template.json` plus a sibling
//!   `manifest.json` that records `{template_id, version, sha256, source}`.
//!
//! This module owns:
//!
//! - `TemplateRef` — the small descriptor stored on `EmployeeRecord`.
//! - `TemplateSnapshot` — the on-disk JSON shape that mirrors the OPS table.
//! - `bootstrap_templates()` — embedded fallback registry (used until the
//!   network loader lands in PR4 follow-up).
//! - `ensure_instance_snapshot()` — idempotently writes `template/` for an
//!   instance directory.
//!
//! IMPORTANT: This module is conservative on purpose. It never deletes
//! existing fields off `EmployeeRecord`; the snapshot is *additional* state.
//! PR4 will switch dispatch / runner reads to the snapshot and then prune
//! the redundant fields.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// Pointer attached to `EmployeeRecord` identifying which template version
/// the instance was hired from. Kept tiny so embedding it in every record
/// is cheap. Older records without this field deserialize as `None` (see
/// `#[serde(default)]` on `EmployeeRecord.template_ref`).
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TemplateRef {
    pub template_id: String,
    pub version: String,
    /// SHA-256 of the canonical template JSON. Empty for bootstrap-derived
    /// snapshots (we don't bother hashing the embedded copy because there's
    /// nothing to verify against).
    #[serde(default)]
    pub sha256: String,
    /// Where this snapshot came from. Stable values:
    /// `"bootstrap"` (embedded), `"ops:<url>"` (downloaded from lotus).
    #[serde(default)]
    pub source: String,
}

/// On-disk shape of `<instance>/template/template.json`.
///
/// This is intentionally a near-mirror of the lotus ops `employee_templates`
/// row. Fields that the desktop side doesn't currently need are ignored at
/// deserialize time via `#[serde(default)]` so future schema additions are
/// non-breaking.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TemplateSnapshot {
    pub template_id: String,
    pub version: String,
    pub name: String,
    #[serde(default)]
    pub avatar: String,
    #[serde(default)]
    pub role: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub badge: String,
    #[serde(default)]
    pub system_prompt_extra: String,
    #[serde(default)]
    pub tool_whitelist: Vec<String>,
    #[serde(default)]
    pub cron: String,
    #[serde(default)]
    pub default_skill_id: String,
    #[serde(default)]
    pub requires_dingtalk: bool,
    /// Free-form attachment spec passed straight through to the frontend
    /// (`{accept, min, max}` or `null`). Stored as `serde_json::Value` so we
    /// don't couple to a specific shape — UI is the only consumer today.
    #[serde(default)]
    pub requires_attachment: serde_json::Value,
    /// JSON Schema describing the resource config form. May be empty for
    /// bootstrap entries; PR5 will fill these in.
    #[serde(default)]
    pub resource_config_schema: serde_json::Value,
    #[serde(default)]
    pub resource_config_ui: serde_json::Value,
}

impl TemplateSnapshot {
    /// True when the snapshot has none of the schema/UI guidance — used by
    /// the wizard to decide whether to fall back to the legacy hardcoded
    /// `ResourceConfigKind` form.
    pub fn has_schema(&self) -> bool {
        self.resource_config_schema.is_object()
            && !self
                .resource_config_schema
                .as_object()
                .map(|o| o.is_empty())
                .unwrap_or(true)
    }
}

/// Metadata sidecar at `<instance>/template/manifest.json`. Separate from
/// the snapshot itself so the snapshot stays byte-identical to the OPS-side
/// JSON we publish to OSS (which lets desktop verify by sha256).
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TemplateManifest {
    pub template_id: String,
    pub version: String,
    pub sha256: String,
    pub source: String,
    pub downloaded_at: chrono::DateTime<chrono::Utc>,
}

/// Bootstrap template JSON. Compiled into the binary so first-run + offline
/// hire works without the network. The text is a JSON array of
/// `TemplateSnapshot` objects.
const BOOTSTRAP_JSON: &str = include_str!("templates_bootstrap.json");

/// Memoized parse of `BOOTSTRAP_JSON`. Lazily computed once per process and
/// cloned to callers (the snapshot list is small — ~11 entries — so cloning
/// is cheaper than wrestling with lifetimes through the public API).
static BOOTSTRAP_CACHE: std::sync::OnceLock<Vec<TemplateSnapshot>> = std::sync::OnceLock::new();

/// Returns the embedded bootstrap templates. Parsing happens at most once
/// per process; subsequent calls clone from the cache.
pub fn bootstrap_templates() -> Result<Vec<TemplateSnapshot>> {
    if let Some(cached) = BOOTSTRAP_CACHE.get() {
        return Ok(cached.clone());
    }
    let parsed: Vec<TemplateSnapshot> = serde_json::from_str(BOOTSTRAP_JSON)
        .context("parsing bootstrap template JSON")?;
    let _ = BOOTSTRAP_CACHE.set(parsed.clone());
    Ok(parsed)
}

/// Look up a single bootstrap template by id. Returns `None` if the embedded
/// registry doesn't know this template_id (e.g. a custom org template that
/// only exists in lotus).
pub fn bootstrap_template(template_id: &str) -> Result<Option<TemplateSnapshot>> {
    Ok(bootstrap_templates()?
        .into_iter()
        .find(|t| t.template_id == template_id))
}

/// Compute SHA-256 of canonical (pretty-printed) snapshot JSON. Hash matches
/// what the OPS-side publish handler computes when uploading to OSS.
pub fn snapshot_sha256(snapshot: &TemplateSnapshot) -> String {
    // Match the OPS side: `json.MarshalIndent(tpl, "", "  ")`.
    // For desktop bootstrap usage the hash is informational only; PR4 will
    // verify against this when fetching from OSS.
    let pretty = serde_json::to_string_pretty(snapshot).unwrap_or_default();
    let mut h = Sha256::new();
    h.update(pretty.as_bytes());
    hex_lower(&h.finalize())
}

fn hex_lower(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        out.push(HEX[(b >> 4) as usize] as char);
        out.push(HEX[(b & 0x0f) as usize] as char);
    }
    out
}

/// Path helpers for an instance's `template/` subdir.
pub fn template_dir(instance_dir: &Path) -> PathBuf {
    instance_dir.join("template")
}
pub fn template_json_path(instance_dir: &Path) -> PathBuf {
    template_dir(instance_dir).join("template.json")
}
pub fn template_manifest_path(instance_dir: &Path) -> PathBuf {
    template_dir(instance_dir).join("manifest.json")
}

/// Idempotently write `<instance>/template/{template.json, manifest.json}`.
/// Safe to call on every load; only rewrites when content actually differs.
pub fn ensure_instance_snapshot(
    instance_dir: &Path,
    snapshot: &TemplateSnapshot,
    source: &str,
) -> Result<TemplateRef> {
    let dir = template_dir(instance_dir);
    fs::create_dir_all(&dir).with_context(|| format!("creating {}", dir.display()))?;

    let snapshot_path = template_json_path(instance_dir);
    let manifest_path = template_manifest_path(instance_dir);

    let snapshot_json = serde_json::to_string_pretty(snapshot)?;
    let sha = {
        let mut h = Sha256::new();
        h.update(snapshot_json.as_bytes());
        hex_lower(&h.finalize())
    };

    let need_write_snapshot = match fs::read_to_string(&snapshot_path) {
        Ok(existing) => existing != snapshot_json,
        Err(_) => true,
    };
    if need_write_snapshot {
        write_atomic(&snapshot_path, snapshot_json.as_bytes())?;
    }

    let manifest = TemplateManifest {
        template_id: snapshot.template_id.clone(),
        version: snapshot.version.clone(),
        sha256: sha.clone(),
        source: source.to_string(),
        downloaded_at: chrono::Utc::now(),
    };
    // Always rewrite manifest if any field differs — `downloaded_at` would
    // churn on every call otherwise, so compare excluding that field.
    let need_write_manifest = match fs::read_to_string(&manifest_path) {
        Ok(existing) => match serde_json::from_str::<TemplateManifest>(&existing) {
            Ok(prev) => prev.sha256 != sha || prev.source != source,
            Err(_) => true,
        },
        Err(_) => true,
    };
    if need_write_manifest {
        let manifest_json = serde_json::to_string_pretty(&manifest)?;
        write_atomic(&manifest_path, manifest_json.as_bytes())?;
    }

    Ok(TemplateRef {
        template_id: snapshot.template_id.clone(),
        version: snapshot.version.clone(),
        sha256: sha,
        source: source.to_string(),
    })
}

/// Read back the snapshot for an instance, if one exists.
pub fn read_instance_snapshot(instance_dir: &Path) -> Result<Option<TemplateSnapshot>> {
    let p = template_json_path(instance_dir);
    if !p.exists() {
        return Ok(None);
    }
    let content = fs::read_to_string(&p)
        .with_context(|| format!("reading {}", p.display()))?;
    let snap: TemplateSnapshot = serde_json::from_str(&content)
        .with_context(|| format!("parsing {}", p.display()))?;
    Ok(Some(snap))
}

// ─── Effective-value helpers ─────────────────────────────────────────────
//
// The architectural goal of the template-as-a-service refactor is that the
// snapshot at `<instance>/template/template.json` is authoritative; the
// legacy fields on `EmployeeRecord` (`tool_whitelist`, `system_prompt_extra`,
// `default_skill_id`, etc.) are only there as a transitional cache and are
// scheduled for deletion in PR6.
//
// These helpers read the snapshot when available and fall back to the
// record field otherwise. Runtime code (dispatch_prompt, chat, scheduler)
// goes through them — so once PR6 physically removes the record fields,
// the fallback branch goes away and nothing else needs to change.
//
// The instance_dir is `<employees_root>/<employee_id>/`. Callers typically
// have only the `EmployeeRecord` in hand; `effective_*_for` takes the
// employees root and derives the path. Errors reading the snapshot are
// swallowed and logged — we return the record-field fallback rather than
// failing a dispatch just because the snapshot file is missing or corrupt.

fn load_snapshot_silent(instance_dir: &Path) -> Option<TemplateSnapshot> {
    match read_instance_snapshot(instance_dir) {
        Ok(Some(s)) => Some(s),
        Ok(None) => None,
        Err(e) => {
            log::warn!(
                "[template_store] snapshot read failed at {}: {e}",
                instance_dir.display()
            );
            None
        }
    }
}

/// Returns the effective tool whitelist for an employee. Snapshot wins;
/// falls back to the record field when no snapshot is present (pre-PR3
/// hired employees that haven't been touched by `stamp_snapshot_for_record`
/// yet).
pub fn effective_tool_whitelist(
    employees_root: &Path,
    employee_id: &str,
    record_fallback: &[String],
) -> Vec<String> {
    if let Some(s) = load_snapshot_silent(&employees_root.join(employee_id)) {
        return s.tool_whitelist;
    }
    record_fallback.to_vec()
}

/// Returns the effective system-prompt-extra string for an employee.
/// Snapshot wins. Empty string in snapshot is treated as "no override"
/// and falls back to the record (which is `Option<String>`).
pub fn effective_system_prompt_extra(
    employees_root: &Path,
    employee_id: &str,
    record_fallback: Option<&str>,
) -> Option<String> {
    if let Some(s) = load_snapshot_silent(&employees_root.join(employee_id)) {
        if !s.system_prompt_extra.is_empty() {
            return Some(s.system_prompt_extra);
        }
    }
    record_fallback.map(|s| s.to_string())
}

/// Returns the effective default skill id for an employee. Snapshot wins;
/// empty string in snapshot → treated as `None` (no skill hint injected).
pub fn effective_default_skill_id(
    employees_root: &Path,
    employee_id: &str,
    record_fallback: Option<&str>,
) -> Option<String> {
    if let Some(s) = load_snapshot_silent(&employees_root.join(employee_id)) {
        if s.default_skill_id.is_empty() {
            return None;
        }
        return Some(s.default_skill_id);
    }
    record_fallback.map(|s| s.to_string())
}

// ─── atomic write helper (small local copy; the existing one in storage::
// is private to that module). ─────────────────────────────────────────────

fn write_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    let tmp = path.with_extension(format!(
        "{}.tmp",
        path.extension()
            .and_then(|e| e.to_str())
            .unwrap_or("tmp")
    ));
    fs::write(&tmp, bytes).with_context(|| format!("writing {}", tmp.display()))?;
    fs::rename(&tmp, path).with_context(|| format!("renaming {}", tmp.display()))?;
    Ok(())
}

// ─── HTTP loader ─────────────────────────────────────────────────────────
//
// Fetch published template versions from lotus ops-portal's public catalog
// and cache them on disk. Layout at `~/.renlijia/employee-templates-cache/`:
//
//   {template_id}/{version}.json       — the canonical snapshot JSON
//
// The cache is content-addressed (the sha256 from the OPS manifest must
// match) and immutable per `(template_id, version)`. Callers always go
// through `fetch_or_cache()`; hitting the cache is free, missing entries
// trigger one HTTP GET. The cache is shared across users on the same
// machine — templates are not user data.

/// Default lotus ops-portal base URL. Can be overridden via the
/// `LOTUS_OPS_BASE_URL` env var (useful for local dev pointing at
/// `http://localhost:8082`).
const DEFAULT_OPS_BASE_URL: &str = "https://ai-ops.renlijia.com";

fn ops_base_url() -> String {
    std::env::var("LOTUS_OPS_BASE_URL")
        .unwrap_or_else(|_| DEFAULT_OPS_BASE_URL.to_string())
        .trim_end_matches('/')
        .to_string()
}

/// Shape of `GET /api/public/employee-templates/:tid/manifest`. Only the
/// fields we actually use are declared — extra fields are ignored by serde.
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct RemoteManifest {
    pub template_id: String,
    pub latest_version: String,
    pub package_url: String,
    pub package_sha256: String,
    #[serde(default)]
    pub package_size: i64,
}

/// Unified OPS API response envelope: `{ code: 0, message: "ok", data: ... }`.
#[derive(Deserialize)]
struct OpsEnvelope<T> {
    #[serde(default)]
    code: i32,
    #[serde(default)]
    message: String,
    data: Option<T>,
}

fn cache_path_for(cache_dir: &Path, template_id: &str, version: &str) -> PathBuf {
    cache_dir.join(template_id).join(format!("{version}.json"))
}

/// Read a cached template snapshot if present + valid. Any parse/IO error
/// is swallowed (returns `None`) so a corrupted cache file is just
/// re-fetched, not a hard failure.
pub fn read_cache(
    cache_dir: &Path,
    template_id: &str,
    version: &str,
) -> Option<TemplateSnapshot> {
    let p = cache_path_for(cache_dir, template_id, version);
    let content = fs::read_to_string(&p).ok()?;
    serde_json::from_str(&content).ok()
}

/// Persist `snapshot` to the cache directory. Atomic (tmp + rename).
pub fn write_cache(
    cache_dir: &Path,
    snapshot: &TemplateSnapshot,
) -> Result<PathBuf> {
    let dir = cache_dir.join(&snapshot.template_id);
    fs::create_dir_all(&dir).with_context(|| format!("creating {}", dir.display()))?;
    let p = dir.join(format!("{}.json", snapshot.version));
    let json = serde_json::to_string_pretty(snapshot)?;
    write_atomic(&p, json.as_bytes())?;
    Ok(p)
}

/// Fetch the manifest for a template from lotus ops-portal.
///
/// `GET {base}/api/public/employee-templates/{template_id}/manifest`
pub async fn fetch_manifest(
    client: &reqwest::Client,
    template_id: &str,
) -> Result<RemoteManifest> {
    let url = format!(
        "{}/api/public/employee-templates/{}/manifest",
        ops_base_url(),
        template_id
    );
    let resp = client
        .get(&url)
        .timeout(std::time::Duration::from_secs(10))
        .send()
        .await
        .with_context(|| format!("GET {url}"))?;
    if !resp.status().is_success() {
        anyhow::bail!("manifest HTTP {} from {url}", resp.status());
    }
    let env: OpsEnvelope<RemoteManifest> = resp
        .json()
        .await
        .with_context(|| format!("decoding manifest envelope for {template_id}"))?;
    if env.code != 0 {
        anyhow::bail!(
            "ops returned code={} message={} for {template_id}",
            env.code,
            env.message
        );
    }
    env.data
        .ok_or_else(|| anyhow::anyhow!("empty data in manifest envelope for {template_id}"))
}

/// Fetch the full published catalog `GET {base}/api/public/employee-templates`.
/// Returns the latest published version per `template_id`, `tenant_scope=global`.
pub async fn fetch_catalog(client: &reqwest::Client) -> Result<Vec<serde_json::Value>> {
    let url = format!("{}/api/public/employee-templates", ops_base_url());
    let resp = client
        .get(&url)
        .timeout(std::time::Duration::from_secs(10))
        .send()
        .await
        .with_context(|| format!("GET {url}"))?;
    if !resp.status().is_success() {
        anyhow::bail!("catalog HTTP {} from {url}", resp.status());
    }
    let env: OpsEnvelope<Vec<serde_json::Value>> = resp
        .json()
        .await
        .context("decoding catalog envelope")?;
    if env.code != 0 {
        anyhow::bail!("ops returned code={} message={}", env.code, env.message);
    }
    Ok(env.data.unwrap_or_default())
}

/// Download the snapshot JSON at `package_url` and verify its sha256 matches
/// the expected value. The URL comes from the manifest; the OPS publish
/// handler uploaded exactly this bytes to OSS.
pub async fn download_snapshot(
    client: &reqwest::Client,
    package_url: &str,
    expected_sha256: &str,
) -> Result<TemplateSnapshot> {
    let resp = client
        .get(package_url)
        .timeout(std::time::Duration::from_secs(20))
        .send()
        .await
        .with_context(|| format!("GET {package_url}"))?;
    if !resp.status().is_success() {
        anyhow::bail!("package HTTP {} from {package_url}", resp.status());
    }
    let bytes = resp
        .bytes()
        .await
        .with_context(|| format!("reading body from {package_url}"))?;

    if !expected_sha256.is_empty() {
        let mut h = Sha256::new();
        h.update(&bytes);
        let got = hex_lower(&h.finalize());
        if !expected_sha256.eq_ignore_ascii_case(&got) {
            anyhow::bail!(
                "sha256 mismatch for {package_url}: expected {expected_sha256}, got {got}"
            );
        }
    }

    let snap: TemplateSnapshot = serde_json::from_slice(&bytes)
        .with_context(|| format!("parsing snapshot from {package_url}"))?;
    Ok(snap)
}

/// Ensure a specific `(template_id, version)` is present in the cache. If
/// missing, fetch the manifest, verify `manifest.latest_version == version`
/// (callers that want a specific older version should use lower-level
/// helpers), download + verify sha256, write to cache.
///
/// Returns the snapshot ready to stamp into an employee instance dir.
pub async fn ensure_cached(
    cache_dir: &Path,
    client: &reqwest::Client,
    template_id: &str,
    version: &str,
) -> Result<TemplateSnapshot> {
    if let Some(s) = read_cache(cache_dir, template_id, version) {
        return Ok(s);
    }
    let manifest = fetch_manifest(client, template_id).await?;
    if manifest.latest_version != version {
        anyhow::bail!(
            "cache miss for {template_id}@{version}, but OPS latest is {}; \
             fetching historical versions is not supported yet",
            manifest.latest_version
        );
    }
    let snap =
        download_snapshot(client, &manifest.package_url, &manifest.package_sha256).await?;
    write_cache(cache_dir, &snap)?;
    Ok(snap)
}

/// Merge the bootstrap list with any cached (downloaded) versions. When
/// both sources have a `template_id`, the cached one wins iff its version
/// string sorts higher. This is the catalog the new-hire wizard should see.
pub fn merge_catalog(
    bootstrap: Vec<TemplateSnapshot>,
    cache_dir: &Path,
) -> Vec<TemplateSnapshot> {
    let mut by_id: std::collections::BTreeMap<String, TemplateSnapshot> = bootstrap
        .into_iter()
        .map(|t| (t.template_id.clone(), t))
        .collect();

    // Walk the cache dir: each subdirectory is a template_id, each `.json`
    // file inside it is a version. Pick the highest-sorting version per id.
    if let Ok(rd) = fs::read_dir(cache_dir) {
        for entry in rd.flatten() {
            let sub = entry.path();
            if !sub.is_dir() {
                continue;
            }
            let mut best: Option<TemplateSnapshot> = None;
            if let Ok(versions) = fs::read_dir(&sub) {
                for v in versions.flatten() {
                    let p = v.path();
                    if p.extension().and_then(|e| e.to_str()) != Some("json") {
                        continue;
                    }
                    if let Ok(content) = fs::read_to_string(&p) {
                        if let Ok(s) = serde_json::from_str::<TemplateSnapshot>(&content) {
                            match best.as_ref() {
                                None => best = Some(s),
                                Some(prev) if s.version > prev.version => best = Some(s),
                                _ => {}
                            }
                        }
                    }
                }
            }
            if let Some(cached) = best {
                match by_id.get(&cached.template_id) {
                    Some(b) if cached.version > b.version => {
                        by_id.insert(cached.template_id.clone(), cached);
                    }
                    None => {
                        by_id.insert(cached.template_id.clone(), cached);
                    }
                    _ => {}
                }
            }
        }
    }

    by_id.into_values().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bootstrap_templates_parse() {
        let list = bootstrap_templates().expect("bootstrap JSON should parse");
        assert!(
            !list.is_empty(),
            "bootstrap registry must contain at least one template"
        );
        // Sanity: every id starts with a known namespace prefix.
        for t in &list {
            assert!(
                t.template_id.starts_with("builtin:")
                    || t.template_id.starts_with("org:")
                    || t.template_id.starts_with("private:"),
                "bad namespace: {}",
                t.template_id
            );
        }
    }

    #[test]
    fn bootstrap_lookup_known_id() {
        let t = bootstrap_template("builtin:xiaoyuan")
            .expect("call ok")
            .expect("xiaoyuan should be in bootstrap");
        assert_eq!(t.name, "小研");
    }

    #[test]
    fn ensure_snapshot_writes_then_idempotent() {
        let dir = tempfile::tempdir().unwrap();
        let inst = dir.path().to_path_buf();
        let snap = TemplateSnapshot {
            template_id: "builtin:test".into(),
            version: "1.0.0".into(),
            name: "Test".into(),
            avatar: "🧪".into(),
            role: "tester".into(),
            description: "".into(),
            badge: "".into(),
            system_prompt_extra: "".into(),
            tool_whitelist: vec!["Read".into()],
            cron: "".into(),
            default_skill_id: "".into(),
            requires_dingtalk: false,
            requires_attachment: serde_json::Value::Null,
            resource_config_schema: serde_json::json!({}),
            resource_config_ui: serde_json::json!({}),
        };

        let r1 = ensure_instance_snapshot(&inst, &snap, "bootstrap").unwrap();
        assert_eq!(r1.template_id, "builtin:test");
        assert!(template_json_path(&inst).exists());
        assert!(template_manifest_path(&inst).exists());

        // Second call with same content: should not error, hash stable.
        let r2 = ensure_instance_snapshot(&inst, &snap, "bootstrap").unwrap();
        assert_eq!(r1.sha256, r2.sha256);
    }

    #[test]
    fn read_back_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let inst = dir.path().to_path_buf();
        let snap = TemplateSnapshot {
            template_id: "builtin:rt".into(),
            version: "1.0.0".into(),
            name: "RT".into(),
            avatar: "".into(),
            role: "".into(),
            description: "".into(),
            badge: "".into(),
            system_prompt_extra: "".into(),
            tool_whitelist: vec![],
            cron: "".into(),
            default_skill_id: "".into(),
            requires_dingtalk: false,
            requires_attachment: serde_json::Value::Null,
            resource_config_schema: serde_json::Value::Null,
            resource_config_ui: serde_json::Value::Null,
        };
        ensure_instance_snapshot(&inst, &snap, "bootstrap").unwrap();
        let read = read_instance_snapshot(&inst).unwrap().unwrap();
        assert_eq!(read.template_id, "builtin:rt");
    }

    fn make_snap(tid: &str, version: &str) -> TemplateSnapshot {
        TemplateSnapshot {
            template_id: tid.into(),
            version: version.into(),
            name: "X".into(),
            avatar: "".into(),
            role: "".into(),
            description: "".into(),
            badge: "".into(),
            system_prompt_extra: "".into(),
            tool_whitelist: vec![],
            cron: "".into(),
            default_skill_id: "".into(),
            requires_dingtalk: false,
            requires_attachment: serde_json::Value::Null,
            resource_config_schema: serde_json::Value::Null,
            resource_config_ui: serde_json::Value::Null,
        }
    }

    #[test]
    fn cache_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let cache = dir.path();
        let snap = make_snap("builtin:cache-rt", "1.2.3");

        assert!(read_cache(cache, "builtin:cache-rt", "1.2.3").is_none());
        write_cache(cache, &snap).unwrap();
        let back = read_cache(cache, "builtin:cache-rt", "1.2.3").unwrap();
        assert_eq!(back.template_id, "builtin:cache-rt");
        assert_eq!(back.version, "1.2.3");
    }

    #[test]
    fn merge_catalog_cache_wins_when_newer() {
        let dir = tempfile::tempdir().unwrap();
        let cache = dir.path();

        // Bootstrap has tid at v1.0.0
        let boot = vec![make_snap("builtin:x", "1.0.0")];
        // Cache has same tid at v1.1.0 — should win
        write_cache(cache, &make_snap("builtin:x", "1.1.0")).unwrap();
        // Plus a brand-new template only in cache
        write_cache(cache, &make_snap("org:custom", "0.1.0")).unwrap();

        let merged = merge_catalog(boot, cache);
        let x = merged.iter().find(|t| t.template_id == "builtin:x").unwrap();
        assert_eq!(x.version, "1.1.0", "cache should override bootstrap");
        let custom = merged
            .iter()
            .find(|t| t.template_id == "org:custom")
            .unwrap();
        assert_eq!(custom.version, "0.1.0");
    }

    #[test]
    fn merge_catalog_bootstrap_wins_when_newer() {
        let dir = tempfile::tempdir().unwrap();
        let cache = dir.path();
        // Bootstrap newer than cache → bootstrap wins.
        let boot = vec![make_snap("builtin:y", "2.0.0")];
        write_cache(cache, &make_snap("builtin:y", "1.0.0")).unwrap();
        let merged = merge_catalog(boot, cache);
        let y = merged.iter().find(|t| t.template_id == "builtin:y").unwrap();
        assert_eq!(y.version, "2.0.0");
    }

    #[test]
    fn merge_catalog_handles_missing_cache_dir() {
        // Non-existent cache dir should still return bootstrap.
        let merged = merge_catalog(
            vec![make_snap("builtin:z", "1.0.0")],
            std::path::Path::new("/nonexistent/path/that/does/not/exist"),
        );
        assert_eq!(merged.len(), 1);
        assert_eq!(merged[0].template_id, "builtin:z");
    }

    #[test]
    fn hex_lower_matches_known_vectors() {
        assert_eq!(hex_lower(b""), "");
        assert_eq!(hex_lower(&[0xde, 0xad, 0xbe, 0xef]), "deadbeef");
        assert_eq!(hex_lower(&[0x00, 0x0f, 0xff]), "000fff");
    }

    #[test]
    fn effective_helpers_prefer_snapshot_over_record_fallback() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let id = "emp-snapshot-wins";
        let inst = root.join(id);

        // Stamp a snapshot with values distinct from the record fallback.
        let snap = TemplateSnapshot {
            template_id: "builtin:x".into(),
            version: "1.0.0".into(),
            name: "X".into(),
            avatar: "".into(),
            role: "".into(),
            description: "".into(),
            badge: "".into(),
            system_prompt_extra: "from-snapshot".into(),
            tool_whitelist: vec!["Snap1".into(), "Snap2".into()],
            cron: "".into(),
            default_skill_id: "snap-skill".into(),
            requires_dingtalk: false,
            requires_attachment: serde_json::Value::Null,
            resource_config_schema: serde_json::Value::Null,
            resource_config_ui: serde_json::Value::Null,
        };
        ensure_instance_snapshot(&inst, &snap, "bootstrap").unwrap();

        let record_fallback_tools = vec!["Record1".into()];
        let tools = effective_tool_whitelist(root, id, &record_fallback_tools);
        assert_eq!(tools, vec!["Snap1".to_string(), "Snap2".to_string()]);

        let extra = effective_system_prompt_extra(root, id, Some("from-record"));
        assert_eq!(extra.as_deref(), Some("from-snapshot"));

        let skill = effective_default_skill_id(root, id, Some("record-skill"));
        assert_eq!(skill.as_deref(), Some("snap-skill"));
    }

    #[test]
    fn effective_helpers_fall_back_when_no_snapshot() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let id = "emp-no-snapshot";
        // No stamping — the instance dir doesn't even exist.

        let record_tools = vec!["Record1".into(), "Record2".into()];
        let tools = effective_tool_whitelist(root, id, &record_tools);
        assert_eq!(tools, record_tools);

        let extra = effective_system_prompt_extra(root, id, Some("from-record"));
        assert_eq!(extra.as_deref(), Some("from-record"));

        let skill = effective_default_skill_id(root, id, Some("record-skill"));
        assert_eq!(skill.as_deref(), Some("record-skill"));
    }

    #[test]
    fn effective_default_skill_id_empty_snapshot_treated_as_none() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let id = "emp-empty-skill";
        let inst = root.join(id);
        let mut snap = make_snap("builtin:test", "1.0.0");
        snap.default_skill_id = "".into();
        ensure_instance_snapshot(&inst, &snap, "bootstrap").unwrap();

        // Empty string in snapshot means "no skill hint" — not "use record".
        // This matches the dispatch_prompt behavior that treats empty string
        // as no-skill.
        let skill = effective_default_skill_id(root, id, Some("record-skill"));
        assert_eq!(skill, None);
    }
}
