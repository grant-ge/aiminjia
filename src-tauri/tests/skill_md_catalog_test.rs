use app_lib::plugin::skill::catalog_prompt::format_skill_catalog_with_budget;
use app_lib::plugin::skill::registry::SkillRegistry;
use app_lib::plugin::skill::types::{DiskSkill, SkillFrontmatter, SkillSource};
use std::path::PathBuf;

fn skill(id: &str, desc: &str) -> DiskSkill {
    DiskSkill {
        id: id.to_string(),
        root: PathBuf::from(format!("/tmp/{id}")),
        frontmatter: SkillFrontmatter {
            name: id.to_string(),
            description: desc.to_string(),
            ..Default::default()
        },
        body: format!("body for {id}"),
        source: SkillSource::User,
    }
}

#[test]
fn catalog_respects_budget_and_desc_cap() {
    let entries = vec![skill("salary-query", &"x".repeat(400))];
    let catalog = format_skill_catalog_with_budget(&entries, 200_000);
    assert!(catalog.contains("salary-query"));
    assert!(catalog.len() < 1_000);
}

#[test]
fn registry_tracks_sent_skill_names_incrementally() {
    let mut registry =
        SkillRegistry::from_skills(vec![skill("a-skill", "A"), skill("b-skill", "B")]);
    let first = registry.catalog_delta_for_agent(None, 200_000);
    assert!(first.contains("a-skill"));
    assert!(first.contains("b-skill"));

    let second = registry.catalog_delta_for_agent(None, 200_000);
    assert!(
        second.is_empty(),
        "second call should send no already-sent skills"
    );

    registry.insert(skill("c-skill", "C"));
    let third = registry.catalog_delta_for_agent(None, 200_000);
    assert!(third.contains("c-skill"));
    assert!(!third.contains("a-skill"));
}
