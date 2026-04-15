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

    /// The permission pipeline returned Ask — user confirmation is required
    /// before this tool can run. Callers that cannot surface a UI prompt
    /// should treat this as a deny and log appropriately.
    #[error("Permission Ask required: {0}")]
    AskRequired(crate::runtime::tools::permission::PermissionDecision),

    #[error("{0}")]
    Other(#[from] anyhow::Error),
}

/// Tool plugin interface — MCP-style (JSON Schema input → execute → structured output).
///
/// # Deprecated — migrate to [`crate::runtime::tools::RuntimeTool`]
///
/// This trait is **legacy**.  It was the original plugin contract when all tools
/// received a full [`PluginContext`] containing every system service.  As of
/// Phase 2 of the runtime refactor, new tools should instead implement the
/// narrower [`crate::runtime::tools::RuntimeTool`] trait, which:
///
/// - Accepts a [`crate::runtime::tools::ToolExecutionContext`] (identity +
///   cancellation + event sink) and optionally a
///   [`crate::runtime::tools::CapabilityContext`] (scoped services).
/// - Does **not** receive `LlmGateway`, `AgentRuntime`, `AuthManager`, or other
///   orchestration-layer services — those must be accessed through dedicated APIs.
///
/// ## Migration guide
///
/// 1. Replace `impl ToolPlugin for MyTool` with
///    `impl RuntimeTool for MyTool` (from `crate::runtime::tools`).
/// 2. Change the `execute` signature from
///    `async fn execute(&self, ctx: &PluginContext, input: Value) -> Result<ToolOutput, ToolError>`
///    to
///    `async fn execute(&self, input: Value, ctx: ToolExecutionContext) -> Result<ToolResult, ToolError>`.
/// 3. Access services via `ctx.capability` (a
///    [`crate::runtime::tools::CapabilityContext`]) instead of the full `PluginContext`.
/// 4. Wrap any field that genuinely requires `PluginContext` (e.g. `gateway`)
///    in its own orchestration helper rather than threading it through the tool.
///
/// ## Bridging during the migration window
///
/// Existing implementations do **not** need to be migrated all at once.
/// [`crate::runtime::tools::LegacyToolAdapter::from_plugin`] wraps a
/// `dyn ToolPlugin` as a `RuntimeTool`, so legacy tools continue to work
/// through the dispatcher.  Only new tools should implement `RuntimeTool`
/// directly.
#[deprecated(
    since = "0.4.0",
    note = "Implement `crate::runtime::tools::RuntimeTool` instead. \
            See doc-comment on this trait for the migration guide. \
            Legacy impls are still supported via LegacyToolAdapter::from_plugin."
)]
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
