//! Plugin management IPC commands.

use std::{
    fs,
    sync::{Arc, Mutex},
};
use tauri::State;

use crate::commands::skill_management::{list_skills_from_registry, SkillInfo};
use crate::plugin::skill::registry::SkillRegistry;
use crate::plugin::{ToolInfo, ToolRegistry};

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillDetailInfo {
    pub id: String,
    pub when_to_use: Option<String>,
    pub allowed_tools: Vec<String>,
    pub argument_hint: Option<String>,
    pub arguments: Vec<String>,
    pub model: Option<String>,
    pub effort: Option<String>,
    pub context: Option<String>,
    pub agent: Option<String>,
    pub user_invocable: bool,
    pub disable_model_invocation: bool,
    pub version: Option<String>,
    pub category: Option<String>,
    pub paths: Vec<String>,
    pub shell: Option<String>,
    pub body: String,
    pub raw_content: String,
}

/// List all registered tools.
#[tauri::command]
pub async fn list_tools(registry: State<'_, Arc<ToolRegistry>>) -> Result<Vec<ToolInfo>, String> {
    Ok(registry.list().await)
}

/// List all SKILL.md-backed skills.
#[tauri::command]
pub fn list_skills(
    registry: State<'_, Arc<Mutex<SkillRegistry>>>,
) -> Result<Vec<SkillInfo>, String> {
    Ok(list_skills_from_registry(registry.inner()))
}

/// Get full detail for one SKILL.md-backed skill. This intentionally stays
/// separate from list_skills so the skill center grid does not load every body.
#[tauri::command]
pub fn get_skill_detail(
    registry: State<'_, Arc<Mutex<SkillRegistry>>>,
    skill_id: String,
) -> Result<Option<SkillDetailInfo>, String> {
    let guard = registry
        .lock()
        .map_err(|_| "skill registry lock poisoned".to_string())?;
    Ok(guard.get(&skill_id).map(|skill| {
        let fm = &skill.frontmatter;
        let raw_content =
            fs::read_to_string(skill.root.join("SKILL.md")).unwrap_or_else(|_| skill.body.clone());
        SkillDetailInfo {
            id: skill.id.clone(),
            when_to_use: fm.when_to_use.clone(),
            allowed_tools: fm.allowed_tools.clone(),
            argument_hint: fm.argument_hint.clone(),
            arguments: fm.arguments.clone(),
            model: fm.model.clone(),
            effort: fm.effort.clone(),
            context: fm.context.clone(),
            agent: fm.agent.clone(),
            user_invocable: fm.user_invocable,
            disable_model_invocation: fm.disable_model_invocation,
            version: fm.version.clone(),
            category: fm.category.clone(),
            paths: fm.paths.clone(),
            shell: fm.shell.clone(),
            body: skill.body.clone(),
            raw_content,
        }
    }))
}

/// Get combined plugin info (tools + skills).
#[tauri::command]
pub async fn get_plugin_info(
    tool_registry: State<'_, Arc<ToolRegistry>>,
    skill_registry: State<'_, Arc<Mutex<SkillRegistry>>>,
) -> Result<serde_json::Value, String> {
    let tools = tool_registry.list().await;
    let skills = list_skills_from_registry(skill_registry.inner());
    Ok(serde_json::json!({
        "tools": tools,
        "skills": skills,
    }))
}
