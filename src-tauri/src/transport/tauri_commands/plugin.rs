use std::sync::Arc;

use crate::plugin::{SkillInfo, SkillRegistry, ToolInfo, ToolRegistry};

#[derive(Clone)]
pub struct TauriPluginCommandAdapter {
    tool_registry: Arc<ToolRegistry>,
    skill_registry: Arc<SkillRegistry>,
}

impl TauriPluginCommandAdapter {
    pub fn new(tool_registry: Arc<ToolRegistry>, skill_registry: Arc<SkillRegistry>) -> Self {
        Self {
            tool_registry,
            skill_registry,
        }
    }

    pub async fn list_tools(&self) -> Vec<ToolInfo> {
        self.tool_registry.list().await
    }

    pub async fn list_skills(&self) -> Vec<SkillInfo> {
        self.skill_registry.list().await
    }

    pub async fn get_plugin_info(&self) -> serde_json::Value {
        let tools = self.tool_registry.list().await;
        let skills = self.skill_registry.list().await;
        serde_json::json!({
            "tools": tools,
            "skills": skills,
        })
    }
}
