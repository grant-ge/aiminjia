use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use app_lib::plugin::registry::ToolRegistry;
use app_lib::runtime::mcp::{
    McpConnection, McpError, McpResult, McpServerConfig, McpToolDefinition,
};
use app_lib::runtime::tools::TOOL_CATALOG;
use async_trait::async_trait;
use serde_json::{json, Value};

struct ReviewMcpConn {
    config: McpServerConfig,
    tools: Vec<McpToolDefinition>,
    connected: Mutex<bool>,
    outputs: Mutex<HashMap<String, Value>>,
}

#[async_trait]
impl McpConnection for ReviewMcpConn {
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
        Ok(self.tools.clone())
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

#[test]
fn review_mcp_tool_definition_uses_prefixed_name_and_mcp_scope() {
    let tool = McpToolDefinition {
        server_name: "review-server".to_string(),
        tool_name: "lookup".to_string(),
        description: "Lookup".to_string(),
        input_schema: json!({ "type": "object" }),
    };

    let definition = tool.to_tool_definition();
    assert_eq!(definition.id, "mcp__review-server__lookup");
    assert_eq!(definition.capability_scope, vec!["mcp".to_string()]);
}

#[tokio::test]
async fn review_unregister_runtime_tools_removes_only_dynamic_mcp_entries() {
    let registry = ToolRegistry::new();
    let connection = Arc::new(ReviewMcpConn {
        config: McpServerConfig {
            name: "review-server".to_string(),
            transport_type: "stdio".to_string(),
            endpoint: "cmd".to_string(),
            env_vars: None,
        },
        tools: vec![McpToolDefinition {
            server_name: "review-server".to_string(),
            tool_name: "lookup".to_string(),
            description: "Lookup".to_string(),
            input_schema: json!({ "type": "object" }),
        }],
        connected: Mutex::new(false),
        outputs: Mutex::new(HashMap::from([("lookup".to_string(), json!({ "ok": true }))])),
    });

    assert!(TOOL_CATALOG.get("execute_python").is_some());
    let ids = registry.register_mcp_server(connection).await.unwrap();
    assert!(TOOL_CATALOG.get("mcp__review-server__lookup").is_some());

    registry.unregister_runtime_tools(&ids).await;

    assert!(TOOL_CATALOG.get("mcp__review-server__lookup").is_none());
    assert!(
        TOOL_CATALOG.get("execute_python").is_some(),
        "builtins must survive dynamic MCP cleanup"
    );
}
