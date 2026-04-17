//! execute_python as RuntimeTool.
//!
//! This is the smallest Phase 3 migration slice:
//! - `stub()` exists for tests and non-production wiring
//! - `check_permissions()` performs static dangerous-code detection
//! - `execute()` remains a placeholder until the full PythonExecution boundary lands
#![allow(deprecated)]

use anyhow::Result;
use async_trait::async_trait;
use serde_json::Value;

use crate::plugin::context::PluginContext;
use crate::runtime::tools::catalog::TOOL_CATALOG;
use crate::runtime::tools::context::ToolExecutionContext;
use crate::runtime::tools::definition::ToolDefinition;
use crate::runtime::tools::executor::{ToolError, ToolResult};
use crate::runtime::tools::permission::{PermissionDecision, PermissionReason};
use crate::runtime::tools::RuntimeTool;

const DANGEROUS_PATTERNS: &[&str] = &[
    "__import__('os').system",
    "__import__('subprocess')",
    "subprocess.call",
    "subprocess.Popen",
    "os.system(",
    "os.popen(",
    "exec(compile(",
    "eval(compile(",
];

pub struct ExecutePythonRuntimeTool {
    stub_mode: bool,
    plugin_ctx: Option<PluginContext>,
}

impl ExecutePythonRuntimeTool {
    pub fn new(plugin_ctx: PluginContext) -> Self {
        Self {
            stub_mode: false,
            plugin_ctx: Some(plugin_ctx),
        }
    }

    pub fn stub() -> Self {
        Self {
            stub_mode: true,
            plugin_ctx: None,
        }
    }
}

#[async_trait]
impl RuntimeTool for ExecutePythonRuntimeTool {
    fn definition(&self) -> ToolDefinition {
        TOOL_CATALOG
            .get("execute_python")
            .cloned()
            .unwrap_or_else(|| ToolDefinition::new("execute_python", "Execute Python code"))
    }

    async fn check_permissions(
        &self,
        input: &Value,
        _ctx: &ToolExecutionContext,
    ) -> Option<PermissionDecision> {
        let code = input.get("code").and_then(Value::as_str).unwrap_or("");
        for pattern in DANGEROUS_PATTERNS {
            if code.contains(pattern) {
                return Some(PermissionDecision::Deny {
                    message: format!(
                        "execute_python: dangerous pattern detected: '{}'",
                        pattern
                    ),
                    reason: PermissionReason::Other("static_code_check".into()),
                });
            }
        }
        None
    }

    async fn execute(
        &self,
        input: Value,
        _ctx: ToolExecutionContext,
    ) -> Result<ToolResult, ToolError> {
        if self.stub_mode {
            return Err(ToolError::ExecutionFailed(
                "ExecutePythonRuntimeTool: stub mode, real execution not available".into(),
            ));
        }

        let plugin_ctx = self.plugin_ctx.as_ref().ok_or_else(|| {
            ToolError::ExecutionFailed(
                "ExecutePythonRuntimeTool: missing PluginContext bridge for live execution".into(),
            )
        })?;
        let content = crate::llm::tool_executor::handle_execute_python(plugin_ctx, &input)
            .await
            .map_err(|err| ToolError::ExecutionFailed(err.to_string()))?;

        Ok(ToolResult::new("execute_python", content, None))
    }
}
