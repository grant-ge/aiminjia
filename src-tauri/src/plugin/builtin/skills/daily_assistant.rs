//! Daily assistant Skill — default free-form conversation mode.
//!
//! This is the fallback Skill used when no other Skill activates.

use async_trait::async_trait;

use std::sync::Arc;

use crate::auth::AuthManager;
use crate::llm::prompts;
use crate::plugin::skill_trait::*;
use crate::runtime::agent::registry::AgentRegistry;
use crate::runtime::tools::catalog::DAILY_ALLOWED_TOOLS;
use crate::storage::file_store::AppStorage;

pub struct DailyAssistantSkill {
    db: Arc<AppStorage>,
    auth_manager: Arc<AuthManager>,
    allowed_tools: Vec<String>,
}

impl DailyAssistantSkill {
    fn fallback_allowed_tools() -> Vec<String> {
        DAILY_ALLOWED_TOOLS
            .iter()
            .map(|tool| tool.to_string())
            .collect()
    }

    pub fn new(db: Arc<AppStorage>, auth_manager: Arc<AuthManager>) -> Self {
        Self {
            db,
            auth_manager,
            allowed_tools: Self::fallback_allowed_tools(),
        }
    }

    pub fn new_with_registry(
        registry: &AgentRegistry,
        db: Arc<AppStorage>,
        auth_manager: Arc<AuthManager>,
    ) -> Self {
        let allowed_tools = registry
            .get("daily_assistant_agent")
            .map(|def| def.allowed_tools.clone())
            .unwrap_or_else(Self::fallback_allowed_tools);

        Self {
            db,
            auth_manager,
            allowed_tools,
        }
    }
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

    fn system_prompt(&self, _state: &SkillState) -> String {
        let persona = self.db.get_active_persona().ok();
        // Use futures::executor::block_on instead of tauri::async_runtime::block_on to avoid
        // "Cannot start a runtime from within a runtime" panic when called on a tokio thread.
        let product_name = futures::executor::block_on(self.auth_manager.get_auth_info())
            .tenant
            .and_then(|t| t.product_name.filter(|n| !n.is_empty()));
        prompts::get_system_prompt(None, persona.as_ref(), product_name.as_deref())
    }

    fn tool_filter(&self, _state: &SkillState) -> ToolFilter {
        ToolFilter::Only(self.allowed_tools.clone())
    }

    fn max_iterations(&self, _state: &SkillState) -> usize {
        10
    }

    fn token_budget(&self, _state: &SkillState) -> u32 {
        8192
    }
}
