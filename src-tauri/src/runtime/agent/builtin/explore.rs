use crate::runtime::agent::definition::{
    AgentDefinition, AgentModel, AgentPermissionMode, AgentPrompt, AgentSource,
};

pub fn explore_agent_definition() -> AgentDefinition {
    AgentDefinition {
        name: "explore".into(),
        description: "只读探索：搜索/读取文件，不修改".into(),
        allowed_tools: vec![
            "Read".into(),
            "Grep".into(),
            "Glob".into(),
            "WebSearch".into(),
        ],
        disallowed_tools: vec![],
        max_iterations: 100,
        model: AgentModel::Inherit,
        system_prompt: AgentPrompt::Inline(
            "你是文件搜索专家，擅长彻底地浏览和探索代码库。\n\
\n\
=== 严格只读模式 — 不允许任何文件修改 ===\n\
这是只读探索任务。严格禁止：\n\
- 创建新文件（不允许 Write、touch 或任何形式的创建）\n\
- 修改现有文件（不允许 Edit 操作）\n\
- 删除文件（不允许 rm 或删除）\n\
- 移动或复制文件（不允许 mv 或 cp）\n\
- 在任何位置创建临时文件，包括 /tmp\n\
- 使用重定向（>、>>、|）或 heredoc 写文件\n\
- 运行任何会改变系统状态的命令\n\
\n\
你的角色仅限于搜索和分析现有代码。你没有文件编辑工具——任何编辑尝试都会失败。\n\
\n\
你的能力：\n\
- 用 search_files 做宽泛文件名匹配\n\
- 用 grep_content 做正则内容搜索\n\
- 用 read_workspace_file 读具体文件\n\
- 用 list_directory 看目录结构\n\
- 必要时用 web_search 补充背景\n\
\n\
工作准则：\n\
- 多次小搜索 > 一次大搜索\n\
- 知道路径就直接读，不知道再搜\n\
- 不要捏造：搜索没结果，如实说\"未找到\"，不要编造路径或代码\n\
- 不要预设结论：先搜索，再下判断\n\
\n\
输出：\n\
- 用 Markdown 列出发现的事实\n\
- 引用代码必须 `path:line` 标注\n\
- 末尾给一段 ≤ 5 行的\"结论\"总结\n\
- 信息不足以回答时，明确说\"信息不足\"，列出还需要查的方向"
                .into(),
        ),
        source: AgentSource::Builtin,
        permission_mode: AgentPermissionMode::AutoDeny,
        background_default: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::agent::tool_whitelist::resolve_agent_tools;

    /// M2.2 / F6 regression: explore.allowed_tools must use the canonical tool
    /// names from runtime/tools/catalog.rs. Earlier names like "read_file" /
    /// "grep" / "glob" don't exist in the catalog, so resolve_agent_tools
    /// would silently strip them and leave explore with only web_search.
    #[test]
    fn explore_tools_match_canonical_catalog_names() {
        let def = explore_agent_definition();
        // Names actually exposed by the runtime tool catalog.
        let available: Vec<String> = [
            "Read",
            "Grep",
            "Glob",
            "WebSearch",
            "Write", // not requested → must not appear
            "Bash",  // not requested → must not appear
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();

        let resolved = resolve_agent_tools(
            &def.allowed_tools,
            &def.disallowed_tools,
            &available,
            false, // is_async
            false, // allow_recursive_spawn
        );

        for required in ["Read", "Grep", "Glob", "WebSearch"] {
            assert!(
                resolved.contains(&required.to_string()),
                "explore must retain {required} after whitelist; got {resolved:?}"
            );
        }
        assert!(
            !resolved.contains(&"Write".to_string()),
            "explore must not gain write_file"
        );
        assert!(
            !resolved.contains(&"Bash".to_string()),
            "explore must not gain bash"
        );
    }
}
