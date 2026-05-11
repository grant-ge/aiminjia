use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use app_lib::plugin::registry::{RequestScopedRuntimeDeps, ToolRegistry};
use app_lib::runtime::mcp::{
    McpConnection, McpError, McpResult, McpServerConfig, McpServerManager, McpServerState,
    McpToolDefinition,
};
use app_lib::runtime::store::permission_store::{PermissionStore, PolicyDecision};
use app_lib::runtime::tools::permission::PermissionDecision;
use app_lib::runtime::tools::{ToolDispatchOutcome, ToolExecutionContext, TOOL_CATALOG};
use async_trait::async_trait;
use serde_json::{json, Value};

struct FullMockMcpServer {
    config: McpServerConfig,
    tools: Mutex<Vec<McpToolDefinition>>,
    outputs: Mutex<HashMap<String, Value>>,
    connected: Mutex<bool>,
}

#[async_trait]
impl McpConnection for FullMockMcpServer {
    async fn connect(&self) -> McpResult<()> {
        *self.connected.lock().unwrap() = true;
        Ok(())
    }

    async fn disconnect(&self) -> McpResult<()> {
        *self.connected.lock().unwrap() = false;
        Ok(())
    }

    fn is_connected(&self) -> bool {
        *self.connected.lock().unwrap()
    }

    fn server_name(&self) -> &str {
        &self.config.name
    }

    async fn list_tools(&self) -> McpResult<Vec<McpToolDefinition>> {
        Ok(self.tools.lock().unwrap().clone())
    }

    async fn call_tool(&self, name: &str, _args: Value) -> McpResult<Value> {
        self.outputs
            .lock()
            .unwrap()
            .get(name)
            .cloned()
            .ok_or_else(|| McpError::ToolNotFound(name.to_string()))
    }

    fn config(&self) -> &McpServerConfig {
        &self.config
    }
}

#[allow(deprecated)]
fn make_test_plugin_ctx(conversation_id: &str) -> app_lib::plugin::context::PluginContext {
    let tmp_dir = tempfile::TempDir::new().expect("TempDir::new failed");
    let tmp = tmp_dir.path().to_path_buf();
    let _ = &tmp_dir;

    let storage = Arc::new(
        app_lib::storage::file_store::AppStorage::new(&tmp).expect("AppStorage::new failed"),
    );
    let file_manager = Arc::new(app_lib::storage::file_manager::FileManager::new(&tmp));

    app_lib::plugin::context::PluginContext {
        storage,
        file_manager,
        workspace_path: tmp.clone(),
        conversation_id: conversation_id.to_string(),
        session_id: app_lib::runtime::ids::SessionId::new(conversation_id.to_string()),
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
async fn mcp_end_to_end_workflow_register_execute_disconnect() {
    let registry = Arc::new(ToolRegistry::new());
    let store = Arc::new(PermissionStore::in_memory());
    registry.set_permission_store(store.clone()).await;
    let manager = McpServerManager::new(registry.clone());

    let connection = Arc::new(FullMockMcpServer {
        config: McpServerConfig {
            name: "e2e-server".to_string(),
            transport_type: "stdio".to_string(),
            endpoint: "cmd".to_string(),
            env_vars: None,
        },
        tools: Mutex::new(vec![McpToolDefinition {
            server_name: "e2e-server".to_string(),
            tool_name: "lookup".to_string(),
            description: "Lookup".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": { "query": { "type": "string" } }
            }),
        }]),
        outputs: Mutex::new(HashMap::from([(
            "lookup".to_string(),
            json!({ "items": ["a", "b"] }),
        )])),
        connected: Mutex::new(false),
    });

    manager.register(connection).await.unwrap();
    let status = manager.connect("e2e-server").await.unwrap();
    assert_eq!(status.state, McpServerState::Ready);
    assert_eq!(
        status.registered_tool_ids,
        vec!["mcp__e2e-server__lookup".to_string()]
    );
    assert!(TOOL_CATALOG.get("mcp__e2e-server__lookup").is_some());

    let dispatcher = registry
        .to_runtime_dispatcher(RequestScopedRuntimeDeps::from_plugin_context(
            &make_test_plugin_ctx("conv-e2e"),
        ))
        .await;
    let ask = dispatcher
        .dispatch(
            "mcp__e2e-server__lookup",
            json!({ "query": "hello" }),
            ToolExecutionContext::for_test("conv-e2e", "run-e2e", "tc-e2e"),
        )
        .await
        .expect("first MCP dispatch should surface a permission decision");

    match ask {
        ToolDispatchOutcome::AskRequired(PermissionDecision::Ask { message, .. }) => {
            assert!(message.contains("MCP") || message.contains("external server"));
        }
        other => panic!(
            "expected AskRequired for first MCP dispatch, got: {:?}",
            other
        ),
    }

    store.record(
        "mcp__e2e-server__lookup:mcp".to_string(),
        PolicyDecision::Allow,
    );

    let outcome = dispatcher
        .dispatch(
            "mcp__e2e-server__lookup",
            json!({ "query": "hello" }),
            ToolExecutionContext::for_test("conv-e2e", "run-e2e", "tc-e2e-allowed"),
        )
        .await
        .expect("MCP dispatch should succeed after allow-once decision");

    match outcome {
        ToolDispatchOutcome::Completed { result, .. } => {
            assert!(result.content.contains("items"));
            assert!(result.content.contains("a"));
        }
        ToolDispatchOutcome::AskRequired(other) => {
            panic!(
                "unexpected AskRequired after allow-once decision: {:?}",
                other
            )
        }
        ToolDispatchOutcome::InteractionRequired(_) => {
            panic!("unexpected InteractionRequired after allow-once decision")
        }
    }

    manager.disconnect("e2e-server").await.unwrap();
    assert!(TOOL_CATALOG.get("mcp__e2e-server__lookup").is_none());
}
