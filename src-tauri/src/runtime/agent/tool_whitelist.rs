//! 子 agent 的三层工具白名单：
//! 1. 系统级 ALL_AGENT_DISALLOWED — 任何 subagent 都禁用
//! 2. async 专属 ASYNC_AGENT_ALLOWED — 后台子 agent 仅允许此子集
//! 3. definition 级 allowed/disallowed — 来自 AgentDefinition
//!
//! 团队工具（TEAMMATE_TOOLS）—— in-process Teammate 协作工具，是运行时基础
//! 设施，不属于 employee/agent 业务配置。当 agent 被 spawn 成 Teammate
//! （`is_teammate = true`）时自动注入：既绕过 ASYNC_AGENT_ALLOWED 过滤，也
//! 强制追加到最终 allowed_tools 末尾。对齐 claude-code-best 的
//! `IN_PROCESS_TEAMMATE_ALLOWED_TOOLS`（src/constants/tools.ts:77）。

/// 任何 subagent 都不能用的工具（系统级 disallowed）。
/// 这些工具是父 agent 与用户交互的专属能力，
/// 子 agent 调用会破坏控制流（如反向问父之外的人，或操纵 plan mode）。
pub const ALL_AGENT_DISALLOWED: &[&str] = &[
    "AskUserQuestion",
    "Agent",    // 防止子 agent 递归 spawn（对齐 claude-code-best 默认）
    "TaskStop", // 子 agent 不能取消兄弟/父 agent 任务
];

/// async（后台）subagent 额外允许集：仅以下工具可用
/// 后台 agent 不应使用阻塞式工具（弹窗确认、user prompt 等）。
///
/// 工具名必须与 `runtime/tools/catalog.rs` + `runtime/tools/builtin/*` 暴露的
/// canonical 名一致；否则 `resolve_agent_tools` 会在 available_names 过滤步骤把
/// async agent 的工具集裁成空。
pub const ASYNC_AGENT_ALLOWED: &[&str] = &[
    "Read",
    "Write",
    "Edit",
    "Bash",
    "Grep",
    "Glob",
    "WebSearch",
    "Agent",
    "TaskOutput",
];

/// In-process Teammate 协作工具。
///
/// 一个 agent 被 spawn 为 Teammate 时（`is_teammate = true`），这些工具
/// **由运行时强制注入到最终白名单**，与 employee/agent 自己的 `tool_whitelist`
/// 无关。employee 模板 / agent definition 里**不需要也不应该**显式列出这些
/// 工具——把团队协作能力当作业务能力配置是 leaky abstraction。
///
/// 设计参考：claude-code-best `IN_PROCESS_TEAMMATE_ALLOWED_TOOLS`
/// （src/constants/tools.ts:77）。我们暂时只注入 LTR P1/P2 核心三件套；
/// 未来若引入 TaskCreate/TaskUpdate/Cron* 等可再扩。
pub const TEAMMATE_TOOLS: &[&str] = &[
    "SendMessage",
    "TaskList",
    "TaskGet",
    "TaskUpdate",
    "TaskClaim",
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
    resolve_agent_tools_ex(
        def_allowed,
        def_disallowed,
        available,
        is_async,
        allow_recursive_spawn,
        /* is_teammate */ false,
    )
}

/// `resolve_agent_tools` 的扩展版本，额外接受 `is_teammate` 信号。
///
/// 当 `is_teammate = true` 时：
/// - `TEAMMATE_TOOLS` 不受 `def_allowed` 限制（即使 employee 没列出也强注入）
/// - `TEAMMATE_TOOLS` 不受 `is_async` 的 `ASYNC_AGENT_ALLOWED` 过滤
/// - `TEAMMATE_TOOLS` 仍然受 `available`（运行时确实注册了）与 `ALL_AGENT_DISALLOWED`
///   约束——但这两组在设计上不会冲突（团队工具不在 ALL_AGENT_DISALLOWED 里）
///
/// 旧调用方走 `resolve_agent_tools` 行为完全不变（is_teammate=false）。
pub fn resolve_agent_tools_ex(
    def_allowed: &[String],
    def_disallowed: &[String],
    available: &[String],
    is_async: bool,
    allow_recursive_spawn: bool,
    is_teammate: bool,
) -> Vec<String> {
    let mut out: Vec<String> = available
        .iter()
        .filter(|t| def_allowed.is_empty() || def_allowed.iter().any(|x| x == *t))
        .filter(|t| !def_disallowed.iter().any(|x| x == *t))
        .filter(|t| !ALL_AGENT_DISALLOWED.contains(&t.as_str()))
        .cloned()
        .collect();

    if is_async {
        out.retain(|t| {
            ASYNC_AGENT_ALLOWED.contains(&t.as_str())
                || (is_teammate && TEAMMATE_TOOLS.contains(&t.as_str()))
        });
    }

    // TODO(phase-2): allow_recursive_spawn is now dead for spawn_subagent because
    // ALL_AGENT_DISALLOWED removes it earlier. Either remove the parameter or
    // move spawn_subagent out of ALL_AGENT_DISALLOWED if recursive spawn must work.
    if !allow_recursive_spawn {
        out.retain(|t| t != "Agent");
    }

    // Inject TEAMMATE_TOOLS unconditionally for Teammate spawns. Filtered by
    // `available` so we never claim a tool the runtime hasn't registered.
    if is_teammate {
        for t in TEAMMATE_TOOLS {
            if available.iter().any(|name| name == *t)
                && !ALL_AGENT_DISALLOWED.contains(t)
                && !out.iter().any(|name| name == *t)
            {
                out.push((*t).to_string());
            }
        }
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
        let allowed = resolve_agent_tools(&[], &[], &vs(&["read_file", "Bash"]), false, false);
        assert_eq!(allowed.len(), 2);
        assert!(allowed.contains(&"read_file".to_string()));
    }

    #[test]
    fn def_allowed_restricts_subset() {
        let allowed = resolve_agent_tools(
            &vs(&["read_file"]),
            &[],
            &vs(&["read_file", "Bash"]),
            false,
            false,
        );
        assert_eq!(allowed, vec!["read_file".to_string()]);
    }

    #[test]
    fn def_disallowed_overrides_def_allowed() {
        let allowed = resolve_agent_tools(
            &vs(&["read_file", "Write"]),
            &vs(&["Write"]),
            &vs(&["read_file", "Write"]),
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
            &vs(&["Read", "AskUserQuestion", "WebSearch", "unknown_tool"]),
            true,
            false,
        );
        // unknown_tool 不在 ASYNC_AGENT_ALLOWED → 被过滤
        assert!(allowed.contains(&"Read".to_string()));
        assert!(allowed.contains(&"WebSearch".to_string()));
        assert!(!allowed.contains(&"AskUserQuestion".to_string()));
        assert!(!allowed.contains(&"unknown_tool".to_string()));
    }

    #[test]
    fn recursive_spawn_blocked_by_default() {
        let allowed = resolve_agent_tools(&[], &[], &vs(&["read_file", "Agent"]), false, false);
        assert!(allowed.contains(&"read_file".to_string()));
        assert!(!allowed.contains(&"Agent".to_string()));
    }

    #[test]
    fn recursive_spawn_blocked_unconditionally_by_system_disallowed() {
        // spawn_subagent 在 ALL_AGENT_DISALLOWED，allow_recursive_spawn=true 也无效
        let allowed = resolve_agent_tools(&[], &[], &vs(&["read_file", "Agent"]), false, true);
        assert!(allowed.contains(&"read_file".to_string()));
        assert!(!allowed.contains(&"Agent".to_string()));
    }

    #[test]
    fn async_spawn_also_blocked_by_system_disallowed() {
        // spawn_subagent 在 ALL_AGENT_DISALLOWED，async + allow_recursive_spawn=true 也无效
        let allowed = resolve_agent_tools(&[], &[], &vs(&["Agent"]), true, true);
        assert!(allowed.is_empty());
    }

    // ── TEAMMATE_TOOLS injection ─────────────────────────────────────────────

    #[test]
    fn teammate_tools_injected_when_definition_has_narrow_whitelist() {
        // Regression: `explore` agent only lists ["Read","Grep","Glob","WebSearch"].
        // Without TEAMMATE_TOOLS injection, it can't be a Teammate because it
        // lacks the runtime collaboration tools. With `is_teammate=true`,
        // resolve_agent_tools_ex must append the 3 tools regardless.
        let allowed = resolve_agent_tools_ex(
            &vs(&["Read", "Grep", "Glob", "WebSearch"]),
            &[],
            &vs(&[
                "Read",
                "Grep",
                "Glob",
                "WebSearch",
                "SendMessage",
                "TaskList",
                "TaskGet",
                "Bash", // present but not in def_allowed — must NOT be added
            ]),
            /* is_async */ false,
            /* allow_recursive_spawn */ false,
            /* is_teammate */ true,
        );
        assert!(allowed.contains(&"Read".to_string()));
        assert!(allowed.contains(&"SendMessage".to_string()));
        assert!(allowed.contains(&"TaskList".to_string()));
        assert!(allowed.contains(&"TaskGet".to_string()));
        assert!(
            !allowed.contains(&"Bash".to_string()),
            "TEAMMATE_TOOLS injection must not leak unrelated tools from def_allowed filter"
        );
    }

    #[test]
    fn teammate_tools_not_injected_when_not_teammate() {
        // Non-teammate spawns must keep their narrow whitelist verbatim.
        let allowed = resolve_agent_tools_ex(
            &vs(&["Read"]),
            &[],
            &vs(&["Read", "SendMessage", "TaskList", "TaskGet"]),
            false,
            false,
            /* is_teammate */ false,
        );
        assert_eq!(allowed, vec!["Read".to_string()]);
    }

    #[test]
    fn teammate_tools_bypass_async_filter_when_teammate_is_async() {
        // Async agents normally drop anything outside ASYNC_AGENT_ALLOWED.
        // SendMessage / TaskList / TaskGet are NOT in ASYNC_AGENT_ALLOWED, but
        // they must still be retained when the async subagent is a Teammate.
        let allowed = resolve_agent_tools_ex(
            &[],
            &[],
            &vs(&[
                "Read",
                "Bash",
                "SendMessage",
                "TaskList",
                "TaskGet",
                "Write",
            ]),
            /* is_async */ true,
            false,
            /* is_teammate */ true,
        );
        assert!(allowed.contains(&"Read".to_string()));
        assert!(allowed.contains(&"Bash".to_string()));
        assert!(allowed.contains(&"SendMessage".to_string()));
        assert!(allowed.contains(&"TaskList".to_string()));
        assert!(allowed.contains(&"TaskGet".to_string()));
    }

    #[test]
    fn teammate_tools_only_injected_if_actually_registered() {
        // If the runtime catalog never registered SendMessage (unusual but
        // possible in tests / misconfigured envs), we must NOT pretend it's
        // available — injection is gated on `available` membership.
        let allowed = resolve_agent_tools_ex(
            &[],
            &[],
            &vs(&["Read", "TaskList", "TaskGet"]), // SendMessage missing on purpose
            false,
            false,
            /* is_teammate */ true,
        );
        assert!(allowed.contains(&"TaskList".to_string()));
        assert!(allowed.contains(&"TaskGet".to_string()));
        assert!(
            !allowed.contains(&"SendMessage".to_string()),
            "TEAMMATE_TOOLS injection must not fabricate tools the registry doesn't expose"
        );
    }

    #[test]
    fn teammate_tools_not_duplicated_if_already_in_def_allowed() {
        // Backwards-compat: employees that historically DID list these tools
        // in their whitelist must not see duplicates in the final set.
        let allowed = resolve_agent_tools_ex(
            &vs(&["Read", "SendMessage"]),
            &[],
            &vs(&["Read", "SendMessage", "TaskList", "TaskGet"]),
            false,
            false,
            /* is_teammate */ true,
        );
        let send_count = allowed.iter().filter(|t| t == &"SendMessage").count();
        assert_eq!(
            send_count, 1,
            "SendMessage must appear exactly once after injection"
        );
    }

    #[test]
    fn resolve_agent_tools_wrapper_matches_ex_with_is_teammate_false() {
        // Public API stability: the legacy 5-arg entry point must be a
        // verbatim passthrough to resolve_agent_tools_ex with is_teammate=false.
        let available = vs(&["Read", "SendMessage"]);
        let a = resolve_agent_tools(&vs(&["Read"]), &[], &available, false, false);
        let b = resolve_agent_tools_ex(&vs(&["Read"]), &[], &available, false, false, false);
        assert_eq!(a, b);
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
