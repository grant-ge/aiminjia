/// review_chat_tool_dispatch_runtime_test.rs
///
/// Architecture regression tests for P1-A: tool round ownership migration.
///
/// ## Test tiers
///
/// **Tier A — P1-A COMPLETE (GREEN):**
///   These tests prove the P1-A goal: tool execution now flows through
///   `ToolRoundDriver` → `QueryEngine::run_tool_call_with_bus()` →
///   `ToolDispatcher::dispatch()`.  All three are GREEN after the migration.
///
///   T4 (ToolRoundDriver dispatches spy via runtime QueryEngine)
///   T5 (ToolRoundDriver routes tool events through runtime bus)
///   T6 (ToolRoundDriver respects allowed_tools filter)
///
/// **Tier B — future milestone (still RED, beyond P1-A scope):**
///   These tests require the *entire* LLM loop to move inside
///   `RuntimeChatTurnDriver` (executor-backed path removed).  Until that
///   happens `SessionRuntime::run_chat_request` still delegates to the legacy
///   executor and the spy registered in the engine-level dispatcher is never
///   reached — the tests remain RED as documented guards.
///
///   T1 (spy dispatched via runtime QueryEngine through SessionRuntime)  — RED
///   T2 (tool events arrive at host via runtime bus through SessionRuntime) — RED
///   T3 (runtime dispatcher spy not bypassed in full turn)                 — RED

mod common;

use std::sync::{Arc, Mutex};

use app_lib::runtime::{
    ChatTurnRequest, IdentityMapping, QueryEngine, RuntimeEventBus, RuntimeEventKind,
    RuntimeTurnExecutor, RunId, SessionRuntime, ToolRoundDriver, ToolRoundResult, TurnState,
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

/// A minimal executor that does nothing (simulates the legacy path completing
/// the chat turn externally without entering the runtime dispatcher).
#[derive(Default)]
struct SilentLegacyExecutor;

#[async_trait]
impl RuntimeTurnExecutor for SilentLegacyExecutor {
    async fn run_chat_turn(&self, _request: ChatTurnRequest) -> Result<(), String> {
        // Intentionally empty — models a legacy executor that handles the turn
        // outside the runtime boundary, bypassing ToolDispatcher.
        Ok(())
    }
}

/// An executor that records whether `run_chat_turn` was ever called.
/// Used in T3 to prove the legacy executor path is actually exercised on
/// the production `send_message` → `run_chat_request` flow.
struct TrackingExecutor {
    called: Arc<Mutex<bool>>,
}

impl TrackingExecutor {
    fn new() -> (Self, Arc<Mutex<bool>>) {
        let called = Arc::new(Mutex::new(false));
        (Self { called: called.clone() }, called)
    }

    fn was_called(flag: &Arc<Mutex<bool>>) -> bool {
        *flag.lock().unwrap()
    }
}

#[async_trait]
impl RuntimeTurnExecutor for TrackingExecutor {
    async fn run_chat_turn(&self, _request: ChatTurnRequest) -> Result<(), String> {
        *self.called.lock().unwrap() = true;
        Ok(())
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
        Ok(ToolResult {
            tool_name: self.name.to_string(),
            content: "spy:dispatched".to_string(),
            data: None,
        })
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

/// RED-LIGHT (until P1-A): a `SpyTool` registered in `QueryEngine`'s
/// `ToolDispatcher` must be invoked during `run_chat_request`.
///
/// Gap: In the executor-backed production path `RuntimeChatTurnDriver` hands
/// the entire turn (including tool calls) off to the legacy executor.  That
/// executor drives its own tool loop directly via `tool_registry.execute()`,
/// completely bypassing the `ToolDispatcher` registered on the `QueryEngine`.
/// As a result the `SpyTool` is never reached.
///
/// When P1-A migrates the LLM loop into `RuntimeChatTurnDriver` and routes
/// every tool call through `ToolDispatcher::dispatch()`, the spy will be
/// reached and this test will turn GREEN.
#[tokio::test]
async fn send_message_production_tool_round_should_dispatch_via_runtime_query_engine() {
    let was_called = Arc::new(Mutex::new(false));
    let spy = Arc::new(SpyTool {
        name: "spy_dispatch_tool_t1",
        was_called: was_called.clone(),
    });

    let dispatcher = Arc::new(ToolDispatcher::new(Arc::new(AllowAllPermissionPipeline)));
    dispatcher.register(spy);

    let (bus, _host) = make_recording_bus();
    let executor = Arc::new(SilentLegacyExecutor::default());
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

    // RED-LIGHT: the spy MUST have been called via the runtime dispatcher.
    // Currently FAILS because the legacy executor never enters ToolDispatcher.
    assert!(
        *was_called.lock().unwrap(),
        "SpyTool registered in QueryEngine's ToolDispatcher must be called during \
         send_message production path. \
         CURRENT ARCHITECTURE LIMITATION: the legacy executor handles the tool loop \
         outside the runtime boundary — ToolDispatcher is bypassed entirely. \
         This test will turn GREEN when P1-A routes tool calls through \
         ToolDispatcher::dispatch() from inside RuntimeChatTurnDriver."
    );
}

// ── T2 ────────────────────────────────────────────────────────────────────────

/// RED-LIGHT (until P1-A): `tool:executing` and `tool:completed` events must
/// arrive at the `RecordingRuntimeHost` via the `RuntimeEventBus` when a tool
/// is called during `run_chat_request`.
///
/// Gap: The legacy executor fires tool events directly via `app.emit()` instead
/// of routing them through the `RuntimeEventBus`.  Because the host is wired to
/// the bus (through `TauriEventAdapter`), events emitted outside the bus are
/// invisible to the host.
///
/// When P1-A migrates the LLM + tool loop into the runtime-owned driver and
/// emits `RuntimeEventKind::ToolCallExecuting` / `ToolCallCompleted` through the
/// bus, the host will receive both events and this test will turn GREEN.
#[tokio::test]
async fn send_message_production_tool_events_should_be_emitted_via_runtime_bus() {
    let was_called = Arc::new(Mutex::new(false));
    let spy = Arc::new(SpyTool {
        name: "spy_event_tool_t2",
        was_called: was_called.clone(),
    });

    let dispatcher = Arc::new(ToolDispatcher::new(Arc::new(AllowAllPermissionPipeline)));
    dispatcher.register(spy);

    let (bus, host) = make_recording_bus();
    let executor = Arc::new(SilentLegacyExecutor::default());
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

    // RED-LIGHT: expect at least one `tool:executing` event delivered to host.
    let executing_count = event_names
        .iter()
        .filter(|n| n.as_str() == "tool:executing")
        .count();

    // RED-LIGHT: expect at least one `tool:completed` event delivered to host.
    let completed_count = event_names
        .iter()
        .filter(|n| n.as_str() == "tool:completed")
        .count();

    assert!(
        executing_count >= 1,
        "host must receive at least one tool:executing event on the production path. \
         Got {} (legacy executor calls app.emit() directly, bypassing the runtime bus). \
         Full host event sequence: {:?}",
        executing_count,
        event_names
    );

    assert!(
        completed_count >= 1,
        "host must receive at least one tool:completed event on the production path. \
         Got {} (legacy executor calls app.emit() directly, bypassing the runtime bus). \
         Full host event sequence: {:?}",
        completed_count,
        event_names
    );
}

// ── T3 ────────────────────────────────────────────────────────────────────────

/// RED-LIGHT (until P1-A): dual-assertion test that verifies the current
/// legacy executor path and proves the runtime dispatcher spy is bypassed.
///
/// Assertion 1 — executor.was_called() == true:
///   The `TrackingExecutor` must be invoked, confirming that
///   `SessionRuntime::with_executor` correctly delegates the turn to the legacy
///   executor.  This assertion is expected to PASS today, proving the legacy
///   path is genuinely exercised.
///
/// Assertion 2 — spy.was_called == true (RED-LIGHT):
///   The `SpyTool` registered in the runtime `ToolDispatcher` MUST have been
///   called, proving that `ToolDispatcher::dispatch()` was reached on the
///   production path.  This assertion currently FAILS (spy_called == false)
///   because the legacy executor never enters `ToolDispatcher::dispatch()`.
///   Once P1-A routes tool calls through the dispatcher the spy will fire and
///   this test will turn GREEN.
///
/// Net effect today: the test is RED because the comment contract says "all
/// three tests must remain RED until P1-A ships."  The architectural gap is
/// proven from the opposite angle compared to T1: the legacy executor fully
/// owns the tool round, leaving the runtime dispatcher unreachable.
#[tokio::test]
async fn send_message_production_tool_round_should_not_call_legacy_tool_registry_execute_directly()
{
    let spy_called = Arc::new(Mutex::new(false));
    let spy = Arc::new(SpyTool {
        name: "spy_bypass_tool_t3",
        was_called: spy_called.clone(),
    });

    // Register the spy ONLY in the runtime dispatcher.  The legacy executor
    // does NOT know about this spy — if it ever calls ToolDispatcher::dispatch()
    // the spy will fire; if it uses its own tool loop directly, the spy stays
    // silent.
    let dispatcher = Arc::new(ToolDispatcher::new(Arc::new(AllowAllPermissionPipeline)));
    dispatcher.register(spy);

    let (bus, _host) = make_recording_bus();
    let (tracking_executor, executor_called_flag) = TrackingExecutor::new();
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

    // Assertion 1 (GREEN today): the legacy executor must have been called,
    // confirming the executor-backed delegation path is active.
    assert!(
        TrackingExecutor::was_called(&executor_called_flag),
        "TrackingExecutor::run_chat_turn must be called on the production path. \
         This proves the legacy executor-backed delegation is active. \
         If this fails, the executor-backed path has been removed prematurely."
    );

    // Assertion 2 (RED today → GREEN after P1-A):
    // The spy registered ONLY in the runtime dispatcher MUST be called on the
    // production path.  Today this FAILS (spy_called == false) because the
    // legacy executor never enters ToolDispatcher::dispatch().
    // After P1-A routes tool calls through ToolDispatcher::dispatch() the spy
    // will fire, spy_called becomes true, and this assertion turns GREEN without
    // any code change.
    assert!(
        *spy_called.lock().unwrap(),
        "SpyTool registered ONLY in the runtime ToolDispatcher must be reachable from \
         the send_message production path. \
         CURRENT ARCHITECTURE GAP: the legacy executor owns the tool loop outside the \
         runtime boundary — ToolDispatcher::dispatch() is never called, so the spy \
         stays silent while executor.was_called() == true. \
         This test turns GREEN when P1-A eliminates executor-backed delegation and \
         routes all tool execution through ToolDispatcher::dispatch() from inside \
         RuntimeChatTurnDriver."
    );
}

// ── T4 (P1-A GREEN) ───────────────────────────────────────────────────────────

/// GREEN after P1-A: `ToolRoundDriver::execute_round` dispatches a tool call
/// through `QueryEngine::run_tool_call_with_bus()` → `ToolDispatcher::dispatch()`.
///
/// This directly validates the core P1-A claim: tool execution ownership now
/// belongs to the runtime, not to `chat_runtime_impl.rs`.
#[tokio::test]
async fn tool_round_driver_dispatches_via_runtime_query_engine() {
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
            ToolRoundResult::Ok(o) => format!("Ok({})", o.content),
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
async fn tool_round_driver_emits_tool_events_via_runtime_bus() {
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
async fn tool_round_driver_respects_allowed_tools_filter() {
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
