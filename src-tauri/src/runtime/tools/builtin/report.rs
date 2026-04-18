//! generate_report as RuntimeTool.

use anyhow::Result;
use async_trait::async_trait;
use serde_json::Value;
use std::sync::Arc;

use crate::runtime::tools::builtin::report_capability::ReportCapability;
use crate::runtime::tools::catalog::TOOL_CATALOG;
use crate::runtime::tools::context::ToolExecutionContext;
use crate::runtime::tools::definition::ToolDefinition;
use crate::runtime::tools::executor::{ToolError, ToolResult};
use crate::runtime::tools::RuntimeTool;

pub struct GenerateReportRuntimeTool {
    stub_mode: bool,
    capability: Option<Arc<dyn ReportCapability>>,
}

impl GenerateReportRuntimeTool {
    pub fn stub() -> Self {
        Self {
            stub_mode: true,
            capability: None,
        }
    }

    pub fn with_capability(capability: Arc<dyn ReportCapability>) -> Self {
        Self {
            stub_mode: false,
            capability: Some(capability),
        }
    }
}

#[async_trait]
impl RuntimeTool for GenerateReportRuntimeTool {
    fn definition(&self) -> ToolDefinition {
        TOOL_CATALOG
            .get("generate_report")
            .unwrap_or_else(|| ToolDefinition::new("generate_report", "Generate report"))
    }

    async fn execute(
        &self,
        input: Value,
        ctx: ToolExecutionContext,
    ) -> Result<ToolResult, ToolError> {
        if self.stub_mode {
            return Err(ToolError::ExecutionFailed(
                "GenerateReportRuntimeTool: stub mode".into(),
            ));
        }

        let capability = self.capability.as_ref().ok_or_else(|| {
            ToolError::ExecutionFailed("GenerateReportRuntimeTool: missing ReportCapability".into())
        })?;
        let storage_cap = ctx
            .capability
            .as_ref()
            .and_then(|cap| cap.storage.as_ref())
            .ok_or_else(|| {
                ToolError::ExecutionFailed(
                    "GenerateReportRuntimeTool: missing workspace capability".into(),
                )
            })?;

        let params = crate::llm::tool_executor::ReportCoreParams {
            workspace_path: &storage_cap.workspace_path,
            authorized_workspace: storage_cap.authorized_workspace.as_ref(),
            conversation_id: ctx.session_id.as_str(),
        };

        let generated = crate::llm::tool_executor::handle_generate_report_core(
            &params,
            &input,
            capability.as_ref(),
        )
        .await
        .map_err(|err| ToolError::ExecutionFailed(err.to_string()))?;

        let mut result = ToolResult::new("generate_report", generated.content, None);
        result.file_meta = Some(generated.file_meta);
        result.is_degraded = generated.is_degraded;
        result.degradation_notice = generated.degradation_notice;
        Ok(result)
    }
}
