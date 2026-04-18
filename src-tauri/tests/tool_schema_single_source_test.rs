use app_lib::runtime::tools::catalog::{ToolCatalog, TOOL_CATALOG};
use app_lib::runtime::tools::definition::ToolKind;

#[test]
fn catalog_contains_all_registered_tools() {
    let required = vec![
        "list_directory", "read_workspace_file", "search_files", "get_file_info",
        "write_file", "edit_file", "bash", "grep_content",
        "web_search", "browse_navigate", "read_page_content", "page_execute_js",
        "extract_table_data", "extract_with_pagination", "load_file",
        "execute_python",
        "browse_data", "generate_report", "export_data",
    ];
    let catalog = ToolCatalog::default_catalog();
    for name in &required {
        assert!(
            catalog.get(name).is_some(),
            "Tool '{}' not found in catalog",
            name
        );
    }
}

#[test]
fn execute_python_is_power_in_catalog() {
    let catalog = ToolCatalog::default_catalog();
    let def = catalog.get("execute_python").expect("execute_python must be in catalog");
    assert!(matches!(def.kind, ToolKind::Power), "execute_python must be Power kind");
}

#[test]
fn browse_data_is_composite_in_catalog() {
    let catalog = ToolCatalog::default_catalog();
    let def = catalog.get("browse_data").expect("browse_data must be in catalog");
    assert!(matches!(def.kind, ToolKind::Composite), "browse_data must be Composite kind");
}

#[test]
fn workspace_tools_are_primitive_in_catalog() {
    let catalog = ToolCatalog::default_catalog();
    for name in &[
        "list_directory",
        "read_workspace_file",
        "search_files",
        "get_file_info",
        "write_file",
        "edit_file",
        "bash",
        "grep_content",
    ] {
        let def = catalog.get(name).unwrap_or_else(|| panic!("{} must be in catalog", name));
        assert!(
            matches!(def.kind, ToolKind::Primitive),
            "{} must be Primitive kind, got {:?}",
            name, def.kind
        );
    }
}

#[test]
fn catalog_tool_ids_have_no_duplicates() {
    let catalog = ToolCatalog::default_catalog();
    let ids = catalog.all_ids();
    let mut sorted = ids.clone();
    sorted.sort();
    sorted.dedup();
    assert_eq!(sorted.len(), ids.len(), "Catalog must not contain duplicate tool IDs");
}
