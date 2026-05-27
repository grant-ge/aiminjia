//! Integration tests for the digital-employee template lifecycle:
//!
//!   1. **First-load**: hire a new employee referencing a builtin template
//!      → backend stamps `template/template.json` from the embedded
//!      bootstrap registry, populates `template_ref`. No network needed.
//!
//!   2. **Legacy migration**: plant a pre-PR2 `employee.json` (no
//!      `templateRef`, no `template/` subdir) → `EmployeeStore::list/get`
//!      auto-stamps the snapshot and back-fills `template_ref`.
//!
//!   3. **Upgrade**: write an existing-instance snapshot at v1.0, then
//!      drop a v2.0 snapshot of the same `template_id` into the global
//!      cache → `merge_catalog(bootstrap, cache)` returns the cached
//!      v2.0 (newer wins).
//!
//! These tests cover the snapshot-on-disk / merge-priority logic that
//! desktop relies on. They do NOT exercise the HTTP loader because that
//! path requires a live `ai-ops.renlijia.com` and the tests must remain
//! hermetic. The HTTP path is covered manually by hitting the
//! production endpoint after deploy (see PR3 commit message).

use std::fs;
use std::path::Path;

use serde_json::json;

use app_lib::runtime::employee::store::{CreateEmployeeRequest, EmployeeStore};
use app_lib::runtime::employee::template_store::{
    bootstrap_template, ensure_instance_snapshot, merge_catalog, read_instance_snapshot,
    write_cache, TemplateSnapshot,
};

/// Helper: produce a stand-in snapshot for tests not relying on bootstrap.
fn synthetic_snap(template_id: &str, version: &str, name: &str) -> TemplateSnapshot {
    TemplateSnapshot {
        template_id: template_id.into(),
        version: version.into(),
        name: name.into(),
        avatar: "🧪".into(),
        role: "test".into(),
        description: "".into(),
        badge: "".into(),
        system_prompt_extra: "".into(),
        tool_whitelist: vec![],
        cron: "".into(),
        default_skill_id: "".into(),
        skill_ids: vec![],
        requires_dingtalk: false,
        requires_attachment: serde_json::Value::Null,
        resource_config_schema: serde_json::Value::Null,
        resource_config_ui: serde_json::Value::Null,
        extra: std::collections::BTreeMap::new(),
    }
}

// ─── Scenario 1: first-load hire stamps snapshot from bootstrap ──────────

#[test]
fn first_load_hire_stamps_bootstrap_snapshot() {
    let tmp = tempfile::tempdir().unwrap();
    let employees_dir = tmp.path().to_path_buf();
    fs::create_dir_all(&employees_dir).unwrap();

    let store = EmployeeStore::new(employees_dir.clone());
    let rec = store
        .create(CreateEmployeeRequest {
            template_id: Some("builtin:xiaoyuan".into()),
            avatar: "🔍".into(),
            name: "小研".into(),
            role: "调研员".into(),
            description: "".into(),
            tool_whitelist: Some(vec![]),
            cron: None,
            timezone: None,
            lifecycle: None,
            cron_enabled: None,
            system_prompt_extra: None,
            default_skill_id: None,
            skill_ids: None,
            resource_config: Some(json!({})),
        })
        .expect("create succeeds");

    // 1. template_ref populated on the returned record.
    let tref = rec.template_ref.as_ref().expect("template_ref stamped");
    assert_eq!(tref.template_id, "builtin:xiaoyuan");
    assert_eq!(tref.version, "1.0");
    assert_eq!(tref.source, "bootstrap");
    assert!(!tref.sha256.is_empty(), "sha256 computed");

    // 2. Snapshot file written to disk.
    let inst = employees_dir.join(&rec.id);
    let snap = read_instance_snapshot(&inst)
        .expect("read")
        .expect("snapshot exists");
    assert_eq!(snap.template_id, "builtin:xiaoyuan");
    assert_eq!(snap.version, "1.0");
    // System prompt comes from bootstrap, not the empty record field.
    assert!(
        snap.system_prompt_extra.contains("调研员") || snap.system_prompt_extra.contains("竞品")
    );

    // 3. Manifest file also written.
    assert!(inst.join("template/manifest.json").exists());
}

#[test]
fn first_load_hire_with_unknown_template_id_skips_stamping() {
    let tmp = tempfile::tempdir().unwrap();
    let employees_dir = tmp.path().to_path_buf();

    let store = EmployeeStore::new(employees_dir.clone());
    let rec = store
        .create(CreateEmployeeRequest {
            template_id: Some("org:custom-not-in-bootstrap".into()),
            avatar: "🤖".into(),
            name: "Custom".into(),
            role: "x".into(),
            description: "".into(),
            tool_whitelist: None,
            cron: None,
            timezone: None,
            lifecycle: None,
            cron_enabled: None,
            system_prompt_extra: None,
            default_skill_id: None,
            skill_ids: None,
            resource_config: None,
        })
        .unwrap();

    // Unknown template_id (not in bootstrap) → no stamping, no template/ dir.
    assert!(rec.template_ref.is_none(), "no ref for unknown id");
    assert!(!employees_dir.join(&rec.id).join("template").exists());
}

// ─── Scenario 2: legacy migration ────────────────────────────────────────

#[test]
fn legacy_record_without_template_ref_is_backfilled_on_get() {
    let tmp = tempfile::tempdir().unwrap();
    let employees_dir = tmp.path().to_path_buf();

    // Plant a pre-PR2 employee.json by hand: no `templateRef` field, no
    // `template/` subdir. Mirrors what's on disk for users hired
    // before PR2 (commit f8a43b1, 2026-05-10) shipped.
    let id = "emp-legacy-fixture";
    let inst_dir = employees_dir.join(id);
    fs::create_dir_all(&inst_dir).unwrap();
    let legacy_json = json!({
        "id": id,
        "name": "小研 (legacy)",
        "role": "调研员",
        "description": "old hire",
        "avatar": "🔍",
        "templateId": "builtin:xiaoyuan",
        "toolWhitelist": ["WebSearch"],
        "cron": null,
        "timezone": "Asia/Shanghai",
        "cronEnabled": true,
        "resourceConfig": {},
        "systemPromptExtra": "old prompt",
        "defaultSkillId": null,
        "createdAt": "2026-05-01T00:00:00Z",
        "updatedAt": "2026-05-01T00:00:00Z",
        "lastRunAt": null,
        "nextRunAt": null
        // NB: no "templateRef" — that's the migration trigger
    });
    fs::write(
        inst_dir.join("employee.json"),
        serde_json::to_string_pretty(&legacy_json).unwrap(),
    )
    .unwrap();
    assert!(!inst_dir.join("template").exists(), "no snapshot before");

    // First read → migration runs.
    let store = EmployeeStore::new(employees_dir.clone());
    let rec = store.get(id).expect("legacy record loads");

    let tref = rec
        .template_ref
        .as_ref()
        .expect("legacy record back-filled");
    assert_eq!(tref.template_id, "builtin:xiaoyuan");
    assert_eq!(tref.version, "1.0");
    assert!(
        inst_dir.join("template/template.json").exists(),
        "snapshot file written"
    );

    // Idempotency: second read sees a record with `templateRef` already set
    // and doesn't churn (no exception, sha256 same).
    let rec2 = store.get(id).expect("idempotent");
    assert_eq!(
        rec.template_ref.as_ref().unwrap().sha256,
        rec2.template_ref.as_ref().unwrap().sha256,
    );

    // employee.json on disk now has `templateRef` persisted.
    let raw = fs::read_to_string(inst_dir.join("employee.json")).unwrap();
    assert!(
        raw.contains("\"templateRef\""),
        "templateRef persisted: {raw}"
    );
}

#[test]
fn legacy_record_list_also_triggers_migration() {
    // Same fixture but verify list() walks all records and migrates.
    let tmp = tempfile::tempdir().unwrap();
    let employees_dir = tmp.path().to_path_buf();
    for (id, tid) in [
        ("emp-legacy-a", "builtin:xiaoyuan"),
        ("emp-legacy-b", "builtin:xiaofa"),
    ] {
        let dir = employees_dir.join(id);
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join("employee.json"),
            serde_json::to_string_pretty(&json!({
                "id": id,
                "name": "n",
                "role": "r",
                "description": "",
                "avatar": "",
                "templateId": tid,
                "toolWhitelist": [],
                "cron": null,
                "timezone": "Asia/Shanghai",
                "cronEnabled": true,
                "resourceConfig": {},
                "systemPromptExtra": null,
                "defaultSkillId": null,
                "createdAt": "2026-05-01T00:00:00Z",
                "updatedAt": "2026-05-01T00:00:00Z",
                "lastRunAt": null,
                "nextRunAt": null,
            }))
            .unwrap(),
        )
        .unwrap();
    }

    let store = EmployeeStore::new(employees_dir.clone());
    let list = store.list().expect("list ok");
    assert_eq!(list.len(), 2);
    for r in &list {
        let tref = r
            .template_ref
            .as_ref()
            .unwrap_or_else(|| panic!("record {} not back-filled", r.id));
        assert_eq!(tref.version, "1.0");
        assert!(employees_dir
            .join(&r.id)
            .join("template/template.json")
            .exists());
    }
}

// ─── Scenario 3: upgrade via cache ───────────────────────────────────────

#[test]
fn merge_catalog_picks_cache_over_bootstrap_when_newer() {
    let tmp = tempfile::tempdir().unwrap();
    let cache_dir = tmp.path();

    // 1. Bootstrap has builtin:xiaoyuan@1.0
    let boot_xiaoyuan = bootstrap_template("builtin:xiaoyuan")
        .expect("ok")
        .expect("present");
    assert_eq!(boot_xiaoyuan.version, "1.0");

    // 2. Drop a v2.0 snapshot for the same template into the cache
    //    (simulates `employee_template_refresh` having downloaded a newer
    //    publish from lotus).
    let upgraded = synthetic_snap("builtin:xiaoyuan", "2.0", "小研 v2");
    write_cache(cache_dir, &upgraded).unwrap();

    // 3. merge_catalog: cache wins.
    let merged = merge_catalog(vec![boot_xiaoyuan], cache_dir);
    let xiaoyuan = merged
        .iter()
        .find(|t| t.template_id == "builtin:xiaoyuan")
        .expect("present");
    assert_eq!(xiaoyuan.version, "2.0");
    assert_eq!(xiaoyuan.name, "小研 v2");
}

#[test]
fn merge_catalog_keeps_bootstrap_when_cache_is_older_or_missing() {
    let tmp = tempfile::tempdir().unwrap();
    let cache_dir = tmp.path();

    let boot = bootstrap_template("builtin:xiaoyuan").unwrap().unwrap();
    // Cache has older v0.9 — bootstrap wins.
    let older = synthetic_snap("builtin:xiaoyuan", "0.9", "stale");
    write_cache(cache_dir, &older).unwrap();

    let merged = merge_catalog(vec![boot.clone()], cache_dir);
    let xiaoyuan = merged
        .iter()
        .find(|t| t.template_id == "builtin:xiaoyuan")
        .expect("present");
    assert_eq!(xiaoyuan.version, "1.0", "bootstrap wins over older cache");
}

#[test]
fn merge_catalog_includes_cache_only_org_templates() {
    let tmp = tempfile::tempdir().unwrap();
    let cache_dir = tmp.path();

    // Cache has a custom org template that bootstrap doesn't know about.
    let custom = synthetic_snap("org:acme-recruiter", "1.0", "Acme Recruiter");
    write_cache(cache_dir, &custom).unwrap();

    let merged = merge_catalog(
        vec![bootstrap_template("builtin:xiaoyuan").unwrap().unwrap()],
        cache_dir,
    );
    let acme = merged
        .iter()
        .find(|t| t.template_id == "org:acme-recruiter")
        .expect("custom template visible from cache");
    assert_eq!(acme.version, "1.0");
    assert_eq!(acme.name, "Acme Recruiter");

    // Bootstrap entries still present.
    assert!(merged.iter().any(|t| t.template_id == "builtin:xiaoyuan"));
}

#[test]
fn upgrade_path_writes_new_snapshot_to_existing_instance() {
    // End-to-end-ish: create an instance at v1.0, then re-stamp with v2.0.
    // Verifies `ensure_instance_snapshot` overwrites cleanly.
    let tmp = tempfile::tempdir().unwrap();
    let inst = tmp.path().to_path_buf();

    let v1 = synthetic_snap("builtin:xiaoyuan", "1.0", "old");
    let r1 = ensure_instance_snapshot(&inst, &v1, "bootstrap").unwrap();
    assert_eq!(r1.version, "1.0");

    let v2 = synthetic_snap("builtin:xiaoyuan", "2.0", "new");
    let r2 = ensure_instance_snapshot(&inst, &v2, "ops:test").unwrap();
    assert_eq!(r2.version, "2.0");
    assert_ne!(r1.sha256, r2.sha256);
    assert_eq!(r2.source, "ops:test");

    // Read back: only the new snapshot remains at the canonical path.
    let read = read_instance_snapshot(&inst).unwrap().unwrap();
    assert_eq!(read.version, "2.0");
    assert_eq!(read.name, "new");

    // Manifest reflects the new sha + source.
    let manifest_path = inst.join("template/manifest.json");
    let m: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&manifest_path).unwrap()).unwrap();
    assert_eq!(m["version"], "2.0");
    assert_eq!(m["source"], "ops:test");
    assert_eq!(m["sha256"].as_str().unwrap().len(), 64);
}

// ─── Helper for legacy fixture validation ────────────────────────────────

#[allow(dead_code)]
fn assert_no_template_dir(p: &Path) {
    assert!(
        !p.join("template").exists(),
        "should not have template/ yet"
    );
}

// ─── Live HTTP smoke (off by default) ────────────────────────────────────
//
// `cargo test -- --ignored` runs these. They hit the production
// ai-ops.renlijia.com endpoints and verify the full chain:
//   manifest → signed OSS URL → download → camelCase serde decode
//
// Marked #[ignore] because the regular `cargo test` must stay hermetic
// (no network) — the production endpoint is fine but flaky CI / offline
// development would otherwise see spurious failures.

#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn live_fetch_manifest_and_download_decodes_camelcase() {
    let client = reqwest::Client::new();
    let manifest =
        app_lib::runtime::employee::template_store::fetch_manifest(&client, "builtin:xiaoyuan")
            .await
            .expect("manifest fetch");
    assert_eq!(manifest.template_id, "builtin:xiaoyuan");
    assert_eq!(manifest.latest_version, "1.0");
    assert!(
        manifest.package_url.starts_with("https://"),
        "expected https public URL, got {}",
        manifest.package_url
    );
    assert!(!manifest.package_sha256.is_empty(), "sha256 present");

    // Download + sha256-verify the snapshot.
    let snap = app_lib::runtime::employee::template_store::download_snapshot(
        &client,
        &manifest.package_url,
        &manifest.package_sha256,
    )
    .await
    .expect("download + sha verify");

    // The smoking-gun assertion: if the OSS-side JSON were still
    // snake_case (publish bug), all these fields would be defaulted.
    assert_eq!(snap.template_id, "builtin:xiaoyuan");
    assert_eq!(snap.version, "1.0");
    assert!(!snap.name.is_empty(), "name decoded");
    assert!(!snap.role.is_empty(), "role decoded");
    assert!(!snap.tool_whitelist.is_empty(), "toolWhitelist decoded");
}

#[tokio::test(flavor = "multi_thread")]
#[ignore]
async fn live_full_refresh_caches_all_published_templates() {
    let tmp = tempfile::tempdir().unwrap();
    let cache_dir = tmp.path();
    let client = reqwest::Client::new();

    let catalog = app_lib::runtime::employee::template_store::fetch_catalog(&client)
        .await
        .expect("catalog");
    assert!(!catalog.is_empty(), "production has at least one template");

    // Just sanity-check one — full refresh per-template would be slow.
    let first = &catalog[0];
    let tid = first.get("template_id").and_then(|v| v.as_str()).unwrap();
    let ver = first.get("version").and_then(|v| v.as_str()).unwrap();
    app_lib::runtime::employee::template_store::ensure_cached(cache_dir, &client, tid, ver)
        .await
        .expect("ensure_cached");

    // File now on disk in the cache.
    assert!(cache_dir.join(tid).join(format!("{ver}.json")).exists());
}
