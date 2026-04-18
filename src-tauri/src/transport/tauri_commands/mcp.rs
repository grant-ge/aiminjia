use std::collections::HashMap;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tauri::State;

use crate::runtime::mcp::{
    McpServerConfig, McpServerManager, McpServerStatus, PendingMcpConnection,
};
use crate::storage::mcp_config_store::McpConfigStore;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct McpServerConfigDto {
    pub name: String,
    pub transport_type: String,
    pub endpoint: String,
    pub env_vars: Option<HashMap<String, String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct McpServerStatusDto {
    pub name: String,
    pub transport_type: String,
    pub endpoint: String,
    pub connected: bool,
    pub registered_tool_ids: Vec<String>,
}

impl From<McpServerConfigDto> for McpServerConfig {
    fn from(value: McpServerConfigDto) -> Self {
        Self {
            name: value.name,
            transport_type: value.transport_type,
            endpoint: value.endpoint,
            env_vars: value.env_vars,
        }
    }
}

impl From<McpServerStatus> for McpServerStatusDto {
    fn from(value: McpServerStatus) -> Self {
        Self {
            name: value.name,
            transport_type: value.transport_type,
            endpoint: value.endpoint,
            connected: value.connected,
            registered_tool_ids: value.registered_tool_ids,
        }
    }
}

#[tauri::command]
pub async fn list_mcp_servers(
    manager: State<'_, Arc<McpServerManager>>,
) -> Result<Vec<McpServerStatusDto>, String> {
    Ok(manager
        .list_servers()
        .await
        .into_iter()
        .map(McpServerStatusDto::from)
        .collect())
}

#[tauri::command]
pub async fn add_mcp_server(
    config: McpServerConfigDto,
    manager: State<'_, Arc<McpServerManager>>,
    config_store: State<'_, Arc<McpConfigStore>>,
) -> Result<(), String> {
    let config: McpServerConfig = config.into();
    config_store.add(config.clone())?;

    let connection = Arc::new(PendingMcpConnection::new(config.clone()));
    if let Err(err) = manager.register(connection).await {
        let _ = config_store.remove(&config.name);
        return Err(err.to_string());
    }

    Ok(())
}

#[tauri::command]
pub async fn remove_mcp_server(
    server_name: String,
    manager: State<'_, Arc<McpServerManager>>,
    config_store: State<'_, Arc<McpConfigStore>>,
) -> Result<(), String> {
    manager
        .unregister(&server_name)
        .await
        .map_err(|err| err.to_string())?;
    config_store.remove(&server_name)?;
    Ok(())
}

#[tauri::command]
pub async fn connect_mcp_server(
    server_name: String,
    manager: State<'_, Arc<McpServerManager>>,
) -> Result<Vec<String>, String> {
    manager
        .connect(&server_name)
        .await
        .map_err(|err| err.to_string())
}

#[tauri::command]
pub async fn disconnect_mcp_server(
    server_name: String,
    manager: State<'_, Arc<McpServerManager>>,
) -> Result<(), String> {
    manager
        .disconnect(&server_name)
        .await
        .map_err(|err| err.to_string())
}
