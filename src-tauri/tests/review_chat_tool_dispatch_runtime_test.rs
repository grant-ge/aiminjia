/// review_chat_tool_dispatch_runtime_test.rs
///
/// Architecture regression tests for P1-A: tool round ownership migration.
///
/// ## Test tiers
///
/// **Tier A — P1-A COMPLETE (GREEN):**
///   These tests prove the P1-A goal: tool execution now flows through
///   `ToolRoundDriver` → `QueryEngine::run_tool_call_with_bus()` →
///   `ToolDispatcher::dispatch()`. All three tests are GREEN.
///
///   T4 (ToolRoundDriver dispatches spy via runtime QueryEngine)
///   T5 (ToolRoundDriver routes tool events through runtime bus)
///   T6 (ToolRoundDriver respects allowed_tools filter)
///
/// Note: T1-T3 which used legacy RuntimeTurnExecutor mocks were removed in S4-T15.
mod common;

use std::sync::{Arc, Mutex};

use app_lib::runtime::chat::tool_round_types::RuntimeToolCallRequest;
use app_lib::runtime::tools::description_context::ToolDescriptionContext;
use app_lib::runtime::tools::{
    AllowAllPermissionPipeline, RuntimeTool, ToolDefinition, ToolDispatcher, ToolError,
    ToolExecutionContext, ToolResult,
};
use app_lib::runtime::{
    IdentityMapping, QueryEngine, RunId, RuntimeEventBus, ToolRoundDriver, ToolRoundResult,
    TurnState,
};
use app_lib::transport::tauri_event_adapter::TauriEventAdapter;
use app_lib::transport::testing::RecordingRuntimeHost;
use async_trait::async_trait;
use serde_json::Value;

/// A minimal `RuntimeTool` that records whether its `execute()` was ever
/// reached.  Used as a spy to detect whether `ToolDispatcher::dispatch()` was
/// called on the production `send_message` path.
struct SpyTool {
    name: &'static str,
    was_called: Arc<Mutex<bool>>,
}

#[async_trait]
impl RuntimeTool for SpyTool {
    fn id(&self) -> &str {
        self.name
    }

    async fn definition(&self, _ctx: &ToolDescriptionContext) -> ToolDefinition {
        ToolDefinition::new(
            self.name,
            "Spy tool — detects whether dispatcher was reached",
        )
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

    let executing_count = event_names
        .iter()
        .filter(|n| n.as_str() == "tool:executing")
        .count();
    let completed_count = event_names
        .iter()
        .filter(|n| n.as_str() == "tool:completed")
        .count();

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
    let driver = ToolRoundDriver::new(engine).with_allowed_tools(vec!["other_tool".to_string()]);

    let mapping = IdentityMapping::from_legacy_conversation_id("conv-t6".to_string());
    let turn = TurnState::new(
        mapping,
        RunId::new("run-t6"),
        "call blocked_spy_t6".to_string(),
    );
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
