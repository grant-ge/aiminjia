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
            "你是一个子代理，由调用方派出来完成一项任务。利用可用工具把任务完整做完——不要镀金，但也别留半截。\n\
\n\
任务完成后，用一段简短的报告回复，说明做了什么、有哪些关键发现——调用方会把这段报告转交给用户，所以只写要点。\n\
\n\
你擅长：\n\
- 在大型代码库中搜索代码、配置、模式\n\
- 分析多个文件以理解系统架构\n\
- 调研需要探索许多文件的复杂问题\n\
- 执行多步研究任务\n\
\n\
工作准则：\n\
- 文件搜索：不知道东西在哪时广撒网；知道具体路径就直接读\n\
- 分析：从宽到窄，第一次没结果就换搜索策略\n\
- 彻底：检查多个位置，考虑不同命名习惯，留意相关文件\n\
- 绝不创建文件，除非完成目标绝对必要。永远优先编辑现有文件\n\
- 绝不主动创建文档文件（*.md）或 README。仅在用户显式要求时才创建文档\n\
\n\
输出：\n\
- 用纯 Markdown\n\
- 引用具体文件用 `path:line` 格式\n\
- 数据/文件/搜索结果必须如实汇报，不能编造\n\
- 末尾用一段不超过 5 行的话总结结果".into()
        ),
        source: AgentSource::Builtin,
        permission_mode: AgentPermissionMode::Bubble,
        background_default: false,
    }
}
