//! browse_data as request-scoped RuntimeTool.
//!
//! The runtime tool owns only the stable launcher boundary. Parent run/agent
//! identity plus cancellation are taken from `ToolExecutionContext` on each
//! invocation so sub-agent launches stay aligned with the active tool call.

use std::sync::Arc;

use anyhow::Result;
use async_trait::async_trait;
use serde_json::Value;

use crate::runtime::cancellation::CancellationToken;
use crate::runtime::ids::{AgentId, RunId, SessionId};
use crate::runtime::tools::catalog::TOOL_CATALOG;
use crate::runtime::tools::context::ToolExecutionContext;
use crate::runtime::tools::definition::ToolDefinition;
use crate::runtime::tools::executor::{ToolError, ToolResult};
use crate::runtime::tools::permission::PermissionMode;
use crate::runtime::tools::RuntimeTool;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BrowseDataLaunchRequest {
    pub task: String,
    pub url: Option<String>,
}

#[derive(Clone)]
pub struct BrowseDataLaunchContext {
    pub session_id: SessionId,
    pub parent_run_id: Option<RunId>,
    pub parent_agent_id: Option<AgentId>,
    pub cancellation: CancellationToken,
    pub permission_mode: PermissionMode,
}

#[derive(Clone, Debug)]
pub struct BrowseDataLaunchResult {
    pub content: String,
    pub ask_decision: Option<crate::runtime::tools::permission::PermissionDecision>,
}

impl BrowseDataLaunchResult {
    pub fn completed(content: impl Into<String>) -> Self {
        Self {
            content: content.into(),
            ask_decision: None,
        }
    }

    pub fn ask(decision: crate::runtime::tools::permission::PermissionDecision) -> Self {
        Self {
            content: String::new(),
            ask_decision: Some(decision),
        }
    }
}

#[async_trait]
pub trait BrowseDataLauncher: Send + Sync {
    async fn launch(
        &self,
        request: BrowseDataLaunchRequest,
        context: BrowseDataLaunchContext,
    ) -> Result<BrowseDataLaunchResult>;
}

pub struct BrowseDataRuntimeTool {
    launcher: Arc<dyn BrowseDataLauncher>,
}

impl BrowseDataRuntimeTool {
    pub fn with_launcher(launcher: Arc<dyn BrowseDataLauncher>) -> Self {
        Self { launcher }
    }
}

#[async_trait]
impl RuntimeTool for BrowseDataRuntimeTool {
    fn definition(&self) -> ToolDefinition {
        TOOL_CATALOG
            .get("browse_data")
            .unwrap_or_else(|| ToolDefinition::new("browse_data", "Browse data"))
    }

    async fn execute(
        &self,
        input: Value,
        ctx: ToolExecutionContext,
    ) -> Result<ToolResult, ToolError> {
        let task = input
            .get("task")
            .and_then(Value::as_str)
            .ok_or_else(|| ToolError::ExecutionFailed("Missing required: task".into()))?;
        let request = BrowseDataLaunchRequest {
            task: task.to_string(),
            url: input.get("url").and_then(Value::as_str).map(str::to_string),
        };
        let launch_ctx = BrowseDataLaunchContext {
            session_id: ctx.session_id.clone(),
            parent_run_id: Some(ctx.run_id.clone()),
            parent_agent_id: ctx.agent_id.clone(),
            cancellation: ctx.cancellation.clone(),
            permission_mode: ctx.permission_mode,
        };

        let launch_result = self
            .launcher
            .launch(request, launch_ctx)
            .await
            .map_err(|err| ToolError::ExecutionFailed(err.to_string()))?;
        if let Some(decision) = launch_result.ask_decision {
            return Err(ToolError::AskRequired(decision));
        }
        Ok(ToolResult::new("browse_data", launch_result.content, None))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::tools::permission::PermissionMode;
    use serde_json::json;
    use std::sync::Mutex;

    struct RecordingLauncher {
        seen_context: Arc<Mutex<Option<BrowseDataLaunchContext>>>,
    }

    #[async_trait]
    impl BrowseDataLauncher for RecordingLauncher {
        async fn launch(
            &self,
            _request: BrowseDataLaunchRequest,
            context: BrowseDataLaunchContext,
        ) -> Result<BrowseDataLaunchResult> {
            *self.seen_context.lock().unwrap() = Some(context);
            Ok(BrowseDataLaunchResult::completed("done"))
        }
    }

    #[tokio::test]
    async fn execute_passes_parent_permission_mode_to_launch_context() {
        let seen_context = Arc::new(Mutex::new(None));
        let tool = BrowseDataRuntimeTool::with_launcher(Arc::new(RecordingLauncher {
            seen_context: seen_context.clone(),
        }));
        let ctx = ToolExecutionContext::for_test("conv-browse-mode", "run-browse-mode", "tc-1")
            .with_permission_mode(PermissionMode::DontAsk);

        tool.execute(json!({ "task": "collect rows" }), ctx)
            .await
            .expect("browse_data execution should succeed");

        let launch_ctx = seen_context
            .lock()
            .unwrap()
            .clone()
            .expect("launcher should receive a launch context");
        assert_eq!(launch_ctx.permission_mode, PermissionMode::DontAsk);
    }
}
