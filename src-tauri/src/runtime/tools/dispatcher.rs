use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use anyhow::Result;
use async_trait::async_trait;
use serde_json::Value;

use crate::runtime::tools::context::ToolExecutionContext;
use crate::runtime::tools::definition::ToolDefinition;
use crate::runtime::tools::executor::{ToolError, ToolResult};
use crate::runtime::tools::permission::{PermissionDecision, PermissionPipeline};
#[cfg(test)]
use crate::runtime::tools::permission::AllowAllPermissionPipeline;

#[async_trait]
pub trait RuntimeTool: Send + Sync {
    fn definition(&self) -> ToolDefinition;
    fn is_concurrency_safe(&self, _input: &Value) -> bool {
        false
    }
    fn is_read_only(&self, _input: &Value) -> bool {
        self.definition().default_read_only
    }
    fn is_destructive(&self, _input: &Value) -> bool {
        self.definition().default_destructive
    }
    async fn execute(
        &self,
        input: Value,
        ctx: ToolExecutionContext,
    ) -> Result<ToolResult, ToolError>;
}

pub enum ToolDispatchOutcome {
    /// The tool completed execution (success or tool-level error encoded in the result).
    Completed {
        result: ToolResult,
        event_names: Vec<String>,
    },
    /// The permission pipeline returned `Ask` — user confirmation is required.
    /// The decision is returned as-is so the TurnDriver can handle it.
    AskRequired(PermissionDecision),
}

pub struct ToolDispatcher {
    tools: RwLock<HashMap<String, Arc<dyn RuntimeTool>>>,
    permission_pipeline: Arc<dyn PermissionPipeline>,
}

impl ToolDispatcher {
    pub fn new(permission_pipeline: Arc<dyn PermissionPipeline>) -> Self {
        Self {
            tools: RwLock::new(HashMap::new()),
            permission_pipeline,
        }
    }

    #[cfg(test)]
    pub fn allow_all() -> Self {
        Self::new(Arc::new(AllowAllPermissionPipeline))
    }

    pub fn register(&self, tool: Arc<dyn RuntimeTool>) {
        self.tools
            .write()
            .unwrap()
            .insert(tool.definition().id.clone(), tool);
    }

    pub async fn dispatch(
        &self,
        tool_name: &str,
        input: Value,
        ctx: ToolExecutionContext,
    ) -> Result<ToolDispatchOutcome, ToolError> {
        let tool = {
            let tools = self.tools.read().unwrap();
            tools
                .get(tool_name)
                .cloned()
                .ok_or_else(|| ToolError::ExecutionFailed(format!("unknown tool: {tool_name}")))?
        };
        let definition = tool.definition();
        // Map PermissionDecision to ToolError / AskRequired.
        // Deny → Err(PermissionDenied)
        // Ask  → Ok(AskRequired) so the TurnDriver can handle user confirmation
        match self.permission_pipeline.authorize(&definition, &input, &ctx) {
            PermissionDecision::Allow { .. } => {}
            PermissionDecision::Deny { message, .. } => {
                return Err(ToolError::PermissionDenied(message));
            }
            decision @ PermissionDecision::Ask { .. } => {
                return Ok(ToolDispatchOutcome::AskRequired(decision));
            }
        }
        ctx.event_sink.emit("tool:executing");
        let result = tool.execute(input, ctx.clone()).await;
        ctx.event_sink.emit("tool:completed");
        let result = result?;
        Ok(ToolDispatchOutcome::Completed {
            result,
            event_names: ctx.event_sink.snapshot(),
        })
    }
}
