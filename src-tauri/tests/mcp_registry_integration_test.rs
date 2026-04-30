use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use app_lib::plugin::registry::{RequestScopedRuntimeDeps, ToolRegistry};
use app_lib::runtime::mcp::{McpConnection, McpError, McpServerConfig, McpToolDefinition};
use app_lib::runtime::store::permission_store::{PermissionStore, PolicyDecision};
use app_lib::runtime::tools::permission::PermissionDecision;
use app_lib::runtime::tools::{ToolDispatchOutcome, ToolExecutionContext, TOOL_CATALOG};
use async_trait::async_trait;
use serde_json::{json, Value};

struct MockMcpServerWithTools {
    config: McpServerConfig,
    tools: Vec<McpToolDefinition>,
    connected: Mutex<bool>,
    tool_outputs: Mutex<HashMap<String, Value>>,
}

#[async_trait]
impl McpConnection for MockMcpServerWithTools {
    async fn connect(&self) -> Result<(), McpError> {
        *self.connected.lock().unwrap() = true;
        Ok(())
    }

    async fn disconnect(&self) -> Result<(), McpError> {
        *self.connected.lock().unwrap() = false;
        Ok(())
    }

    fn is_connected(&self) -> bool {
        *self.connected.lock().unwrap()
    }

    fn server_name(&self) -> &str {
        &self.config.name
    }

    async fn list_tools(&self) -> Result<Vec<McpToolDefinition>, McpError> {
        Ok(self.tools.clone())
    }

    async fn call_tool(&self, name: &str, _args: Value) -> Result<Value, McpError> {
        self.tool_outputs
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
        skill_registry: None,
        authorized_workspace: None,
        read_file_state: None,
        cancellation: None,
        permission_mode: app_lib::runtime::tools::permission::PermissionMode::Default,
        dingtalk_bridge: None,
            runtime_resolver: None,
    }
}

#[tokio::test]
async fn register_mcp_server_registers_fully_qualified_tools_and_dispatches_them() {
    let registry = ToolRegistry::new();
    let store = Arc::new(PermissionStore::in_memory());
    registry.set_permission_store(store.clone()).await;

    let connection = Arc::new(MockMcpServerWithTools {
        config: McpServerConfig {
            name: "test-mcp".to_string(),
            transport_type: "stdio".to_string(),
            endpoint: "test".to_string(),
            env_vars: None,
        },
        tools: vec![McpToolDefinition {
            server_name: "test-mcp".to_string(),
            tool_name: "lookup".to_string(),
            description: "Lookup from MCP".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "query": { "type": "string" }
                },
                "required": ["query"]
            }),
        }],
        connected: Mutex::new(false),
        tool_outputs: Mutex::new(HashMap::from([(
            "lookup".to_string(),
            json!({ "result": "ok" }),
        )])),
    });

    let registered_ids = registry
        .register_mcp_server(connection.clone())
        .await
        .unwrap();
    assert_eq!(registered_ids, vec!["mcp__test-mcp__lookup".to_string()]);
    assert!(
        connection.is_connected(),
        "register_mcp_server should connect when needed"
    );

    let catalog_entry = TOOL_CATALOG
        .get_entry("mcp__test-mcp__lookup")
        .expect("catalog entry should exist");
    assert_eq!(
        catalog_entry.json_schema,
        json!({
            "type": "object",
            "properties": {
                "query": { "type": "string" }
            },
            "required": ["query"]
        })
    );

    let schemas = registry.get_all_schemas().await;
    assert!(
        schemas
            .iter()
            .any(|schema| schema.name == "mcp__test-mcp__lookup"),
        "runtime schema list should expose the fully-qualified MCP tool name"
    );

    let dispatcher = registry
        .to_runtime_dispatcher(RequestScopedRuntimeDeps::from_plugin_context(
            &make_test_plugin_ctx("conv-mcp"),
        ))
        .await;
    let ask = dispatcher
        .dispatch(
            "mcp__test-mcp__lookup",
            json!({ "query": "hello" }),
            ToolExecutionContext::for_test("conv-mcp", "run-mcp", "tc-mcp"),
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
        "mcp__test-mcp__lookup:mcp".to_string(),
        PolicyDecision::Allow,
    );

    let outcome = dispatcher
        .dispatch(
            "mcp__test-mcp__lookup",
            json!({ "query": "hello" }),
            ToolExecutionContext::for_test("conv-mcp", "run-mcp", "tc-mcp-allowed"),
        )
        .await
        .expect("MCP dispatch should succeed after allow-once decision");

    match outcome {
        ToolDispatchOutcome::Completed { result, .. } => {
            assert!(result.content.contains("ok"));
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
}
