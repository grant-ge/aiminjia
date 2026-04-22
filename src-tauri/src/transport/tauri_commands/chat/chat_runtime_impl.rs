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
    "list_directory",
    "read_workspace_file",
    "search_files",
    "get_file_info",
    "write_file",
    "edit_file",
    "bash",
    "grep_content",
];

pub(crate) async fn build_visible_tool_defs(
    registry: &ToolRegistry,
    has_authorized_workspace: bool,
    allowed_tools: Option<&std::collections::HashSet<String>>,
) -> Vec<crate::llm::streaming::ToolDefinition> {
    let defs = if has_authorized_workspace {
        registry.get_schemas_filtered(&ToolFilter::All).await
    } else {
        registry
            .get_schemas_filtered(&ToolFilter::Exclude(
                WORKSPACE_TOOL_NAMES
                    .iter()
                    .map(|name| name.to_string())
                    .collect(),
            ))
            .await
    };

    match allowed_tools {
        Some(allowed) => defs
            .into_iter()
            .filter(|def| allowed.contains(def.name.as_str()))
            .collect(),
        None => defs,
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

    // 未绑定工作目录时，fallback 到 ~/.renlijia/defaultFolder/。
    let default_path = crate::storage::aijia_home::AiJiaHome::from_home().default_folder();
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
    file_attachments: &[serde_json::Value],
    has_authorized_workspace: bool,
) -> String {
    if file_attachments.is_empty() {
        return content.to_string();
    }

    let file_refs: Vec<String> = file_attachments
        .iter()
        .map(|file| {
            let name = file["originalName"].as_str().unwrap_or("unknown");
            let file_type = file["fileType"].as_str().unwrap_or("unknown");
            let file_id = file["id"].as_str().unwrap_or("");
            format!("- {} (file_id: \"{}\", 类型: {})", name, file_id, file_type)
        })
        .collect();

    let hint = if has_authorized_workspace {
        "提示：对这些已上传文件请调用 load_file(file_id) 加载数据；对已连接本地目录请优先使用 list_directory / read_workspace_file / search_files / get_file_info，需要计算或生成文件时再结合 execute_python。"
    } else {
        "提示：对这些已上传文件请先调用 load_file(file_id) 加载文件数据，然后在 execute_python 中直接使用 _df（表格数据）或 _text（文本）变量。"
    };

    format!(
        "{}\n\n[已上传文件]\n{}\n\n{}",
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
    use serde_json::json;

    #[tokio::test]
    async fn test_build_visible_tool_defs_with_authorized_workspace() {
        let registry = ToolRegistry::new();
        register_builtin_tools(&registry).await;

        let defs = build_visible_tool_defs(&registry, true, None).await;
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

        let defs = build_visible_tool_defs(&registry, false, None).await;
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
            names.contains(&"execute_python"),
            "non-workspace tools should remain visible without authorization"
        );
    }

    #[tokio::test]
    async fn test_build_visible_tool_defs_applies_allowed_tools_filter_after_workspace_filter() {
        let registry = ToolRegistry::new();
        register_builtin_tools(&registry).await;
        let allowed = std::collections::HashSet::from([
            "execute_python".to_string(),
            "list_directory".to_string(),
        ]);

        let defs = build_visible_tool_defs(&registry, false, Some(&allowed)).await;
        let names: Vec<&str> = defs.iter().map(|def| def.name.as_str()).collect();

        assert_eq!(names, vec!["execute_python"]);
    }

    #[test]
    fn test_build_llm_content_with_authorized_workspace_scopes_load_file_to_uploads() {
        let attachments = vec![json!({
            "id": "file-1",
            "originalName": "sales.xlsx",
            "fileType": "xlsx"
        })];

        let content = build_llm_content("请分析这个目录里的销售数据", &attachments, true);

        assert!(content.contains("[已上传文件]"));
        assert!(content.contains("对这些已上传文件请调用 load_file(file_id)"));
        assert!(content.contains(
            "对已连接本地目录请优先使用 list_directory / read_workspace_file / search_files / get_file_info"
        ));
        assert!(!content.contains("提示：先调用 load_file(file_id) 加载文件数据"));
    }
}
