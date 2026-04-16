use serde_json::Value;

use crate::plugin::tool_trait::FileMeta;

#[derive(Clone, Debug)]
pub struct ToolResult {
    pub tool_name: String,
    pub content: String,
    pub data: Option<Value>,
    pub file_meta: Option<FileMeta>,
    pub is_degraded: bool,
    pub degradation_notice: Option<String>,
}

impl ToolResult {
    pub fn new(tool_name: &str, content: String, data: Option<Value>) -> Self {
        Self {
            tool_name: tool_name.to_string(),
            content,
            data,
            file_meta: None,
            is_degraded: false,
            degradation_notice: None,
        }
    }

    pub fn output_text(&self) -> &str {
        &self.content
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ToolError {
    #[error("permission denied: {0}")]
    PermissionDenied(String),
    #[error("tool execution failed: {0}")]
    ExecutionFailed(String),
    #[error(transparent)]
    Other(#[from] anyhow::Error),
}
