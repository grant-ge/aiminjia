//! execute_python as RuntimeTool.
//!
//! This is the smallest Phase 3 migration slice:
//! - `stub()` exists for tests and non-production wiring
//! - `check_permissions()` performs static dangerous-code detection
//! - `execute()` remains a placeholder until the full PythonExecution boundary lands

use anyhow::Result;
use async_trait::async_trait;
use serde_json::Value;

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
}

impl ExecutePythonRuntimeTool {
    pub fn stub() -> Self {
        Self { stub_mode: true }
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
        _input: Value,
        _ctx: ToolExecutionContext,
    ) -> Result<ToolResult, ToolError> {
        if self.stub_mode {
            return Err(ToolError::ExecutionFailed(
                "ExecutePythonRuntimeTool: stub mode, real execution not available".into(),
            ));
        }

        Err(ToolError::ExecutionFailed(
            "execute_python full RuntimeTool migration pending".into(),
        ))
    }
}
