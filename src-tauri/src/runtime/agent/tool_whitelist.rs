//! 子 agent 的三层工具白名单：
//! 1. 系统级 ALL_AGENT_DISALLOWED — 任何 subagent 都禁用
//! 2. async 专属 ASYNC_AGENT_ALLOWED — 后台子 agent 仅允许此子集
//! 3. definition 级 allowed/disallowed — 来自 AgentDefinition

/// 任何 subagent 都不能用的工具（系统级 disallowed）。
/// 这些工具是父 agent 与用户交互的专属能力，
/// 子 agent 调用会破坏控制流（如反向问父之外的人，或操纵 plan mode）。
pub const ALL_AGENT_DISALLOWED: &[&str] = &[
    "AskUserQuestion",
    "spawn_subagent",  // 防止子 agent 递归 spawn（对齐 claude-code-best 默认）
];

/// async（后台）subagent 额外允许集：仅以下工具可用
/// 后台 agent 不应使用阻塞式工具（弹窗确认、user prompt 等）。
///
/// 工具名必须与 `runtime/tools/catalog.rs` + `runtime/tools/builtin/*` 暴露的
/// canonical 名一致；否则 `resolve_agent_tools` 会在 available_names 过滤步骤把
/// async agent 的工具集裁成空。
pub const ASYNC_AGENT_ALLOWED: &[&str] = &[
    "read_workspace_file", "write_file", "edit_file",
    "bash", "grep_content", "search_files", "get_file_info",
    "web_search",
    "spawn_subagent",
    "task_output",
    "browse_and_extract", "browse_navigate", "read_page_content",
    "page_execute_js", "extract_table_data", "extract_with_pagination",
];

/// 解析 subagent 最终可用工具集
///
/// # 参数
/// - `def_allowed`：来自 AgentDefinition.allowed_tools。空 = 不限定（全集）。
/// - `def_disallowed`：来自 AgentDefinition.disallowed_tools。
/// - `available`：当前 ToolRegistry 全集中的工具名。
/// - `is_async`：是否后台 agent。
/// - `allow_recursive_spawn`：是否允许子 agent 再 spawn 子 agent。默认 false。
///   **注意**：当前对 `spawn_subagent` 无效，因为它已在 ALL_AGENT_DISALLOWED 中被提前过滤。
pub fn resolve_agent_tools(
    def_allowed: &[String],
    def_disallowed: &[String],
    available: &[String],
    is_async: bool,
    allow_recursive_spawn: bool,
) -> Vec<String> {
    let mut out: Vec<String> = available
        .iter()
        .filter(|t| def_allowed.is_empty() || def_allowed.iter().any(|x| x == *t))
        .filter(|t| !def_disallowed.iter().any(|x| x == *t))
        .filter(|t| !ALL_AGENT_DISALLOWED.contains(&t.as_str()))
        .cloned()
        .collect();

    if is_async {
        out.retain(|t| ASYNC_AGENT_ALLOWED.contains(&t.as_str()));
    }

    // TODO(phase-2): allow_recursive_spawn is now dead for spawn_subagent because
    // ALL_AGENT_DISALLOWED removes it earlier. Either remove the parameter or
    // move spawn_subagent out of ALL_AGENT_DISALLOWED if recursive spawn must work.
    if !allow_recursive_spawn {
        out.retain(|t| t != "spawn_subagent");
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vs(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn empty_def_allowed_means_full_subset() {
        let allowed = resolve_agent_tools(
            &[],
            &[],
            &vs(&["read_file", "bash"]),
            false,
            false,
        );
        assert_eq!(allowed.len(), 2);
        assert!(allowed.contains(&"read_file".to_string()));
    }

    #[test]
    fn def_allowed_restricts_subset() {
        let allowed = resolve_agent_tools(
            &vs(&["read_file"]),
            &[],
            &vs(&["read_file", "bash"]),
            false,
            false,
        );
        assert_eq!(allowed, vec!["read_file".to_string()]);
    }

    #[test]
    fn def_disallowed_overrides_def_allowed() {
        let allowed = resolve_agent_tools(
            &vs(&["read_file", "write_file"]),
            &vs(&["write_file"]),
            &vs(&["read_file", "write_file"]),
            false,
            false,
        );
        assert_eq!(allowed, vec!["read_file".to_string()]);
    }

    #[test]
    fn system_disallowed_blocks_ask_user_question() {
        let allowed = resolve_agent_tools(
            &[],
            &[],
            &vs(&["read_file", "AskUserQuestion"]),
            false,
            false,
        );
        assert!(allowed.contains(&"read_file".to_string()));
        assert!(!allowed.contains(&"AskUserQuestion".to_string()));
    }

    #[test]
    fn async_filter_keeps_only_async_allowed_subset() {
        let allowed = resolve_agent_tools(
            &[],
            &[],
            &vs(&[
                "read_workspace_file",
                "AskUserQuestion",
                "extract_table_data",
                "unknown_tool",
            ]),
            true,
            false,
        );
        // unknown_tool 不在 ASYNC_AGENT_ALLOWED → 被过滤
        assert!(allowed.contains(&"read_workspace_file".to_string()));
        assert!(allowed.contains(&"extract_table_data".to_string()));
        assert!(!allowed.contains(&"AskUserQuestion".to_string()));
        assert!(!allowed.contains(&"unknown_tool".to_string()));
    }

    #[test]
    fn recursive_spawn_blocked_by_default() {
        let allowed = resolve_agent_tools(
            &[],
            &[],
            &vs(&["read_file", "spawn_subagent"]),
            false,
            false,
        );
        assert!(allowed.contains(&"read_file".to_string()));
        assert!(!allowed.contains(&"spawn_subagent".to_string()));
    }

    #[test]
    fn recursive_spawn_blocked_unconditionally_by_system_disallowed() {
        // spawn_subagent 在 ALL_AGENT_DISALLOWED，allow_recursive_spawn=true 也无效
        let allowed = resolve_agent_tools(
            &[],
            &[],
            &vs(&["read_file", "spawn_subagent"]),
            false,
            true,
        );
        assert!(allowed.contains(&"read_file".to_string()));
        assert!(!allowed.contains(&"spawn_subagent".to_string()));
    }

    #[test]
    fn async_spawn_also_blocked_by_system_disallowed() {
        // spawn_subagent 在 ALL_AGENT_DISALLOWED，async + allow_recursive_spawn=true 也无效
        let allowed = resolve_agent_tools(
            &[],
            &[],
            &vs(&["spawn_subagent"]),
            true,
            true,
        );
        assert!(allowed.is_empty());
    }
}

#[cfg(test)]
mod disallowed_validation_tests {
    use super::ALL_AGENT_DISALLOWED;
    use crate::runtime::tools::catalog::TOOL_CATALOG;

    #[test]
    fn all_agent_disallowed_names_match_catalog_exactly() {
        for name in ALL_AGENT_DISALLOWED {
            assert!(
                TOOL_CATALOG.get(name).is_some(),
                "ALL_AGENT_DISALLOWED contains '{}' which is not in TOOL_CATALOG (likely case mismatch or stale name)",
                name
            );
        }
    }
}
