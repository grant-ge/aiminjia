use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// MCP server 配置（最小化）。
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct McpServerConfig {
    pub name: String,
    /// 可以是 "stdio" / "http" / "sse" 等。
    pub transport_type: String,
    /// 对于 stdio：命令路径；对于 http/sse：URL。
    pub endpoint: String,
    pub env_vars: Option<HashMap<String, String>>,
}

/// MCP server 响应的工具定义。
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct McpToolDefinition {
    /// 原始 server 名称（未归一化）。
    pub server_name: String,
    /// 原始 tool 名称（未归一化）。
    pub tool_name: String,
    pub description: String,
    /// JSON Schema describing input parameters.
    pub input_schema: Value,
}

impl McpToolDefinition {
    /// 对标 claude-code-best：MCP 工具名必须 fully-qualified，
    /// 形如 `mcp__<server>__<tool>`。
    pub fn qualified_name(&self) -> String {
        build_mcp_tool_name(&self.server_name, &self.tool_name)
    }

    /// 转换为 RuntimeTool 的 ToolDefinition。
    pub fn to_tool_definition(&self) -> crate::runtime::tools::ToolDefinition {
        crate::runtime::tools::ToolDefinition::new(self.qualified_name(), &self.description)
            .with_kind(crate::runtime::tools::definition::ToolKind::Primitive)
            .with_capability_scope(["mcp"])
    }

    /// 转换为 CatalogEntry。
    pub fn to_catalog_entry(&self) -> crate::runtime::tools::CatalogEntry {
        crate::runtime::tools::CatalogEntry::new(
            self.to_tool_definition(),
            self.input_schema.clone(),
        )
    }
}

/// 与 claude-code-best `buildMcpToolName()` 对齐的最小 helper。
pub fn build_mcp_tool_name(server_name: &str, tool_name: &str) -> String {
    format!(
        "mcp__{}__{}",
        normalize_name_for_mcp(server_name),
        normalize_name_for_mcp(tool_name),
    )
}

/// 最小归一化：去首尾空白，并把空格/点替换为下划线。
pub fn normalize_name_for_mcp(name: &str) -> String {
    name.trim().replace([' ', '.'], "_")
}
