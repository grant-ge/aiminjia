use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use app_lib::runtime::mcp::{
    McpConnection, McpError, McpResult, McpRuntimeTool, McpServerConfig, McpToolDefinition,
};
use app_lib::runtime::tools::{RuntimeTool, ToolExecutionContext};
use async_trait::async_trait;
use serde_json::{json, Value};

struct MockMcpConn {
    config: McpServerConfig,
    connected: Mutex<bool>,
    tool_results: Mutex<HashMap<String, Value>>,
    called_tool_names: Mutex<Vec<String>>,
}

#[async_trait]
impl McpConnection for MockMcpConn {
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
        Ok(vec![])
    }

    async fn call_tool(&self, tool_name: &str, _args: Value) -> McpResult<Value> {
        self.called_tool_names
            .lock()
            .unwrap()
            .push(tool_name.to_string());
        self.tool_results
            .lock()
            .unwrap()
            .get(tool_name)
            .cloned()
            .ok_or_else(|| McpError::ToolNotFound(tool_name.to_string()))
    }

    fn config(&self) -> &McpServerConfig {
        &self.config
    }
}

fn sample_tool_definition() -> McpToolDefinition {
    McpToolDefinition {
        server_name: "test-server".to_string(),
        tool_name: "example_tool".to_string(),
        description: "Example tool".to_string(),
        input_schema: json!({ "type": "object" }),
    }
}

#[tokio::test]
async fn mcp_runtime_tool_executes_remote_call_using_raw_tool_name() {
    let connection = Arc::new(MockMcpConn {
        config: McpServerConfig {
            name: "test-server".to_string(),
            transport_type: "stdio".to_string(),
            endpoint: "test".to_string(),
            env_vars: None,
        },
        connected: Mutex::new(true),
        tool_results: Mutex::new(HashMap::from([(
            "example_tool".to_string(),
            json!({ "result": "success", "items": [1, 2, 3] }),
        )])),
        called_tool_names: Mutex::new(Vec::new()),
    });

    let tool = McpRuntimeTool::new(sample_tool_definition(), connection.clone());

    let result = tool
        .execute(
            json!({ "param": "value" }),
            ToolExecutionContext::for_test("session-1", "run-1", "tool-call-1"),
        )
        .await
        .unwrap();

    assert_eq!(result.tool_name, "mcp__test-server__example_tool");
    assert!(result.content.contains("success"));
    assert_eq!(
        connection.called_tool_names.lock().unwrap().as_slice(),
        &["example_tool".to_string()],
        "runtime tool must call the MCP server using the raw tool_name, not the fully-qualified dispatcher id",
    );
}

#[tokio::test]
async fn mcp_runtime_tool_fails_when_server_not_connected() {
    let connection = Arc::new(MockMcpConn {
        config: McpServerConfig {
            name: "test-server".to_string(),
            transport_type: "stdio".to_string(),
            endpoint: "test".to_string(),
            env_vars: None,
        },
        connected: Mutex::new(false),
        tool_results: Mutex::new(HashMap::new()),
        called_tool_names: Mutex::new(Vec::new()),
    });

    let tool = McpRuntimeTool::new(sample_tool_definition(), connection);
    let err = tool
        .execute(
            json!({}),
            ToolExecutionContext::for_test("session-1", "run-1", "tool-call-1"),
        )
        .await
        .unwrap_err();

    assert!(
        err.to_string().contains("not connected"),
        "unexpected error: {err}"
    );
}

#[tokio::test]
async fn mcp_runtime_tool_definition_uses_fully_qualified_id() {
    let connection = Arc::new(MockMcpConn {
        config: McpServerConfig {
            name: "test-server".to_string(),
            transport_type: "stdio".to_string(),
            endpoint: "test".to_string(),
            env_vars: None,
        },
        connected: Mutex::new(true),
        tool_results: Mutex::new(HashMap::new()),
        called_tool_names: Mutex::new(Vec::new()),
    });

    let tool = McpRuntimeTool::new(sample_tool_definition(), connection);
    let ctx = app_lib::runtime::tools::ToolDescriptionContext::default();
    let definition = tool.definition(&ctx).await;

    assert_eq!(definition.id, "mcp__test-server__example_tool");
    assert_eq!(definition.description, "Example tool");
    assert_eq!(definition.capability_scope, vec!["mcp".to_string()]);
}
