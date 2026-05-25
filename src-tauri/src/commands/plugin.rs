//! Plugin management IPC commands.

use std::sync::{Arc, Mutex};
use tauri::State;

use crate::commands::skill_management::{list_skills_from_registry, SkillInfo};
use crate::plugin::skill::registry::SkillRegistry;
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
) -> Result<Vec<SkillInfo>, String> {
    Ok(list_skills_from_registry(registry.inner()))
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
