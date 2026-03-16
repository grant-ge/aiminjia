//! Built-in Skill plugins.

pub mod daily_assistant;

use std::sync::Arc;
use crate::plugin::SkillRegistry;
use crate::storage::file_store::AppStorage;

/// Register all built-in skills.
pub async fn register_builtin_skills(registry: &SkillRegistry, db: Arc<AppStorage>) {
    registry.register(
        Arc::new(daily_assistant::DailyAssistantSkill { db }),
        "builtin",
    ).await;
}
