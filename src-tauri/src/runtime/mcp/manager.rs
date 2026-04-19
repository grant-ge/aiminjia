use std::collections::HashMap;
use std::sync::Arc;

use tokio::sync::RwLock;

use crate::plugin::registry::ToolRegistry;
use crate::runtime::mcp::{McpError, McpResult, SharedMcpConnection};

struct ManagedMcpServer {
    connection: SharedMcpConnection,
    config: crate::runtime::mcp::McpServerConfig,
    registered_tool_ids: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct McpServerStatus {
    pub name: String,
    pub transport_type: String,
    pub endpoint: String,
    pub connected: bool,
    pub registered_tool_ids: Vec<String>,
}

/// 管理 MCP server 连接及其在 runtime tool pool 中的注册状态。
pub struct McpServerManager {
    registry: Arc<ToolRegistry>,
    servers: Arc<RwLock<HashMap<String, ManagedMcpServer>>>,
}

impl McpServerManager {
    pub fn new(registry: Arc<ToolRegistry>) -> Self {
        Self {
            registry,
            servers: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub async fn register(&self, connection: SharedMcpConnection) -> McpResult<()> {
        let mut servers = self.servers.write().await;
        let server_name = connection.server_name().to_string();
        if servers.contains_key(&server_name) {
            return Err(McpError::AlreadyConnected);
        }

        servers.insert(
            server_name,
            ManagedMcpServer {
                config: connection.config().clone(),
                connection,
                registered_tool_ids: Vec::new(),
            },
        );
        Ok(())
    }

    pub async fn connect(&self, server_name: &str) -> McpResult<Vec<String>> {
        let (connection, old_ids) = {
            let servers = self.servers.read().await;
            let server = servers.get(server_name).ok_or_else(|| {
                McpError::ConnectionFailed(format!("MCP server '{server_name}' not registered"))
            })?;
            (
                server.connection.clone(),
                server.registered_tool_ids.clone(),
            )
        };

        if !old_ids.is_empty() {
            self.registry.unregister_runtime_tools(&old_ids).await;
        }

        if !connection.is_connected() {
            connection.connect().await?;
        }

        let registered_tool_ids = self
            .registry
            .register_mcp_server(connection)
            .await
            .map_err(McpError::ConnectionFailed)?;

        let mut servers = self.servers.write().await;
        if let Some(server) = servers.get_mut(server_name) {
            server.registered_tool_ids = registered_tool_ids.clone();
        }

        Ok(registered_tool_ids)
    }

    pub async fn refresh(&self, server_name: &str) -> McpResult<Vec<String>> {
        self.connect(server_name).await
    }

    pub async fn disconnect(&self, server_name: &str) -> McpResult<()> {
        let (connection, registered_tool_ids) = {
            let servers = self.servers.read().await;
            let server = servers.get(server_name).ok_or_else(|| {
                McpError::ConnectionFailed(format!("MCP server '{server_name}' not registered"))
            })?;
            (
                server.connection.clone(),
                server.registered_tool_ids.clone(),
            )
        };

        self.registry
            .unregister_runtime_tools(&registered_tool_ids)
            .await;

        let disconnect_result = if connection.is_connected() {
            connection.disconnect().await
        } else {
            Ok(())
        };

        let mut servers = self.servers.write().await;
        if let Some(server) = servers.get_mut(server_name) {
            server.registered_tool_ids.clear();
        }

        disconnect_result
    }

    pub async fn unregister(&self, server_name: &str) -> McpResult<()> {
        let server = {
            let mut servers = self.servers.write().await;
            servers.remove(server_name).ok_or_else(|| {
                McpError::ConnectionFailed(format!("MCP server '{server_name}' not registered"))
            })?
        };

        self.registry
            .unregister_runtime_tools(&server.registered_tool_ids)
            .await;
        if server.connection.is_connected() {
            server.connection.disconnect().await?;
        }
        Ok(())
    }

    pub async fn get_connection(&self, server_name: &str) -> McpResult<SharedMcpConnection> {
        let connection = {
            let servers = self.servers.read().await;
            servers
                .get(server_name)
                .ok_or_else(|| {
                    McpError::ConnectionFailed(format!("MCP server '{server_name}' not registered"))
                })?
                .connection
                .clone()
        };

        if connection.is_connected() {
            Ok(connection)
        } else {
            Err(McpError::NotConnected)
        }
    }

    pub async fn list_servers(&self) -> Vec<McpServerStatus> {
        let servers = self.servers.read().await;
        let mut items: Vec<_> = servers
            .values()
            .map(|server| McpServerStatus {
                name: server.config.name.clone(),
                transport_type: server.config.transport_type.clone(),
                endpoint: server.config.endpoint.clone(),
                connected: server.connection.is_connected(),
                registered_tool_ids: server.registered_tool_ids.clone(),
            })
            .collect();
        items.sort_by(|a, b| a.name.cmp(&b.name));
        items
    }

    pub async fn connect_all(&self) -> Vec<(String, McpResult<Vec<String>>)> {
        let mut names: Vec<_> = self.servers.read().await.keys().cloned().collect();
        names.sort();

        let mut results = Vec::with_capacity(names.len());
        for name in names {
            let result = self.connect(&name).await;
            results.push((name, result));
        }
        results
    }

    pub async fn disconnect_all(&self) -> Vec<(String, McpResult<()>)> {
        let mut names: Vec<_> = self.servers.read().await.keys().cloned().collect();
        names.sort();

        let mut results = Vec::with_capacity(names.len());
        for name in names {
            let result = self.disconnect(&name).await;
            results.push((name, result));
        }
        results
    }
}

impl Default for McpServerManager {
    fn default() -> Self {
        Self::new(Arc::new(ToolRegistry::new()))
    }
}
