/// send_message_production_path_test.rs
///
/// Gating tests that document the production-path gap between the current
/// executor-backed chat turn and the target runtime-first architecture.
///
/// Status contract:
///   T1 (streaming:done via host)    — RED-LIGHT until P1-B ships
///   T2 (tool dispatch via QueryEngine) — RED-LIGHT until P1-A ships
///   T3 (run_id consistency)           — GREEN  (P2-A already fixed, regression gate)
///   T4 (message:updated via host)     — RED-LIGHT until P1-B ships
///
/// When P1-A and P1-B are complete all four tests must be GREEN.

mod common;

use std::sync::{Arc, Mutex};

use app_lib::runtime::{
    ChatTurnRequest, QueryEngine, RuntimeEventBus, RuntimeEventKind, RuntimeTurnExecutor,
    SessionRuntime,
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

/// A minimal executor that records requests it receives.
#[derive(Default)]
struct CapturingExecutor {
    requests: Mutex<Vec<ChatTurnRequest>>,
}

#[async_trait]
impl RuntimeTurnExecutor for CapturingExecutor {
    async fn run_chat_turn(&self, request: ChatTurnRequest) -> Result<(), String> {
        self.requests.lock().unwrap().push(request);
        Ok(())
    }

    // Override to return a mock tool call so ToolRoundDriver can dispatch
    // it to the SpyTool registered in QueryEngine's ToolDispatcher.
    async fn run_chat_turn_with_calls(
        &self,
        request: ChatTurnRequest,
    ) -> Result<Vec<app_lib::runtime::chat::tool_round_types::RuntimeToolCallRequest>, String> {
        self.requests.lock().unwrap().push(request.clone());
        Ok(vec![
            app_lib::runtime::chat::tool_round_types::RuntimeToolCallRequest {
                tool_call_id: "mock-tc-spy-1".to_string(),
                tool_name: "spy_dispatch_tool".to_string(),
                args: serde_json::json!({}),
                purpose: None,
            },
        ])
    }
}

impl CapturingExecutor {
    fn captured_requests(&self) -> Vec<ChatTurnRequest> {
        self.requests.lock().unwrap().clone()
    }
}

/// A minimal `RuntimeTool` that records whether it was ever called.
struct SpyTool {
    name: &'static str,
    was_called: Arc<Mutex<bool>>,
}

#[async_trait]
impl RuntimeTool for SpyTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition::new(self.name, "Spy tool to detect dispatch path")
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

/// Construct a `RuntimeEventBus` already wired to a `RecordingRuntimeHost` via
/// `TauriEventAdapter`, and return both.
fn make_recording_bus() -> (RuntimeEventBus, Arc<RecordingRuntimeHost>) {
    let host = RecordingRuntimeHost::new();
    let bus = RuntimeEventBus::new();
    bus.subscribe(Arc::new(TauriEventAdapter::new(host.clone())));
    (bus, host)
}

// ── T1 ────────────────────────────────────────────────────────────────────────

/// RED-LIGHT (until P1-B): `streaming:done` must be delivered to the host.
///
/// Gap: In the executor-backed production path `RuntimeChatTurnDriver` calls
/// `event_bus.record_only(stream_done_event)` after the legacy executor returns.
/// `record_only` writes the event into the internal log but does NOT notify
/// subscribers — so `TauriEventAdapter` is never called and the
/// `RecordingRuntimeHost` never receives `streaming:done`.
///
/// When P1-B migrates the LLM loop back into `RuntimeChatTurnDriver` it will
/// call `event_bus.emit(stream_done)` (which notifies subscribers), turning
/// this test GREEN.
#[tokio::test]
async fn send_message_production_path_full_turn_must_not_delegate_to_legacy_executor() {
    let (bus, host) = make_recording_bus();
    let executor = Arc::new(CapturingExecutor::default());
    let runtime = SessionRuntime::with_executor(QueryEngine::new(), bus, executor);

    runtime
        .run_chat_request(ChatTurnRequest::new(
            "conv-t1-streaming-done",
            "hello streaming-done gate",
            vec![],
        ))
        .await
        .unwrap();

    let event_names = host.trace().event_names();

    // RED-LIGHT: expect exactly one `streaming:done` delivered to the host.
    // Currently this FAILS because record_only does not notify TauriEventAdapter.
    let done_count = event_names
        .iter()
        .filter(|n| n.as_str() == "streaming:done")
        .count();
    assert_eq!(
        done_count, 1,
        "host must receive exactly one streaming:done event on the production path. \
         Got {} (record_only does not notify subscribers). \
         Full host event sequence: {:?}",
        done_count, event_names
    );
}

// ── T2 ────────────────────────────────────────────────────────────────────────

/// RED-LIGHT (until P1-A): tool calls must route through `QueryEngine::run_tool_with_bus`
/// (i.e. through `ToolDispatcher::dispatch()`), not bypass the runtime dispatcher.
///
/// Gap: In the executor-backed production path the legacy executor owns the LLM
/// loop and tool execution entirely outside the runtime boundary.  A `SpyTool`
/// registered in `QueryEngine`'s `ToolDispatcher` is never reached during a
/// `run_chat_request` call — proving that the runtime dispatcher is bypassed.
///
/// Architecture note: we cannot inject a mock LLM response here without
/// significant infrastructure, so we observe the bypass indirectly: after a full
/// chat turn completes, the spy tool registered in the runtime dispatcher should
/// have been called (target state), but currently it never is (current gap).
///
/// When P1-A routes tool execution through `ToolDispatcher::dispatch()` from
/// within the runtime-owned LLM loop the spy will be reached and the test turns
/// GREEN.  The explicit `run_tool_with_bus` scenario is already covered by
/// `chat_runtime_dispatcher_production_path_test.rs`; this test focuses on the
/// production `send_message` → `run_chat_request` path.
#[tokio::test]
async fn send_message_production_tool_round_must_dispatch_via_runtime_query_engine() {
    let was_called = Arc::new(Mutex::new(false));
    let spy = Arc::new(SpyTool {
        name: "spy_dispatch_tool",
        was_called: was_called.clone(),
    });

    let dispatcher = Arc::new(ToolDispatcher::new(Arc::new(AllowAllPermissionPipeline)));
    dispatcher.register(spy);

    // Wire the bus to a RecordingHost so we can also observe legacy events.
    let (bus, _host) = make_recording_bus();
    let executor = Arc::new(CapturingExecutor::default());
    let engine = QueryEngine::with_dispatcher(dispatcher);
    let runtime = SessionRuntime::with_executor(engine, bus, executor);

    runtime
        .run_chat_request(ChatTurnRequest::new(
            "conv-t2-tool-dispatch",
            "please call spy_dispatch_tool",
            vec![],
        ))
        .await
        .unwrap();

    // RED-LIGHT: the spy must have been invoked via ToolDispatcher::dispatch()
    // during the chat turn.  Currently FAILS because the legacy executor owns
    // the tool loop outside the runtime boundary and never reaches the spy.
    assert!(
        *was_called.lock().unwrap(),
        "SpyTool registered in QueryEngine's ToolDispatcher must be called \
         during send_message production path. \
         CURRENT ARCHITECTURE LIMITATION: the legacy executor handles tool calls \
         outside the runtime boundary — the runtime dispatcher is bypassed. \
         This will turn GREEN when P1-A routes tool execution through \
         ToolDispatcher::dispatch() from inside RuntimeChatTurnDriver."
    );
}

// ── T3 ────────────────────────────────────────────────────────────────────────

/// GREEN (regression gate — P2-A already fixed this).
///
/// The `run_id` generated by `SessionRuntime::run_chat_request` must be the same
/// id that appears in the `RunStarted` event AND in the `ChatTurnRequest` handed
/// to the legacy executor.
///
/// P2-A wired the authoritative `run_id` into both the `TurnState` (which emits
/// `RunStarted`) and the `request` that is forwarded to the executor.  This test
/// is a regression gate: it must remain GREEN across future refactors.
#[tokio::test]
async fn send_message_production_path_must_use_single_run_id() {
    let executor = Arc::new(CapturingExecutor::default());
    let bus = RuntimeEventBus::new();
    let runtime = SessionRuntime::with_executor(QueryEngine::new(), bus, executor.clone());

    runtime
        .run_chat_request(ChatTurnRequest::new(
            "conv-t3-run-id",
            "run_id consistency check",
            vec![],
        ))
        .await
        .unwrap();

    // Extract the run_id from the RunStarted event on the bus.
    let recorded = runtime.recorded_events();
    let run_started_run_id = recorded
        .iter()
        .find(|e| matches!(e.kind, RuntimeEventKind::RunStarted))
        .map(|e| e.run_id.as_str().to_string())
        .expect("RunStarted event must be present in runtime.recorded_events()");

    // Extract the run_id from the request captured by the executor.
    let captured = executor.captured_requests();
    assert_eq!(
        captured.len(),
        1,
        "executor must have been called exactly once, got {} calls",
        captured.len()
    );
    let executor_run_id = captured[0].run_id.as_str().to_string();

    // Both must be the same authoritative id.
    assert_eq!(
        run_started_run_id,
        executor_run_id,
        "run_id in RunStarted event ({}) must equal run_id in executor ChatTurnRequest ({}).\
         \nP2-A regression gate — if this fails, the run_id propagation has been broken.",
        run_started_run_id,
        executor_run_id
    );
}

// ── T4 ────────────────────────────────────────────────────────────────────────

/// RED-LIGHT (until P1-B): `message:updated` must be delivered to the host.
///
/// Gap: In the executor-backed production path `RuntimeChatTurnDriver` calls
/// `event_bus.record_only(message_persisted_event)` after the legacy executor
/// returns.  `record_only` writes the event into the internal log but does NOT
/// notify subscribers — so `TauriEventAdapter` is never called and the
/// `RecordingRuntimeHost` never receives `message:updated`.
///
/// When P1-B migrates message persistence back into `RuntimeChatTurnDriver` it
/// will call `event_bus.emit(message_persisted)` (which notifies subscribers),
/// turning this test GREEN.
#[tokio::test]
async fn send_message_production_path_message_persisted_must_be_emitted_not_record_only() {
    let (bus, host) = make_recording_bus();
    let executor = Arc::new(CapturingExecutor::default());
    let runtime = SessionRuntime::with_executor(QueryEngine::new(), bus, executor);

    runtime
        .run_chat_request(ChatTurnRequest::new(
            "conv-t4-message-updated",
            "hello message-updated gate",
            vec![],
        ))
        .await
        .unwrap();

    let event_names = host.trace().event_names();

    // RED-LIGHT: expect at least one `message:updated` delivered to the host.
    // Currently this FAILS because record_only does not notify TauriEventAdapter.
    let updated_count = event_names
        .iter()
        .filter(|n| n.as_str() == "message:updated")
        .count();
    assert!(
        updated_count >= 1,
        "host must receive at least one message:updated event on the production path. \
         Got {} (record_only does not notify subscribers). \
         Full host event sequence: {:?}",
        updated_count, event_names
    );
}
