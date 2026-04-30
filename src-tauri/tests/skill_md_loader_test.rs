use std::fs;

use app_lib::plugin::skill::loader::load_skill_roots;
use tempfile::TempDir;

fn write_skill(root: &std::path::Path, id: &str, desc: &str, body: &str) {
    let dir = root.join(id);
    fs::create_dir_all(&dir).unwrap();
    fs::write(
        dir.join("SKILL.md"),
        format!("---\nname: {id}\ndescription: {desc}\n---\n\n{body}\n"),
    )
    .unwrap();
}

#[test]
fn loads_user_and_global_skills_with_user_precedence() {
    let global = TempDir::new().unwrap();
    let user = TempDir::new().unwrap();
    write_skill(global.path(), "salary-query", "global desc", "global body");
    write_skill(user.path(), "salary-query", "user desc", "user body");
    write_skill(global.path(), "biz-writing", "biz desc", "biz body");

    let skills = load_skill_roots(&[user.path().to_path_buf(), global.path().to_path_buf()]);
    let skills = skills.expect("skills should load");

    assert_eq!(skills.len(), 2);
    assert_eq!(skills.get("salary-query").unwrap().frontmatter.description, "user desc");
    assert_eq!(skills.get("biz-writing").unwrap().frontmatter.description, "biz desc");
}

#[test]
fn skips_hidden_draft_and_invalid_entries() {
    let root = TempDir::new().unwrap();
    write_skill(root.path(), "valid-skill", "valid", "body");
    write_skill(root.path(), "_draft", "draft", "body");
    fs::write(root.path().join("loose.md"), "---\nname: loose\ndescription: no\n---\n").unwrap();

    let skills = load_skill_roots(&[root.path().to_path_buf()]).unwrap();
    assert!(skills.contains_key("valid-skill"));
    assert!(!skills.contains_key("_draft"));
    assert!(!skills.contains_key("loose"));
}

#[test]
fn rejects_directory_name_that_is_not_skill_id() {
    let root = TempDir::new().unwrap();
    write_skill(root.path(), "BadSkill", "bad", "body");
    let skills = load_skill_roots(&[root.path().to_path_buf()]).unwrap();
    assert!(skills.is_empty());
}
