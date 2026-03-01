//! Daily assistant Skill — default free-form conversation mode.
//!
//! This is the fallback Skill used when no other Skill activates.

use async_trait::async_trait;

use crate::llm::prompts;
use crate::plugin::skill_trait::*;

pub struct DailyAssistantSkill;

#[async_trait]
impl Skill for DailyAssistantSkill {
    fn id(&self) -> &str { "daily-assistant" }
    fn display_name(&self) -> &str { "日常助手" }
    fn description(&self) -> &str { "Daily HR consultation and general assistance" }

    fn should_activate(
        &self,
        _message: &str,
        _has_files: bool,
        _current_skill: &str,
    ) -> bool {
        // Default skill — never self-activates.
        // Active when no other skill matches, or after another skill finishes.
        false
    }

    fn system_prompt(&self, _state: &SkillState) -> String {
        prompts::get_system_prompt(None)
    }

    fn tool_filter(&self, _state: &SkillState) -> ToolFilter {
        ToolFilter::All
    }

    fn max_iterations(&self, _state: &SkillState) -> usize {
        10
    }

    fn token_budget(&self, _state: &SkillState) -> u32 {
        4096
    }
}
