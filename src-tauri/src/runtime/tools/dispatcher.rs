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
    async fn check_permissions(
        &self,
        _input: &Value,
        _ctx: &ToolExecutionContext,
    ) -> Option<crate::runtime::tools::permission::PermissionDecision> {
        None
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
        let permission_decision = if let Some(decision) = tool.check_permissions(&input, &ctx).await
        {
            decision
        } else {
            self.permission_pipeline.authorize(&definition, &input, &ctx)
        };

        // Map PermissionDecision to ToolError / AskRequired.
        // Deny → Err(PermissionDenied)
        // Ask  → Ok(AskRequired) so the TurnDriver can handle user confirmation
        match permission_decision {
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

    pub async fn dispatch_batch(
        &self,
        calls: Vec<(String, Value, ToolExecutionContext)>,
    ) -> Vec<Result<ToolDispatchOutcome, ToolError>> {
        const MAX_CONCURRENCY: usize = 10;

        struct Batch {
            concurrent: bool,
            calls: Vec<(String, Value, ToolExecutionContext)>,
        }

        let mut batches: Vec<Batch> = Vec::new();

        for (name, input, ctx) in calls {
            let is_concurrent = {
                let tools = self.tools.read().unwrap();
                tools
                    .get(&name)
                    .map(|tool| tool.is_concurrency_safe(&input))
                    .unwrap_or(false)
            };

            let should_start_new_batch = match batches.last() {
                None => true,
                Some(batch) if !is_concurrent => true,
                Some(batch) => !batch.concurrent,
            };

            if should_start_new_batch {
                batches.push(Batch {
                    concurrent: is_concurrent,
                    calls: vec![(name, input, ctx)],
                });
            } else {
                batches
                    .last_mut()
                    .unwrap()
                    .calls
                    .push((name, input, ctx));
            }
        }

        let mut results = Vec::new();

        for batch in batches {
            if batch.concurrent {
                for chunk in batch.calls.chunks(MAX_CONCURRENCY) {
                    let futures = chunk.iter().map(|(name, input, ctx)| {
                        self.dispatch(name, input.clone(), ctx.clone())
                    });
                    results.extend(futures::future::join_all(futures).await);
                }
            } else {
                for (name, input, ctx) in batch.calls {
                    results.push(self.dispatch(&name, input, ctx).await);
                }
            }
        }

        results
    }
}
