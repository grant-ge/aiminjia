use std::sync::Arc;

use async_trait::async_trait;
use serde_json::Value;

use crate::runtime::mcp::types::{McpServerConfig, McpToolDefinition};

/// 错误类型（MCP 连接和调用相关）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum McpError {
    ConnectionFailed(String),
    ServerNotResponding,
    ToolNotFound(String),
    ToolExecutionFailed(String),
    InvalidResponse(String),
    AlreadyConnected,
    NotConnected,
}

impl std::fmt::Display for McpError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ConnectionFailed(msg) => write!(f, "MCP connection failed: {msg}"),
            Self::ServerNotResponding => write!(f, "MCP server not responding"),
            Self::ToolNotFound(name) => write!(f, "MCP tool not found: {name}"),
            Self::ToolExecutionFailed(msg) => write!(f, "MCP tool execution failed: {msg}"),
            Self::InvalidResponse(msg) => write!(f, "Invalid MCP response: {msg}"),
            Self::AlreadyConnected => write!(f, "MCP server already connected"),
            Self::NotConnected => write!(f, "MCP server not connected"),
        }
    }
}

impl std::error::Error for McpError {}

/// Result type for MCP operations。
pub type McpResult<T> = Result<T, McpError>;

/// Abstract MCP connection interface.
#[async_trait]
pub trait McpConnection: Send + Sync {
    /// 连接到 MCP server。
    async fn connect(&self) -> McpResult<()>;

    /// 断开连接。
    async fn disconnect(&self) -> McpResult<()>;

    /// 检查连接是否活跃。
    fn is_connected(&self) -> bool;

    /// 获取 server 名称。
    fn server_name(&self) -> &str;

    /// 列出此 server 提供的所有工具定义。
    async fn list_tools(&self) -> McpResult<Vec<McpToolDefinition>>;

    /// 调用指定工具。
    async fn call_tool(&self, tool_name: &str, arguments: Value) -> McpResult<Value>;

    /// 获取原始配置（用于日志和管理 UI）。
    fn config(&self) -> &McpServerConfig;
}

pub type SharedMcpConnection = Arc<dyn McpConnection>;
