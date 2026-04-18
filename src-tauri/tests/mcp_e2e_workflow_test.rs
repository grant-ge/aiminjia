use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use app_lib::plugin::registry::ToolRegistry;
use app_lib::runtime::mcp::{
    McpConnection, McpError, McpResult, McpServerConfig, McpServerManager, McpToolDefinition,
};
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
        app_lib::storage::file_store::AppStorage::new(&tmp)
            .expect("AppStorage::new failed"),
    );
    let file_manager = Arc::new(app_lib::storage::file_manager::FileManager::new(&tmp));
    let session_manager = Arc::new(app_lib::python::session::PythonSessionManager::new(
        tmp.clone(),
        None,
    ));

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
async fn mcp_end_to_end_workflow_register_execute_disconnect() {
    let registry = Arc::new(ToolRegistry::new());
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
    let ids = manager.connect("e2e-server").await.unwrap();
    assert_eq!(ids, vec!["mcp__e2e-server__lookup".to_string()]);
    assert!(TOOL_CATALOG.get("mcp__e2e-server__lookup").is_some());

    let dispatcher = registry
        .to_runtime_dispatcher(make_test_plugin_ctx("conv-e2e"))
        .await;
    let outcome = dispatcher
        .dispatch(
            "mcp__e2e-server__lookup",
            json!({ "query": "hello" }),
            ToolExecutionContext::for_test("conv-e2e", "run-e2e", "tc-e2e"),
        )
        .await
        .unwrap();

    match outcome {
        ToolDispatchOutcome::Completed { result, .. } => {
            assert!(result.content.contains("items"));
            assert!(result.content.contains("a"));
        }
        ToolDispatchOutcome::AskRequired(_) => panic!("unexpected AskRequired"),
    }

    manager.disconnect("e2e-server").await.unwrap();
    assert!(TOOL_CATALOG.get("mcp__e2e-server__lookup").is_none());
}
