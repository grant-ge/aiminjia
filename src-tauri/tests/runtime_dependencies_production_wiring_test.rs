use std::path::PathBuf;
use std::sync::Arc;

use app_lib::llm::sub_agent::SubAgentRuntimeDeps;
use app_lib::plugin::context::PluginContext;
use app_lib::plugin::registry::RequestScopedRuntimeDeps;
use app_lib::runtime::dependencies::{
    InstalledRuntimeResolver, RuntimeDependencyError, RuntimeInstallPlan, RuntimeInstaller,
    RuntimePaths, RuntimeResolver, StaticRuntimeResolver, WorkspaceDependencies,
};
use app_lib::runtime::ids::{RunId, SessionId};
use app_lib::runtime::tools::permission::PermissionMode;
use app_lib::storage::file_manager::FileManager;
use app_lib::storage::file_store::AppStorage;
use tempfile::TempDir;

fn managed_runtime_resolver() -> Arc<dyn RuntimeResolver> {
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

    RequestScopedRuntimeDeps {
        storage,
        file_manager: Arc::new(FileManager::new(workspace)),
        workspace_path: workspace.to_path_buf(),
        conversation_id: "runtime-production-wiring".to_string(),
        session_id: SessionId::new("runtime-production-wiring"),
        run_id: Some(RunId::new("parent-run")),
        agent_id: None,
        tavily_api_key: None,
        bocha_api_key: None,
        app_handle: None,
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
        permission_ctx: None,
        current_persona_id: None,
    }
}

fn plugin_context(workspace: &std::path::Path) -> PluginContext {
    let request = request_scoped_deps(workspace);
    PluginContext {
        storage: request.storage,
        file_manager: request.file_manager,
        workspace_path: request.workspace_path,
        conversation_id: request.conversation_id,
        session_id: request.session_id,
        run_id: request.run_id,
        agent_id: request.agent_id,
        tavily_api_key: request.tavily_api_key,
        bocha_api_key: request.bocha_api_key,
        app_handle: request.app_handle,
        auth_manager: request.auth_manager,
        connector_engine: request.connector_engine,
        dingtalk_bridge: None,
        use_cloud: request.use_cloud,
        model: request.model,
        gateway: request.gateway,
        tool_registry: request.tool_registry,
        app_settings: request.app_settings,
        agent_runtime: request.agent_runtime,
        event_bus: request.event_bus,
        skill_registry: request.skill_registry,
        authorized_workspace: request.authorized_workspace,
        read_file_state: request.read_file_state,
        cancellation: request.cancellation,
        permission_mode: request.permission_mode,
        runtime_resolver: request.runtime_resolver,
        permission_ctx: None,
        current_persona_id: None,
    }
}

#[test]
fn subagent_runtime_deps_preserve_managed_runtime_resolver() {
    let workspace = TempDir::new().expect("tempdir should exist");
    let parent = request_scoped_deps(workspace.path());

    let subagent_deps = SubAgentRuntimeDeps {
        storage: parent.storage.clone(),
        file_manager: parent.file_manager.clone(),
        workspace_path: parent.workspace_path.clone(),
        conversation_id: parent.conversation_id.clone(),
        session_id: parent.session_id.clone(),
        run_id: parent.run_id.clone(),
        agent_id: parent.agent_id.clone(),
        connector_engine: parent.connector_engine.clone(),
        agent_runtime: parent.agent_runtime.clone(),
        event_bus: parent.event_bus.clone(),
        authorized_workspace: parent.authorized_workspace.clone(),
        read_file_state: parent.read_file_state.clone(),
        app_handle: parent.app_handle.clone(),
        runtime_resolver: parent.runtime_resolver.clone(),
        skill_registry: None,
        permission_ctx: None,
        current_persona_id: None,
    };

    let child = subagent_deps.request_scoped_tool_deps(RunId::new("child-run"), None, None, None);

    let deps = child
        .runtime_resolver
        .expect("sub-agent should retain runtime resolver")
        .workspace_dependencies()
        .expect("managed dependencies should resolve");
    assert_eq!(
        deps.python,
        PathBuf::from("/tmp/renlijia-managed-python/bin/python3")
    );
}

#[test]
fn plugin_context_bridge_preserves_managed_runtime_resolver() {
    let workspace = TempDir::new().expect("tempdir should exist");
    let ctx = plugin_context(workspace.path());

    let request = RequestScopedRuntimeDeps::from_plugin_context(&ctx);

    let deps = request
        .runtime_resolver
        .expect("legacy PluginContext bridge should retain runtime resolver")
        .workspace_dependencies()
        .expect("managed dependencies should resolve");
    assert_eq!(
        deps.python,
        PathBuf::from("/tmp/renlijia-managed-python/bin/python3")
    );
}

#[test]
fn installed_runtime_resolver_reads_current_pointer_layout() {
    let temp = TempDir::new().expect("tempdir should exist");
    let paths = RuntimePaths::new(temp.path().to_path_buf(), "renlijia-primary-runtime")
        .expect("valid runtime paths");
    let version_dir = paths.version_dir("2026.04.25").expect("version dir");
    RuntimeInstaller::new(paths.clone())
        .ensure(RuntimeInstallPlan::already_local("2026.04.25"))
        .expect("installer should create payload and current pointer");

    let resolver = InstalledRuntimeResolver::new(paths.bundle_root());
    let deps = resolver
        .workspace_dependencies()
        .expect("installed runtime paths should resolve from current pointer");

    assert_eq!(
        deps,
        WorkspaceDependencies::from_install_dir(&version_dir).unwrap()
    );
    assert_eq!(deps.python, version_dir.join("python/bin/python3"));
    assert_eq!(deps.node, version_dir.join("node/bin/node"));
}

#[test]
fn installed_runtime_resolver_rejects_missing_current_pointer() {
    let temp = TempDir::new().expect("tempdir should exist");
    let bundle_root = temp.path().join("renlijia-primary-runtime");
    std::fs::create_dir_all(&bundle_root).expect("bundle root should be created");

    let error = InstalledRuntimeResolver::new(bundle_root)
        .workspace_dependencies()
        .expect_err("missing current pointer should fail clearly");

    assert!(matches!(
        error,
        RuntimeDependencyError::ResolverUnavailable(_)
    ));
}
