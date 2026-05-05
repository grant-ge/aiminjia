use crate::runtime::agent::definition::{AgentDefinition, AgentModel, AgentPermissionMode, AgentPrompt, AgentSource};
use crate::runtime::tools::catalog::DAILY_ALLOWED_TOOLS;

pub fn daily_assistant_agent_definition() -> AgentDefinition {
    AgentDefinition {
        name: "daily_assistant_agent".to_string(),
        description: "日常对话助手，受限工具集保持安全边界".to_string(),
        allowed_tools: DAILY_ALLOWED_TOOLS.iter().map(|s| s.to_string()).collect(),
        disallowed_tools: vec![],
        max_iterations: 20,
        model: AgentModel::Inherit,
        system_prompt: AgentPrompt::Inline(String::new()),
        source: AgentSource::Builtin,
        permission_mode: AgentPermissionMode::Bubble,
        background_default: false,
    }
}
