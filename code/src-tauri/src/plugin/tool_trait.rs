//! ToolPlugin trait — MCP-style protocol for tool plugins.
//!
//! Each tool exposes a JSON Schema for inputs, executes against a shared
//! [`PluginContext`], and returns structured [`ToolOutput`].
#![allow(dead_code)]

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use super::context::PluginContext;

/// Tool execution output.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolOutput {
    pub content: String,
    pub is_error: bool,
    /// Optional structured data (for programmatic consumption).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<serde_json::Value>,
    /// IDs of files generated during execution.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub generated_files: Vec<String>,
}

impl ToolOutput {
    /// Create a successful output with text content.
    pub fn success(content: impl Into<String>) -> Self {
        Self {
            content: content.into(),
            is_error: false,
            data: None,
            generated_files: Vec::new(),
        }
    }

    /// Create an error output.
    pub fn error(content: impl Into<String>) -> Self {
        Self {
            content: content.into(),
            is_error: true,
            data: None,
            generated_files: Vec::new(),
        }
    }
}

/// Errors from tool execution.
#[derive(Debug, thiserror::Error)]
pub enum ToolError {
    #[error("Missing required argument: {0}")]
    MissingArgument(String),

    #[error("Invalid argument: {0}")]
    InvalidArgument(String),

    #[error("Execution failed: {0}")]
    ExecutionFailed(String),

    #[error("{0}")]
    Other(#[from] anyhow::Error),
}

/// Tool plugin interface — MCP-style (JSON Schema input → execute → structured output).
#[async_trait]
pub trait ToolPlugin: Send + Sync + 'static {
    /// Unique tool identifier (e.g., "web_search").
    fn name(&self) -> &str;

    /// Short description (LLM uses this to understand the tool's purpose).
    fn description(&self) -> &str;

    /// Input parameter JSON Schema (LLM uses this to construct call arguments).
    fn input_schema(&self) -> serde_json::Value;

    /// Execute the tool.
    async fn execute(
        &self,
        ctx: &PluginContext,
        input: serde_json::Value,
    ) -> Result<ToolOutput, ToolError>;
}
