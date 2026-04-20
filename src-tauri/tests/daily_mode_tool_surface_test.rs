//! 验证 daily 模式不再默认暴露所有工具，composite 工具不能是 Primitive。

use app_lib::runtime::tools::catalog::ToolCatalog;
use app_lib::runtime::tools::definition::ToolKind;

#[test]
fn composite_tools_are_not_primitive() {
    let catalog = ToolCatalog::default_catalog();
    let composite_ids = [
        "browse_data",
        "generate_report",
        "export_data",
        "generate_chart",
    ];
    for id in &composite_ids {
        let def = catalog
            .get(id)
            .unwrap_or_else(|| panic!("{} must be in catalog", id));
        assert!(
            !matches!(def.kind, ToolKind::Primitive),
            "Composite tool '{}' must NOT be Primitive kind",
            id
        );
    }
}

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
    for id in [
        "save_memory",
        "search_memory",
        "core_memory",
        "distill_memory",
    ] {
        assert!(
            catalog.get(id).is_none(),
            "retired memory tool '{}' must not remain in TOOL_CATALOG",
            id
        );
    }
}
