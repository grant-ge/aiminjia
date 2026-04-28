use std::fs;
use std::sync::{Arc, Mutex};

use app_lib::plugin::skill::loader::load_skill_roots;
use app_lib::plugin::skill::registry::SkillRegistry;
use tempfile::TempDir;

fn write_skill(root: &std::path::Path, id: &str, label: &str) {
    let dir = root.join(id);
    fs::create_dir_all(&dir).unwrap();
    fs::write(
        dir.join("SKILL.md"),
        format!("---\nname: {id}\ndescription: desc {id}\nmetadata:\n  label: {label}\n---\nbody"),
    )
    .unwrap();
}

#[test]
fn list_skills_returns_only_skill_md_entries() {
    let global = TempDir::new().unwrap();
    let user = TempDir::new().unwrap();
    write_skill(global.path(), "biz-writing", "商务写作");
    write_skill(user.path(), "salary-query", "薪酬查询");
    write_skill(user.path(), "biz-writing", "用户覆盖");

    let skills = load_skill_roots(&[user.path().to_path_buf(), global.path().to_path_buf()]).unwrap();
    let registry = Arc::new(Mutex::new(SkillRegistry::from_skills(skills.into_values().collect())));

    let infos = app_lib::commands::skill_management::list_skills_from_registry(&registry);

    let ids = infos.iter().map(|s| s.id.clone()).collect::<Vec<_>>();
    assert!(ids.contains(&"salary-query".to_string()));
    assert!(ids.contains(&"biz-writing".to_string()));
    let biz = infos.iter().find(|s| s.id == "biz-writing").unwrap();
    assert_eq!(biz.display_name, "用户覆盖", "user scope must override global");
    assert!(!ids.contains(&"daily-assistant".to_string()));
}
