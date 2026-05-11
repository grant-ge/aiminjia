//! Runtime-owned tool round driver.
//!
//! `ToolRoundDriver` accepts a batch of LLM-issued tool calls, applies the
//! allowed-tools filter, and dispatches permitted calls through the runtime
//! `QueryEngine` (which routes through `ToolDispatcher`).  Blocked calls
//! produce a standardised error message without touching the dispatcher.
//!
//! This module lives in `runtime::chat` and is transport-neutral — it must
//! NOT depend on `tauri::*` or any transport-layer type.

use crate::runtime::cancellation::CancellationReason;
use crate::runtime::chat::tool_round_types::{
    BlockedToolOutcome, RuntimeToolCallOutcome, RuntimeToolCallRequest,
};
use crate::runtime::event_bus::RuntimeEventBus;
use crate::runtime::query_engine::QueryEngine;
use crate::runtime::state::TurnState;
use crate::runtime::tools::InterruptBehavior;
use crate::telemetry::{record_diagnostic, DiagnosticEvent, DiagnosticSource};

/// Unified result for a single tool call within a round.
#[derive(Debug, Clone)]
pub enum ToolRoundResult {
    /// Tool was dispatched and returned an outcome (success or tool-level error).
    Ok(RuntimeToolCallOutcome),
    /// Tool was blocked by the allowed-tools filter before execution.
    Blocked(BlockedToolOutcome),
}

/// Drives a batch of tool calls through the runtime dispatcher.
///
/// Constructed per tool-round inside the agent loop.  Stateless beyond the
/// injected `QueryEngine` and optional allowed-tools filter.
pub struct ToolRoundDriver {
    query_engine: QueryEngine,
    allowed_tools: Option<Vec<String>>,
}

fn record_tool_round_diagnostic(
    turn: &TurnState,
    event: &str,
    tool_call_id: &str,
    tool_name: &str,
    ok: Option<bool>,
    error: Option<String>,
    payload: Option<serde_json::Value>,
) {
    let workspace = crate::telemetry::diagnostics_workspace();
    let mut diag = DiagnosticEvent::new(event, DiagnosticSource::Backend)
        .conversation_id(turn.session_id().as_str())
        .run_id(turn.run_id().as_str())
        .tool_call_id(tool_call_id)
        .payload(serde_json::json!({ "toolName": tool_name }));
    if let Some(ok) = ok {
        diag = diag.ok(ok);
    }
    if let Some(error) = error {
        diag = diag.error(error);
    }
    if let Some(mut payload_value) = payload {
        if let serde_json::Value::Object(ref mut map) = payload_value {
            map.insert("toolName".to_string(), serde_json::json!(tool_name));
        }
        diag = diag.payload(payload_value);
    }
    record_diagnostic(&workspace, diag);
}

impl ToolRoundDriver {
    pub fn new(query_engine: QueryEngine) -> Self {
        Self {
            query_engine,
            allowed_tools: None,
        }
    }

    /// Set the allowed-tools filter.  When `Some`, only tool names in the
    /// vector are dispatched; all others are blocked with a descriptive reason.
    pub fn with_allowed_tools(mut self, allowed: Vec<String>) -> Self {
        self.allowed_tools = Some(allowed);
        self
    }

    /// Convenience: set the filter from an `Option<Vec<String>>`.
    pub fn with_allowed_tools_opt(mut self, allowed: Option<Vec<String>>) -> Self {
        self.allowed_tools = allowed;
        self
    }

    /// Execute a batch of tool calls, returning results in the same order as
    /// the input.
    ///
    /// - Blocked calls are resolved immediately (no dispatcher round-trip).
    /// - A single permitted call is dispatched sequentially.
    /// - Multiple permitted calls are partitioned by `RuntimeTool::is_concurrency_safe`:
    ///   unsafe calls run sequentially, safe calls run concurrently via
    ///   `futures::future::join_all`.
    pub async fn execute_round(
        &self,
        turn: &TurnState,
        bus: &RuntimeEventBus,
        calls: Vec<RuntimeToolCallRequest>,
    ) -> Vec<ToolRoundResult> {
        record_tool_round_diagnostic(
            turn,
            "tool.round.started",
            "",
            "",
            Some(true),
            None,
            Some(serde_json::json!({ "callCount": calls.len() })),
        );
        // Partition into blocked / permitted while preserving original indices.
        let mut results: Vec<(usize, ToolRoundResult)> = Vec::with_capacity(calls.len());
        let mut permitted: Vec<(usize, RuntimeToolCallRequest)> = Vec::new();

        for (idx, call) in calls.into_iter().enumerate() {
            if let Some(blocked) = self.check_blocked(&call) {
                results.push((idx, ToolRoundResult::Blocked(blocked)));
            } else {
                permitted.push((idx, call));
            }
        }

        // Dispatch permitted calls.
        if permitted.len() <= 1 {
            // Sequential dispatch (single tool or empty).
            for (idx, call) in permitted {
                results.push(self.dispatch_serial_call(turn, bus, idx, call).await);
            }
        } else {
            let (safe, unsafe_calls): (Vec<_>, Vec<_>) =
                permitted.into_iter().partition(|(_, call)| {
                    self.query_engine
                        .tool_concurrency_safe(&call.tool_name, &call.args)
                });

            // Non-concurrency-safe tools must honor the RuntimeTool contract and run serially.
            for (idx, call) in unsafe_calls {
                results.push(self.dispatch_serial_call(turn, bus, idx, call).await);
            }

            // Concurrent dispatch for tools that explicitly report concurrency safety.
            if !safe.is_empty() {
                let sibling_cancel = turn.cancellation().child_token();
                let futures: Vec<_> = safe
                    .into_iter()
                    .map(|(idx, call)| {
                        let engine = self.query_engine.clone();
                        let turn_clone = turn.clone();
                        let bus_clone = bus.clone();
                        let sibling_cancel_clone = sibling_cancel.clone();
                        let interrupt_behavior =
                            self.query_engine.tool_interrupt_behavior(&call.tool_name);
                        async move {
                            let tool_cancel = match interrupt_behavior {
                                InterruptBehavior::Cancel => sibling_cancel_clone.child_token(),
                                InterruptBehavior::Block => sibling_cancel_clone
                                    .child_token_ignoring_reason(CancellationReason::Interrupt),
                            };
                            let tool_turn = turn_clone.with_cancellation(tool_cancel);
                            let outcome = engine
                                .run_tool_call_with_bus(&tool_turn, &bus_clone, call.clone())
                                .await;
                            match outcome {
                                Ok(o) => {
                                    if o.is_error() {
                                        sibling_cancel_clone
                                            .cancel_with_reason(CancellationReason::SiblingError);
                                    }
                                    (idx, ToolRoundResult::Ok(o))
                                }
                                Err(e) => {
                                    sibling_cancel_clone
                                        .cancel_with_reason(CancellationReason::SiblingError);
                                    (
                                        idx,
                                        ToolRoundResult::Ok(RuntimeToolCallOutcome::Completed {
                                            tool_call_id: call.tool_call_id,
                                            tool_name: call.tool_name,
                                            content: format!("Error: {}", e),
                                            is_error: true,
                                            msg_id: format!("tool-{}", uuid::Uuid::new_v4()),
                                            file_meta: None,
                                            is_degraded: false,
                                            degradation_notice: None,
                                            max_result_size_chars: 8_000,
                                            context_modifier_message: None,
                                        }),
                                    )
                                }
                            }
                        }
                    })
                    .collect();

                let concurrent_results = futures::future::join_all(futures).await;
                results.extend(concurrent_results);
            }
        }

        // Sort by original index to preserve input ordering.
        results.sort_by_key(|(idx, _)| *idx);
        let final_results: Vec<ToolRoundResult> = results.into_iter().map(|(_, r)| r).collect();
        record_tool_round_diagnostic(
            turn,
            "tool.round.completed",
            "",
            "",
            Some(true),
            None,
            Some(serde_json::json!({ "callCount": final_results.len() })),
        );
        final_results
    }

    async fn dispatch_serial_call(
        &self,
        turn: &TurnState,
        bus: &RuntimeEventBus,
        idx: usize,
        call: RuntimeToolCallRequest,
    ) -> (usize, ToolRoundResult) {
        let outcome = self
            .query_engine
            .run_tool_call_with_bus(turn, bus, call)
            .await;
        match outcome {
            Ok(o) => (idx, ToolRoundResult::Ok(o)),
            Err(e) => {
                // Infrastructure error (dispatcher missing, bus failure, etc.).
                // Wrap as an error outcome so the LLM receives feedback.
                (
                    idx,
                    ToolRoundResult::Ok(RuntimeToolCallOutcome::Completed {
                        tool_call_id: String::new(),
                        tool_name: String::new(),
                        content: format!("Error: {}", e),
                        is_error: true,
                        msg_id: format!("tool-{}", uuid::Uuid::new_v4()),
                        file_meta: None,
                        is_degraded: false,
                        degradation_notice: None,
                        max_result_size_chars: 8_000,
                        context_modifier_message: None,
                    }),
                )
            }
        }
    }

    /// Check whether a call should be blocked by the allowed-tools filter.
    /// Returns `Some(BlockedToolOutcome)` if blocked, `None` if permitted.
    fn check_blocked(&self, call: &RuntimeToolCallRequest) -> Option<BlockedToolOutcome> {
        let allowed = self.allowed_tools.as_ref()?;
        if allowed.contains(&call.tool_name) {
            return None;
        }

        log::warn!(
            "[ToolRoundDriver] Blocked tool '{}' (id={}) — not in allowed set",
            call.tool_name,
            call.tool_call_id
        );

        let reason = format!(
            "Error: Tool '{}' is not available in the current analysis step. \
             Available tools: {}",
            call.tool_name,
            allowed.join(", ")
        );

        Some(BlockedToolOutcome {
            tool_call_id: call.tool_call_id.clone(),
            tool_name: call.tool_name.clone(),
            reason,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::identity::IdentityMapping;
    use crate::runtime::ids::RunId;
    use crate::runtime::tools::{
        AllowAllPermissionPipeline, RuntimeTool, ToolDefinition, ToolDispatcher, ToolError,
        ToolExecutionContext, ToolResult,
    };
    use async_trait::async_trait;
    use serde_json::{json, Value};
    use std::sync::{Arc, Mutex};

    struct RecordingTool {
        name: String,
        calls: Arc<Mutex<Vec<Value>>>,
    }

    #[async_trait]
    impl RuntimeTool for RecordingTool {
        fn definition(&self) -> ToolDefinition {
            ToolDefinition::new(&self.name, "Recording test tool")
        }

        async fn execute(
            &self,
            input: Value,
            _ctx: ToolExecutionContext,
        ) -> Result<ToolResult, ToolError> {
            self.calls.lock().unwrap().push(input);
            Ok(ToolResult::new(
                self.name.clone(),
                format!("ok:{}", self.name),
                None,
            ))
        }
    }

    fn make_turn() -> TurnState {
        let mapping = IdentityMapping::from_legacy_conversation_id("test-conv");
        TurnState::new(mapping, RunId::new("test-run"), "test".to_string())
    }

    #[tokio::test]
    async fn permitted_tools_are_dispatched() {
        let calls = Arc::new(Mutex::new(Vec::new()));
        let tool = Arc::new(RecordingTool {
            name: "my_tool".to_string(),
            calls: calls.clone(),
        });
        let dispatcher = Arc::new(ToolDispatcher::new(Arc::new(AllowAllPermissionPipeline)));
        dispatcher.register(tool);

        let engine = QueryEngine::with_dispatcher(dispatcher);
        let driver = ToolRoundDriver::new(engine);
        let bus = RuntimeEventBus::new();
        let turn = make_turn();

        let results = driver
            .execute_round(
                &turn,
                &bus,
                vec![RuntimeToolCallRequest {
                    tool_call_id: "tc-1".into(),
                    tool_name: "my_tool".into(),
                    args: json!({"a": 1}),
                    purpose: None,
                }],
            )
            .await;

        assert_eq!(results.len(), 1);
        assert!(matches!(&results[0], ToolRoundResult::Ok(o) if !o.is_error()));
        assert_eq!(calls.lock().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn blocked_tools_produce_blocked_result() {
        let dispatcher = Arc::new(ToolDispatcher::new(Arc::new(AllowAllPermissionPipeline)));
        let engine = QueryEngine::with_dispatcher(dispatcher);
        let driver =
            ToolRoundDriver::new(engine).with_allowed_tools(vec!["allowed_tool".to_string()]);
        let bus = RuntimeEventBus::new();
        let turn = make_turn();

        let results = driver
            .execute_round(
                &turn,
                &bus,
                vec![RuntimeToolCallRequest {
                    tool_call_id: "tc-blocked".into(),
                    tool_name: "forbidden_tool".into(),
                    args: json!({}),
                    purpose: None,
                }],
            )
            .await;

        assert_eq!(results.len(), 1);
        match &results[0] {
            ToolRoundResult::Blocked(b) => {
                assert_eq!(b.tool_name, "forbidden_tool");
                assert!(b.reason.contains("not available"));
            }
            other => panic!("expected Blocked, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn mixed_blocked_and_permitted_preserves_order() {
        let calls = Arc::new(Mutex::new(Vec::new()));
        let tool = Arc::new(RecordingTool {
            name: "ok_tool".to_string(),
            calls: calls.clone(),
        });
        let dispatcher = Arc::new(ToolDispatcher::new(Arc::new(AllowAllPermissionPipeline)));
        dispatcher.register(tool);

        let engine = QueryEngine::with_dispatcher(dispatcher);
        let driver = ToolRoundDriver::new(engine).with_allowed_tools(vec!["ok_tool".to_string()]);
        let bus = RuntimeEventBus::new();
        let turn = make_turn();

        let results = driver
            .execute_round(
                &turn,
                &bus,
                vec![
                    RuntimeToolCallRequest {
                        tool_call_id: "tc-0".into(),
                        tool_name: "blocked_tool".into(),
                        args: json!({}),
                        purpose: None,
                    },
                    RuntimeToolCallRequest {
                        tool_call_id: "tc-1".into(),
                        tool_name: "ok_tool".into(),
                        args: json!({"x": 1}),
                        purpose: None,
                    },
                    RuntimeToolCallRequest {
                        tool_call_id: "tc-2".into(),
                        tool_name: "another_blocked".into(),
                        args: json!({}),
                        purpose: None,
                    },
                ],
            )
            .await;

        assert_eq!(results.len(), 3);
        assert!(matches!(&results[0], ToolRoundResult::Blocked(_)));
        assert!(matches!(&results[1], ToolRoundResult::Ok(_)));
        assert!(matches!(&results[2], ToolRoundResult::Blocked(_)));
    }

    #[tokio::test]
    async fn no_filter_permits_all() {
        let calls = Arc::new(Mutex::new(Vec::new()));
        let tool = Arc::new(RecordingTool {
            name: "any_tool".to_string(),
            calls: calls.clone(),
        });
        let dispatcher = Arc::new(ToolDispatcher::new(Arc::new(AllowAllPermissionPipeline)));
        dispatcher.register(tool);

        let engine = QueryEngine::with_dispatcher(dispatcher);
        let driver = ToolRoundDriver::new(engine); // no filter
        let bus = RuntimeEventBus::new();
        let turn = make_turn();

        let results = driver
            .execute_round(
                &turn,
                &bus,
                vec![RuntimeToolCallRequest {
                    tool_call_id: "tc-any".into(),
                    tool_name: "any_tool".into(),
                    args: json!({}),
                    purpose: None,
                }],
            )
            .await;

        assert_eq!(results.len(), 1);
        assert!(matches!(&results[0], ToolRoundResult::Ok(o) if !o.is_error()));
    }
}
