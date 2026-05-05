#![allow(deprecated)]

use std::path::Path;
use std::sync::{Arc, Mutex};

use app_lib::plugin::context::PluginContext;
use app_lib::plugin::registry::{RequestScopedRuntimeDeps, ToolRegistry};
use app_lib::runtime::cancellation::CancellationToken;
use app_lib::runtime::chat::tool_round_types::RuntimeToolCallRequest;
use app_lib::runtime::event_bus::RuntimeEventBus;
use app_lib::runtime::identity::IdentityMapping;
use app_lib::runtime::ids::{AgentId, RunId};
use app_lib::runtime::query_engine::QueryEngine;
use app_lib::runtime::state::TurnState;
use app_lib::runtime::tools::capability::CapabilityContext;
use app_lib::runtime::tools::{
    AllowAllPermissionPipeline, RuntimeTool, ToolDefinition, ToolDispatcher, ToolError,
    ToolExecutionContext, ToolResult,
};
use app_lib::storage::file_manager::FileManager;
use app_lib::storage::file_store::AppStorage;
use async_trait::async_trait;
use serde_json::Value;
use tempfile::TempDir;

#[derive(Clone, Debug, PartialEq, Eq)]
struct SeenContext {
    agent_id: Option<String>,
    is_subagent: bool,
}

struct CaptureSubagentContextTool {
    seen: Arc<Mutex<Vec<SeenContext>>>,
}

#[async_trait]
impl RuntimeTool for CaptureSubagentContextTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition::new("capture_subagent_context", "capture subagent context")
    }

    async fn execute(
        &self,
        _input: Value,
        ctx: ToolExecutionContext,
    ) -> Result<ToolResult, ToolError> {
        let seen = SeenContext {
            agent_id: ctx.agent_id.as_ref().map(|id| id.as_str().to_string()),
            is_subagent: ctx
                .capability
                .as_ref()
                .map(|cap| cap.is_subagent)
                .unwrap_or(false),
        };
        self.seen.lock().unwrap().push(seen);
        Ok(ToolResult::new("capture_subagent_context", "ok", None))
    }
}

#[allow(deprecated)]
fn make_plugin_ctx(workspace: &Path, agent_id: Option<AgentId>) -> PluginContext {
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
        conversation_id: "subagent-capability-conv".to_string(),
        session_id: app_lib::runtime::ids::SessionId::new("subagent-capability-conv"),
        run_id: Some(app_lib::runtime::ids::RunId::new("subagent-capability-run")),
        agent_id,
        tavily_api_key: None,
        bocha_api_key: None,
        app_handle: None,
        session_manager,
        auth_manager: None,
        connector_engine: None,
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
    }
}

#[test]
fn test_h4_1_capability_is_subagent_field() {
    let parent = CapabilityContext::with_workspace(std::path::PathBuf::from("/tmp"), "ws-parent");
    let child = CapabilityContext::with_workspace(std::path::PathBuf::from("/tmp"), "ws-child")
        .with_subagent(true);

    assert!(!parent.is_subagent);
    assert!(child.is_subagent);
}

#[tokio::test]
async fn test_h4_2_registry_injects_is_subagent_for_child_contexts() {
    let tmp = TempDir::new().expect("TempDir::new failed");
    let seen = Arc::new(Mutex::new(Vec::new()));
    let registry = ToolRegistry::new();
    registry
        .register_runtime(Arc::new(CaptureSubagentContextTool { seen: seen.clone() }))
        .await;

    let parent_ctx = make_plugin_ctx(tmp.path(), None);
    registry
        .execute(
            "capture_subagent_context",
            &RequestScopedRuntimeDeps::from_plugin_context(&parent_ctx),
            serde_json::json!({}),
            CancellationToken::new(),
        )
        .await
        .expect("parent runtime tool call should succeed");

    let child_ctx = make_plugin_ctx(tmp.path(), Some(AgentId::new("child-agent")));
    registry
        .execute(
            "capture_subagent_context",
            &RequestScopedRuntimeDeps::from_plugin_context(&child_ctx),
            serde_json::json!({}),
            CancellationToken::new(),
        )
        .await
        .expect("child runtime tool call should succeed");

    let seen = seen.lock().unwrap().clone();
    assert_eq!(seen.len(), 2);
    assert_eq!(
        seen[0],
        SeenContext {
            agent_id: None,
            is_subagent: false,
        }
    );
    assert_eq!(
        seen[1],
        SeenContext {
            agent_id: Some("child-agent".to_string()),
            is_subagent: true,
        }
    );
}

#[tokio::test]
async fn test_h4_3_query_engine_keeps_primary_context_non_subagent() {
    let tmp = TempDir::new().expect("TempDir::new failed");
    let seen = Arc::new(Mutex::new(Vec::new()));
    let dispatcher = Arc::new(ToolDispatcher::new(Arc::new(AllowAllPermissionPipeline)));
    dispatcher.register(Arc::new(CaptureSubagentContextTool { seen: seen.clone() }));

    let engine =
        QueryEngine::with_dispatcher(dispatcher).with_workspace_path(tmp.path().to_path_buf());
    let mapping =
        IdentityMapping::from_legacy_conversation_id("subagent-capability-conv".to_string());
    let turn = TurnState::new(
        mapping,
        RunId::new("run-capability-main"),
        "capture".to_string(),
    );
    let bus = RuntimeEventBus::new();

    engine
        .run_tool_call_with_bus(
            &turn,
            &bus,
            RuntimeToolCallRequest {
                tool_call_id: "tc-capability-main".to_string(),
                tool_name: "capture_subagent_context".to_string(),
                args: serde_json::json!({}),
                purpose: None,
            },
        )
        .await
        .expect("primary query engine tool call should succeed");

    let seen = seen.lock().unwrap().clone();
    assert_eq!(seen.len(), 1);
    assert_eq!(
        seen[0],
        SeenContext {
            agent_id: None,
            is_subagent: false,
        }
    );
}
