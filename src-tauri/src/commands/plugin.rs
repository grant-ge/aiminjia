//! Plugin management IPC commands.

use std::{
    fs,
    sync::{Arc, Mutex},
};
use tauri::State;

use crate::commands::skill_management::{list_skills_from_registry_with_enablement, SkillInfo};
use crate::plugin::skill::enablement::SkillEnablementStore;
use crate::plugin::skill::registry::SkillRegistry;
use crate::plugin::{ToolInfo, ToolRegistry};
use tauri::AppHandle;

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

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VisibleToolInfo {
    pub name: String,
    pub description: String,
    pub input_schema: serde_json::Value,
}

/// List all registered tools.
#[tauri::command]
pub async fn list_tools(registry: State<'_, Arc<ToolRegistry>>) -> Result<Vec<ToolInfo>, String> {
    Ok(registry.list().await)
}

/// E2E-only style read endpoint: return the tool schemas a normal chat request
/// would expose for the current conversation/request context.
#[tauri::command]
pub async fn get_visible_tools_for_current_request(
    app: AppHandle,
    registry: State<'_, Arc<ToolRegistry>>,
    conversation_id: Option<String>,
) -> Result<Vec<VisibleToolInfo>, String> {
    let conversation_id = conversation_id
        .as_deref()
        .filter(|id| !id.trim().is_empty())
        .unwrap_or("__aijia_visible_tools_probe__");
    let tool_ctx =
        crate::transport::tauri_commands::chat::chat_runtime_impl::build_tool_description_context(
            &app,
        )
        .await;
    let request_scoped_overrides =
        crate::transport::tauri_commands::chat::chat_runtime_impl::build_request_scoped_tool_overrides(
            &app,
            &tool_ctx,
        )
        .await;
    let authorized_workspace =
        crate::transport::tauri_commands::chat::chat_runtime_impl::load_authorized_workspace(
            &app,
            conversation_id,
        );
    let defs = crate::transport::tauri_commands::chat::chat_runtime_impl::build_visible_tool_defs(
        registry.inner().as_ref(),
        authorized_workspace.is_some(),
        crate::transport::tauri_commands::chat::chat_runtime_impl::ToolSchemaFilter::DailyWhitelist,
        &tool_ctx,
        &request_scoped_overrides,
    )
    .await;

    Ok(defs
        .into_iter()
        .map(|def| VisibleToolInfo {
            name: def.name,
            description: def.description,
            input_schema: def.parameters,
        })
        .collect())
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
