//! generate_chart as RuntimeTool.

use anyhow::Result;
use async_trait::async_trait;
use serde_json::Value;
use std::sync::Arc;

use crate::runtime::tools::builtin::chart_capability::ChartCapability;
use crate::runtime::tools::catalog::TOOL_CATALOG;
use crate::runtime::tools::context::ToolExecutionContext;
use crate::runtime::tools::definition::ToolDefinition;
use crate::runtime::tools::executor::{ToolError, ToolResult};
use crate::runtime::tools::RuntimeTool;

pub struct GenerateChartRuntimeTool {
    stub_mode: bool,
    capability: Option<Arc<dyn ChartCapability>>,
}

impl GenerateChartRuntimeTool {
    pub fn stub() -> Self {
        Self {
            stub_mode: true,
            capability: None,
        }
    }

    pub fn with_capability(capability: Arc<dyn ChartCapability>) -> Self {
        Self {
            stub_mode: false,
            capability: Some(capability),
        }
    }
}

#[async_trait]
impl RuntimeTool for GenerateChartRuntimeTool {
    fn definition(&self) -> ToolDefinition {
        TOOL_CATALOG
            .get("generate_chart")
            .unwrap_or_else(|| ToolDefinition::new("generate_chart", "Generate chart"))
    }

    async fn execute(
        &self,
        input: Value,
        ctx: ToolExecutionContext,
    ) -> Result<ToolResult, ToolError> {
        if self.stub_mode {
            return Err(ToolError::ExecutionFailed(
                "GenerateChartRuntimeTool: stub mode".into(),
            ));
        }

        let capability = self.capability.as_ref().ok_or_else(|| {
            ToolError::ExecutionFailed("GenerateChartRuntimeTool: missing ChartCapability".into())
        })?;
        let storage_cap = ctx
            .capability
            .as_ref()
            .and_then(|cap| cap.storage.as_ref())
            .ok_or_else(|| {
                ToolError::ExecutionFailed(
                    "GenerateChartRuntimeTool: missing workspace capability".into(),
                )
            })?;

        let params = crate::llm::tool_executor::ChartCoreParams {
            workspace_path: &storage_cap.workspace_path,
            conversation_id: ctx.session_id.as_str(),
        };
        let generated = crate::llm::tool_executor::handle_generate_chart_core(
            &params,
            &input,
            capability.as_ref(),
        )
        .await
        .map_err(|err| ToolError::ExecutionFailed(err.to_string()))?;

        let mut result = ToolResult::new("generate_chart", generated.content, None);
        result.file_meta = Some(generated.file_meta);
        result.is_degraded = generated.is_degraded;
        result.degradation_notice = generated.degradation_notice;
        Ok(result)
    }
}
