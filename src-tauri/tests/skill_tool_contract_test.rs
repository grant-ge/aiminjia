//! 验证 skill/workflow 中引用的工具名都能在 ToolCatalog 中解析到。
//!
//! 修改 skill 配置中的工具名时，必须同步更新这里的常量列表。

use app_lib::runtime::tools::catalog::ToolCatalog;

/// daily assistant skill 允许的工具集（需与 plugin/builtin/skills/daily_assistant.rs 同步）。
const DAILY_ALLOWED_TOOLS: &[&str] = &[
    "list_directory",
    "read_workspace_file",
    "search_files",
    "get_file_info",
    "write_file",
    "edit_file",
    "bash",
    "grep_content",
    "web_search",
    "browse_navigate",
    "read_page_content",
    "load_file",
    "execute_python",
    "browse_data",
    "generate_report",
    "generate_chart",
    "export_data",
    "plan_update",
    "progress_update",
    "save_analysis_note",
    "save_memory",
    "search_memory",
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

/// analysis skill 允许的工具集。
const ANALYSIS_ALLOWED_TOOLS: &[&str] = &[
    "load_file",
    "execute_python",
    "generate_report",
    "generate_chart",
    "export_data",
    "hypothesis_test",
    "detect_anomalies",
    "save_analysis_note",
    "progress_update",
    "web_search",
];

#[test]
fn analysis_skill_allowed_tools_all_exist_in_catalog() {
    let catalog = ToolCatalog::default_catalog();
    let missing: Vec<&&str> = ANALYSIS_ALLOWED_TOOLS
        .iter()
        .filter(|name| catalog.get(name).is_none())
        .collect();
    assert!(
        missing.is_empty(),
        "Tools referenced in analysis skill but not in catalog: {:?}",
        missing
    );
}
