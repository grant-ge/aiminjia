#![allow(deprecated)]

use std::sync::Arc;

use app_lib::plugin::builtin::tools::register_builtin_tools;
use app_lib::plugin::registry::{RequestScopedRuntimeDeps, ToolRegistry};
use app_lib::python::session::{migrate_loaded_keys_to_run_scope, session_key_for_run};
use app_lib::runtime::cancellation::CancellationToken;
use app_lib::runtime::ids::{RunId, SessionId};
use app_lib::storage::file_manager::FileManager;
use app_lib::storage::file_store::AppStorage;
use serde_json::json;
use tempfile::TempDir;

#[test]
fn python_sessions_are_scoped_by_run_id_and_legacy_loaded_keys_are_migrated() {
    let parent_key = session_key_for_run(&RunId::new("run-parent"));
    let child_key = session_key_for_run(&RunId::new("run-child"));
    let migrated = migrate_loaded_keys_to_run_scope("conv-1", &RunId::new("run-child"));
    assert_ne!(parent_key, child_key);
    assert_eq!(migrated.source_prefix, "loaded:conv-1");
    assert_eq!(migrated.target_prefix, "loaded:run-child");
}

fn build_test_plugin_ctx(
    workspace_path: std::path::PathBuf,
    conversation_id: &str,
    run_id: Option<RunId>,
) -> app_lib::plugin::context::PluginContext {
    let storage =
        Arc::new(AppStorage::new(&workspace_path).expect("AppStorage::new should succeed"));
    storage
        .create_conversation(conversation_id, "Test Conversation")
        .expect("conversation should be created");

    app_lib::plugin::context::PluginContext {
        storage,
        file_manager: Arc::new(FileManager::new(&workspace_path)),
        workspace_path: workspace_path.clone(),
        conversation_id: conversation_id.to_string(),
        session_id: SessionId::new(conversation_id),
        run_id,
        agent_id: None,
        tavily_api_key: None,
        bocha_api_key: None,
        app_handle: None,
        session_manager: Arc::new(app_lib::python::session::PythonSessionManager::new(
            workspace_path,
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
        skill_sessions: None,
        authorized_workspace: None,
        read_file_state: None,
        cancellation: None,
        permission_mode: app_lib::runtime::tools::permission::PermissionMode::Default,
    }
}

#[tokio::test]
async fn analysis_execute_python_requires_run_id() {
    let tmp = TempDir::new().expect("TempDir::new should succeed");
    let conversation_id = "conv-analysis-missing-run";
    let ctx = build_test_plugin_ctx(tmp.path().to_path_buf(), conversation_id, None);
    ctx.storage
        .upsert_analysis_state(
            conversation_id,
            1,
            r#"{"step1_status":"in_progress"}"#,
            r#"{}"#,
        )
        .expect("analysis state should be created");

    let registry = ToolRegistry::new();
    register_builtin_tools(&registry).await;

    let result = registry
        .execute(
            "execute_python",
            &RequestScopedRuntimeDeps::from_plugin_context(&ctx),
            json!({"code": "print('hello from analysis')"}),
            CancellationToken::new(),
        )
        .await;

    let err = result.expect_err("analysis execute_python without run_id must fail");
    let message = err.to_string();
    assert!(
        message.contains("run_id"),
        "error must mention missing run_id, got: {}",
        message
    );
}
