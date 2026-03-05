//! Plugin management IPC commands.

use std::sync::Arc;
use tauri::State;

use crate::plugin::{ToolRegistry, SkillRegistry, ToolInfo, SkillInfo};

/// List all registered tools.
#[tauri::command]
pub async fn list_tools(
    registry: State<'_, Arc<ToolRegistry>>,
) -> Result<Vec<ToolInfo>, String> {
    Ok(registry.list().await)
}

/// List all registered skills.
#[tauri::command]
pub async fn list_skills(
    registry: State<'_, Arc<SkillRegistry>>,
) -> Result<Vec<SkillInfo>, String> {
    Ok(registry.list().await)
}

/// Get combined plugin info (tools + skills).
#[tauri::command]
pub async fn get_plugin_info(
    tool_registry: State<'_, Arc<ToolRegistry>>,
    skill_registry: State<'_, Arc<SkillRegistry>>,
) -> Result<serde_json::Value, String> {
    let tools = tool_registry.list().await;
    let skills = skill_registry.list().await;
    Ok(serde_json::json!({
        "tools": tools,
        "skills": skills,
    }))
}
