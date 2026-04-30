#![allow(deprecated)]

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use app_lib::plugin::registry::ToolRegistry;
use app_lib::plugin::skill_trait::ToolFilter;
use app_lib::runtime::mcp::{McpConnection, McpError, McpServerConfig, McpToolDefinition};
use app_lib::runtime::tools::{
    RuntimeTool, ToolDefinition, ToolError, ToolExecutionContext, ToolResult,
};
use async_trait::async_trait;
use serde_json::{json, Value};

struct FakeBuiltinTool {
    id: &'static str,
}

#[async_trait]
impl RuntimeTool for FakeBuiltinTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition::new(self.id, "fake builtin tool")
    }

    async fn execute(
        &self,
        _input: Value,
        _ctx: ToolExecutionContext,
    ) -> Result<ToolResult, ToolError> {
        Ok(ToolResult::new(self.id, "ok", None))
    }
}

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

async fn build_registry() -> ToolRegistry {
    let registry = ToolRegistry::new();
    for id in ["write_file", "bash", "list_directory"] {
        registry
            .register_runtime(Arc::new(FakeBuiltinTool { id }))
            .await;
    }

    let connection = Arc::new(MockMcpServerWithTools {
        config: McpServerConfig {
            name: "test-mcp".to_string(),
            transport_type: "stdio".to_string(),
            endpoint: "test".to_string(),
            env_vars: None,
        },
        tools: vec![
            McpToolDefinition {
                server_name: "test-mcp".to_string(),
                tool_name: "lookup".to_string(),
                description: "lookup".to_string(),
                input_schema: json!({"type": "object"}),
            },
            McpToolDefinition {
                server_name: "alpha-mcp".to_string(),
                tool_name: "status".to_string(),
                description: "status".to_string(),
                input_schema: json!({"type": "object"}),
            },
        ],
        connected: Mutex::new(false),
        tool_outputs: Mutex::new(HashMap::new()),
    });

    registry.register_mcp_server(connection).await.unwrap();
    registry
}

fn split_partitions(names: &[String]) -> (Vec<String>, Vec<String>) {
    let mut builtin = Vec::new();
    let mut mcp = Vec::new();
    for name in names {
        if name.starts_with("mcp__") {
            mcp.push(name.clone());
        } else {
            builtin.push(name.clone());
        }
    }
    (builtin, mcp)
}

#[tokio::test]
async fn review_builtin_tools_precede_mcp_tools_in_filtered_schema() {
    let registry = build_registry().await;
    let schemas = registry.get_schemas_filtered(&ToolFilter::All).await;
    let names: Vec<String> = schemas.into_iter().map(|schema| schema.name).collect();

    let first_mcp = names
        .iter()
        .position(|name| name.starts_with("mcp__"))
        .expect("mcp tool missing");
    let last_builtin = names
        .iter()
        .rposition(|name| !name.starts_with("mcp__"))
        .expect("builtin tool missing");
    assert!(
        last_builtin < first_mcp,
        "all built-ins must appear before all MCP tools: {names:?}"
    );
}

#[tokio::test]
async fn review_builtin_partition_is_internally_sorted() {
    let registry = build_registry().await;
    let schemas = registry.get_schemas_filtered(&ToolFilter::All).await;
    let names: Vec<String> = schemas.into_iter().map(|schema| schema.name).collect();
    let (builtin, _) = split_partitions(&names);
    let mut sorted = builtin.clone();
    sorted.sort();
    assert_eq!(
        builtin, sorted,
        "built-in partition must remain alphabetically sorted"
    );
}

#[tokio::test]
async fn review_mcp_partition_is_internally_sorted() {
    let registry = build_registry().await;
    let schemas = registry.get_all_schemas().await;
    let names: Vec<String> = schemas.into_iter().map(|schema| schema.name).collect();
    let (_, mcp) = split_partitions(&names);
    let mut sorted = mcp.clone();
    sorted.sort();
    assert_eq!(
        mcp, sorted,
        "MCP partition must remain alphabetically sorted"
    );
}

#[tokio::test]
async fn review_builtin_partition_is_stable_when_mcp_added() {
    let registry = ToolRegistry::new();
    for id in ["write_file", "bash", "list_directory"] {
        registry
            .register_runtime(Arc::new(FakeBuiltinTool { id }))
            .await;
    }
    let before: Vec<String> = registry
        .get_all_schemas()
        .await
        .into_iter()
        .map(|schema| schema.name)
        .filter(|name| !name.starts_with("mcp__"))
        .collect();

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
            description: "lookup".to_string(),
            input_schema: json!({"type": "object"}),
        }],
        connected: Mutex::new(false),
        tool_outputs: Mutex::new(HashMap::new()),
    });
    registry.register_mcp_server(connection).await.unwrap();

    let after: Vec<String> = registry
        .get_all_schemas()
        .await
        .into_iter()
        .map(|schema| schema.name)
        .filter(|name| !name.starts_with("mcp__"))
        .collect();

    assert_eq!(
        before, after,
        "adding MCP tools must not perturb builtin ordering"
    );
}
