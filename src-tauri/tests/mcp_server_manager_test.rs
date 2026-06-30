use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use app_lib::plugin::registry::ToolRegistry;
use app_lib::runtime::mcp::{
    McpConnection, McpError, McpResult, McpServerConfig, McpServerManager, McpServerState,
    McpToolDefinition,
};
use app_lib::runtime::tools::TOOL_CATALOG;
use async_trait::async_trait;
use serde_json::{json, Value};

struct TestMcpConn {
    config: McpServerConfig,
    connected: Mutex<bool>,
    tools: Mutex<Vec<McpToolDefinition>>,
    outputs: Mutex<HashMap<String, Value>>,
}

#[async_trait]
impl McpConnection for TestMcpConn {
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

fn tool(server_name: &str, tool_name: &str) -> McpToolDefinition {
    McpToolDefinition {
        server_name: server_name.to_string(),
        tool_name: tool_name.to_string(),
        description: format!("{tool_name} on {server_name}"),
        input_schema: json!({ "type": "object" }),
    }
}

#[tokio::test]
async fn manager_connect_and_disconnect_sync_runtime_tools() {
    let registry = Arc::new(ToolRegistry::new());
    let manager = McpServerManager::new(registry.clone());

    let connection = Arc::new(TestMcpConn {
        config: McpServerConfig {
            name: "server-a".to_string(),
            transport_type: "stdio".to_string(),
            endpoint: "cmd".to_string(),
            env_vars: None,
        },
        connected: Mutex::new(false),
        tools: Mutex::new(vec![tool("server-a", "lookup")]),
        outputs: Mutex::new(HashMap::from([(
            "lookup".to_string(),
            json!({ "ok": true }),
        )])),
    });

    manager.register(connection.clone()).await.unwrap();

    let status = manager.connect("server-a").await.unwrap();
    assert_eq!(
        status.registered_tool_ids,
        vec!["mcp__server-a__lookup".to_string()]
    );
    assert_eq!(status.state, McpServerState::Ready);
    assert!(connection.is_connected());
    assert!(TOOL_CATALOG.get("mcp__server-a__lookup").is_some());

    let servers = manager.list_servers().await;
    assert_eq!(servers.len(), 1);
    assert_eq!(servers[0].state, McpServerState::Ready);
    assert_eq!(servers[0].registered_tool_ids, status.registered_tool_ids);

    manager.disconnect("server-a").await.unwrap();
    assert!(!connection.is_connected());
    assert!(TOOL_CATALOG.get("mcp__server-a__lookup").is_none());

    let servers = manager.list_servers().await;
    assert_eq!(servers[0].state, McpServerState::Disconnected);
    assert_eq!(servers[0].registered_tool_ids, Vec::<String>::new());

    let schemas = registry.get_all_schemas().await;
    assert!(
        schemas
            .iter()
            .all(|schema| schema.name != "mcp__server-a__lookup"),
        "disconnect should remove MCP tool schema from runtime tool pool"
    );
}

#[tokio::test]
async fn manager_refresh_replaces_stale_tool_ids() {
    let registry = Arc::new(ToolRegistry::new());
    let manager = McpServerManager::new(registry.clone());

    let connection = Arc::new(TestMcpConn {
        config: McpServerConfig {
            name: "server-b".to_string(),
            transport_type: "stdio".to_string(),
            endpoint: "cmd".to_string(),
            env_vars: None,
        },
        connected: Mutex::new(false),
        tools: Mutex::new(vec![tool("server-b", "lookup")]),
        outputs: Mutex::new(HashMap::from([
            ("lookup".to_string(), json!({ "kind": "lookup" })),
            ("status".to_string(), json!({ "kind": "status" })),
        ])),
    });

    manager.register(connection.clone()).await.unwrap();
    manager.connect("server-b").await.unwrap();
    assert!(TOOL_CATALOG.get("mcp__server-b__lookup").is_some());

    *connection.tools.lock().unwrap() = vec![tool("server-b", "status")];
    let refreshed = manager.refresh("server-b").await.unwrap();

    assert_eq!(refreshed.state, McpServerState::Ready);
    assert_eq!(
        refreshed.registered_tool_ids,
        vec!["mcp__server-b__status".to_string()]
    );
    assert!(TOOL_CATALOG.get("mcp__server-b__lookup").is_none());
    assert!(TOOL_CATALOG.get("mcp__server-b__status").is_some());
}

#[tokio::test]
async fn manager_connect_all_and_disconnect_all_cover_all_servers() {
    let registry = Arc::new(ToolRegistry::new());
    let manager = McpServerManager::new(registry);

    for server_name in ["server-c", "server-d"] {
        let connection = Arc::new(TestMcpConn {
            config: McpServerConfig {
                name: server_name.to_string(),
                transport_type: "stdio".to_string(),
                endpoint: "cmd".to_string(),
                env_vars: None,
            },
            connected: Mutex::new(false),
            tools: Mutex::new(vec![tool(server_name, "lookup")]),
            outputs: Mutex::new(HashMap::from([(
                "lookup".to_string(),
                json!({ "ok": true }),
            )])),
        });
        manager.register(connection).await.unwrap();
    }

    let connect_results = manager.connect_all().await;
    assert_eq!(connect_results.len(), 2);
    assert!(connect_results.iter().all(|(_, result)| result.is_ok()));
    assert!(connect_results.iter().all(|(_, result)| {
        result
            .as_ref()
            .map(|status| status.state == McpServerState::Ready)
            .unwrap_or(false)
    }));

    let disconnect_results = manager.disconnect_all().await;
    assert_eq!(disconnect_results.len(), 2);
    assert!(disconnect_results.iter().all(|(_, result)| result.is_ok()));
}

#[tokio::test]
async fn manager_rejects_duplicate_registration() {
    let registry = Arc::new(ToolRegistry::new());
    let manager = McpServerManager::new(registry);

    let connection = Arc::new(TestMcpConn {
        config: McpServerConfig {
            name: "server-e".to_string(),
            transport_type: "stdio".to_string(),
            endpoint: "cmd".to_string(),
            env_vars: None,
        },
        connected: Mutex::new(false),
        tools: Mutex::new(vec![tool("server-e", "lookup")]),
        outputs: Mutex::new(HashMap::from([(
            "lookup".to_string(),
            json!({ "ok": true }),
        )])),
    });

    manager.register(connection.clone()).await.unwrap();
    let err = manager.register(connection).await.unwrap_err();
    assert!(matches!(err, McpError::AlreadyConnected));
}

#[tokio::test]
async fn manager_marks_server_failed_when_tool_list_is_empty() {
    let registry = Arc::new(ToolRegistry::new());
    let manager = McpServerManager::new(registry.clone());

    let connection = Arc::new(TestMcpConn {
        config: McpServerConfig {
            name: "server-empty".to_string(),
            transport_type: "stdio".to_string(),
            endpoint: "cmd".to_string(),
            env_vars: None,
        },
        connected: Mutex::new(false),
        tools: Mutex::new(vec![]),
        outputs: Mutex::new(HashMap::new()),
    });

    manager.register(connection).await.unwrap();

    let status = manager.connect("server-empty").await.unwrap();
    assert_eq!(status.state, McpServerState::Failed);
    assert_eq!(status.registered_tool_ids, Vec::<String>::new());
    assert!(
        status
            .last_error
            .as_deref()
            .unwrap_or_default()
            .contains("no tools"),
        "expected no-tools failure, got: {:?}",
        status.last_error
    );

    assert!(TOOL_CATALOG.get("mcp__server-empty__lookup").is_none());
    let schemas = registry.get_all_schemas().await;
    assert!(schemas
        .iter()
        .all(|schema| !schema.name.starts_with("mcp__server-empty__")));
}
