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
) -> Vec<crate::llm::streaming::ToolDefinition> {
    let defs = if has_authorized_workspace {
        registry.get_schemas_filtered(&ToolFilter::All).await
    } else {
        registry
            .get_schemas_filtered(&ToolFilter::Exclude(
                WORKSPACE_TOOL_NAMES.iter().map(|s| s.to_string()).collect(),
            ))
            .await
    };

    match schema_filter {
        ToolSchemaFilter::DailyWhitelist => {
            let allowed: std::collections::HashSet<&str> =
                crate::runtime::tools::catalog::DAILY_ALLOWED_TOOLS
                    .iter()
                    .copied()
                    .collect();
            defs.into_iter()
                .filter(|d| allowed.contains(d.name.as_str()))
                .collect()
        }
        ToolSchemaFilter::EmployeeWhitelist(allowed) => defs
            .into_iter()
            .filter(|d| allowed.contains(d.name.as_str()))
            .collect(),
        ToolSchemaFilter::None => defs,
    }
}

/// 只查真实绑定，不做 defaultFolder fallback。用于列表展示场景。
pub(crate) fn load_explicit_workspace(
    app: &AppHandle,
    conversation_id: &str,
) -> Option<crate::runtime::store::AuthorizedWorkspaceRef> {
    app.try_state::<Arc<RuntimeRepositoryFacade>>()
        .and_then(|facade| {
            facade
                .authorized_workspace_store()
                .get_current_for_session(&SessionId::new(conversation_id.to_string()))
                .ok()
                .flatten()
        })
        .map(|aw| crate::runtime::store::AuthorizedWorkspaceRef {
            id: aw.id,
            root_path: aw.root_path,
            display_name: aw.display_name,
        })
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
                .get_current_for_session(&SessionId::new(conversation_id.to_string()))
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
                file.file_name,
                file.file_path,
                file.file_type
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

        let defs = build_visible_tool_defs(&registry, true, ToolSchemaFilter::None).await;
        let names: Vec<&str> = defs.iter().map(|def| def.name.as_str()).collect();

        for tool_name in WORKSPACE_TOOL_NAMES {
            assert!(
                names.contains(tool_name),
                "workspace tool '{}' should be visible when authorized, got {:?}",
                tool_name,
                names
            );
        }
    }

    #[tokio::test]
    async fn test_build_visible_tool_defs_without_authorized_workspace() {
        let registry = ToolRegistry::new();
        register_builtin_tools(&registry).await;

        let defs = build_visible_tool_defs(&registry, false, ToolSchemaFilter::None).await;
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
        let allowed = std::collections::HashSet::from([
            "Grep".to_string(),
            "Read".to_string(),
        ]);

        let defs = build_visible_tool_defs(
            &registry,
            false,
            ToolSchemaFilter::EmployeeWhitelist(allowed),
        )
        .await;
        let names: Vec<&str> = defs.iter().map(|def| def.name.as_str()).collect();

        // read_workspace_file is in WORKSPACE_TOOL_NAMES so it gets filtered out first;
        // grep_content is also a workspace tool so both are excluded without auth.
        assert!(names.is_empty(), "all allowed tools are workspace-scoped, expect empty after double filter; got {:?}", names);
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
