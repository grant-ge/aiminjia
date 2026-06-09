//! Task 4: Chat runtime dispatcher production-path contract tests.
//!
//! These tests lock three constraints that define the "dispatcher contract" in
//! the production chat main-line:
//!
//! 1. When the runtime dispatcher has a tool registered, tool calls are routed
//!    through `ToolDispatcher::dispatch()` (not short-circuited elsewhere).
//!
//! 2. The authorized workspace injected into `QueryEngine` is preserved in the
//!    `CapabilityContext` that workspace-scoped tools receive during execution.
//!
//! 3. Tool events (`tool:executing` / `tool:completed`) are emitted exactly once
//!    per tool call — no double-emit from concurrent runtime + legacy paths.
//!
//! Note: These tests verify the *runtime dispatcher path* (`QueryEngine::run_tool_with_bus`).
//! The actual LLM loop in production still runs via the legacy executor path
//! (`chat_runtime_impl.rs`). The dispatcher contract here is the long-term target
//! path; `tool_runtime_integration_test.rs` covers the transitional wiring.

use std::sync::{Arc, Mutex};

use app_lib::runtime::event_bus::{RuntimeEventBus, RuntimeEventSubscriber};
use app_lib::runtime::identity::IdentityMapping;
use app_lib::runtime::ids::RunId;
use app_lib::runtime::query_engine::QueryEngine;
use app_lib::runtime::state::TurnState;
use app_lib::runtime::store::AuthorizedWorkspaceRef;
use app_lib::runtime::tools::description_context::ToolDescriptionContext;
use app_lib::runtime::tools::{
    AllowAllPermissionPipeline, RuntimeTool, ToolDefinition, ToolDispatcher, ToolError,
    ToolExecutionContext, ToolResult,
};
use app_lib::transport::tauri_event_adapter::TauriEventAdapter;
use app_lib::transport::testing::RecordingRuntimeHost;
use async_trait::async_trait;
use serde_json::Value;
use tempfile::TempDir;

// ── Shared test helpers ───────────────────────────────────────────────────────

/// A minimal `RuntimeTool` that records which events fire and whether capability
/// context was present at call time.
struct CapturingRuntimeTool {
    name: &'static str,
    /// Captures the `authorized_workspace` root path seen in `CapabilityContext.storage`.
    captured_authorized_root: Arc<Mutex<Option<std::path::PathBuf>>>,
}

#[async_trait]
impl RuntimeTool for CapturingRuntimeTool {
    fn id(&self) -> &str {
        self.name
    }

    async fn definition(&self, _ctx: &ToolDescriptionContext) -> ToolDefinition {
        ToolDefinition::new(self.name, "Test tool that captures capability context")
    }

    async fn execute(
        &self,
        _input: Value,
        ctx: ToolExecutionContext,
    ) -> Result<ToolResult, ToolError> {
        let authorized_root = ctx
            .capability
            .as_ref()
            .and_then(|cap| cap.storage.as_ref())
            .and_then(|storage| storage.authorized_workspace.as_ref())
            .map(|aw| aw.root_path.clone());
        *self.captured_authorized_root.lock().unwrap() = authorized_root;
        Ok(ToolResult::new(
            self.name,
            format!("dispatched:{}", self.name),
            None,
        ))
    }
}

fn make_capturing_tool(
    name: &'static str,
) -> (
    Arc<CapturingRuntimeTool>,
    Arc<Mutex<Option<std::path::PathBuf>>>,
) {
    let captured = Arc::new(Mutex::new(None));
    let tool = Arc::new(CapturingRuntimeTool {
        name,
        captured_authorized_root: captured.clone(),
    });
    (tool, captured)
}

// ── Test 1 ────────────────────────────────────────────────────────────────────

/// When the runtime dispatcher has a tool registered, `QueryEngine::run_tool_with_bus`
/// routes the call through `ToolDispatcher::dispatch()`.
///
/// Evidence: the tool's `execute()` is reached (output == `"dispatched:prod_tool"`)
/// and the call does not return an error due to routing failure.
#[tokio::test]
async fn runtime_chat_mainline_dispatches_tools_via_runtime_dispatcher() {
    let (tool, _captured) = make_capturing_tool("prod_tool");

    let dispatcher = Arc::new(ToolDispatcher::new(Arc::new(AllowAllPermissionPipeline)));
    dispatcher.register(tool);

    let engine = QueryEngine::with_dispatcher(dispatcher);
    let mapping = IdentityMapping::from_legacy_conversation_id("conv-prod".to_string());
    let turn = TurnState::new(mapping, RunId::new("run-prod"), "dispatch test".to_string());
    let bus = RuntimeEventBus::new();

    let result = engine.run_tool_with_bus(&turn, &bus, "prod_tool").await;

    assert!(
        result.is_ok(),
        "runtime dispatcher should route prod_tool call successfully: {:?}",
        result
    );
}

// ── Test 2 ────────────────────────────────────────────────────────────────────

/// When `QueryEngine` is constructed with `with_authorized_workspace(Some(...))`,
/// the authorized workspace path must arrive in the `CapabilityContext` that the
/// dispatched tool receives.
///
/// This is the Workspace-First guarantee: workspace-scoped tools executed through
/// `QueryEngine` always see the session-authorized directory.
#[tokio::test]
async fn runtime_chat_mainline_preserves_workspace_first_authorized_directory() {
    let internal_workspace = TempDir::new().unwrap();
    let authorized_workspace = TempDir::new().unwrap();

    let (tool, captured) = make_capturing_tool("workspace_capture_tool");

    let dispatcher = Arc::new(ToolDispatcher::new(Arc::new(AllowAllPermissionPipeline)));
    dispatcher.register(tool);

    let engine = QueryEngine::with_dispatcher(dispatcher)
        .with_workspace_path(internal_workspace.path().to_path_buf())
        .with_authorized_workspace(Some(AuthorizedWorkspaceRef {
            id: "aw-prod".to_string(),
            root_path: authorized_workspace.path().to_path_buf(),
            display_name: "prod-authorized".to_string(),
        }));

    let mapping = IdentityMapping::from_legacy_conversation_id("conv-workspace-first".to_string());
    let turn = TurnState::new(
        mapping,
        RunId::new("run-workspace-first"),
        "workspace capture".to_string(),
    );
    let bus = RuntimeEventBus::new();

    engine
        .run_tool_with_bus(&turn, &bus, "workspace_capture_tool")
        .await
        .expect("workspace capture tool should dispatch successfully");

    let seen = captured.lock().unwrap().clone();
    assert_eq!(
        seen,
        Some(authorized_workspace.path().to_path_buf()),
        "authorized_workspace root path must be preserved in CapabilityContext \
         passed to the dispatched tool; got {:?}",
        seen
    );
}

// ── Test 3 ────────────────────────────────────────────────────────────────────

/// Tool events (`tool:executing` / `tool:completed`) must be emitted exactly
/// once per tool call when dispatched through `QueryEngine::run_tool_with_bus`.
///
/// A double-emit would occur if both the runtime bus path and the legacy
/// `app.emit()` path fire for the same tool. This test verifies the runtime
/// path emits the events exactly once (the legacy path is guarded by the
/// executor-backed flag in production).
#[tokio::test]
async fn runtime_chat_mainline_emits_tool_events_once() {
    let (tool, _captured) = make_capturing_tool("event_count_tool");

    let dispatcher = Arc::new(ToolDispatcher::new(Arc::new(AllowAllPermissionPipeline)));
    dispatcher.register(tool);

    let engine = QueryEngine::with_dispatcher(dispatcher);

    let host = RecordingRuntimeHost::new();
    let bus = RuntimeEventBus::new();
    let _adapter: Arc<dyn RuntimeEventSubscriber> = Arc::new(TauriEventAdapter::new(host.clone()));
    bus.subscribe(_adapter.clone());

    let mapping = IdentityMapping::from_legacy_conversation_id("conv-event-count".to_string());
    let turn = TurnState::new(
        mapping,
        RunId::new("run-event-count"),
        "event count".to_string(),
    );

    engine
        .run_tool_with_bus(&turn, &bus, "event_count_tool")
        .await
        .expect("event_count_tool should dispatch successfully");

    let event_names = host.trace().event_names();

    // Count how many times each tool event appears.
    let executing_count = event_names
        .iter()
        .filter(|e| e.as_str() == "tool:executing")
        .count();
    let completed_count = event_names
        .iter()
        .filter(|e| e.as_str() == "tool:completed")
        .count();

    assert_eq!(
        executing_count, 1,
        "tool:executing should be emitted exactly once per tool call; got {} times. \
         Full event sequence: {:?}",
        executing_count, event_names
    );
    assert_eq!(
        completed_count, 1,
        "tool:completed should be emitted exactly once per tool call; got {} times. \
         Full event sequence: {:?}",
        completed_count, event_names
    );
}
