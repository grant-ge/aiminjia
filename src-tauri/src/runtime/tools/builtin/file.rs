//! load_file as RuntimeTool.
//!
//! `LoadFileRuntimeTool` receives its file-loading capability through
//! `CapabilityContext.file_ops` — a [`crate::runtime::tools::capability::FileOperations`]
//! accessor injected at the orchestration layer.  It does **not** accept or
//! reconstruct a `PluginContext`.

use anyhow::Result;
use async_trait::async_trait;
use serde_json::Value;

use crate::runtime::tools::catalog::TOOL_CATALOG;
use crate::runtime::tools::context::ToolExecutionContext;
use crate::runtime::tools::definition::ToolDefinition;
use crate::runtime::tools::executor::{ToolError, ToolResult};
use crate::runtime::tools::RuntimeTool;

// ── LoadFileRuntimeTool ───────────────────────────────────────────────────────

pub struct LoadFileRuntimeTool;

impl LoadFileRuntimeTool {
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl RuntimeTool for LoadFileRuntimeTool {
    fn definition(&self) -> ToolDefinition {
        TOOL_CATALOG
            .get("load_file")
            .unwrap_or_else(|| {
                ToolDefinition::new(
                    "load_file",
                    "加载已上传文件，使数据可在 execute_python 中以 _df/_text 变量使用",
                )
            })
    }

    async fn execute(&self, input: Value, ctx: ToolExecutionContext) -> Result<ToolResult, ToolError> {
        let file_ops = ctx
            .capability
            .as_ref()
            .and_then(|cap| cap.file_ops.as_ref())
            .ok_or_else(|| {
                ToolError::Other(anyhow::anyhow!(
                    "load_file requires FileOperations capability, but file_ops is None"
                ))
            })?;

        let loaded = file_ops
            .load_file(&input)
            .await
            .map_err(ToolError::Other)?;

        Ok(ToolResult::new(
            "load_file",
            loaded.content.clone(),
            Some(Value::String(loaded.content)),
        ))
    }
}
