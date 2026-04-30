//! 验证 skill/workflow 中引用的工具名都能在 ToolCatalog 中解析到。
//!
//! 修改 skill 配置中的工具名时，运行时常量必须能被 ToolCatalog 解析。

use app_lib::runtime::tools::catalog::{ToolCatalog, DAILY_ALLOWED_TOOLS};

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
        "tests/skill_tool_contract_test.rs must derive from the runtime catalog"
    );
}
