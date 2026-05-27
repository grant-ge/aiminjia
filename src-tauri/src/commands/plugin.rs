//! Plugin management IPC commands.

use std::sync::{Arc, Mutex};
use tauri::State;

use crate::commands::skill_management::{
    SkillInfo, list_skills_from_registry, list_skills_from_registry_with_resolver,
};
use crate::plugin::skill::registry::SkillRegistry;
use crate::plugin::skill::updated_at::DirMtimeResolver;
use crate::plugin::{ToolInfo, ToolRegistry};

/// List all registered tools.
#[tauri::command]
pub async fn list_tools(registry: State<'_, Arc<ToolRegistry>>) -> Result<Vec<ToolInfo>, String> {
    Ok(registry.list().await)
}

/// List all SKILL.md-backed skills.
#[tauri::command]
pub fn list_skills(
    registry: State<'_, Arc<Mutex<SkillRegistry>>>,
    language: Option<String>,
) -> Result<Vec<SkillInfo>, String> {
    Ok(list_skills_from_registry_with_resolver(
        registry.inner(),
        &DirMtimeResolver,
        language.as_deref().unwrap_or("zh-CN"),
    ))
}

/// Get combined plugin info (tools + skills).
#[tauri::command]
pub async fn get_plugin_info(
    tool_registry: State<'_, Arc<ToolRegistry>>,
    skill_registry: State<'_, Arc<Mutex<SkillRegistry>>>,
    language: Option<String>,
) -> Result<serde_json::Value, String> {
    let tools = tool_registry.list().await;
    let skills = if let Some(language) = language.as_deref() {
        list_skills_from_registry_with_resolver(skill_registry.inner(), &DirMtimeResolver, language)
    } else {
        list_skills_from_registry(skill_registry.inner())
    };
    Ok(serde_json::json!({
        "tools": tools,
        "skills": skills,
    }))
}
