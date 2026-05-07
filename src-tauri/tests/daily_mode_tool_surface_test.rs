//! 验证 daily 模式工具集合的约束。

use app_lib::runtime::tools::catalog::{ToolCatalog, DAILY_ALLOWED_TOOLS};
use app_lib::runtime::tools::definition::ToolKind;

#[test]
fn all_catalog_tools_have_valid_kind() {
    let catalog = ToolCatalog::default_catalog();
    // 确认所有工具都有 kind（编译期保证，但 runtime 验证一遍）
    for id in catalog.all_ids() {
        let def = catalog.get(&id).unwrap();
        // 任何 ToolKind variant 都是合法的——此测试只确保没有 panic
        let _kind = &def.kind;
    }
}

#[test]
fn retired_memory_tools_are_not_in_daily_catalog() {
    let catalog = ToolCatalog::default_catalog();
    for id in ["save_memory", "load_core_memory", "distill_memories"] {
        assert!(
            catalog.get(id).is_none(),
            "retired memory tool '{}' must not remain in TOOL_CATALOG",
            id
        );
    }
}

#[test]
fn runtime_memory_tools_are_in_catalog_and_daily_allowlist() {
    let catalog = ToolCatalog::default_catalog();

    for id in ["write_memory", "search_memory"] {
        assert!(
            catalog.get(id).is_some(),
            "runtime memory tool '{}' should be present in TOOL_CATALOG",
            id
        );
    }

    assert!(
        DAILY_ALLOWED_TOOLS.contains(&"write_memory"),
        "write_memory should be available in DAILY_ALLOWED_TOOLS"
    );
    assert!(
        DAILY_ALLOWED_TOOLS.contains(&"search_memory"),
        "search_memory should be available in DAILY_ALLOWED_TOOLS"
    );
}
