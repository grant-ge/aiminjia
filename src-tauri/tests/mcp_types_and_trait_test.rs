use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use app_lib::runtime::mcp::{
    build_mcp_tool_name, McpConnection, McpResult, McpServerConfig, McpToolDefinition,
    SharedMcpConnection,
};
use app_lib::runtime::tools::definition::ToolKind;
use async_trait::async_trait;
use serde_json::{json, Value};

#[test]
fn mcp_tool_definition_to_runtime_tool_uses_fully_qualified_name() {
    let mcp_def = McpToolDefinition {
        server_name: "example-server".to_string(),
        tool_name: "example_tool".to_string(),
        description: "An example MCP tool".to_string(),
        input_schema: json!({
            "type": "object",
            "properties": {
                "param1": { "type": "string" }
            },
            "required": ["param1"]
        }),
    };

    let rt_def = mcp_def.to_tool_definition();
    assert_eq!(rt_def.id, "mcp__example-server__example_tool");
    assert_eq!(rt_def.description, "An example MCP tool");
    assert!(matches!(rt_def.kind, ToolKind::Primitive));
    assert_eq!(rt_def.capability_scope, vec!["mcp".to_string()]);
}

#[test]
fn mcp_tool_definition_to_catalog_entry_preserves_schema() {
    let input_schema = json!({
        "type": "object",
        "properties": {
            "query": { "type": "string" }
        }
    });
    let mcp_def = McpToolDefinition {
        server_name: "search-server".to_string(),
        tool_name: "search".to_string(),
        description: "Search through MCP".to_string(),
        input_schema: input_schema.clone(),
    };

    let entry = mcp_def.to_catalog_entry();
    assert_eq!(entry.definition.id, "mcp__search-server__search");
    assert_eq!(entry.json_schema, input_schema);
}

#[test]
fn build_mcp_tool_name_normalizes_whitespace_and_dots() {
    assert_eq!(
        build_mcp_tool_name("My Server", "list.items"),
        "mcp__My_Server__list_items"
    );
}

struct MockConnection {
    config: McpServerConfig,
    connected: AtomicBool,
}

#[async_trait]
impl McpConnection for MockConnection {
    async fn connect(&self) -> McpResult<()> {
        self.connected.store(true, Ordering::SeqCst);
        Ok(())
    }

    async fn disconnect(&self) -> McpResult<()> {
        self.connected.store(false, Ordering::SeqCst);
        Ok(())
    }

    fn is_connected(&self) -> bool {
        self.connected.load(Ordering::SeqCst)
    }

    fn server_name(&self) -> &str {
        &self.config.name
    }

    async fn list_tools(&self) -> McpResult<Vec<McpToolDefinition>> {
        Ok(vec![McpToolDefinition {
            server_name: self.config.name.clone(),
            tool_name: "search".to_string(),
            description: "Search".to_string(),
            input_schema: json!({"type": "object", "properties": {}}),
        }])
    }

    async fn call_tool(&self, tool_name: &str, arguments: Value) -> McpResult<Value> {
        Ok(json!({
            "tool": tool_name,
            "arguments": arguments,
        }))
    }

    fn config(&self) -> &McpServerConfig {
        &self.config
    }
}

#[tokio::test]
async fn shared_mcp_connection_trait_object_supports_basic_calls() {
    let connection: SharedMcpConnection = Arc::new(MockConnection {
        config: McpServerConfig {
            name: "demo-server".to_string(),
            transport_type: "stdio".to_string(),
            endpoint: "demo-command".to_string(),
            env_vars: Some(HashMap::from([(
                String::from("TOKEN"),
                String::from("abc"),
            )])),
        },
        connected: AtomicBool::new(false),
    });

    connection.connect().await.unwrap();
    assert!(connection.is_connected());
    assert_eq!(connection.server_name(), "demo-server");

    let tools = connection.list_tools().await.unwrap();
    assert_eq!(tools.len(), 1);
    assert_eq!(tools[0].qualified_name(), "mcp__demo-server__search");

    let result = connection
        .call_tool("search", json!({"query": "hello"}))
        .await
        .unwrap();
    assert_eq!(result["tool"], "search");

    connection.disconnect().await.unwrap();
    assert!(!connection.is_connected());
}
