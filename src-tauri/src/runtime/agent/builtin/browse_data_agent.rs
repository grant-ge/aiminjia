use crate::runtime::agent::definition::{AgentDefinition, AgentModel, AgentPermissionMode, AgentPrompt, AgentSource};

pub fn browse_data_agent_definition() -> AgentDefinition {
    AgentDefinition {
        name: "browse_data_agent".to_string(),
        description: "浏览器数据提取专家，从内部业务系统中提取表格数据".to_string(),
        allowed_tools: vec![
            "browse_and_extract".to_string(),
            "browse_navigate".to_string(),
            "read_page_content".to_string(),
            "page_execute_js".to_string(),
            "extract_table_data".to_string(),
            "extract_with_pagination".to_string(),
        ],
        disallowed_tools: vec![],
        max_iterations: 30,
        model: AgentModel::Inherit,
        system_prompt: AgentPrompt::Inline(String::new()),
        source: AgentSource::Builtin,
        permission_mode: AgentPermissionMode::Bubble,
        background_default: false,
    }
}
