use app_lib::runtime::query_engine::QueryEngine;
use app_lib::runtime::tools::testing::single_legacy_tool_dispatcher;

#[tokio::test]
async fn query_engine_routes_tool_calls_through_dispatcher_and_permission_pipeline() {
    let dispatcher = single_legacy_tool_dispatcher("python_exec");
    let engine = QueryEngine::for_test(dispatcher);
    let trace = engine
        .run_single_tool_turn("conv-1", "run-1", "python_exec")
        .await
        .unwrap();
    assert_eq!(
        trace,
        vec!["tool:executing", "tool:completed", "streaming:done"]
    );
}

// ── F10: QueryEngine injects capability context for workspace tools ───────────

/// Verify that when `QueryEngine::with_workspace_path` is set, the engine injects
/// a `CapabilityContext` into `ToolExecutionContext` before dispatching to a
/// workspace-scoped RuntimeTool.
///
/// Without the fix, workspace tools (which call `require_workspace_root`) would
/// return `PermissionDenied` because `ctx.capability` would be `None`.
///
/// The test exercises the full QueryEngine dispatch path (not a direct tool call)
/// to confirm that capability injection happens inside the engine's dispatch logic.
#[tokio::test]
async fn query_engine_injects_capability_context_for_workspace_tool() {
    use app_lib::runtime::chat::tool_round_types::RuntimeToolCallRequest;
    use app_lib::runtime::chat::tool_round_types::RuntimeToolCallOutcome;
    use app_lib::runtime::event_bus::RuntimeEventBus;
    use app_lib::runtime::identity::IdentityMapping;
    use app_lib::runtime::ids::RunId;
    use app_lib::runtime::state::TurnState;
    use app_lib::runtime::tools::builtin::workspace::SearchFilesRuntimeTool;
    use app_lib::runtime::tools::description_context::ToolDescriptionContext;
use app_lib::runtime::tools::{AllowAllPermissionPipeline, ToolDispatcher};
    use std::sync::Arc;
    use tempfile::TempDir;

    let tmp = TempDir::new().unwrap();
    std::fs::write(tmp.path().join("hello.txt"), b"world").unwrap();

    // Build a dispatcher with the workspace tool registered and allow-all permissions.
    let dispatcher = Arc::new(ToolDispatcher::new(Arc::new(AllowAllPermissionPipeline)));
    dispatcher.register(Arc::new(SearchFilesRuntimeTool));

    // ── Negative case: no workspace_path → capability is None → error outcome ──

    let engine_no_workspace = QueryEngine::with_dispatcher(dispatcher.clone());

    let mapping_neg = IdentityMapping::from_legacy_conversation_id("conv-ws-neg".to_string());
    let turn_neg = TurnState::new(
        mapping_neg,
        RunId::new("run-ws-neg"),
        "search without workspace".to_string(),
    );
    let bus_neg = RuntimeEventBus::new();
    let call_neg = RuntimeToolCallRequest {
        tool_call_id: "tc-neg-1".to_string(),
        tool_name: "Glob".to_string(),
        args: serde_json::json!({ "pattern": "*" }),
        purpose: None,
    };
    let outcome_neg = engine_no_workspace
        .run_tool_call_with_bus(&turn_neg, &bus_neg, call_neg)
        .await
        .unwrap();
    match outcome_neg {
        RuntimeToolCallOutcome::Completed { is_error, .. } => assert!(
            is_error,
            "search_files without workspace_path should produce an error outcome"
        ),
        other => panic!(
            "Expected Completed outcome for no-workspace case, got: {:?}",
            other
        ),
    }

    // ── Positive case: with_workspace_path → QueryEngine injects capability → success ──

    let engine = QueryEngine::with_dispatcher(dispatcher)
        .with_workspace_path(tmp.path().to_path_buf());

    let mapping = IdentityMapping::from_legacy_conversation_id("conv-ws".to_string());
    let turn = TurnState::new(
        mapping,
        RunId::new("run-ws"),
        "search with workspace".to_string(),
    );
    let bus = RuntimeEventBus::new();
    let call = RuntimeToolCallRequest {
        tool_call_id: "tc-1".to_string(),
        tool_name: "Glob".to_string(),
        args: serde_json::json!({ "pattern": "*" }),
        purpose: None,
    };
    let outcome = engine
        .run_tool_call_with_bus(&turn, &bus, call)
        .await
        .unwrap();
    match outcome {
        RuntimeToolCallOutcome::Completed { is_error, .. } => assert!(
            !is_error,
            "search_files should succeed when QueryEngine injects capability context"
        ),
        other => panic!("Expected Completed outcome, got: {:?}", other),
    }
}

// ── Workspace-First: authorized workspace injection ──────────────────────────

#[tokio::test]
async fn query_engine_injects_authorized_workspace_into_capability_context() {
    use app_lib::runtime::event_bus::RuntimeEventBus;
    use app_lib::runtime::identity::IdentityMapping;
    use app_lib::runtime::ids::RunId;
    use app_lib::runtime::state::TurnState;
    use app_lib::runtime::store::AuthorizedWorkspaceRef;
    use app_lib::runtime::tools::{
        AllowAllPermissionPipeline, RuntimeTool, ToolDefinition, ToolDispatcher, ToolError,
        ToolExecutionContext, ToolResult,
    };
    use app_lib::runtime::tools::description_context::ToolDescriptionContext;
    use async_trait::async_trait;
    use serde_json::Value;
    use std::path::PathBuf;
    use std::sync::{Arc, Mutex};
    use tempfile::TempDir;

    struct CaptureAuthorizedWorkspaceTool {
        seen_root: Arc<Mutex<Option<PathBuf>>>,
    }

    #[async_trait]
    impl RuntimeTool for CaptureAuthorizedWorkspaceTool {
        fn id(&self) -> &str {
            "capture_authorized_workspace"
        }

        async fn definition(&self, _ctx: &ToolDescriptionContext) -> ToolDefinition {
            ToolDefinition::new("capture_authorized_workspace", "Capture capability context")
        }

        async fn execute(
            &self,
            _input: Value,
            ctx: ToolExecutionContext,
        ) -> Result<ToolResult, ToolError> {
            let seen = ctx
                .capability
                .as_ref()
                .and_then(|cap| cap.storage.as_ref())
                .and_then(|storage| storage.authorized_workspace.as_ref())
                .map(|aw| aw.root_path.clone());
            *self.seen_root.lock().unwrap() = seen;
            Ok(ToolResult {
                tool_name: "capture_authorized_workspace".to_string(),
                content: "ok".to_string(),
                data: None,
                file_meta: None,
                is_degraded: false,
                degradation_notice: None,
            })
        }
    }

    let internal_workspace = TempDir::new().unwrap();
    let authorized_workspace = TempDir::new().unwrap();
    let seen_root = Arc::new(Mutex::new(None));

    let dispatcher = Arc::new(ToolDispatcher::new(Arc::new(AllowAllPermissionPipeline)));
    dispatcher.register(Arc::new(CaptureAuthorizedWorkspaceTool {
        seen_root: seen_root.clone(),
    }));

    let engine = QueryEngine::with_dispatcher(dispatcher)
        .with_workspace_path(internal_workspace.path().to_path_buf())
        .with_authorized_workspace(Some(AuthorizedWorkspaceRef {
            id: "aw-1".to_string(),
            root_path: authorized_workspace.path().to_path_buf(),
            display_name: "authorized".to_string(),
        }));

    let mapping = IdentityMapping::from_legacy_conversation_id("conv-authorized".to_string());
    let turn = TurnState::new(
        mapping,
        RunId::new("run-authorized"),
        "capture authorized workspace".to_string(),
    );
    let bus = RuntimeEventBus::new();

    engine
        .run_tool_with_bus(&turn, &bus, "capture_authorized_workspace")
        .await
        .unwrap();

    let captured = seen_root.lock().unwrap().clone();
    assert_eq!(
        captured,
        Some(authorized_workspace.path().to_path_buf()),
        "QueryEngine should inject authorized_workspace into capability context"
    );
}
