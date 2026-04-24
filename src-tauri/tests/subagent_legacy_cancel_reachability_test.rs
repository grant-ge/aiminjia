#![allow(deprecated)]

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use app_lib::plugin::context::PluginContext;
use app_lib::plugin::registry::{RequestScopedRuntimeDeps, ToolRegistry};
use app_lib::plugin::tool_trait::{ToolError, ToolOutput, ToolPlugin};
use app_lib::runtime::cancellation::{CancellationReason, CancellationToken};
use app_lib::runtime::ids::{AgentId, RunId, SessionId};
use app_lib::storage::file_manager::FileManager;
use app_lib::storage::file_store::AppStorage;
use async_trait::async_trait;
use serde_json::Value;
use tempfile::TempDir;

struct BlockingLegacyTool;

#[async_trait]
impl ToolPlugin for BlockingLegacyTool {
    fn name(&self) -> &str {
        "blocking_legacy"
    }

    fn description(&self) -> &str {
        "waits until bridged cancellation is observed"
    }

    fn input_schema(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {}
        })
    }

    async fn execute(&self, ctx: &PluginContext, _input: Value) -> Result<ToolOutput, ToolError> {
        let cancellation = ctx
            .cancellation
            .clone()
            .expect("legacy plugin should receive bridged cancellation token");

        loop {
            if cancellation.is_cancelled() {
                return Ok(ToolOutput::success(format!(
                    "cancelled:{:?}",
                    cancellation
                        .reason()
                        .expect("cancelled token should carry a reason")
                )));
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }
}

#[allow(deprecated)]
fn make_plugin_ctx(workspace: &Path) -> PluginContext {
    let storage = Arc::new(AppStorage::new(workspace).expect("AppStorage::new failed"));
    let file_manager = Arc::new(FileManager::new(workspace));
    let session_manager = Arc::new(app_lib::python::session::PythonSessionManager::new(
        workspace.to_path_buf(),
        None,
    ));

    PluginContext {
        storage,
        file_manager,
        workspace_path: workspace.to_path_buf(),
        conversation_id: "subagent-cancel-conv".to_string(),
        session_id: SessionId::new("subagent-cancel-conv"),
        run_id: Some(RunId::new("child-run-cancel")),
        agent_id: Some(AgentId::new("child-agent-cancel")),
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
        skill_registry: None,
        skill_sessions: None,
        authorized_workspace: None,
        read_file_state: None,
        cancellation: None,
        permission_mode: app_lib::runtime::tools::permission::PermissionMode::Default,
    }
}

#[tokio::test]
async fn subagent_legacy_tool_observes_parent_cancel_via_registry_bridge() {
    let tmp = TempDir::new().expect("TempDir::new failed");
    let registry = ToolRegistry::new();
    registry
        .register(Arc::new(BlockingLegacyTool), "builtin")
        .await;

    let plugin_ctx = make_plugin_ctx(tmp.path());
    let parent = CancellationToken::new();
    let exec_cancel = parent.child_token();

    let cancel_task = tokio::spawn({
        let parent = parent.clone();
        async move {
            tokio::time::sleep(Duration::from_millis(50)).await;
            parent.cancel_with_reason(CancellationReason::Interrupt);
        }
    });

    let result = tokio::time::timeout(
        Duration::from_secs(1),
        registry.execute(
            "blocking_legacy",
            &RequestScopedRuntimeDeps::from_plugin_context(&plugin_ctx),
            serde_json::json!({}),
            exec_cancel,
        ),
    )
    .await
    .expect("legacy tool should finish once cancellation is bridged")
    .expect("legacy tool should return a bridged-cancel success payload");

    cancel_task.await.expect("cancel task should finish");
    assert_eq!(result.content, "cancelled:Interrupt");
}
