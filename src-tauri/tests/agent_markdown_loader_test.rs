use std::fs;
use tempfile::TempDir;

use app_lib::runtime::agent::definition::{
    AgentModel, AgentPermissionMode, AgentPrompt, AgentSource,
};
use app_lib::runtime::agent::markdown_loader::load_agent_from_markdown;

#[test]
fn parses_frontmatter_with_all_fields() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("explore.md");
    fs::write(
        &path,
        r#"---
name: explore
description: 只读探索代码库
allowed_tools: ["read_file", "grep"]
disallowed_tools: ["write_file"]
max_iterations: 15
model: haiku
permission_mode: auto_deny
background_default: false
---
You are a read-only explorer. Search and report findings."#,
    )
    .unwrap();

    let def = load_agent_from_markdown(&path).expect("must parse");
    assert_eq!(def.name, "explore");
    assert_eq!(def.description, "只读探索代码库");
    assert_eq!(def.allowed_tools, vec!["read_file", "grep"]);
    assert_eq!(def.disallowed_tools, vec!["write_file"]);
    assert_eq!(def.max_iterations, 15);
    assert!(matches!(def.model, AgentModel::Fixed(ref m) if m == "haiku"));
    assert!(matches!(def.permission_mode, AgentPermissionMode::AutoDeny));
    assert!(!def.background_default);
    assert!(matches!(def.source, AgentSource::User));
    match &def.system_prompt {
        AgentPrompt::Inline(s) => assert!(s.contains("read-only explorer")),
        _ => panic!("expected Inline system_prompt"),
    }
}

#[test]
fn defaults_apply_when_optional_fields_omitted() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("min.md");
    fs::write(
        &path,
        r#"---
name: min
description: minimal agent
---
body"#,
    )
    .unwrap();

    let def = load_agent_from_markdown(&path).expect("must parse");
    assert_eq!(def.allowed_tools.len(), 0);
    assert_eq!(def.disallowed_tools.len(), 0);
    assert_eq!(def.max_iterations, 20);
    assert!(matches!(def.model, AgentModel::Inherit));
    assert!(matches!(def.permission_mode, AgentPermissionMode::Bubble));
    assert!(!def.background_default);
}

#[test]
fn rejects_missing_frontmatter() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("bad.md");
    fs::write(&path, "no frontmatter at all").unwrap();
    assert!(load_agent_from_markdown(&path).is_err());
}

#[test]
fn rejects_missing_required_name() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("bad.md");
    fs::write(&path, "---\ndescription: no name\n---\nbody").unwrap();
    assert!(load_agent_from_markdown(&path).is_err());
}

#[test]
fn rejects_unknown_permission_mode() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("bad.md");
    fs::write(
        &path,
        r#"---
name: x
description: y
permission_mode: surprise
---
body"#,
    )
    .unwrap();
    let err = load_agent_from_markdown(&path).expect_err("should reject");
    assert!(
        err.to_string().contains("unknown permission_mode")
            || err.chain().any(|e| e.to_string().contains("permission_mode")),
        "expected error to mention permission_mode: {err}"
    );
}
