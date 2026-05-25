use std::fs;
use std::sync::{Arc, Mutex};
use tempfile::TempDir;

use app_lib::commands::skill_management::{
    install_custom_skill_to_dir_with_force, list_skills_from_registry,
};
use app_lib::plugin::skill::loader::load_skill_roots;
use app_lib::plugin::skill::registry::SkillRegistry;

fn write_skill(parent: &std::path::Path, id: &str, description: &str) {
    let dir = parent.join(id);
    fs::create_dir_all(&dir).unwrap();
    fs::write(
        dir.join("SKILL.md"),
        format!(
            "---\nname: {}\ndescription: {}\n---\nbody\n",
            id, description
        ),
    )
    .unwrap();
}

#[test]
fn install_then_refresh_makes_skill_visible_via_list() {
    let user_root = TempDir::new().unwrap();
    let global_root = TempDir::new().unwrap();
    let staging = TempDir::new().unwrap();

    // 1. Initial registry from empty roots
    let loaded = load_skill_roots(&[
        user_root.path().to_path_buf(),
        global_root.path().to_path_buf(),
    ])
    .unwrap();
    let registry = Arc::new(Mutex::new(SkillRegistry::from_skills(
        loaded.into_values().collect(),
    )));
    assert!(list_skills_from_registry(&registry).is_empty());

    // 2. Stage a skill source dir, install it into user_root
    write_skill(staging.path(), "alpha", "First skill");
    let src = staging.path().join("alpha");
    install_custom_skill_to_dir_with_force(&src, user_root.path(), false).unwrap();

    // 3. Manually refresh (mirrors refresh_skill_registry's internal logic)
    let loaded = load_skill_roots(&[
        user_root.path().to_path_buf(),
        global_root.path().to_path_buf(),
    ])
    .unwrap();
    registry
        .lock()
        .unwrap()
        .replace_all(loaded.into_values().collect());

    let infos = list_skills_from_registry(&registry);
    assert_eq!(infos.len(), 1);
    assert_eq!(infos[0].id, "alpha");
}

#[test]
fn user_root_takes_precedence_over_global_for_same_id() {
    let user_root = TempDir::new().unwrap();
    let global_root = TempDir::new().unwrap();

    write_skill(user_root.path(), "shared", "user version");
    write_skill(global_root.path(), "shared", "global version");

    let loaded = load_skill_roots(&[
        user_root.path().to_path_buf(),
        global_root.path().to_path_buf(),
    ])
    .unwrap();
    let registry = Arc::new(Mutex::new(SkillRegistry::from_skills(
        loaded.into_values().collect(),
    )));

    let infos = list_skills_from_registry(&registry);
    assert_eq!(infos.len(), 1);
    // SkillInfo.description comes from frontmatter — verify it's the user one
    assert_eq!(infos[0].description, "user version");
}
