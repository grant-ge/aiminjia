use std::fs;
use std::io::Write;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use app_lib::plugin::skill::global_sync::{
    configured_global_skills_manifest_url, extract_global_skills_zip,
    install_prepared_global_skills, should_skip_manifest, spawn_global_skill_sync,
    write_global_skills_state, GlobalSkillSyncConfig, GlobalSkillsArtifact, GlobalSkillsManifest,
    GlobalSkillsState, DEFAULT_GLOBAL_SKILLS_MANIFEST_URL,
};
use app_lib::plugin::skill::registry::SkillRegistry;
use tempfile::tempdir;
use zip::write::SimpleFileOptions;

const VALID_SKILL_MD: &str = r#"---
name: Demo Skill
description: A valid demo skill
---
# Demo
"#;

fn write_skill(root: &std::path::Path, id: &str, skill_md: &str) {
    let dir = root.join(id);
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("SKILL.md"), skill_md).unwrap();
}

fn make_zip(path: &std::path::Path, entries: &[(&str, &[u8])]) {
    let file = fs::File::create(path).unwrap();
    let mut zip = zip::ZipWriter::new(file);
    for (name, bytes) in entries {
        zip.start_file(*name, SimpleFileOptions::default()).unwrap();
        zip.write_all(bytes).unwrap();
    }
    zip.finish().unwrap();
}

#[test]
fn manifest_parse_accepts_zip_and_env_override() {
    std::env::remove_var("RENLIJIA_GLOBAL_SKILLS_MANIFEST_URL");
    assert_eq!(
        configured_global_skills_manifest_url(),
        DEFAULT_GLOBAL_SKILLS_MANIFEST_URL
    );

    std::env::set_var(
        "RENLIJIA_GLOBAL_SKILLS_MANIFEST_URL",
        "  https://example.test/m.json  ",
    );
    assert_eq!(
        configured_global_skills_manifest_url(),
        "https://example.test/m.json"
    );

    let manifest = GlobalSkillsManifest::from_json(
        r#"{
          "bundleVersion": "2026.04.28",
          "artifact": {
            "url": "https://example.test/global.zip",
            "sha256": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            "sizeBytes": 123,
            "archiveFormat": "zip"
          }
        }"#,
    )
    .unwrap();
    assert_eq!(manifest.bundle_version, "2026.04.28");
    assert_eq!(manifest.artifact.archive_format, "zip");

    let error = GlobalSkillsManifest::from_json(
        r#"{
          "bundleVersion": "bad",
          "artifact": {
            "url": "https://example.test/global.tar",
            "sha256": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            "sizeBytes": 123,
            "archiveFormat": "tar"
          }
        }"#,
    )
    .unwrap_err();
    assert!(error.to_string().contains("archiveFormat"));

    std::env::remove_var("RENLIJIA_GLOBAL_SKILLS_MANIFEST_URL");
}

#[test]
fn global_state_parses_global_skills_and_absence() {
    let state = GlobalSkillsState::from_global_state_json(
        r#"{
          "migrations": {"v1": true},
          "globalSkills": {
            "bundleVersion": "v2",
            "artifactSha256": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
            "installedAtUnixSeconds": 1777342830
          }
        }"#,
    )
    .unwrap();
    let state = state.unwrap();
    assert_eq!(state.bundle_version, "v2");
    assert_eq!(
        state.artifact_sha256,
        "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
    );
    assert_eq!(state.installed_at_unix_seconds, 1777342830);

    assert!(
        GlobalSkillsState::from_global_state_json(r#"{"migrations":{"v1":true}}"#)
            .unwrap()
            .is_none()
    );
}

#[test]
fn prepared_install_installs_new_skill_and_overwrites_existing() {
    let temp = tempdir().unwrap();
    let prepared = temp.path().join("prepared");
    let global = temp.path().join("global");
    fs::create_dir_all(&prepared).unwrap();
    fs::create_dir_all(&global).unwrap();

    write_skill(&global, "existing-skill", VALID_SKILL_MD);
    fs::write(global.join("existing-skill").join("old.txt"), "old").unwrap();

    write_skill(
        &prepared,
        "existing-skill",
        r#"---
name: Replaced Skill
description: Replaced content
---
# New
"#,
    );
    fs::write(prepared.join("existing-skill").join("new.txt"), "new").unwrap();
    write_skill(&prepared, "new_skill", VALID_SKILL_MD);
    write_skill(&prepared, "BadSkill", VALID_SKILL_MD);
    write_skill(&prepared, "_", VALID_SKILL_MD);

    let report = install_prepared_global_skills(&prepared, &global).unwrap();

    assert_eq!(report.installed, vec!["existing-skill", "new_skill"]);
    assert_eq!(report.skipped, vec!["BadSkill", "_"]);
    assert!(global.join("existing-skill").join("new.txt").is_file());
    assert!(!global.join("existing-skill").join("old.txt").exists());
    assert!(global.join("new_skill").join("SKILL.md").is_file());
    assert!(prepared.join("existing-skill").join("SKILL.md").is_file());
}

#[test]
fn invalid_skill_md_does_not_delete_existing_target() {
    let temp = tempdir().unwrap();
    let prepared = temp.path().join("prepared");
    let global = temp.path().join("global");
    fs::create_dir_all(&prepared).unwrap();
    fs::create_dir_all(&global).unwrap();

    write_skill(&global, "stable-skill", VALID_SKILL_MD);
    fs::write(global.join("stable-skill").join("keep.txt"), "keep").unwrap();
    write_skill(&prepared, "stable-skill", "# missing frontmatter");

    let report = install_prepared_global_skills(&prepared, &global).unwrap();

    assert!(report.installed.is_empty());
    assert_eq!(report.skipped, vec!["stable-skill"]);
    assert_eq!(
        fs::read_to_string(global.join("stable-skill").join("keep.txt")).unwrap(),
        "keep"
    );
}

#[test]
fn zip_extraction_extracts_safe_entries_and_rejects_zip_slip() {
    let temp = tempdir().unwrap();
    let zip_path = temp.path().join("skills.zip");
    let out = temp.path().join("out");
    fs::create_dir_all(&out).unwrap();
    fs::write(out.join("stale.txt"), "stale").unwrap();

    make_zip(
        &zip_path,
        &[("good-skill/SKILL.md", VALID_SKILL_MD.as_bytes())],
    );
    extract_global_skills_zip(&zip_path, &out).unwrap();
    assert!(out.join("good-skill").join("SKILL.md").is_file());
    assert!(!out.join("stale.txt").exists());

    let bad_zip = temp.path().join("bad.zip");
    make_zip(&bad_zip, &[("../evil.txt", b"evil")]);
    let error = extract_global_skills_zip(&bad_zip, &out).unwrap_err();
    assert!(error.to_string().contains("unsafe zip entry"));
    assert!(!temp.path().join("evil.txt").exists());
}

#[test]
fn should_skip_manifest_only_compares_bundle_version() {
    let manifest = GlobalSkillsManifest::from_json(
        r#"{
          "bundleVersion": "v1",
          "artifact": {
            "url": "https://example.test/changed.zip",
            "sha256": "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb",
            "sizeBytes": 999,
            "archiveFormat": "zip"
          }
        }"#,
    )
    .unwrap();

    assert!(should_skip_manifest(
        Some(&GlobalSkillsState {
            bundle_version: "v1".to_string(),
            artifact_sha256: "old".to_string(),
            installed_at_unix_seconds: 1,
        }),
        &manifest
    ));
    assert!(!should_skip_manifest(None, &manifest));
    assert!(!should_skip_manifest(
        Some(&GlobalSkillsState {
            bundle_version: "v2".to_string(),
            artifact_sha256: "old".to_string(),
            installed_at_unix_seconds: 1,
        }),
        &manifest
    ));
}

#[test]
fn write_global_skills_state_preserves_existing_migrations() {
    let temp = tempdir().unwrap();
    let state_path = temp.path().join("state.json");
    fs::write(
        &state_path,
        r#"{"migrations":{"legacyToGlobal":true},"other":{"keep":1}}"#,
    )
    .unwrap();

    write_global_skills_state(
        &state_path,
        &GlobalSkillsState::from_manifest(&GlobalSkillsManifest {
            bundle_version: "v3".to_string(),
            artifact: GlobalSkillsArtifact {
                url: "https://example.test/global.zip".to_string(),
                sha256: "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc"
                    .to_string(),
                size_bytes: 42,
                archive_format: "zip".to_string(),
            },
        }),
    )
    .unwrap();

    let value: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(state_path).unwrap()).unwrap();
    assert_eq!(value["migrations"]["legacyToGlobal"], true);
    assert_eq!(value["other"]["keep"], 1);
    assert_eq!(value["globalSkills"]["bundleVersion"], "v3");
    assert_eq!(
        value["globalSkills"]["artifactSha256"],
        "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc"
    );
    assert!(value["globalSkills"]["installedAtUnixSeconds"]
        .as_u64()
        .is_some());
}

#[test]
fn report_should_persist_state_only_when_at_least_one_skill_installed() {
    use app_lib::plugin::skill::global_sync::should_persist_success_state;

    assert!(!should_persist_success_state(
        &app_lib::plugin::skill::global_sync::GlobalSkillInstallReport {
            installed: Vec::new(),
            skipped: vec!["bad-skill".to_string()],
        }
    ));
    assert!(!should_persist_success_state(
        &app_lib::plugin::skill::global_sync::GlobalSkillInstallReport {
            installed: vec!["good-skill".to_string()],
            skipped: vec!["bad-skill".to_string()],
        }
    ));
    assert!(should_persist_success_state(
        &app_lib::plugin::skill::global_sync::GlobalSkillInstallReport {
            installed: vec!["good-skill".to_string()],
            skipped: Vec::new(),
        }
    ));
}

#[tokio::test]
async fn spawn_global_skill_sync_returns_immediately() {
    let temp = tempdir().unwrap();
    let config = GlobalSkillSyncConfig {
        manifest_url: "http://127.0.0.1:9/unreachable-manifest.json".to_string(),
        state_path: temp.path().join("state.json"),
        downloads_dir: temp.path().join("downloads"),
        prepared_dir: temp.path().join("prepared"),
        global_skills_dir: temp.path().join("global"),
        skill_roots_for_reload: vec![temp.path().join("user"), temp.path().join("global")],
    };
    let registry = Arc::new(Mutex::new(SkillRegistry::new()));

    let start = Instant::now();
    let handle = spawn_global_skill_sync(config, registry);
    assert!(start.elapsed() < Duration::from_millis(100));
    handle.abort();
}
