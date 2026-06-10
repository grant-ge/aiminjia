//! Plugin management IPC commands.

use std::sync::{Arc, Mutex};
use tauri::State;

use crate::commands::skill_management::{list_skills_from_registry_with_enablement, SkillInfo};
use crate::plugin::skill::enablement::SkillEnablementStore;
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
    enablement_store: State<'_, Arc<SkillEnablementStore>>,
) -> Result<Vec<SkillInfo>, String> {
    let enablement = enablement_store.load_or_default();
    Ok(list_skills_from_registry_with_enablement(
        registry.inner(),
        &enablement,
    ))
}

/// Get combined plugin info (tools + skills).
#[tauri::command]
pub async fn get_plugin_info(
    tool_registry: State<'_, Arc<ToolRegistry>>,
    skill_registry: State<'_, Arc<Mutex<SkillRegistry>>>,
    enablement_store: State<'_, Arc<SkillEnablementStore>>,
) -> Result<serde_json::Value, String> {
    let tools = tool_registry.list().await;
    let enablement = enablement_store.load_or_default();
    let skills = list_skills_from_registry_with_enablement(skill_registry.inner(), &enablement);
    Ok(serde_json::json!({
        "tools": tools,
        "skills": skills,
    }))
}
