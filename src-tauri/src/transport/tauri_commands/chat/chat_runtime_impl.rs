use std::sync::Arc;

use tauri::{AppHandle, Manager};

use crate::plugin::registry::ToolRegistry;
use crate::plugin::skill_trait::ToolFilter;
use crate::runtime::ids::SessionId;
use crate::storage::file_store::RuntimeRepositoryFacade;

/// Transport-side helpers used by the runtime-owned chat path.
///
/// This module intentionally stays helper-only: production turn ownership lives
/// in `SessionRuntime -> RuntimeChatTurnDriver`, mirroring the single main-loop
/// owner design in `claude-code-best`.

/// 根据当前 session 是否有授权目录，决定暴露给 LLM 的工具 schema 列表。
/// 有授权目录：暴露所有工具（含 workspace 工具）。
/// 无授权目录：排除 workspace 工具，避免 LLM 看到不可用工具。
const WORKSPACE_TOOL_NAMES: &[&str] = &[
    "Read",
    "Glob",
    "Write",
    "Edit",
    "Bash",
    "PowerShell",
    "Grep",
];

/// 决定 schema 过滤策略。和"运行时权限白名单"是两回事——
/// 后者由 TurnConfigOverrides.allowed_tools 控制，进入 tool_round_driver。
#[derive(Debug, Clone)]
pub enum ToolSchemaFilter {
    /// 普通对话：用 DAILY_ALLOWED_TOOLS 白名单过滤
    DailyWhitelist,
    /// Employee 派活：用员工自定义白名单过滤
    EmployeeWhitelist(std::collections::HashSet<String>),
    /// 无过滤（subagent 路径或显式全量）
    None,
}

pub async fn build_visible_tool_defs(
    registry: &ToolRegistry,
    has_authorized_workspace: bool,
    schema_filter: ToolSchemaFilter,
    ctx: &crate::runtime::tools::ToolDescriptionContext,
    request_scoped_overrides: &std::collections::HashMap<
        String,
        crate::llm::streaming::ToolDefinition,
    >,
) -> Vec<crate::llm::streaming::ToolDefinition> {
    let defs = if has_authorized_workspace {
        registry
            .get_schemas_filtered(&ToolFilter::All, ctx, request_scoped_overrides)
            .await
    } else {
        registry
            .get_schemas_filtered(
                &ToolFilter::Exclude(WORKSPACE_TOOL_NAMES.iter().map(|s| s.to_string()).collect()),
                ctx,
                request_scoped_overrides,
            )
            .await
    };

    match schema_filter {
        ToolSchemaFilter::DailyWhitelist => {
            let allowed: std::collections::HashSet<&str> =
                crate::runtime::tools::catalog::daily_allowed_tools_for_current_platform()
                    .collect();
            defs.into_iter()
                .filter(|d| allowed.contains(d.name.as_str()))
                .collect()
        }
        ToolSchemaFilter::EmployeeWhitelist(allowed) => defs
            .into_iter()
            .filter(|d| {
                allowed.contains(d.name.as_str())
                    && crate::runtime::tools::catalog::tool_available_on_current_platform(
                        d.name.as_str(),
                    )
            })
            .collect(),
        ToolSchemaFilter::None => defs
            .into_iter()
            .filter(|d| {
                crate::runtime::tools::catalog::tool_available_on_current_platform(d.name.as_str())
            })
            .collect(),
    }
}

pub(crate) const EXPERT_TEAM_DIRECTOR_ALLOWED_TOOLS: &[&str] =
    &["TeamCreate", "TeamDelete", "Agent", "SendMessage"];

pub(crate) fn is_expert_team_director_allowed_tool(tool_name: &str) -> bool {
    EXPERT_TEAM_DIRECTOR_ALLOWED_TOOLS.contains(&tool_name)
}

pub(crate) fn filter_expert_team_director_tool_defs(
    defs: &mut Vec<crate::llm::streaming::ToolDefinition>,
) {
    defs.retain(|def| is_expert_team_director_allowed_tool(def.name.as_str()));
}

pub(crate) fn filter_expert_team_director_allowed_tools(
    allowed_tools: &mut std::collections::HashSet<String>,
) {
    allowed_tools.retain(|name| is_expert_team_director_allowed_tool(name.as_str()));
}

/// Build a [`ToolDescriptionContext`] from app state.
///
/// Reads:
/// - [`AgentRegistry`] — for `<available_subagent_types>` listing
/// - [`CurrentUserStorage`] → `EmployeeStore` — for
///   `<available_subagent_types>` listing (Active employees混排进单段，由 source=Employee 区分)
///
/// Returns an empty context when state is unavailable (logged out, very
/// early boot) — tools then fall back to their static base description.
pub async fn build_tool_description_context(
    app: &AppHandle,
) -> crate::runtime::tools::ToolDescriptionContext {
    use crate::runtime::tools::{AgentDefSummary, ToolDescriptionContext};

    let agents: Vec<AgentDefSummary> = app
        .try_state::<Arc<crate::runtime::agent::registry::AgentRegistry>>()
        .map(|s| s.inner().clone())
        .map(|reg| {
            reg.list()
                .into_iter()
                .map(|def| AgentDefSummary {
                    name: def.name.clone(),
                    description: first_sentence(&def.description, 120),
                    source: def.source.clone(),
                })
                .collect()
        })
        .unwrap_or_default();

    ToolDescriptionContext {
        agents,
        mcp_servers: Vec::new(), // MCP listing is future work
    }
}

/// Build per-turn description overrides for request-scoped tools.
///
/// Currently handles `Agent` only — its description must list available
/// subagent types and hired employees, so we instantiate the tool here
/// (chat layer has both AgentRegistry and EmployeeStore via app state)
/// and call `definition(ctx).await`.  The resulting `ToolDefinition` is
/// converted into the `llm::streaming::ToolDefinition` shape used by
/// the LLM gateway.  Returns an empty map when required state is missing.
pub async fn build_request_scoped_tool_overrides(
    app: &AppHandle,
    ctx: &crate::runtime::tools::ToolDescriptionContext,
) -> std::collections::HashMap<String, crate::llm::streaming::ToolDefinition> {
    use crate::runtime::tools::RuntimeTool;

    let employee_count = ctx
        .agents
        .iter()
        .filter(|a| {
            matches!(
                a.source,
                crate::runtime::agent::definition::AgentSource::Employee
            )
        })
        .count();
    log::info!(
        "[tool-desc-trace] enter: ctx agents={} (employees within={})",
        ctx.agents.len(),
        employee_count
    );
    let mut out = std::collections::HashMap::new();

    // Resolve dependencies needed to construct SpawnSubagentRuntimeTool.
    // Note: the tool's launcher is irrelevant for description rendering
    // (we never call execute() on this throwaway instance), but the
    // constructor demands one — use a stub that errors if invoked.
    let agent_registry =
        match app.try_state::<Arc<crate::runtime::agent::registry::AgentRegistry>>() {
            Some(s) => s.inner().clone(),
            None => return out,
        };

    let stub_launcher = Arc::new(StubLauncher);
    let tool: Arc<dyn RuntimeTool> = Arc::new(
        crate::runtime::tools::builtin::spawn_subagent::SpawnSubagentRuntimeTool::new(
            stub_launcher,
            agent_registry,
        ),
    );

    let rendered = tool.definition(ctx).await;
    let first_emp_id = ctx
        .agents
        .iter()
        .find(|a| {
            matches!(
                a.source,
                crate::runtime::agent::definition::AgentSource::Employee
            )
        })
        .map(|a| a.name.clone());
    log::info!(
        "[tool-desc-trace] Agent rendered: desc_len={} contains_emp_section={} contains_emp_id={}",
        rendered.description.len(),
        rendered.description.contains("<available_subagent_types>"),
        first_emp_id
            .as_ref()
            .map(|id| rendered.description.contains(id))
            .unwrap_or(false),
    );
    let parameters = crate::runtime::tools::TOOL_CATALOG
        .get_entry("Agent")
        .map(|e| e.json_schema.clone())
        .unwrap_or_else(|| serde_json::json!({"type": "object"}));
    out.insert(
        "Agent".to_string(),
        crate::llm::streaming::ToolDefinition {
            name: rendered.id,
            description: rendered.description,
            parameters,
        },
    );

    if let (Some(skill_registry), Some(enablement_store)) = (
        app.try_state::<Arc<std::sync::Mutex<crate::plugin::skill::registry::SkillRegistry>>>(),
        app.try_state::<Arc<crate::plugin::skill::enablement::SkillEnablementStore>>(),
    ) {
        let skill_tool: Arc<dyn RuntimeTool> = Arc::new(
            crate::runtime::tools::builtin::load_skill::LoadSkillRuntimeTool::with_enablement(
                skill_registry.inner().clone(),
                enablement_store.inner().clone(),
            ),
        );
        let rendered = skill_tool.definition(ctx).await;
        let parameters = crate::runtime::tools::TOOL_CATALOG
            .get_entry("Skill")
            .map(|e| e.json_schema.clone())
            .unwrap_or_else(|| serde_json::json!({"type": "object"}));
        out.insert(
            "Skill".to_string(),
            crate::llm::streaming::ToolDefinition {
                name: rendered.id,
                description: rendered.description,
                parameters,
            },
        );
    }

    log::debug!("[tool-desc-trace] returning {} overrides", out.len());
    out
}

/// Stub launcher: never actually invoked (the description-rendering tool
/// instance is thrown away after `definition()`).
struct StubLauncher;

#[async_trait::async_trait]
impl crate::runtime::tools::builtin::spawn_subagent::SpawnSubagentLauncher for StubLauncher {
    async fn launch_sync(
        &self,
        _request: crate::runtime::tools::builtin::spawn_subagent::SpawnSubagentRequest,
        _context: crate::runtime::tools::builtin::spawn_subagent::SpawnSubagentContext,
    ) -> anyhow::Result<String> {
        anyhow::bail!("StubLauncher should never be invoked")
    }

    async fn launch_async(
        &self,
        _request: crate::runtime::tools::builtin::spawn_subagent::SpawnSubagentRequest,
        _context: crate::runtime::tools::builtin::spawn_subagent::SpawnSubagentContext,
    ) -> anyhow::Result<crate::runtime::tools::builtin::spawn_subagent::SpawnAsyncOutcome> {
        anyhow::bail!("StubLauncher should never be invoked")
    }
}

/// Trim a description to its first sentence (or `max_chars` whichever is
/// shorter), without breaking at a UTF-8 char boundary.
fn first_sentence(s: &str, max_chars: usize) -> String {
    let s = s.trim();
    if s.is_empty() {
        return String::new();
    }
    let stop_chars = ['。', '\n', '!', '?', '!', '?'];
    let head: String = s.chars().take(max_chars).collect();
    if let Some(idx) = head.find(|c| stop_chars.contains(&c)) {
        head[..idx].trim_end().to_string()
    } else if let Some(idx) = head.find(". ") {
        head[..idx].trim_end().to_string()
    } else {
        head
    }
}

pub(crate) fn load_authorized_workspace(
    app: &AppHandle,
    conversation_id: &str,
) -> Option<crate::runtime::store::AuthorizedWorkspaceRef> {
    let explicit = app
        .try_state::<Arc<RuntimeRepositoryFacade>>()
        .and_then(|facade| {
            facade
                .authorized_workspace_store()
                .get_current_for_session(
                    conversation_id,
                    &SessionId::new(conversation_id.to_string()),
                )
                .ok()
                .flatten()
        })
        .map(|aw| crate::runtime::store::AuthorizedWorkspaceRef {
            id: aw.id,
            root_path: aw.root_path,
            display_name: aw.display_name,
        });

    if explicit.is_some() {
        return explicit;
    }

    // 未绑定工作目录时，fallback 到 managed AiJiaHome 的 defaultFolder。
    let default_path = app
        .try_state::<Arc<crate::storage::AiJiaHome>>()
        .map(|home| home.default_folder())
        .unwrap_or_else(|| {
            log::warn!("[workspace-auth] AiJiaHome not in managed state, using hardcoded fallback");
            dirs::home_dir()
                .map(|h| h.join(".renlijia").join("defaultFolder"))
                .expect("Cannot determine home directory")
        });
    if let Err(e) = std::fs::create_dir_all(&default_path) {
        log::warn!("[workspace-auth] failed to create defaultFolder: {}", e);
        return None;
    }
    Some(crate::runtime::store::AuthorizedWorkspaceRef {
        id: "default".to_string(),
        root_path: default_path,
        display_name: "默认文件夹".to_string(),
    })
}

pub(crate) fn build_llm_content(
    content: &str,
    attachments: &[crate::runtime::chat::chat_turn_driver::ChatAttachmentRef],
    has_authorized_workspace: bool,
) -> String {
    if attachments.is_empty() {
        return content.to_string();
    }

    let file_refs: Vec<String> = attachments
        .iter()
        .map(|file| {
            format!(
                "- {} (path: \"{}\", 类型: {})",
                file.file_name, file.file_path, file.file_type
            )
        })
        .collect();

    let hint = if has_authorized_workspace {
        "本轮附件已自动加入授权目录（read 自由；write 默认询问，acceptEdits 模式自动允许）：\n- 文件附件：所在目录被授权\n- 文件夹附件：该目录及子树被授权\n请优先使用 Read / Glob / Grep 读取附件；需要计算时再结合 Bash 处理内容。"
    } else {
        "本轮附件已自动加入授权目录（read 自由；write 默认询问，acceptEdits 模式自动允许）：\n- 文件附件：所在目录被授权\n- 文件夹附件：该目录及子树被授权\n请优先使用 Read / Glob / Grep 读取附件；需要计算时再结合 Bash 处理内容。"
    };

    format!(
        "{}\n\n[当前消息附件]\n{}\n\n{}",
        content,
        file_refs.join("\n"),
        hint
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plugin::builtin::tools::register_builtin_tools;
    use crate::plugin::registry::ToolRegistry;

    #[tokio::test]
    async fn test_build_visible_tool_defs_with_authorized_workspace() {
        let registry = ToolRegistry::new();
        register_builtin_tools(&registry).await;

        let defs = build_visible_tool_defs(
            &registry,
            true,
            ToolSchemaFilter::None,
            &crate::runtime::tools::ToolDescriptionContext::empty(),
            &std::collections::HashMap::new(),
        )
        .await;
        let names: Vec<&str> = defs.iter().map(|def| def.name.as_str()).collect();

        // Bash / PowerShell are mutually exclusive (registered per-platform):
        // unix → Bash, windows → PowerShell. Only assert the platform-relevant one.
        let unavailable_shell: &str = if cfg!(windows) { "Bash" } else { "PowerShell" };
        for tool_name in WORKSPACE_TOOL_NAMES {
            if *tool_name == unavailable_shell {
                continue;
            }
            assert!(
                names.contains(tool_name),
                "workspace tool '{}' should be visible when authorized, got {:?}",
                tool_name,
                names
            );
        }
    }

    #[tokio::test]
    async fn test_daily_whitelist_excludes_unavailable_platform_shell() {
        let registry = ToolRegistry::new();
        register_builtin_tools(&registry).await;

        let defs = build_visible_tool_defs(
            &registry,
            true,
            ToolSchemaFilter::DailyWhitelist,
            &crate::runtime::tools::ToolDescriptionContext::empty(),
            &std::collections::HashMap::new(),
        )
        .await;
        let names: Vec<&str> = defs.iter().map(|def| def.name.as_str()).collect();

        if cfg!(windows) {
            assert!(!names.contains(&"Bash"), "Windows must not expose Bash");
            assert!(
                names.contains(&"PowerShell"),
                "Windows should expose PowerShell"
            );
        } else {
            assert!(
                !names.contains(&"PowerShell"),
                "Unix must not expose PowerShell"
            );
            assert!(names.contains(&"Bash"), "Unix should expose Bash");
        }
    }

    #[tokio::test]
    async fn test_build_visible_tool_defs_without_authorized_workspace() {
        let registry = ToolRegistry::new();
        register_builtin_tools(&registry).await;

        let defs = build_visible_tool_defs(
            &registry,
            false,
            ToolSchemaFilter::None,
            &crate::runtime::tools::ToolDescriptionContext::empty(),
            &std::collections::HashMap::new(),
        )
        .await;
        let names: Vec<&str> = defs.iter().map(|def| def.name.as_str()).collect();

        for tool_name in WORKSPACE_TOOL_NAMES {
            assert!(
                !names.contains(tool_name),
                "workspace tool '{}' should be hidden without authorization, got {:?}",
                tool_name,
                names
            );
        }

        assert!(
            !names.is_empty(),
            "non-workspace tools should remain visible without authorization, got empty list"
        );
        // ask_user_question is a non-workspace tool registered in register_builtin_tools
        // and should remain visible even without an authorized workspace
        assert!(
            names.iter().any(|n| !WORKSPACE_TOOL_NAMES.contains(n)),
            "at least one non-workspace tool should remain visible, got {:?}",
            names
        );
    }

    #[tokio::test]
    async fn test_build_visible_tool_defs_applies_allowed_tools_filter_after_workspace_filter() {
        let registry = ToolRegistry::new();
        register_builtin_tools(&registry).await;
        let allowed = std::collections::HashSet::from(["Grep".to_string(), "Read".to_string()]);

        let defs = build_visible_tool_defs(
            &registry,
            false,
            ToolSchemaFilter::EmployeeWhitelist(allowed),
            &crate::runtime::tools::ToolDescriptionContext::empty(),
            &std::collections::HashMap::new(),
        )
        .await;
        let names: Vec<&str> = defs.iter().map(|def| def.name.as_str()).collect();

        // read_workspace_file is in WORKSPACE_TOOL_NAMES so it gets filtered out first;
        // grep_content is also a workspace tool so both are excluded without auth.
        assert!(
            names.is_empty(),
            "all allowed tools are workspace-scoped, expect empty after double filter; got {:?}",
            names
        );
    }

    #[tokio::test]
    async fn expert_team_director_filter_removes_task_polling_tools_but_keeps_team_tools() {
        let registry = ToolRegistry::new();
        register_builtin_tools(&registry).await;

        let mut defs = build_visible_tool_defs(
            &registry,
            true,
            ToolSchemaFilter::DailyWhitelist,
            &crate::runtime::tools::ToolDescriptionContext::empty(),
            &std::collections::HashMap::new(),
        )
        .await;
        filter_expert_team_director_tool_defs(&mut defs);

        let names: std::collections::HashSet<&str> =
            defs.iter().map(|def| def.name.as_str()).collect();
        for blocked in [
            "AskUserQuestion",
            "TaskCreate",
            "TaskUpdate",
            "TaskList",
            "TaskGet",
            "TaskClaim",
            "TaskStop",
            "TaskOutput",
            "Bash",
            "Read",
            "Write",
            "Edit",
            "Glob",
            "Grep",
        ] {
            assert!(
                !names.contains(blocked),
                "expert-team Lead should not see {blocked}; got {names:?}"
            );
        }
        for allowed in EXPERT_TEAM_DIRECTOR_ALLOWED_TOOLS {
            assert!(
                names.contains(allowed),
                "expert-team Lead still needs {allowed}; got {names:?}"
            );
        }
    }

    #[test]
    fn expert_team_director_filter_removes_blocked_tools_from_runtime_allowlist() {
        let mut allowed = std::collections::HashSet::from([
            "Agent".to_string(),
            "TaskOutput".to_string(),
            "TaskCreate".to_string(),
            "TeamCreate".to_string(),
            "TeamDelete".to_string(),
            "SendMessage".to_string(),
            "Bash".to_string(),
            "Read".to_string(),
        ]);

        filter_expert_team_director_allowed_tools(&mut allowed);

        assert!(!allowed.contains("TaskOutput"));
        assert!(!allowed.contains("TaskCreate"));
        assert!(!allowed.contains("Bash"));
        assert!(!allowed.contains("Read"));
        assert!(allowed.contains("Agent"));
        assert!(allowed.contains("TeamCreate"));
        assert!(allowed.contains("TeamDelete"));
        assert!(allowed.contains("SendMessage"));
    }

    #[test]
    fn test_build_llm_content_with_authorized_workspace_uses_workspace_tools_hint() {
        let attachments = vec![crate::runtime::chat::chat_turn_driver::ChatAttachmentRef {
            id: "attachment-1".to_string(),
            file_name: "sales.xlsx".to_string(),
            file_path: "/tmp/sales.xlsx".to_string(),
            kind: "file".to_string(),
            file_size: 0,
            file_type: "xlsx".to_string(),
            mime_type: Some("application/vnd.ms-excel".to_string()),
        }];

        let content = build_llm_content("请分析这个目录里的销售数据", &attachments, true);

        assert!(content.contains("[当前消息附件]"));
        assert!(content.contains("附件"));
        assert!(content.contains("授权目录"));
        assert!(!content.contains("load_file(file_id)"));
    }
}
