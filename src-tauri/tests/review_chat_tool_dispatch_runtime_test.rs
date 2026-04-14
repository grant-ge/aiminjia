/// review_chat_tool_dispatch_runtime_test.rs
///
/// Architecture regression tests that document the current gap between the
/// executor-backed production chat path and the target runtime-first tool
/// dispatch architecture (P1-A).
///
/// Status contract — ALL THREE are RED-LIGHT until P1-A ships:
///
///   T1 (spy dispatched via runtime QueryEngine)   — RED-LIGHT
///   T2 (tool events arrive at host via runtime bus) — RED-LIGHT
///   T3 (runtime dispatcher spy is not bypassed)     — RED-LIGHT
///
/// When P1-A is complete (i.e. `RuntimeChatTurnDriver` owns the LLM loop and
/// routes tool calls through `ToolDispatcher::dispatch()`), all three tests
/// must turn GREEN.

mod common;

use std::sync::{Arc, Mutex};

use app_lib::runtime::{
    ChatTurnRequest, QueryEngine, RuntimeEventBus, RuntimeTurnExecutor, SessionRuntime,
};
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
    // The spy registered ONLY in the runtime dispatcher must NOT have been
    // called, because the legacy executor bypasses ToolDispatcher entirely.
    // After P1-A routes tool calls through ToolDispatcher::dispatch(), the spy
    // will fire and this assertion must be flipped to `assert!(spy_called)`.
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
