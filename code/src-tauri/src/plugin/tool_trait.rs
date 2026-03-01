//! ToolPlugin trait — MCP-style protocol for tool plugins.
//!
//! Each tool exposes a JSON Schema for inputs, executes against a shared
//! [`PluginContext`], and returns structured [`ToolOutput`].
#![allow(dead_code)]

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use super::context::PluginContext;

/// Metadata for a file produced by a file-generating tool.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileMeta {
    pub file_id: String,
    pub file_name: String,
    pub requested_format: String,
    pub actual_format: String,
    pub file_size: u64,
    pub stored_path: String,
    pub category: String,
}

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
    /// Whether the tool output was degraded (e.g. PDF→HTML fallback).
    #[serde(default)]
    pub is_degraded: bool,
    /// Human-readable notice when degradation occurred.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub degradation_notice: Option<String>,
    /// Metadata for the generated file (if any).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file_meta: Option<FileMeta>,
}

impl ToolOutput {
    /// Create a successful output with text content.
    pub fn success(content: impl Into<String>) -> Self {
        Self {
            content: content.into(),
            is_error: false,
            data: None,
            generated_files: Vec::new(),
            is_degraded: false,
            degradation_notice: None,
            file_meta: None,
        }
    }

    /// Create an error output.
    pub fn error(content: impl Into<String>) -> Self {
        Self {
            content: content.into(),
            is_error: true,
            data: None,
            generated_files: Vec::new(),
            is_degraded: false,
            degradation_notice: None,
            file_meta: None,
        }
    }
}

impl From<crate::llm::tool_executor::FileGenResult> for ToolOutput {
    fn from(r: crate::llm::tool_executor::FileGenResult) -> Self {
        let mut output = Self::success(r.content);
        output.is_degraded = r.is_degraded;
        output.degradation_notice = r.degradation_notice;
        output.file_meta = Some(r.file_meta);
        output
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
