use std::sync::Arc;

use app_lib::plugin::declarative_skill::DeclarativeSkill;
use app_lib::plugin::manifest::read_manifest_from_skill_dir;
use app_lib::plugin::{SkillRegistry, ToolRegistry};
use app_lib::runtime::chat::SkillSessionStore;
use app_lib::runtime::tools::builtin::switch_skill::SwitchSkillRuntimeTool;
use app_lib::runtime::tools::{RuntimeTool, ToolExecutionContext};
use serde_json::json;
use tempfile::TempDir;

fn write_skill_dir(root: &std::path::Path, id: &str, body: &str) -> std::path::PathBuf {
    let dir = root.join("source").join(id);
    std::fs::create_dir_all(dir.join("prompts")).unwrap();
    std::fs::write(
        dir.join("SKILL.md"),
        format!(
            r#"---
id: "{id}"
name: "Payroll Analysis"
description: "Summarise payroll analysis workflow"
keywords:
  - "payroll"
include_app_base: false
---
{body}
"#
        ),
    )
    .unwrap();
    std::fs::write(dir.join("prompts/base.md"), "BASE PROMPT CONTENT").unwrap();
    dir
}

fn install_skill_to(custom_dir: &std::path::Path, source: &std::path::Path) -> String {
    let manifest = read_manifest_from_skill_dir(source).expect("source manifest should parse");
    let plugin_id = manifest.plugin.id;
    std::fs::create_dir_all(custom_dir).unwrap();
    let dest = custom_dir.join(&plugin_id);
    if dest.exists() {
        std::fs::remove_dir_all(&dest).unwrap();
    }
    copy_dir(source, &dest);
    plugin_id
}

fn copy_dir(source: &std::path::Path, dest: &std::path::Path) {
    std::fs::create_dir_all(dest).unwrap();
    for entry in std::fs::read_dir(source).unwrap() {
        let entry = entry.unwrap();
        let from = entry.path();
        let to = dest.join(entry.file_name());
        if from.is_dir() {
            copy_dir(&from, &to);
        } else {
            std::fs::copy(&from, &to).unwrap();
        }
    }
}

#[test]
fn skill_install_copies_manifest_to_plugin_id_directory_and_overwrites_same_id() {
    let tmp = TempDir::new().unwrap();
    let source = write_skill_dir(tmp.path(), "payroll-skill", "FULL SKILL BODY v1");
    let custom_dir = tmp.path().join(".renlijia").join("skills");

    let plugin_id = install_skill_to(&custom_dir, &source);
    assert_eq!(plugin_id, "payroll-skill");
    let installed = custom_dir.join("payroll-skill");
    assert!(installed.is_dir());
    let manifest = read_manifest_from_skill_dir(&installed).unwrap();
    assert_eq!(manifest.plugin.id, "payroll-skill");

    std::fs::write(source.join("SKILL.md"), r#"---
id: "payroll-skill"
name: "Payroll Analysis"
description: "Summarise payroll analysis workflow"
---
FULL SKILL BODY v2
"#).unwrap();
    install_skill_to(&custom_dir, &source);

    let installed_dirs: Vec<_> = std::fs::read_dir(&custom_dir)
        .unwrap()
        .filter_map(|entry| entry.ok())
        .filter(|entry| entry.path().is_dir())
        .collect();
    assert_eq!(installed_dirs.len(), 1, "same plugin_id should overwrite, not duplicate");
    assert!(std::fs::read_to_string(installed.join("SKILL.md")).unwrap().contains("v2"));
}

#[tokio::test]
async fn skill_registry_listing_exposes_summary_without_full_skill_body() {
    let tmp = TempDir::new().unwrap();
    let source = write_skill_dir(tmp.path(), "payroll-skill", "FULL SECRET SKILL BODY");
    let manifest = read_manifest_from_skill_dir(&source).unwrap();
    let skill = DeclarativeSkill::load(&manifest, &source).unwrap();
    let registry = SkillRegistry::new("payroll-skill");
    registry.register(Arc::new(skill), "custom").await;

    let listed = registry.list().await;
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].id, "payroll-skill");
    assert_eq!(listed[0].display_name, "Payroll Analysis");
    assert_eq!(listed[0].description, "Summarise payroll analysis workflow");
    let summary = serde_json::to_string(&listed).unwrap();
    assert!(!summary.contains("FULL SECRET SKILL BODY"));
    assert!(!summary.contains("BASE PROMPT CONTENT"));
}

#[tokio::test]
async fn switch_skill_returns_skill_runtime_patch_and_launching_result_once_per_call() {
    let tmp = TempDir::new().unwrap();
    let source = write_skill_dir(tmp.path(), "payroll-skill", "FULL SKILL BODY");
    let manifest = read_manifest_from_skill_dir(&source).unwrap();
    let skill = DeclarativeSkill::load(&manifest, &source).unwrap();
    let registry = Arc::new(SkillRegistry::new("payroll-skill"));
    registry.register(Arc::new(skill), "custom").await;

    let tool_registry = Arc::new(ToolRegistry::new());
    let sessions = Arc::new(SkillSessionStore::new());
    let tool = SwitchSkillRuntimeTool::new(registry, sessions, tool_registry);

    let result = tool
        .execute(
            json!({ "skill_id": "payroll-skill" }),
            ToolExecutionContext::for_test("conv-skill-loading", "run", "tc"),
        )
        .await
        .unwrap();

    assert_eq!(result.tool_name, "switch_skill");
    assert!(result.content.contains("Switched to skill 'payroll-skill'"));
    let data = result.data.expect("switch_skill should return runtime patch data");
    assert_eq!(data["skill_control"]["skill_id"], "payroll-skill");
    assert!(data["skill_control"]["system_prompt"].as_str().unwrap().contains("BASE PROMPT CONTENT"));
}

#[tokio::test]
async fn resolving_same_skill_conversation_reuses_single_session_summary_state() {
    let tmp = TempDir::new().unwrap();
    let source = write_skill_dir(tmp.path(), "payroll-skill", "FULL SKILL BODY");
    let manifest = read_manifest_from_skill_dir(&source).unwrap();
    let skill = DeclarativeSkill::load(&manifest, &source).unwrap();
    let registry = SkillRegistry::new("payroll-skill");
    registry.register(Arc::new(skill), "custom").await;
    let sessions = SkillSessionStore::new();
    let all_tools = vec!["switch_skill".to_string()];

    let first = sessions
        .resolve_turn_context(&registry, &all_tools, "conv-skill-loading", "payroll", false)
        .await
        .unwrap();
    let second = sessions
        .resolve_turn_context(&registry, &all_tools, "conv-skill-loading", "payroll again", false)
        .await
        .unwrap();

    assert_eq!(first.skill_id, "payroll-skill");
    assert_eq!(second.skill_id, "payroll-skill");
    assert_eq!(first.system_prompt, second.system_prompt);
}
