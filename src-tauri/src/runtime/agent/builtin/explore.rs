use crate::runtime::agent::definition::{
    AgentDefinition, AgentModel, AgentPermissionMode, AgentPrompt, AgentSource,
};

pub fn explore_agent_definition() -> AgentDefinition {
    AgentDefinition {
        name: "explore".into(),
        description: "只读探索：搜索/读取文件，不修改".into(),
        allowed_tools: vec![
            "read_file".into(),
            "grep".into(),
            "glob".into(),
            "list_directory".into(),
            "web_search".into(),
        ],
        disallowed_tools: vec![],
        max_iterations: 25,
        model: AgentModel::Inherit,
        system_prompt: AgentPrompt::Inline(
            "You are a read-only explorer. Search and read files to answer questions. Never modify anything.".into()
        ),
        source: AgentSource::Builtin,
        permission_mode: AgentPermissionMode::AutoDeny,
        background_default: false,
    }
}
