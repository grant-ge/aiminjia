//! Built-in Skill plugins.

pub mod daily_assistant;

use std::sync::Arc;
use crate::plugin::SkillRegistry;
use crate::storage::file_store::AppStorage;
use crate::auth::AuthManager;

/// Register all built-in skills.
pub async fn register_builtin_skills(registry: &SkillRegistry, db: Arc<AppStorage>, auth_manager: Arc<AuthManager>) {
    registry.register(
        Arc::new(daily_assistant::DailyAssistantSkill { db, auth_manager }),
        "builtin",
    ).await;
}
