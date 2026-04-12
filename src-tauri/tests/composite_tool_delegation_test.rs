use app_lib::runtime::tools::catalog::ToolCatalog;
use app_lib::runtime::tools::definition::ToolKind;

#[test]
fn composite_tools_are_composite_kind() {
    let catalog = ToolCatalog::default_catalog();
    let composite_ids = [
        "browse_data", "generate_report", "export_data",
        "generate_chart", "generate_slides", "browse_and_extract",
    ];
    for id in &composite_ids {
        let def = catalog.get(id).unwrap_or_else(|| panic!("{} not in catalog", id));
        assert!(
            matches!(def.kind, ToolKind::Composite),
            "{} should be Composite kind, got {:?}",
            id, def.kind
        );
    }
}

#[test]
fn composite_tools_description_signals_composite_nature() {
    let catalog = ToolCatalog::default_catalog();
    let composite_ids = [
        "browse_data", "generate_report", "export_data",
        "generate_chart", "generate_slides", "browse_and_extract",
    ];
    for id in &composite_ids {
        let def = catalog.get(id).unwrap_or_else(|| panic!("{} not in catalog", id));
        assert!(
            def.description.contains("Composite") || def.description.contains("composite") || def.description.contains("【"),
            "Composite tool '{}' description should signal its composite nature. Got: {}",
            id, def.description
        );
    }
}

#[test]
fn browse_data_capability_scope_includes_browser_and_workspace_write() {
    let catalog = ToolCatalog::default_catalog();
    let def = catalog.get("browse_data").expect("browse_data must be in catalog");
    assert!(
        def.capability_scope.iter().any(|s| s == "browser"),
        "browse_data must require browser scope"
    );
    assert!(
        def.capability_scope.iter().any(|s| s == "workspace:write"),
        "browse_data must require workspace:write scope"
    );
}

#[test]
fn execute_python_is_power_with_correct_scopes() {
    let catalog = ToolCatalog::default_catalog();
    let def = catalog.get("execute_python").expect("execute_python must be in catalog");
    assert!(matches!(def.kind, ToolKind::Power), "execute_python must be Power kind");
    assert!(
        def.capability_scope.iter().any(|s| s == "python:exec"),
        "execute_python must require python:exec scope"
    );
    assert!(
        def.capability_scope.iter().any(|s| s == "workspace:write"),
        "execute_python must require workspace:write scope"
    );
}

#[test]
fn generate_report_requires_workspace_write() {
    let catalog = ToolCatalog::default_catalog();
    let def = catalog.get("generate_report").expect("generate_report must be in catalog");
    assert!(
        def.capability_scope.iter().any(|s| s == "workspace:write"),
        "generate_report must require workspace:write scope"
    );
}
