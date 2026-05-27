use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use anyhow::Result;
use async_trait::async_trait;
use serde_json::Value;

use crate::runtime::hooks::config::HookEvent;
use crate::runtime::hooks::{HookDecision, HookRunner};
use crate::runtime::tools::context::ToolExecutionContext;
use crate::runtime::tools::definition::ToolDefinition;
use crate::runtime::tools::description_context::ToolDescriptionContext;
use crate::runtime::tools::executor::{ToolError, ToolResult};
#[cfg(test)]
use crate::runtime::tools::permission::AllowAllPermissionPipeline;
use crate::runtime::tools::permission::{
    apply_async_auto_deny, apply_permission_mode, PermissionDecision, PermissionPipeline,
};
use crate::telemetry::{record_diagnostic, DiagnosticEvent, DiagnosticSource};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InterruptBehavior {
    Cancel,
    Block,
}

/// Coarse-grained category for any `ToolError` that leaves the dispatcher.
/// Mirrors the variants we care about for SLS aggregation — the server
/// dashboards can group by `category` without parsing free-form messages.
///
/// `bash`/`PowerShell` already emit a finer-grained `tool.{tool}.failure`
/// event via `emit_shell_failure_diagnostic` (with stderr signature and
/// exit-code category). The metric written here is the **only** failure
/// signal for every other tool (Read / Write / Edit / Fetch / MCP / …), so
/// the gap closes the moment this lands.
fn failure_category(err: &ToolError) -> &'static str {
    match err {
        ToolError::PermissionDenied(_) => "permission_denied",
        ToolError::InputValidationError { .. } => "input_validation",
        ToolError::ExecutionFailed(_) => "execution_failed",
        ToolError::Other(_) => "other",
        // AskRequired / InteractionRequired are filtered out by the caller —
        // they are not real failures. Keep an explicit category in case a
        // bug surfaces one through this path so the SLS row tells us so.
        ToolError::AskRequired(_) => "ask_required_leak",
        ToolError::InteractionRequired(_) => "interaction_required_leak",
    }
}

fn record_tool_failure_metric(
    workspace: &std::path::Path,
    session_id: &str,
    run_id: &str,
    tool_call_id: &str,
    tool_name: &str,
    err: &ToolError,
) {
    let category = failure_category(err);
    let message = err.to_string();
    // Truncate to keep metrics.jsonl entries small — full text already
    // lives in `renlijia.log` for the same run_id.
    let trimmed: String = message.chars().take(400).collect();
    let event = DiagnosticEvent::new("tool.execute.failed", DiagnosticSource::Backend)
        .conversation_id(session_id)
        .run_id(run_id)
        .tool_call_id(tool_call_id)
        .ok(false)
        .error(trimmed.clone())
        .payload(serde_json::json!({
            "toolName": tool_name,
            "category": category,
            "errorMessage": trimmed,
        }));
    record_diagnostic(workspace, event);
}

#[async_trait]
pub trait RuntimeTool: Send + Sync {
    /// Stable identifier — must match the name registered with the
    /// runtime and the static `TOOL_CATALOG` key. Sync because callers
    /// at registration / dispatch time need it without context.
    fn id(&self) -> &str;

    /// Default read-only flag — used by permission policies that key off
    /// the tool's nature rather than its input.  Implementations that
    /// vary per-input should override `is_read_only`.
    fn default_read_only(&self) -> bool {
        false
    }

    /// Default destructive flag — symmetric with `default_read_only`.
    fn default_destructive(&self) -> bool {
        false
    }

    /// Render the LLM-facing tool definition for this turn.
    ///
    /// `ctx` carries session-scoped context (available subagent types,
    /// hired employees, connected MCP servers) so a tool whose
    /// description depends on session state — notably the `Agent` tool
    /// listing dispatchable subagents / employees — can render a fresh
    /// description per turn.  Tools whose description is truly static
    /// can ignore `ctx`.
    ///
    /// Aligns with claude-code-best `tool.prompt({ agents, tools, ... })`
    /// (see `src/utils/api.ts::buildToolBlock` — the result is passed
    /// directly as `description` to the Anthropic Messages API).
    async fn definition(&self, ctx: &ToolDescriptionContext) -> ToolDefinition;

    fn is_concurrency_safe(&self, _input: &Value) -> bool {
        false
    }
    fn is_read_only(&self, _input: &Value) -> bool {
        self.default_read_only()
    }
    fn is_destructive(&self, _input: &Value) -> bool {
        self.default_destructive()
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
            .insert(tool.id().to_string(), tool);
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

    pub async fn tool_definition(&self, tool_name: &str) -> Option<ToolDefinition> {
        let tool = self.tools.read().unwrap().get(tool_name).cloned()?;
        Some(tool.definition(&ToolDescriptionContext::empty()).await)
    }

    pub async fn dispatch(
        &self,
        tool_name: &str,
        input: Value,
        ctx: ToolExecutionContext,
    ) -> Result<ToolDispatchOutcome, ToolError> {
        let workspace = crate::telemetry::diagnostics_workspace();
        // Capture identifiers up-front because dispatch_inner takes ctx by
        // value and we still need them on the failure path.
        let session_id = ctx.session_id.as_str().to_string();
        let run_id = ctx.run_id.as_str().to_string();
        let tool_call_id = ctx.tool_call_id.as_str().to_string();
        let result = self
            .dispatch_inner(tool_name, input, ctx)
            .await;
        if let Err(ref err) = result {
            // Ask / InteractionRequired are not really "failures" — the inner
            // function returns them as Err(ToolError::AskRequired/...) only as
            // an internal control-flow detour, and converts them to
            // Ok(AskRequired/...) outcomes before returning. If one slips out
            // unconverted that *is* a bug worth logging; treat anything
            // reaching here as a real failure.
            record_tool_failure_metric(
                &workspace,
                &session_id,
                &run_id,
                &tool_call_id,
                tool_name,
                err,
            );
        }
        result
    }

    async fn dispatch_inner(
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
        // dispatch-time only needs static fields (default_max_result_size_chars,
        // default_read_only/destructive, id). Use empty description ctx —
        // rendering happens upstream when building the LLM tools array.
        let definition = tool.definition(&ToolDescriptionContext::empty()).await;
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
        let permission_decision =
            apply_async_auto_deny(permission_decision, &definition.id, ctx.is_async);

        log::info!(
            "[dispatcher][permission-trace] tool='{}' is_async={} mode={:?} decision={}",
            definition.id,
            ctx.is_async,
            ctx.permission_mode,
            match &permission_decision {
                PermissionDecision::Allow { .. } => "Allow".to_string(),
                PermissionDecision::Deny { message, .. } => format!("Deny({})", message),
                PermissionDecision::Ask { message, .. } => format!("Ask({})", message),
            }
        );

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
            let decision = apply_async_auto_deny(decision, &definition.id, ctx.is_async);
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
                Some(_) if !is_concurrent => true,
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

#[cfg(test)]
mod failure_metric_tests {
    use super::*;
    use tempfile::TempDir;

    /// Reads {workspace}/logs/metrics.jsonl and returns the parsed events.
    /// Each line is `<json>\t<plaintext-trailer>`; we only care about the
    /// JSON head per `parse_metrics_jsonl_line` upstream.
    fn read_metrics(workspace: &std::path::Path) -> Vec<serde_json::Value> {
        let path = workspace.join("logs").join("metrics.jsonl");
        let raw = std::fs::read_to_string(&path).expect("metrics.jsonl missing");
        raw.lines()
            .filter(|l| !l.trim().is_empty())
            .map(|l| {
                let json = l.split_once('\t').map_or(l, |(json, _)| json);
                serde_json::from_str(json).unwrap()
            })
            .collect()
    }

    #[test]
    fn records_execution_failed_with_category() {
        let dir = TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join("logs")).unwrap();
        let err = ToolError::ExecutionFailed("boom".into());
        record_tool_failure_metric(dir.path(), "conv-1", "run-1", "tc-1", "Read", &err);

        let events = read_metrics(dir.path());
        let row = events
            .iter()
            .find(|e| e["event"] == "tool.execute.failed")
            .expect("tool.execute.failed event not written");
        assert_eq!(row["payload"]["toolName"], "Read");
        assert_eq!(row["payload"]["category"], "execution_failed");
        assert_eq!(row["ok"], false);
        // error & errorMessage must both be present (server dashboards key off
        // either depending on the dashboard).
        assert!(row["error"].as_str().unwrap().contains("boom"));
        assert!(row["payload"]["errorMessage"]
            .as_str()
            .unwrap()
            .contains("boom"));
    }

    #[test]
    fn records_permission_denied_with_category() {
        let dir = TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join("logs")).unwrap();
        let err = ToolError::PermissionDenied("not allowed in workspace".into());
        record_tool_failure_metric(dir.path(), "conv-2", "run-2", "tc-2", "Write", &err);

        let events = read_metrics(dir.path());
        let row = events
            .iter()
            .find(|e| e["event"] == "tool.execute.failed")
            .expect("tool.execute.failed event not written");
        assert_eq!(row["payload"]["category"], "permission_denied");
        assert_eq!(row["payload"]["toolName"], "Write");
    }

    #[test]
    fn records_input_validation_with_category() {
        let dir = TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join("logs")).unwrap();
        let err = ToolError::InputValidationError {
            tool_name: "Edit".into(),
            message: "missing field 'path'".into(),
        };
        record_tool_failure_metric(dir.path(), "conv-3", "run-3", "tc-3", "Edit", &err);

        let events = read_metrics(dir.path());
        let row = events
            .iter()
            .find(|e| e["event"] == "tool.execute.failed")
            .expect("tool.execute.failed event not written");
        assert_eq!(row["payload"]["category"], "input_validation");
    }

    #[test]
    fn truncates_long_error_message() {
        let dir = TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join("logs")).unwrap();
        let big: String = std::iter::repeat('x').take(2000).collect();
        let err = ToolError::ExecutionFailed(big);
        record_tool_failure_metric(dir.path(), "conv-4", "run-4", "tc-4", "Fetch", &err);

        let events = read_metrics(dir.path());
        let row = events
            .iter()
            .find(|e| e["event"] == "tool.execute.failed")
            .expect("tool.execute.failed event not written");
        let stored = row["payload"]["errorMessage"].as_str().unwrap();
        // 400 char chars cap from record_tool_failure_metric, plus the
        // "tool execution failed: " prefix from the Display impl. Stay loose
        // — we only care it's bounded, not exact.
        assert!(stored.chars().count() <= 450, "got {} chars", stored.chars().count());
    }
}
