#![allow(deprecated)]

use std::sync::Arc;

use app_lib::plugin::builtin::tools::register_builtin_tools;
use app_lib::plugin::context::PluginContext;
use app_lib::plugin::registry::ToolRegistry;
use app_lib::plugin::skill_trait::ToolFilter;
use app_lib::runtime::ids::SessionId;
use app_lib::runtime::tools::catalog::TOOL_CATALOG;
use app_lib::runtime::tools::ToolExecutionContext;
use serde_json::json;
use tempfile::TempDir;

fn build_test_plugin_ctx(workspace_path: std::path::PathBuf) -> PluginContext {
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

    PluginContext {
        storage,
        file_manager,
        workspace_path: workspace_path.clone(),
        conversation_id: "review-conv".to_string(),
        session_id: SessionId::new("review-conv"),
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
        read_file_state: None,
    }
}

#[tokio::test]
async fn runtime_dispatcher_should_reject_browser_tool_without_browser_capability() {
    let registry = ToolRegistry::new();
    register_builtin_tools(&registry).await;

    let tmp = TempDir::new().unwrap();
    let plugin_ctx = build_test_plugin_ctx(tmp.path().to_path_buf());
    let dispatcher = registry.to_runtime_dispatcher(plugin_ctx).await;

    let exec_ctx = ToolExecutionContext::for_test("review-conv", "run-1", "tc-1");
    let err = match dispatcher
        .dispatch(
            "browse_navigate",
            json!({"url": "https://example.com"}),
            exec_ctx,
        )
        .await
    {
        Ok(_) => panic!("browse_navigate without browser capability should be rejected"),
        Err(err) => err,
    };

    let msg = err.to_string();
    assert!(
        msg.contains("browser capability"),
        "dispatcher should fail in permission layer, got: {msg}"
    );
}

#[tokio::test]
async fn request_scoped_web_search_schema_should_come_from_catalog() {
    let registry = ToolRegistry::new();
    register_builtin_tools(&registry).await;

    let schema = registry
        .get_all_schemas()
        .await
        .into_iter()
        .find(|s| s.name == "web_search")
        .expect("web_search schema missing");
    let catalog = TOOL_CATALOG
        .get_entry("web_search")
        .expect("web_search missing from TOOL_CATALOG");

    assert_eq!(
        schema.description,
        catalog.definition.description,
        "web_search schema description should come from ToolCatalog"
    );
    assert_eq!(
        schema.parameters,
        catalog.json_schema,
        "web_search JSON schema should come from ToolCatalog"
    );
}

#[tokio::test]
async fn request_scoped_load_file_filtered_schema_should_come_from_catalog() {
    let registry = ToolRegistry::new();
    register_builtin_tools(&registry).await;

    let schema = registry
        .get_schemas_filtered(&ToolFilter::Only(vec!["load_file".to_string()]))
        .await
        .into_iter()
        .find(|s| s.name == "load_file")
        .expect("load_file schema missing from filtered view");
    let catalog = TOOL_CATALOG
        .get_entry("load_file")
        .expect("load_file missing from TOOL_CATALOG");

    assert_eq!(
        schema.description,
        catalog.definition.description,
        "load_file filtered schema description should come from ToolCatalog"
    );
    assert_eq!(
        schema.parameters,
        catalog.json_schema,
        "load_file filtered JSON schema should come from ToolCatalog"
    );
}
