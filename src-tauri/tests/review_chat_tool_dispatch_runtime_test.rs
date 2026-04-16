/// review_chat_tool_dispatch_runtime_test.rs
///
/// Architecture regression tests for P1-A: tool round ownership migration.
///
/// ## Test tiers
///
/// **Tier A — P1-A COMPLETE (GREEN):**
///   These tests prove the P1-A goal: tool execution now flows through
///   `ToolRoundDriver` → `QueryEngine::run_tool_call_with_bus()` →
///   `ToolDispatcher::dispatch()`. All six tests are GREEN after the migration.
///
///   T1 (spy dispatched via runtime QueryEngine through SessionRuntime)
///   T2 (tool events arrive at host via runtime bus through SessionRuntime)
///   T3 (full turn uses runtime dispatcher without bypass)
///   T4 (ToolRoundDriver dispatches spy via runtime QueryEngine)
///   T5 (ToolRoundDriver routes tool events through runtime bus)
///   T6 (ToolRoundDriver respects allowed_tools filter)

mod common;

use std::sync::{Arc, Mutex};

use app_lib::runtime::{
    ChatTurnRequest, IdentityMapping, QueryEngine, RuntimeEventBus, RuntimeTurnExecutor, RunId,
    SessionRuntime, ToolRoundDriver, ToolRoundResult, TurnState,
};
use app_lib::runtime::chat::tool_round_types::RuntimeToolCallRequest;
use app_lib::runtime::tools::{
    AllowAllPermissionPipeline, RuntimeTool, ToolDefinition, ToolDispatcher, ToolError,
    ToolExecutionContext, ToolResult,
};
use app_lib::transport::tauri_event_adapter::TauriEventAdapter;
use app_lib::transport::testing::RecordingRuntimeHost;
use async_trait::async_trait;
use serde_json::Value;

// ── Shared helpers ────────────────────────────────────────────────────────────

/// An executor that overrides `run_llm_step` to return one tool call on the
/// first invocation, then empty Vec on subsequent calls (loop done).
/// Used in T1 and T2 to route a tool call through the runtime dispatcher.
struct MockLlmExecutor {
    tool_name: &'static str,
    returned_calls: Arc<Mutex<bool>>,
}

impl MockLlmExecutor {
    fn new(tool_name: &'static str) -> Self {
        Self {
            tool_name,
            returned_calls: Arc::new(Mutex::new(false)),
        }
    }
}

#[async_trait]
impl RuntimeTurnExecutor for MockLlmExecutor {
    async fn run_chat_turn(&self, _request: ChatTurnRequest) -> Result<(), String> {
        Ok(())
    }

    async fn run_llm_step(
        &self,
        _request: ChatTurnRequest,
        _previous_tool_results: &[app_lib::runtime::chat::tool_round_types::RuntimeToolCallOutcome],
    ) -> Result<Vec<app_lib::runtime::chat::tool_round_types::RuntimeToolCallRequest>, String> {
        let mut returned = self.returned_calls.lock().unwrap();
        if !*returned {
            *returned = true;
            Ok(vec![app_lib::runtime::chat::tool_round_types::RuntimeToolCallRequest {
                tool_call_id: format!("tc-mock-{}", self.tool_name),
                tool_name: self.tool_name.to_string(),
                args: serde_json::json!({}),
                purpose: None,
            }])
        } else {
            Ok(vec![])
        }
    }
}

/// An executor that records whether `run_llm_step` was ever called AND returns
/// one tool call on the first invocation.
/// Used in T3 to verify both "executor was exercised" AND "spy was reached".
struct TrackingMockExecutor {
    called: Arc<Mutex<bool>>,
    tool_name: &'static str,
    returned_calls: Arc<Mutex<bool>>,
}

impl TrackingMockExecutor {
    fn new(tool_name: &'static str) -> (Self, Arc<Mutex<bool>>) {
        let called = Arc::new(Mutex::new(false));
        (
            Self {
                called: called.clone(),
                tool_name,
                returned_calls: Arc::new(Mutex::new(false)),
            },
            called,
        )
    }
}

#[async_trait]
impl RuntimeTurnExecutor for TrackingMockExecutor {
    async fn run_chat_turn(&self, _request: ChatTurnRequest) -> Result<(), String> {
        Ok(())
    }

    async fn run_llm_step(
        &self,
        _request: ChatTurnRequest,
        _previous_tool_results: &[app_lib::runtime::chat::tool_round_types::RuntimeToolCallOutcome],
    ) -> Result<Vec<app_lib::runtime::chat::tool_round_types::RuntimeToolCallRequest>, String> {
        *self.called.lock().unwrap() = true;
        let mut returned = self.returned_calls.lock().unwrap();
        if !*returned {
            *returned = true;
            Ok(vec![app_lib::runtime::chat::tool_round_types::RuntimeToolCallRequest {
                tool_call_id: format!("tc-mock-{}", self.tool_name),
                tool_name: self.tool_name.to_string(),
                args: serde_json::json!({}),
                purpose: None,
            }])
        } else {
            Ok(vec![])
        }
    }
}


/// A minimal `RuntimeTool` that records whether its `execute()` was ever
/// reached.  Used as a spy to detect whether `ToolDispatcher::dispatch()` was
/// called on the production `send_message` path.
struct SpyTool {
    name: &'static str,
    was_called: Arc<Mutex<bool>>,
}

#[async_trait]
impl RuntimeTool for SpyTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition::new(self.name, "Spy tool — detects whether dispatcher was reached")
    }

    async fn execute(
        &self,
        _input: Value,
        _ctx: ToolExecutionContext,
    ) -> Result<ToolResult, ToolError> {
        *self.was_called.lock().unwrap() = true;
        Ok(ToolResult::new(self.name, "spy:dispatched", None))
    }
}

/// Build a `RuntimeEventBus` subscribed to a `RecordingRuntimeHost` via
/// `TauriEventAdapter`.  Returns both the bus and the host so callers can
/// inspect which events arrived at the host layer.
fn make_recording_bus() -> (RuntimeEventBus, Arc<RecordingRuntimeHost>) {
    let host = RecordingRuntimeHost::new();
    let bus = RuntimeEventBus::new();
    bus.subscribe(Arc::new(TauriEventAdapter::new(host.clone())));
    (bus, host)
}

// ── T1 ────────────────────────────────────────────────────────────────────────

/// GREEN after P1-A: a `SpyTool` registered in `QueryEngine`'s
/// `ToolDispatcher` must be invoked during `run_chat_request`.
///
/// `MockLlmExecutor` overrides `run_llm_step` to return a tool call for
/// "spy_dispatch_tool_t1" on the first step.  `RuntimeChatTurnDriver` routes
/// it through `ToolRoundDriver` → `ToolDispatcher::dispatch()`, so the spy
/// fires before the loop ends.
#[tokio::test]
async fn review_send_message_production_tool_round_should_dispatch_via_runtime_query_engine() {
    let was_called = Arc::new(Mutex::new(false));
    let spy = Arc::new(SpyTool {
        name: "spy_dispatch_tool_t1",
        was_called: was_called.clone(),
    });

    let dispatcher = Arc::new(ToolDispatcher::new(Arc::new(AllowAllPermissionPipeline)));
    dispatcher.register(spy);

    let (bus, _host) = make_recording_bus();
    let executor = Arc::new(MockLlmExecutor::new("spy_dispatch_tool_t1"));
    let engine = QueryEngine::with_dispatcher(dispatcher);
    let runtime = SessionRuntime::with_executor(engine, bus, executor);

    runtime
        .run_chat_request(ChatTurnRequest::new(
            "conv-t1-spy-dispatch",
            "please call spy_dispatch_tool_t1",
            vec![],
        ))
        .await
        .unwrap();

    // GREEN: the spy MUST have been called via the runtime dispatcher.
    assert!(
        *was_called.lock().unwrap(),
        "SpyTool registered in QueryEngine's ToolDispatcher must be called during \
         send_message production path. \
         MockLlmExecutor issues one tool call via run_llm_step; RuntimeChatTurnDriver \
         routes it through ToolRoundDriver → ToolDispatcher::dispatch()."
    );
}

// ── T2 ────────────────────────────────────────────────────────────────────────

/// GREEN after P1-A: `tool:executing` and `tool:completed` events must
/// arrive at the `RecordingRuntimeHost` via the `RuntimeEventBus` when a tool
/// is called during `run_chat_request`.
///
/// `MockLlmExecutor` overrides `run_llm_step` to issue one tool call.
/// `RuntimeChatTurnDriver` routes it through `ToolRoundDriver` →
/// `QueryEngine::run_tool_call_with_bus()` which emits
/// `RuntimeEventKind::ToolCallExecuting` / `ToolCallCompleted` on the session
/// bus → `TauriEventAdapter` → host receives `tool:executing` / `tool:completed`.
#[tokio::test]
async fn review_send_message_production_tool_events_should_be_emitted_via_runtime_bus() {
    let was_called = Arc::new(Mutex::new(false));
    let spy = Arc::new(SpyTool {
        name: "spy_event_tool_t2",
        was_called: was_called.clone(),
    });

    let dispatcher = Arc::new(ToolDispatcher::new(Arc::new(AllowAllPermissionPipeline)));
    dispatcher.register(spy);

    let (bus, host) = make_recording_bus();
    let executor = Arc::new(MockLlmExecutor::new("spy_event_tool_t2"));
    let engine = QueryEngine::with_dispatcher(dispatcher);
    let runtime = SessionRuntime::with_executor(engine, bus, executor);

    runtime
        .run_chat_request(ChatTurnRequest::new(
            "conv-t2-tool-events",
            "please call spy_event_tool_t2",
            vec![],
        ))
        .await
        .unwrap();

    let event_names = host.trace().event_names();

    // GREEN: expect at least one `tool:executing` event delivered to host.
    let executing_count = event_names
        .iter()
        .filter(|n| n.as_str() == "tool:executing")
        .count();

    // GREEN: expect at least one `tool:completed` event delivered to host.
    let completed_count = event_names
        .iter()
        .filter(|n| n.as_str() == "tool:completed")
        .count();

    assert!(
        executing_count >= 1,
        "host must receive at least one tool:executing event on the production path. \
         Got {} events. Full host event sequence: {:?}",
        executing_count,
        event_names
    );

    assert!(
        completed_count >= 1,
        "host must receive at least one tool:completed event on the production path. \
         Got {} events. Full host event sequence: {:?}",
        completed_count,
        event_names
    );
}

// ── T3 ────────────────────────────────────────────────────────────────────────

/// GREEN after P1-A: dual-assertion test verifying the executor was exercised
/// AND the runtime dispatcher spy was reached.
///
/// Assertion 1 — executor was called:
///   `TrackingMockExecutor::run_llm_step` sets a flag; this proves the
///   executor-backed path is still exercised by `SessionRuntime`.
///
/// Assertion 2 — spy was called:
///   The `SpyTool` registered in the runtime `ToolDispatcher` MUST have been
///   called, proving that `ToolDispatcher::dispatch()` was reached.
///   `TrackingMockExecutor` issues one tool call via `run_llm_step`, which
///   `RuntimeChatTurnDriver` routes through `ToolRoundDriver` → dispatcher.
#[tokio::test]
async fn review_send_message_production_tool_round_should_not_call_legacy_tool_registry_execute_directly()
{
    let spy_called = Arc::new(Mutex::new(false));
    let spy = Arc::new(SpyTool {
        name: "spy_bypass_tool_t3",
        was_called: spy_called.clone(),
    });

    let dispatcher = Arc::new(ToolDispatcher::new(Arc::new(AllowAllPermissionPipeline)));
    dispatcher.register(spy);

    let (bus, _host) = make_recording_bus();
    let (tracking_executor, executor_called_flag) = TrackingMockExecutor::new("spy_bypass_tool_t3");
    let executor = Arc::new(tracking_executor);
    let engine = QueryEngine::with_dispatcher(dispatcher);
    let runtime = SessionRuntime::with_executor(engine, bus, executor);

    runtime
        .run_chat_request(ChatTurnRequest::new(
            "conv-t3-bypass",
            "please call spy_bypass_tool_t3",
            vec![],
        ))
        .await
        .unwrap();

    // Assertion 1 (GREEN): the executor must have been invoked via run_llm_step.
    assert!(
        *executor_called_flag.lock().unwrap(),
        "TrackingMockExecutor::run_llm_step must be called on the production path. \
         This proves the executor-backed delegation is active."
    );

    // Assertion 2 (GREEN): the spy registered ONLY in the runtime dispatcher MUST
    // be called, proving ToolDispatcher::dispatch() was reached.
    assert!(
        *spy_called.lock().unwrap(),
        "SpyTool registered ONLY in the runtime ToolDispatcher must be reachable from \
         the send_message production path. \
         TrackingMockExecutor issues one tool call via run_llm_step; \
         RuntimeChatTurnDriver routes it through ToolRoundDriver → ToolDispatcher::dispatch()."
    );
}

// ── T4 (P1-A GREEN) ───────────────────────────────────────────────────────────

/// GREEN after P1-A: `ToolRoundDriver::execute_round` dispatches a tool call
/// through `QueryEngine::run_tool_call_with_bus()` → `ToolDispatcher::dispatch()`.
///
/// This directly validates the core P1-A claim: tool execution ownership now
/// belongs to the runtime, not to `chat_runtime_impl.rs`.
#[tokio::test]
async fn review_tool_round_driver_dispatches_via_runtime_query_engine() {
    let was_called = Arc::new(Mutex::new(false));
    let spy = Arc::new(SpyTool {
        name: "spy_t4",
        was_called: was_called.clone(),
    });

    let dispatcher = Arc::new(ToolDispatcher::new(Arc::new(AllowAllPermissionPipeline)));
    dispatcher.register(spy);

    let engine = QueryEngine::with_dispatcher(dispatcher);
    let driver = ToolRoundDriver::new(engine);

    let mapping = IdentityMapping::from_legacy_conversation_id("conv-t4".to_string());
    let turn = TurnState::new(mapping, RunId::new("run-t4"), "call spy_t4".to_string());
    let bus = RuntimeEventBus::new();

    let calls = vec![RuntimeToolCallRequest {
        tool_call_id: "tc-t4-001".to_string(),
        tool_name: "spy_t4".to_string(),
        args: serde_json::json!({}),
        purpose: None,
    }];

    let results = driver.execute_round(&turn, &bus, calls).await;

    assert_eq!(results.len(), 1);
    assert!(
        matches!(results[0], ToolRoundResult::Ok(_)),
        "Expected Ok outcome, got: {:?}",
        match &results[0] {
            ToolRoundResult::Blocked(b) => format!("Blocked({})", b.reason),
            ToolRoundResult::Ok(o) => format!("Ok({})", o.content()),
        }
    );
    assert!(
        *was_called.lock().unwrap(),
        "SpyTool must be called when ToolRoundDriver dispatches through runtime QueryEngine. \
         P1-A regression: ToolRoundDriver must route through ToolDispatcher::dispatch()."
    );
}

// ── T5 (P1-A GREEN) ───────────────────────────────────────────────────────────

/// GREEN after P1-A: `ToolRoundDriver::execute_round` emits `tool:executing` and
/// `tool:completed` through the runtime bus → `TauriEventAdapter` → host.
///
/// This validates that tool lifecycle events are owned by the runtime bus, not
/// by `app.emit()` calls in `chat_runtime_impl.rs`.
#[tokio::test]
async fn review_tool_round_driver_emits_tool_events_via_runtime_bus() {
    let was_called = Arc::new(Mutex::new(false));
    let spy = Arc::new(SpyTool {
        name: "spy_t5",
        was_called: was_called.clone(),
    });

    let dispatcher = Arc::new(ToolDispatcher::new(Arc::new(AllowAllPermissionPipeline)));
    dispatcher.register(spy);

    let engine = QueryEngine::with_dispatcher(dispatcher);
    let driver = ToolRoundDriver::new(engine);

    let host = RecordingRuntimeHost::new();
    let bus = RuntimeEventBus::new();
    bus.subscribe(Arc::new(TauriEventAdapter::new(host.clone())));

    let mapping = IdentityMapping::from_legacy_conversation_id("conv-t5".to_string());
    let turn = TurnState::new(mapping, RunId::new("run-t5"), "call spy_t5".to_string());

    let calls = vec![RuntimeToolCallRequest {
        tool_call_id: "tc-t5-001".to_string(),
        tool_name: "spy_t5".to_string(),
        args: serde_json::json!({}),
        purpose: None,
    }];

    driver.execute_round(&turn, &bus, calls).await;

    let event_names = host.trace().event_names();

    let executing_count = event_names.iter().filter(|n| n.as_str() == "tool:executing").count();
    let completed_count = event_names.iter().filter(|n| n.as_str() == "tool:completed").count();

    assert_eq!(
        executing_count, 1,
        "tool:executing must be emitted exactly once via runtime bus. \
         Got {} times. Full event sequence: {:?}",
        executing_count, event_names
    );
    assert_eq!(
        completed_count, 1,
        "tool:completed must be emitted exactly once via runtime bus. \
         Got {} times. Full event sequence: {:?}",
        completed_count, event_names
    );
}

// ── T6 (P1-A GREEN) ───────────────────────────────────────────────────────────

/// GREEN after P1-A: `ToolRoundDriver` respects the `allowed_tools` filter —
/// blocked tools produce a `ToolRoundResult::Blocked` outcome and the spy is
/// never called.
///
/// This validates that the allowed-tools gate has been migrated from
/// `chat_runtime_impl.rs` into the runtime layer.
#[tokio::test]
async fn review_tool_round_driver_respects_allowed_tools_filter() {
    let was_called = Arc::new(Mutex::new(false));
    let spy = Arc::new(SpyTool {
        name: "blocked_spy_t6",
        was_called: was_called.clone(),
    });

    let dispatcher = Arc::new(ToolDispatcher::new(Arc::new(AllowAllPermissionPipeline)));
    dispatcher.register(spy);

    let engine = QueryEngine::with_dispatcher(dispatcher);
    // allowed_tools does NOT include "blocked_spy_t6"
    let driver = ToolRoundDriver::new(engine)
        .with_allowed_tools(vec!["other_tool".to_string()]);

    let mapping = IdentityMapping::from_legacy_conversation_id("conv-t6".to_string());
    let turn = TurnState::new(mapping, RunId::new("run-t6"), "call blocked_spy_t6".to_string());
    let bus = RuntimeEventBus::new();

    let calls = vec![RuntimeToolCallRequest {
        tool_call_id: "tc-t6-001".to_string(),
        tool_name: "blocked_spy_t6".to_string(),
        args: serde_json::json!({}),
        purpose: None,
    }];

    let results = driver.execute_round(&turn, &bus, calls).await;

    assert_eq!(results.len(), 1);
    assert!(
        matches!(results[0], ToolRoundResult::Blocked(_)),
        "Tool not in allowed_tools must produce Blocked outcome. \
         Got Ok instead — allowed_tools filter has been bypassed."
    );
    assert!(
        !*was_called.lock().unwrap(),
        "Blocked tool must NOT reach ToolDispatcher::dispatch(). \
         spy.was_called == true means the filter was not applied."
    );
}
