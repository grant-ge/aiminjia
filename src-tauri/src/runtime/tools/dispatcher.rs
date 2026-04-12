use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use anyhow::Result;
use async_trait::async_trait;
use serde_json::Value;

use crate::runtime::tools::context::ToolExecutionContext;
use crate::runtime::tools::definition::ToolDefinition;
use crate::runtime::tools::executor::{ToolError, ToolResult};
use crate::runtime::tools::permission::{AllowAllPermissionPipeline, PermissionPipeline};

#[async_trait]
pub trait RuntimeTool: Send + Sync {
    fn definition(&self) -> ToolDefinition;
    async fn execute(
        &self,
        input: Value,
        ctx: ToolExecutionContext,
    ) -> Result<ToolResult, ToolError>;
}

pub struct ToolDispatchOutcome {
    pub result: ToolResult,
    pub event_names: Vec<String>,
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
        self.permission_pipeline
            .authorize(&definition, &input, &ctx)
            .map_err(|err| ToolError::PermissionDenied(err.to_string()))?;
        ctx.event_sink.emit("tool:executing");
        let result = tool.execute(input, ctx.clone()).await;
        ctx.event_sink.emit("tool:completed");
        let result = result?;
        Ok(ToolDispatchOutcome {
            result,
            event_names: ctx.event_sink.snapshot(),
        })
    }
}
