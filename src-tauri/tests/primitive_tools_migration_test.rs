//! Verify all 11 primitive tools are registered in catalog as RuntimeTool structs.
//!
//! These tests do NOT execute tools (that would require live browser / Python
//! environment).  They only assert that:
//!   1. Every primitive tool ID exists in the default catalog.
//!   2. Each entry carries the correct `ToolKind::Primitive`.
//!   3. The browser tools carry the `browser` capability scope.
//!   4. `web_search` carries the `network` scope.
//!   5. `load_file` carries the `workspace:read` scope.

use app_lib::runtime::tools::catalog::ToolCatalog;
use app_lib::runtime::tools::definition::ToolKind;

/// All 10 primitive tools must be in catalog as Primitive kind.
#[test]
fn all_primitive_tools_in_catalog() {
    let catalog = ToolCatalog::default_catalog();
    let primitives = [
        "list_directory",
        "read_workspace_file",
        "search_files",
        "get_file_info",
        "web_search",
        "browse_navigate",
        "read_page_content",
        "page_execute_js",
        "extract_table_data",
        "extract_with_pagination",
    ];
    for id in &primitives {
        let def = catalog
            .get(id)
            .unwrap_or_else(|| panic!("{} not in catalog", id));
        assert!(
            matches!(def.kind, ToolKind::Primitive),
            "{} should be Primitive, got {:?}",
            id,
            def.kind
        );
    }
}

/// Browser tools must carry the `browser` capability scope.
#[test]
fn browser_runtime_tools_have_correct_definition() {
    let catalog = ToolCatalog::default_catalog();
    for id in &[
        "browse_navigate",
        "read_page_content",
        "page_execute_js",
        "extract_table_data",
        "extract_with_pagination",
    ] {
        let def = catalog
            .get(id)
            .unwrap_or_else(|| panic!("{} must be in catalog", id));
        assert!(
            matches!(def.kind, ToolKind::Primitive),
            "{} should be Primitive",
            id
        );
        assert!(
            def.capability_scope.contains(&"browser".to_string()),
            "{} must have browser scope",
            id
        );
    }
}

/// `web_search` must be Primitive with `network` scope.
#[test]
fn web_search_is_primitive_with_network_scope() {
    let catalog = ToolCatalog::default_catalog();
    let def = catalog
        .get("web_search")
        .expect("web_search must be in catalog");
    assert!(matches!(def.kind, ToolKind::Primitive));
    assert!(
        def.capability_scope.contains(&"network".to_string()),
        "web_search must have network scope"
    );
}

/// `load_file` must be Power with `workspace:read`, `workspace:write`, `python:exec` scope.
#[test]
fn load_file_is_power_with_correct_scopes() {
    let catalog = ToolCatalog::default_catalog();
    let def = catalog
        .get("load_file")
        .expect("load_file must be in catalog");
    assert!(matches!(def.kind, ToolKind::Power));
    assert!(
        def.capability_scope.contains(&"workspace:read".to_string()),
        "load_file must have workspace:read scope"
    );
    assert!(
        def.capability_scope.contains(&"workspace:write".to_string()),
        "load_file must have workspace:write scope"
    );
    assert!(
        def.capability_scope.contains(&"python:exec".to_string()),
        "load_file must have python:exec scope"
    );
}

/// Workspace primitive tools must carry `workspace:read` scope.
#[test]
fn workspace_primitives_have_correct_scope() {
    let catalog = ToolCatalog::default_catalog();
    for id in &[
        "list_directory",
        "read_workspace_file",
        "search_files",
        "get_file_info",
    ] {
        let def = catalog
            .get(id)
            .unwrap_or_else(|| panic!("{} must be in catalog", id));
        assert!(
            matches!(def.kind, ToolKind::Primitive),
            "{} should be Primitive",
            id
        );
        assert!(
            def.capability_scope.contains(&"workspace:read".to_string()),
            "{} must have workspace:read scope",
            id
        );
    }
}

/// Module-level smoke test: network, browser, and file modules are reachable.
#[test]
fn builtin_modules_compile() {
    // If these use-statements compile, the modules are correctly wired in mod.rs.
    use app_lib::runtime::tools::builtin::browser::BrowserDeps;
    use app_lib::runtime::tools::builtin::file::LoadFileDeps;
    use app_lib::runtime::tools::builtin::network::SearchDeps;
    // Confirm the Deps structs are constructible (field names exist).
    let _ = SearchDeps {
        tavily_api_key: None,
        bocha_api_key: None,
        use_cloud: false,
        auth_manager: None,
    };
    // BrowserDeps and LoadFileDeps require Arc<ConnectorEngine> / Arc<AppStorage>
    // which need real paths — construction is tested in integration tests.
    // Here we only verify the type names resolve.
    let _: fn() -> Option<BrowserDeps> = || None;
    let _: fn() -> Option<LoadFileDeps> = || None;
}
