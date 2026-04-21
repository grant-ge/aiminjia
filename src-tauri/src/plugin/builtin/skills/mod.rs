//! Built-in Skill plugins.

pub mod daily_assistant;

use crate::auth::AuthManager;
use crate::plugin::SkillRegistry;
use crate::runtime::agent::registry::AgentRegistry;
use crate::storage::file_store::AppStorage;
use std::sync::Arc;

/// Register all built-in skills.
pub async fn register_builtin_skills(
    registry: &SkillRegistry,
    db: Arc<AppStorage>,
    auth_manager: Arc<AuthManager>,
    agent_registry: Option<&AgentRegistry>,
) {
    let daily_assistant_skill: Arc<dyn crate::plugin::skill_trait::Skill> = match agent_registry {
        Some(agent_registry) => Arc::new(daily_assistant::DailyAssistantSkill::new_with_registry(
            agent_registry,
            db,
            auth_manager,
        )),
        None => Arc::new(daily_assistant::DailyAssistantSkill::new(db, auth_manager)),
    };
    registry
        .register(daily_assistant_skill, "builtin")
        .await;
}
