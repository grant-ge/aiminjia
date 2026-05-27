use std::fs;
use std::sync::{Arc, Mutex};
use tempfile::TempDir;

use app_lib::commands::skill_management::list_skills_from_registry;
use app_lib::plugin::skill::registry::SkillRegistry;
use app_lib::plugin::skill::types::{DiskSkill, SkillFrontmatter, SkillSource};

fn make_disk_skill(id: &str, root: std::path::PathBuf) -> DiskSkill {
    let mut fm = SkillFrontmatter::default();
    fm.name = id.to_string();
    fm.description = "desc".to_string();
    DiskSkill {
        id: id.to_string(),
        root,
        frontmatter: fm,
        body: String::new(),
        localized: Default::default(),
        source: SkillSource::User,
    }
}

#[test]
fn list_skills_from_registry_populates_updated_at_from_dir_mtime() {
    let tmp = TempDir::new().unwrap();
    let skill_dir = tmp.path().join("my-skill");
    fs::create_dir(&skill_dir).unwrap();

    let skill = make_disk_skill("my-skill", skill_dir);
    let registry = Arc::new(Mutex::new(SkillRegistry::from_skills(vec![skill])));

    let infos = list_skills_from_registry(&registry);
    assert_eq!(infos.len(), 1);
    let info = &infos[0];
    assert_eq!(info.id, "my-skill");
    // RFC 3339 长这样：2026-05-13T16:30:00.123456789+00:00 或 ...Z
    let stamp = info
        .updated_at
        .as_deref()
        .expect("updated_at should be present");
    assert!(stamp.contains('T'), "expected RFC 3339, got: {stamp}",);
}

#[test]
fn list_skills_from_registry_returns_none_when_dir_missing() {
    let tmp = TempDir::new().unwrap();
    let missing = tmp.path().join("never-existed");

    let skill = make_disk_skill("ghost-skill", missing);
    let registry = Arc::new(Mutex::new(SkillRegistry::from_skills(vec![skill])));

    let infos = list_skills_from_registry(&registry);
    assert_eq!(infos.len(), 1);
    assert!(infos[0].updated_at.is_none());
}
