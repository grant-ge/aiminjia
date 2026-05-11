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
}
