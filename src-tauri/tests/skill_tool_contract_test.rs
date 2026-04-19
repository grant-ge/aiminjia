//! 验证 skill/workflow 中引用的工具名都能在 ToolCatalog 中解析到。
//!
//! 修改 skill 配置中的工具名时，必须同步更新这里的常量列表。

use app_lib::runtime::tools::catalog::ToolCatalog;

/// daily assistant skill 允许的工具集（需与 plugin/builtin/skills/daily_assistant.rs 同步）。
const DAILY_ALLOWED_TOOLS: &[&str] = &[
    "bash",
    "read_workspace_file",
    "write_file",
    "edit_file",
    "list_directory",
    "search_files",
    "get_file_info",
    "grep_content",
];

#[test]
fn daily_skill_allowed_tools_all_exist_in_catalog() {
    let catalog = ToolCatalog::default_catalog();
    let missing: Vec<&&str> = DAILY_ALLOWED_TOOLS
        .iter()
        .filter(|name| catalog.get(name).is_none())
        .collect();
    assert!(
        missing.is_empty(),
        "Tools referenced in daily skill but not in catalog: {:?}",
        missing
    );
}

#[test]
fn daily_skill_allowed_tools_match_runtime_constant() {
    assert_eq!(
        DAILY_ALLOWED_TOOLS,
        app_lib::runtime::tools::catalog::DAILY_ALLOWED_TOOLS,
        "tests/skill_tool_contract_test.rs must stay in sync with runtime catalog"
    );
}
