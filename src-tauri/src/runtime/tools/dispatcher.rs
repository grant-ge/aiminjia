use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use anyhow::Result;
use async_trait::async_trait;
use serde_json::Value;

use crate::runtime::hooks::config::HookEvent;
use crate::runtime::hooks::{HookDecision, HookRunner};
use crate::runtime::tools::context::ToolExecutionContext;
use crate::runtime::tools::definition::ToolDefinition;
use crate::runtime::tools::executor::{ToolError, ToolResult};
#[cfg(test)]
use crate::runtime::tools::permission::AllowAllPermissionPipeline;
use crate::runtime::tools::permission::{
    apply_permission_mode, PermissionDecision, PermissionPipeline,
};
use crate::telemetry::{record_diagnostic, DiagnosticEvent, DiagnosticSource};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InterruptBehavior {
    Cancel,
    Block,
}

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
    fn interrupt_behavior(&self) -> InterruptBehavior {
        InterruptBehavior::Block
    }
    fn context_modifier(&self) -> Option<Value> {
        None
    }
    fn validate_input(&self, _input: &Value) -> Option<ToolError> {
        None
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

#[derive(Debug)]
pub enum ToolDispatchOutcome {
    /// The tool completed execution (success or tool-level error encoded in the result).
    Completed {
        result: ToolResult,
        event_names: Vec<String>,
        max_result_size_chars: usize,
        prevent_continuation: bool,
        stop_reason: Option<String>,
        context_modifier_message: Option<Value>,
    },
    /// The permission pipeline returned `Ask` — user confirmation is required.
    /// The decision is returned as-is so the TurnDriver can handle it.
    AskRequired(PermissionDecision),
    /// A tool requires structured user input before it can finish.
    InteractionRequired(Box<crate::runtime::interaction::InteractionRequest>),
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

    pub fn tool_interrupt_behavior(&self, tool_name: &str) -> Option<InterruptBehavior> {
        self.tools
            .read()
            .unwrap()
            .get(tool_name)
            .map(|tool| tool.interrupt_behavior())
    }

    /// Returns whether the named tool reports `is_concurrency_safe` for the given input.
    /// `None` if tool is not registered.
    pub fn tool_concurrency_safe(&self, tool_name: &str, input: &Value) -> Option<bool> {
        self.tools
            .read()
            .unwrap()
            .get(tool_name)
            .map(|tool| tool.is_concurrency_safe(input))
    }

    pub fn tool_definition(&self, tool_name: &str) -> Option<ToolDefinition> {
        self.tools
            .read()
            .unwrap()
            .get(tool_name)
            .map(|tool| tool.definition())
    }

    pub async fn dispatch(
        &self,
        tool_name: &str,
        mut input: Value,
        ctx: ToolExecutionContext,
    ) -> Result<ToolDispatchOutcome, ToolError> {
        let workspace = crate::telemetry::diagnostics_workspace();
        let diag =
            |event: &str, ok: Option<bool>, error: Option<String>, payload: Option<Value>| {
                let mut event_obj = DiagnosticEvent::new(event, DiagnosticSource::Backend)
                    .conversation_id(ctx.session_id.as_str())
                    .run_id(ctx.run_id.as_str())
                    .tool_call_id(ctx.tool_call_id.as_str());
                if let Some(ok) = ok {
                    event_obj = event_obj.ok(ok);
                }
                if let Some(error) = error {
                    event_obj = event_obj.error(error);
                }
                if let Some(payload) = payload {
                    event_obj = event_obj.payload(payload);
                }
                record_diagnostic(&workspace, event_obj);
            };
        let tool = {
            let tools = self.tools.read().unwrap();
            tools
                .get(tool_name)
                .cloned()
                .ok_or_else(|| ToolError::ExecutionFailed(format!("unknown tool: {tool_name}")))?
        };
        let definition = tool.definition();
        diag(
            "tool.execute.started",
            Some(true),
            None,
            Some(serde_json::json!({ "toolName": tool_name })),
        );

        if let Some(registry) = ctx.hook_registry.as_ref() {
            let runner = HookRunner::new();
            let hooks = registry.hooks_for(HookEvent::PreToolUse, tool_name);
            if !hooks.is_empty() {
                let workspace_root = ctx
                    .capability
                    .as_ref()
                    .and_then(|cap| cap.storage.as_ref())
                    .map(|storage| storage.workspace_path.as_path());
                let outcome = runner
                    .run_hooks_in_workspace(&hooks, tool_name, &input, workspace_root)
                    .await
                    .map_err(|err| {
                        ToolError::ExecutionFailed(format!("pre-tool hook error: {err}"))
                    })?;
                match outcome.decision {
                    HookDecision::Deny { message } => {
                        return Err(ToolError::PermissionDenied(message));
                    }
                    HookDecision::Allow => {
                        if let Some(updated_input) = outcome.updated_input {
                            input = updated_input;
                        }
                    }
                }
            }
        }

        let permission_decision = if let Some(decision) = ctx.permission_override.clone() {
            decision
        } else if let Some(decision) = tool.check_permissions(&input, &ctx).await {
            decision
        } else {
            self.permission_pipeline
                .authorize(&definition, &input, &ctx)
        };
        let permission_decision =
            apply_permission_mode(permission_decision, &definition.id, ctx.permission_mode);

        // Map PermissionDecision to ToolError / AskRequired.
        // Deny → Err(PermissionDenied)
        // Ask  → Ok(AskRequired) so the TurnDriver can handle user confirmation
        match permission_decision {
            PermissionDecision::Allow { .. } => {}
            PermissionDecision::Deny { message, .. } => {
                diag(
                    "permission.resolve.completed",
                    Some(true),
                    None,
                    Some(serde_json::json!({ "toolName": tool_name, "resolution": "deny" })),
                );
                return Err(ToolError::PermissionDenied(message));
            }
            decision @ PermissionDecision::Ask { .. } => {
                diag(
                    "permission.resolve.completed",
                    Some(true),
                    None,
                    Some(serde_json::json!({ "toolName": tool_name, "resolution": "ask" })),
                );
                return Ok(ToolDispatchOutcome::AskRequired(decision));
            }
        }
        if let Some(validation_err) = tool.validate_input(&input) {
            return Err(validation_err);
        }
        let context_modifier_message = if !tool.is_concurrency_safe(&input) {
            tool.context_modifier()
        } else {
            None
        };
        ctx.event_sink.emit("tool:executing");
        let result = tool.execute(input, ctx.clone()).await;
        if let Err(ToolError::AskRequired(decision)) = result {
            let decision = apply_permission_mode(decision, &definition.id, ctx.permission_mode);
            return match decision {
                PermissionDecision::Allow { .. } => Err(ToolError::ExecutionFailed(
                    "tool returned AskRequired transformed into Allow unexpectedly".into(),
                )),
                PermissionDecision::Deny { message, .. } => {
                    Err(ToolError::PermissionDenied(message))
                }
                decision @ PermissionDecision::Ask { .. } => {
                    Ok(ToolDispatchOutcome::AskRequired(decision))
                }
            };
        }
        if let Err(ToolError::InteractionRequired(request)) = result {
            diag(
                "interaction.required.received",
                Some(true),
                None,
                Some(serde_json::json!({
                    "toolName": tool_name,
                    "interactionId": request.interaction_id.as_str(),
                })),
            );
            return Ok(ToolDispatchOutcome::InteractionRequired(request));
        }
        ctx.event_sink.emit("tool:completed");
        let result = result?;
        diag(
            "tool.execute.completed",
            Some(true),
            None,
            Some(serde_json::json!({
                "toolName": tool_name,
                "isError": false,
            })),
        );
        let mut prevent_continuation = false;
        let mut stop_reason = None;
        if let Some(registry) = ctx.hook_registry.as_ref() {
            let runner = HookRunner::new();
            let hooks = registry.hooks_for(HookEvent::PostToolUse, tool_name);
            if !hooks.is_empty() {
                let result_value = serde_json::to_value(&result.content).unwrap_or(Value::Null);
                let workspace_root = ctx
                    .capability
                    .as_ref()
                    .and_then(|cap| cap.storage.as_ref())
                    .map(|storage| storage.workspace_path.as_path());
                if let Ok(outcome) = runner
                    .run_hooks_in_workspace(&hooks, tool_name, &result_value, workspace_root)
                    .await
                {
                    prevent_continuation = outcome.prevent_continuation;
                    stop_reason = outcome.stop_reason;
                }
            }
        }
        Ok(ToolDispatchOutcome::Completed {
            result,
            event_names: ctx.event_sink.snapshot(),
            max_result_size_chars: definition.default_max_result_size_chars,
            prevent_continuation,
            stop_reason,
            context_modifier_message,
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
                batches.last_mut().unwrap().calls.push((name, input, ctx));
            }
        }

        let mut results = Vec::new();

        for batch in batches {
            if batch.concurrent {
                for chunk in batch.calls.chunks(MAX_CONCURRENCY) {
                    let futures = chunk
                        .iter()
                        .map(|(name, input, ctx)| self.dispatch(name, input.clone(), ctx.clone()));
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
