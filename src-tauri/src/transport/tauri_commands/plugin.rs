use std::sync::{Arc, Mutex};

use crate::commands::skill_management::{list_skills_from_registry, SkillInfo};
use crate::plugin::skill::registry::SkillRegistry;
use crate::plugin::{ToolInfo, ToolRegistry};

#[derive(Clone)]
pub struct TauriPluginCommandAdapter {
    tool_registry: Arc<ToolRegistry>,
    skill_registry: Arc<Mutex<SkillRegistry>>,
}

impl TauriPluginCommandAdapter {
    pub fn new(tool_registry: Arc<ToolRegistry>, skill_registry: Arc<Mutex<SkillRegistry>>) -> Self {
        Self {
            tool_registry,
            skill_registry,
        }
    }

    pub async fn list_tools(&self) -> Vec<ToolInfo> {
        self.tool_registry.list().await
    }

    pub fn list_skills(&self) -> Vec<SkillInfo> {
        list_skills_from_registry(&self.skill_registry)
    }

    pub async fn get_plugin_info(&self) -> serde_json::Value {
        let tools = self.tool_registry.list().await;
        let skills = self.list_skills();
        serde_json::json!({
            "tools": tools,
            "skills": skills,
        })
    }
}
