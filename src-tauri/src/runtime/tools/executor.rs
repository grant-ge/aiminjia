use serde_json::Value;

#[derive(Clone, Debug)]
pub struct ToolResult {
    pub tool_name: String,
    pub content: String,
    pub data: Option<Value>,
}

impl ToolResult {
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
