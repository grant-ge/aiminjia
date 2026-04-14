//! Daily assistant Skill — default free-form conversation mode.
//!
//! This is the fallback Skill used when no other Skill activates.

use async_trait::async_trait;

use std::sync::Arc;

use crate::auth::AuthManager;
use crate::llm::prompts;
use crate::plugin::skill_trait::*;
use crate::runtime::tools::catalog::DAILY_ALLOWED_TOOLS;
use crate::storage::file_store::AppStorage;

pub struct DailyAssistantSkill {
    pub db: Arc<AppStorage>,
    pub auth_manager: Arc<AuthManager>,
}

#[async_trait]
impl Skill for DailyAssistantSkill {
    fn id(&self) -> &str {
        "daily-assistant"
    }
    fn display_name(&self) -> &str {
        "日常助手"
    }
    fn description(&self) -> &str {
        "Daily work assistance"
    }

    fn should_activate(&self, _message: &str, _has_files: bool, _current_skill: &str) -> bool {
        // Default skill — never self-activates.
        // Active when no other skill matches, or after another skill finishes.
        false
    }

    fn system_prompt(&self, _state: &SkillState) -> String {
        let persona = self.db.get_active_persona().ok();
        let product_name = tauri::async_runtime::block_on(self.auth_manager.get_auth_info())
            .tenant
            .and_then(|t| t.product_name.filter(|n| !n.is_empty()));
        prompts::get_system_prompt(None, persona.as_ref(), product_name.as_deref())
    }

    fn tool_filter(&self, _state: &SkillState) -> ToolFilter {
        ToolFilter::Only(DAILY_ALLOWED_TOOLS.iter().map(|s| s.to_string()).collect())
    }

    fn max_iterations(&self, _state: &SkillState) -> usize {
        10
    }

    fn token_budget(&self, _state: &SkillState) -> u32 {
        4096
    }
}
