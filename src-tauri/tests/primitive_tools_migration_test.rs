//! Verify primitive tools are registered in catalog as RuntimeTool structs.
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

/// Primitive runtime tools must be in catalog as Primitive kind.
#[test]
fn all_primitive_tools_in_catalog() {
    let catalog = ToolCatalog::default_catalog();
    let primitives = [
        "read_workspace_file",
        "search_files",
        "get_file_info",
        "grep_content",
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
        def.capability_scope
            .contains(&"workspace:write".to_string()),
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
        "read_workspace_file",
        "search_files",
        "get_file_info",
        "grep_content",
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
    use app_lib::runtime::tools::builtin::file::LoadFileRuntimeTool;
    use app_lib::runtime::tools::builtin::network::SearchDeps;
    // Confirm the Deps structs are constructible (field names exist).
    let _ = SearchDeps {
        tavily_api_key: None,
        bocha_api_key: None,
        use_cloud: false,
        auth_manager: None,
    };
    // BrowserDeps requires Arc<ConnectorEngine> which needs real paths —
    // construction is tested in integration tests.
    // Here we only verify the type names resolve.
    let _: fn() -> Option<BrowserDeps> = || None;
    // LoadFileRuntimeTool is stateless — its deps come from CapabilityContext.file_ops.
    let _ = LoadFileRuntimeTool::new();
}

// Task 3.2 tests

#[test]
fn execute_python_tool_is_registered_as_runtime_tool_in_request_scope() {
    use app_lib::runtime::tools::builtin::python::ExecutePythonRuntimeTool;
    use app_lib::runtime::tools::RuntimeTool;

    let tool = ExecutePythonRuntimeTool::stub();
    assert_eq!(tool.definition().id, "execute_python");
}

#[test]
fn execute_python_runtime_tool_has_correct_catalog_kind() {
    use app_lib::runtime::tools::catalog::ToolCatalog;
    use app_lib::runtime::tools::definition::ToolKind;

    let catalog = ToolCatalog::default_catalog();
    let def = catalog
        .get("execute_python")
        .expect("execute_python must be in catalog");
    assert!(matches!(def.kind, ToolKind::Power));
}

#[test]
fn execute_python_check_permissions_denies_dangerous_code() {
    use app_lib::runtime::tools::builtin::python::ExecutePythonRuntimeTool;
    use app_lib::runtime::tools::permission::PermissionDecision;
    use app_lib::runtime::tools::{RuntimeTool, ToolExecutionContext};
    use serde_json::json;

    let rt = tokio::runtime::Runtime::new().unwrap();
    let tool = ExecutePythonRuntimeTool::stub();
    let ctx = ToolExecutionContext::for_test("c", "r", "t");
    let input = json!({"code": "__import__('os').system('rm -rf /')"});
    let result = rt.block_on(tool.check_permissions(&input, &ctx));

    assert!(
        matches!(result, Some(PermissionDecision::Deny { .. })),
        "dangerous code should be denied by check_permissions"
    );
}

// Task 3.3 tests

#[test]
fn generate_report_tool_is_runtime_tool_type() {
    use app_lib::runtime::tools::builtin::report::GenerateReportRuntimeTool;
    use app_lib::runtime::tools::RuntimeTool;

    let tool = GenerateReportRuntimeTool::stub();
    assert_eq!(tool.definition().id, "generate_report");
    assert!(
        !tool.is_concurrency_safe(&serde_json::json!({})),
        "generate_report writes files, not concurrency safe"
    );
}

#[test]
fn generate_chart_tool_is_runtime_tool_type() {
    use app_lib::runtime::tools::builtin::chart::GenerateChartRuntimeTool;
    use app_lib::runtime::tools::RuntimeTool;

    let tool = GenerateChartRuntimeTool::stub();
    assert_eq!(tool.definition().id, "generate_chart");
    assert!(
        !tool.is_concurrency_safe(&serde_json::json!({})),
        "generate_chart writes files, not concurrency safe"
    );
}
