use std::fs;
use tempfile::TempDir;

use app_lib::runtime::agent::registry_loader::load_registry_with_user_dir;

fn write_md(dir: &std::path::Path, name: &str, content: &str) {
    fs::write(dir.join(name), content).unwrap();
}

#[test]
fn builtin_loaded_when_no_user_files() {
    let reg = load_registry_with_user_dir(None, None);
    // 内置 agent 名称由 P0.1/P9.1 决定，已知存在 browse_data_agent / daily_assistant_agent / general-purpose / explore
    assert!(reg.get("browse_data_agent").is_some());
    assert!(reg.get("daily_assistant_agent").is_some());
    assert!(reg.get("general-purpose").is_some());
    assert!(reg.get("explore").is_some());
}

#[test]
fn user_md_overrides_builtin_same_name() {
    let dir = TempDir::new().unwrap();
    write_md(
        dir.path(),
        "browse_data_agent.md",
        r#"---
name: browse_data_agent
description: User custom override
allowed_tools: ["custom_tool"]
---
custom system prompt"#,
    );
    let reg = load_registry_with_user_dir(Some(dir.path()), None);
    let def = reg.get("browse_data_agent").expect("must exist");
    assert_eq!(def.allowed_tools, vec!["custom_tool".to_string()]);
    assert_eq!(def.description, "User custom override");
}

#[test]
fn project_md_overrides_user_md_same_name() {
    let user = TempDir::new().unwrap();
    let project = TempDir::new().unwrap();
    write_md(
        user.path(),
        "shared.md",
        "---\nname: shared\ndescription: from-user\n---\nuser body",
    );
    write_md(
        project.path(),
        "shared.md",
        "---\nname: shared\ndescription: from-project\n---\nproject body",
    );
    let reg = load_registry_with_user_dir(Some(user.path()), Some(project.path()));
    let def = reg.get("shared").expect("must exist");
    assert_eq!(def.description, "from-project");
}

#[test]
fn malformed_files_silently_skipped_others_load() {
    let dir = TempDir::new().unwrap();
    // 1) 没有 frontmatter
    write_md(dir.path(), "broken.md", "no frontmatter here");
    // 2) 不是 .md 后缀（被忽略）
    fs::write(dir.path().join("ignore.txt"), "garbage").unwrap();
    // 3) 合法的
    write_md(
        dir.path(),
        "good.md",
        "---\nname: good\ndescription: ok\n---\nbody",
    );
    let reg = load_registry_with_user_dir(Some(dir.path()), None);
    // 内置仍在
    assert!(reg.get("browse_data_agent").is_some());
    // 合法的加载了
    assert!(reg.get("good").is_some());
    // broken 没有进 registry
    assert!(reg.get("broken").is_none());
}

#[test]
fn nonexistent_dir_does_not_error() {
    let dir = TempDir::new().unwrap();
    let nonexistent = dir.path().join("does-not-exist");
    let reg = load_registry_with_user_dir(Some(&nonexistent), None);
    assert!(reg.get("browse_data_agent").is_some());
}
