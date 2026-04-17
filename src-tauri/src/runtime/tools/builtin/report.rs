//! generate_report as RuntimeTool.
//!
//! This is the minimum migration skeleton for Phase 3:
//! - `stub()` is available for tests and non-production wiring
//! - `execute()` stays a placeholder until the report capability boundary lands

use anyhow::Result;
use async_trait::async_trait;
use serde_json::Value;

use crate::runtime::tools::catalog::TOOL_CATALOG;
use crate::runtime::tools::context::ToolExecutionContext;
use crate::runtime::tools::definition::ToolDefinition;
use crate::runtime::tools::executor::{ToolError, ToolResult};
use crate::runtime::tools::RuntimeTool;

pub struct GenerateReportRuntimeTool {
    stub_mode: bool,
}

impl GenerateReportRuntimeTool {
    pub fn stub() -> Self {
        Self { stub_mode: true }
    }
}

#[async_trait]
impl RuntimeTool for GenerateReportRuntimeTool {
    fn definition(&self) -> ToolDefinition {
        TOOL_CATALOG
            .get("generate_report")
            .cloned()
            .unwrap_or_else(|| ToolDefinition::new("generate_report", "Generate report"))
    }

    async fn execute(
        &self,
        _input: Value,
        _ctx: ToolExecutionContext,
    ) -> Result<ToolResult, ToolError> {
        if self.stub_mode {
            return Err(ToolError::ExecutionFailed(
                "GenerateReportRuntimeTool: stub mode".into(),
            ));
        }

        Err(ToolError::ExecutionFailed(
            "generate_report full migration pending".into(),
        ))
    }
}
