pub mod connection;
pub mod types;

pub use connection::{McpConnection, McpError, McpResult, SharedMcpConnection};
pub use types::{
    build_mcp_tool_name, normalize_name_for_mcp, McpServerConfig, McpToolDefinition,
};
