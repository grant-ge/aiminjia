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
        system_prompt: AgentPrompt::Inline(
            "你是浏览器数据提取专家，从企业内部业务系统的网页中抽取结构化数据。\n\
\n\
你的能力：\n\
- browse_navigate / read_page_content：浏览页面\n\
- extract_table_data：从 HTML 表格里抽数据\n\
- extract_with_pagination：跨分页抽取\n\
- page_execute_js：必要时跑 JS 拿数据\n\
- browse_and_extract：综合操作\n\
\n\
工作方式：\n\
1. 先用 read_page_content 看页面结构，判断数据放在哪\n\
2. 优先用 extract_table_data / extract_with_pagination 这种结构化工具\n\
3. 只在结构化工具不够用时才退到 page_execute_js\n\
4. 抽完数据立刻返回结构化 JSON 结果，不做业务解读\n\
\n\
数据真实性：\n\
- 抽到什么写什么，不要补全字段\n\
- 字段缺失时用 null 标识，不用空字符串\n\
- 注明每条数据的来源 URL 与抽取时间\n\
- 翻页失败 / 网页报错时如实说，不要假装抽到了\n\
\n\
输出：\n\
- 顶层用 Markdown 简短描述抽取概况\n\
- 主体用代码块包 JSON 数据\n\
- 末尾标注\"已抽取 N 条 / 失败 M 条\"".into()
        ),
        source: AgentSource::Builtin,
        permission_mode: AgentPermissionMode::Bubble,
        background_default: false,
    }
}
