//! Verify that runtime-native workspace tools are registered at startup
//! and that ToolRegistry::execute() dispatches to them (not legacy ToolPlugin).

#![allow(deprecated)]

use app_lib::plugin::builtin::tools::register_builtin_tools;
use app_lib::plugin::registry::ToolRegistry;
use std::sync::Arc;
use tempfile::TempDir;

// ─── Helper ─────────────────────────────────────────────────────────────────

fn build_test_plugin_ctx(
    workspace_path: std::path::PathBuf,
) -> app_lib::plugin::context::PluginContext {
    let storage = Arc::new(
        app_lib::storage::file_store::AppStorage::new(&workspace_path)
            .expect("AppStorage::new failed"),
    );
    let file_manager =
        Arc::new(app_lib::storage::file_manager::FileManager::new(&workspace_path));
    let session_manager = Arc::new(app_lib::python::session::PythonSessionManager::new(
        workspace_path.clone(),
        None,
    ));
    #[allow(deprecated)]
    app_lib::plugin::context::PluginContext {
        storage,
        file_manager,
        workspace_path: workspace_path.clone(),
        conversation_id: "test-conv".to_string(),
        session_id: app_lib::runtime::ids::SessionId::new("test-conv"),
        run_id: None,
        agent_id: None,
        tavily_api_key: None,
        bocha_api_key: None,
        app_handle: None,
        session_manager,
        auth_manager: None,
        connector_engine: None,
        use_cloud: false,
        model: String::new(),
        gateway: None,
        tool_registry: None,
        app_settings: None,
        agent_runtime: None,
        event_bus: None,
        authorized_workspace: None,
    }
}

// ─── Test 1: register_builtin_tools registers workspace RuntimeTools ─────────

/// register_builtin_tools should call register_runtime for the four workspace
/// RuntimeTool implementations.  After registration the runtime_tools map
/// must contain all four names.
#[tokio::test]
async fn register_builtin_tools_registers_workspace_runtime_tools() {
    let registry = ToolRegistry::new();
    register_builtin_tools(&registry).await;

    let tmp = TempDir::new().unwrap();
    std::fs::write(tmp.path().join("test.csv"), b"col\n1\n").unwrap();

    let ctx = build_test_plugin_ctx(tmp.path().to_path_buf());

    // list_directory should now be routed to the RuntimeTool (not legacy).
    // The tool requires workspace capability; our ctx has workspace_path set,
    // which is enough to satisfy the CapabilityPermissionPipeline check.
    let result = registry
        .execute("list_directory", &ctx, serde_json::json!({"path": "."}))
        .await;
    assert!(
        result.is_ok(),
        "list_directory should succeed via runtime tool: {:?}",
        result
    );
    let output = result.unwrap();
    assert!(
        output.content.contains("test.csv"),
        "Should list test.csv, got: {}",
        output.content
    );
}

// ─── Test 2: all four workspace tools are reachable via execute() ────────────

#[tokio::test]
async fn all_four_workspace_runtime_tools_are_registered() {
    let registry = ToolRegistry::new();
    register_builtin_tools(&registry).await;

    // Verify all four tools appear in get_all_schemas()
    let schemas = registry.get_all_schemas().await;
    let names: Vec<&str> = schemas.iter().map(|s| s.name.as_str()).collect();

    for tool_name in &[
        "list_directory",
        "read_workspace_file",
        "search_files",
        "get_file_info",
    ] {
        assert!(
            names.contains(tool_name),
            "Expected '{}' in schemas, got: {:?}",
            tool_name,
            names
        );
    }
}

// ─── Test 3: execute() routes to RuntimeTool, not legacy ToolPlugin ──────────

/// Confirm that ToolRegistry::execute() dispatches to the RuntimeTool path
/// (not the legacy path) by checking the output format.
/// RuntimeTool produces JSON content; legacy ToolPlugin produces plain text.
#[tokio::test]
async fn execute_dispatches_to_runtime_tool_not_legacy() {
    let registry = ToolRegistry::new();
    register_builtin_tools(&registry).await;

    let tmp = TempDir::new().unwrap();
    let ctx = build_test_plugin_ctx(tmp.path().to_path_buf());

    let result = registry
        .execute("list_directory", &ctx, serde_json::json!({"path": "."}))
        .await;
    assert!(
        result.is_ok(),
        "execute should succeed for list_directory: {:?}",
        result
    );
    let output = result.unwrap();
    // RuntimeTool produces JSON-formatted content (via tool_result())
    let parsed: serde_json::Value =
        serde_json::from_str(&output.content).expect("RuntimeTool should return valid JSON");
    assert!(
        parsed.get("files").is_some(),
        "Expected 'files' key in JSON output from RuntimeTool, got: {}",
        output.content
    );
}

// ─── Test 4: to_runtime_dispatcher uses CapabilityPermissionPipeline ─────────

/// Verify that to_runtime_dispatcher() wraps tools with CapabilityPermissionPipeline.
/// A workspace RuntimeTool dispatched WITHOUT capability context should be
/// rejected (workspace:read scope requires storage capability).
#[tokio::test]
async fn to_runtime_dispatcher_uses_capability_permission_pipeline() {
    use app_lib::runtime::tools::builtin::workspace::ListDirectoryRuntimeTool;

    let registry = ToolRegistry::new();
    registry
        .register_runtime(Arc::new(ListDirectoryRuntimeTool))
        .await;

    let tmp = TempDir::new().unwrap();
    let ctx = build_test_plugin_ctx(tmp.path().to_path_buf());
    let dispatcher = registry.to_runtime_dispatcher(ctx).await;

    // Dispatch WITHOUT capability context → CapabilityPermissionPipeline should
    // reject the call because workspace:read requires storage capability.
    let exec_ctx = app_lib::runtime::tools::ToolExecutionContext::for_test(
        "test-conv",
        "run-1",
        "tc-1",
    );
    // No capability attached → permission denied
    let outcome = dispatcher
        .dispatch("list_directory", serde_json::json!({"path": "."}), exec_ctx)
        .await;
    assert!(
        outcome.is_err(),
        "list_directory without capability should be rejected by CapabilityPermissionPipeline"
    );
}
