//! Built-in Skill plugins.

pub mod daily_assistant;

use std::sync::Arc;
use crate::plugin::SkillRegistry;

/// Register all built-in skills.
pub async fn register_builtin_skills(registry: &SkillRegistry) {
    registry.register(
        Arc::new(daily_assistant::DailyAssistantSkill),
        "builtin",
    ).await;
}
