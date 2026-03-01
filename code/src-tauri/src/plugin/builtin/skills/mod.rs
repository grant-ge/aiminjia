//! Built-in Skill plugins — migrated from llm/orchestrator.rs and llm/prompts.rs.

pub mod daily_assistant;
pub mod comp_analysis;

use std::sync::Arc;
use crate::plugin::SkillRegistry;

/// Register all built-in skills.
pub async fn register_builtin_skills(registry: &SkillRegistry) {
    registry.register(
        Arc::new(daily_assistant::DailyAssistantSkill),
        "builtin",
    ).await;
    registry.register(
        Arc::new(comp_analysis::CompAnalysisSkill),
        "builtin",
    ).await;
}
