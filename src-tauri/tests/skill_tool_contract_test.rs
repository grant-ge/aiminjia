//! 验证 skill/workflow 中引用的工具名都能在 ToolCatalog 中解析到。
//!
//! 修改 skill 配置中的工具名时，必须同步更新这里的常量列表。

use app_lib::runtime::tools::catalog::ToolCatalog;

/// daily assistant skill 允许的工具集（需与 plugin/builtin/skills/daily_assistant.rs 同步）。
const DAILY_ALLOWED_TOOLS: &[&str] = &[
    "web_search",
    "execute_python",
    "load_file",
    "list_directory",
    "read_workspace_file",
    "search_files",
    "get_file_info",
    "generate_report",
    "generate_chart",
    "export_data",
    "browse_navigate",
    "read_page_content",
    "browse_data",
    "save_analysis_note",
    "plan_update",
    "progress_update",
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
