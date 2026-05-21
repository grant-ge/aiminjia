#![allow(deprecated)]

use std::sync::Arc;

use app_lib::plugin::builtin::tools::register_builtin_tools;
use app_lib::plugin::context::PluginContext;
use app_lib::plugin::registry::ToolRegistry;
use app_lib::runtime::ids::SessionId;
use app_lib::runtime::tools::catalog::TOOL_CATALOG;

fn build_test_plugin_ctx(workspace_path: std::path::PathBuf) -> PluginContext {
    let storage = Arc::new(
        app_lib::storage::file_store::AppStorage::new(&workspace_path)
            .expect("AppStorage::new failed"),
    );
    let file_manager = Arc::new(app_lib::storage::file_manager::FileManager::new(
        &workspace_path,
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
        auth_manager: None,
        dingtalk_bridge: None,
        use_cloud: false,
        model: String::new(),
        gateway: None,
        tool_registry: None,
        app_settings: None,
        agent_runtime: None,
        event_bus: None,
        skill_registry: None,
        authorized_workspace: None,
        read_file_state: None,
        cancellation: None,
        permission_mode: app_lib::runtime::tools::permission::PermissionMode::Default,
        runtime_resolver: None,
        permission_ctx: None,
        current_persona_id: None,
    }
}

#[tokio::test]
async fn request_scoped_web_search_schema_should_come_from_catalog() {
    let registry = ToolRegistry::new();
    register_builtin_tools(&registry).await;

    let schema = registry
        .get_all_schemas()
        .await
        .into_iter()
        .find(|s| s.name == "WebSearch")
        .expect("web_search schema missing");
    let catalog = TOOL_CATALOG
        .get_entry("WebSearch")
        .expect("web_search missing from TOOL_CATALOG");

    assert_eq!(
        schema.description, catalog.definition.description,
        "web_search schema description should come from ToolCatalog"
    );
    assert_eq!(
        schema.parameters, catalog.json_schema,
        "web_search JSON schema should come from ToolCatalog"
    );
}
