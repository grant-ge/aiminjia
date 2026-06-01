//! Integration tests for skill hot-reload behavior.
//! Tests that refresh_skill_registry sees new SKILL.md files on disk
//! without requiring app restart.

use std::fs;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use tempfile::TempDir;

use app_lib::plugin::skill::loader::load_skill_roots;
use app_lib::plugin::skill::registry::SkillRegistry;

#[test]
fn refresh_reads_new_skill_md_added_after_initial_scan() {
    let tmp = TempDir::new().unwrap();
    let user_dir = tmp.path().join("users").join("scope_x").join("skills");
    let global_dir = tmp.path().join("skills");
    fs::create_dir_all(&user_dir).unwrap();
    fs::create_dir_all(&global_dir).unwrap();

    // Initial scan: empty registry
    let roots: Vec<PathBuf> = vec![user_dir.clone(), global_dir.clone()];
    let initial = load_skill_roots(&roots).expect("initial scan ok");
    let registry = Arc::new(Mutex::new(SkillRegistry::new()));
    registry
        .lock()
        .unwrap()
        .replace_all(initial.into_values().collect());
    assert_eq!(registry.lock().unwrap().skill_ids().len(), 0);

    // 模拟 lotus_skill.py install: 写一个新 SKILL.md
    let new_skill_dir = user_dir.join("foo-skill");
    fs::create_dir_all(&new_skill_dir).unwrap();
    fs::write(
        new_skill_dir.join("SKILL.md"),
        "---\nname: foo-skill\ndescription: test skill\n---\n# foo-skill\n\nbody\n",
    )
    .unwrap();

    // Re-scan + replace
    let after = load_skill_roots(&roots).expect("rescan ok");
    registry
        .lock()
        .unwrap()
        .replace_all(after.into_values().collect());

    // 验收：新 skill 在 registry 里
    let ids = registry.lock().unwrap().skill_ids();
    assert!(
        ids.iter().any(|id| id == "foo-skill"),
        "foo-skill must be in registry after re-scan; got: {:?}",
        ids
    );
}
