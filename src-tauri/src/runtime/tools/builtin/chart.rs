//! generate_chart as RuntimeTool.
//!
//! This is the minimum migration skeleton for Phase 3:
//! - `stub()` is available for tests and non-production wiring
//! - `execute()` stays a placeholder until the chart capability boundary lands

use anyhow::Result;
use async_trait::async_trait;
use serde_json::Value;

use crate::runtime::tools::catalog::TOOL_CATALOG;
use crate::runtime::tools::context::ToolExecutionContext;
use crate::runtime::tools::definition::ToolDefinition;
use crate::runtime::tools::executor::{ToolError, ToolResult};
use crate::runtime::tools::RuntimeTool;

pub struct GenerateChartRuntimeTool {
    stub_mode: bool,
}

impl GenerateChartRuntimeTool {
    pub fn stub() -> Self {
        Self { stub_mode: true }
    }
}

#[async_trait]
impl RuntimeTool for GenerateChartRuntimeTool {
    fn definition(&self) -> ToolDefinition {
        TOOL_CATALOG
            .get("generate_chart")
            .cloned()
            .unwrap_or_else(|| ToolDefinition::new("generate_chart", "Generate chart"))
    }

    async fn execute(
        &self,
        _input: Value,
        _ctx: ToolExecutionContext,
    ) -> Result<ToolResult, ToolError> {
        if self.stub_mode {
            return Err(ToolError::ExecutionFailed(
                "GenerateChartRuntimeTool: stub mode".into(),
            ));
        }

        Err(ToolError::ExecutionFailed(
            "generate_chart full migration pending".into(),
        ))
    }
}
