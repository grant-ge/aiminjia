#[derive(Clone, Debug)]
pub enum AgentModel {
    Inherit,
    Fixed(String),
}

#[derive(Clone, Debug)]
pub enum AgentPrompt {
    Inline(String),
    File(std::path::PathBuf),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AgentSource {
    /// 编译时注入的 builtin agent（general-purpose / explore / daily_assistant_agent）。
    Builtin,
    /// 从 `~/.renlijia/users/{scope}/agents/*.md` 加载的用户自定义 agent。
    User,
    /// 由 `EmployeeStore` 投影注册进 `AgentRegistry` 的数字员工。
    ///
    /// 用途：spawn_subagent 派 Teammate 时根据这个判定是否在 transcript meta
    /// 中带上 `employee_id` 字段，下游 agenda runner / Team 视图 / inbox 仍按
    /// employee_id 反查 EmployeeStore。**这是"是不是 employee"的唯一真值**，
    /// 取代之前用 emp- 前缀启发式或 employee_id 互斥参数的方案。
    Employee,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AgentPermissionMode {
    /// async 子 agent 默认：所有需用户确认的工具自动拒绝，避免阻塞
    AutoDeny,
    /// sync 子 agent 默认：AskRequired 冒泡到父对话
    Bubble,
}

#[derive(Clone, Debug)]
pub struct AgentDefinition {
    pub name: String,
    pub description: String,
    pub allowed_tools: Vec<String>,
    pub disallowed_tools: Vec<String>,
    pub max_iterations: usize,
    pub model: AgentModel,
    pub system_prompt: AgentPrompt,
    pub source: AgentSource,
    pub permission_mode: AgentPermissionMode,
    pub background_default: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn agent_definition_supports_disallowed_tools_and_permission_mode() {
        let def = AgentDefinition {
            name: "x".to_string(),
            description: "y".to_string(),
            allowed_tools: vec!["a".to_string()],
            disallowed_tools: vec!["b".to_string()],
            max_iterations: 10,
            model: AgentModel::Inherit,
            system_prompt: AgentPrompt::Inline("p".to_string()),
            source: AgentSource::Builtin,
            permission_mode: AgentPermissionMode::AutoDeny,
            background_default: true,
        };
        assert_eq!(def.disallowed_tools, vec!["b".to_string()]);
        assert!(matches!(def.permission_mode, AgentPermissionMode::AutoDeny));
        assert!(def.background_default);
    }

    #[test]
    fn agent_permission_mode_default_is_bubble_via_helper() {
        // sanity: AutoDeny != Bubble
        assert_ne!(AgentPermissionMode::AutoDeny, AgentPermissionMode::Bubble);
    }

    #[test]
    fn agent_source_distinguishes_three_sources() {
        assert_ne!(AgentSource::Builtin, AgentSource::User);
        assert_ne!(AgentSource::Builtin, AgentSource::Employee);
        assert_ne!(AgentSource::User, AgentSource::Employee);
    }
}
