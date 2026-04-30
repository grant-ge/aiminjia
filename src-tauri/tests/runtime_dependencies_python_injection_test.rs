use std::path::PathBuf;
use std::sync::Arc;

use app_lib::plugin::builtin::tools::register_builtin_tools;
use app_lib::plugin::registry::{RequestScopedRuntimeDeps, ToolRegistry};
use app_lib::runtime::cancellation::CancellationToken;
use app_lib::runtime::dependencies::StaticRuntimeResolver;
use app_lib::runtime::ids::{RunId, SessionId};
use app_lib::runtime::tools::permission::PermissionMode;
use app_lib::storage::file_manager::FileManager;
use app_lib::storage::file_store::AppStorage;
use tempfile::TempDir;

fn managed_runtime_resolver() -> Arc<StaticRuntimeResolver> {
    Arc::new(StaticRuntimeResolver::new(
        PathBuf::from("/tmp/renlijia-managed-python/bin/python3"),
        PathBuf::from("/tmp/renlijia-managed-node/bin/node"),
        PathBuf::from("/tmp/renlijia-managed-node/bin/npm"),
        PathBuf::from("/tmp/renlijia-managed-node/bin/npx"),
        PathBuf::from("/tmp/renlijia-managed-uv/bin/uv"),
        PathBuf::from("/tmp/renlijia-managed-uv/bin/uvx"),
        PathBuf::from("/tmp/renlijia-managed-node/node_modules"),
        PathBuf::from("/tmp/renlijia-managed-python/site-packages"),
    ))
}

fn request_scoped_deps(workspace: &std::path::Path) -> RequestScopedRuntimeDeps {
    let storage = Arc::new(AppStorage::new(workspace).expect("storage should init"));
    storage
        .create_conversation("runtime-python-injection", "Runtime Python Injection")
        .expect("conversation should be created");

    RequestScopedRuntimeDeps {
        storage,
        file_manager: Arc::new(FileManager::new(workspace)),
        workspace_path: workspace.to_path_buf(),
        conversation_id: "runtime-python-injection".to_string(),
        session_id: SessionId::new("runtime-python-injection"),
        run_id: Some(RunId::new("managed-run")),
        agent_id: None,
        tavily_api_key: None,
        bocha_api_key: None,
        app_handle: None,
        session_manager: Arc::new(app_lib::python::session::PythonSessionManager::new(
            workspace.to_path_buf(),
            None,
        )),
        auth_manager: None,
        connector_engine: None,
        use_cloud: false,
        model: "test-model".to_string(),
        gateway: None,
        tool_registry: None,
        app_settings: None,
        agent_runtime: None,
        event_bus: None,
        skill_registry: None,
        authorized_workspace: None,
        read_file_state: None,
        cancellation: None,
        permission_mode: PermissionMode::Default,
        runtime_resolver: Some(managed_runtime_resolver()),
    }
}

#[tokio::test]
async fn execute_python_prefers_managed_runtime_resolver_python() {
    let workspace = TempDir::new().expect("tempdir should exist");
    let registry = ToolRegistry::new();
    register_builtin_tools(&registry).await;

    let result = registry
        .execute(
            "execute_python",
            &request_scoped_deps(workspace.path()),
            serde_json::json!({"code": "print('hello')"}),
            CancellationToken::new(),
        )
        .await;

    let message = result
        .expect_err("managed python path does not exist, so execution should fail")
        .to_string();
    assert!(
        message.contains("/tmp/renlijia-managed-python/bin/python3"),
        "execute_python should try resolver-provided python path, got: {message}"
    );
}

#[tokio::test]
async fn execute_python_without_runtime_resolver_fails_instead_of_falling_back_to_system_python() {
    let workspace = TempDir::new().expect("tempdir should exist");
    let registry = ToolRegistry::new();
    register_builtin_tools(&registry).await;
    let mut deps = request_scoped_deps(workspace.path());
    deps.runtime_resolver = None;

    let result = registry
        .execute(
            "execute_python",
            &deps,
            serde_json::json!({"code": "print('hello')"}),
            CancellationToken::new(),
        )
        .await;

    let message = result
        .expect_err("missing managed resolver should be surfaced")
        .to_string();
    assert!(
        message.contains("RequestScopedRuntimeDeps has no RuntimeResolver"),
        "execute_python must not fallback to system python, got: {message}"
    );
}
