use crate::runtime::agent::definition::{
    AgentDefinition, AgentModel, AgentPermissionMode, AgentPrompt, AgentSource,
};

pub fn general_purpose_agent_definition() -> AgentDefinition {
    AgentDefinition {
        name: "general-purpose".into(),
        description: "通用 subagent，可调用绝大多数工具完成任务".into(),
        allowed_tools: vec![], // 空 = 全集（受 ALL_AGENT_DISALLOWED 过滤）
        disallowed_tools: vec![],
        max_iterations: 30,
        model: AgentModel::Inherit,
        system_prompt: AgentPrompt::Inline(
            "You are a general-purpose sub-agent. Complete the assigned task and return a concise final answer.".into()
        ),
        source: AgentSource::Builtin,
        permission_mode: AgentPermissionMode::Bubble,
        background_default: false,
    }
}
