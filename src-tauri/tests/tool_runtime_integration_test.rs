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
/// Without the fix, `list_directory` (which calls `require_workspace_root`) would
/// return `PermissionDenied` because `ctx.capability` would be `None`.
#[tokio::test]
async fn query_engine_injects_capability_context_for_workspace_tool() {
    use app_lib::runtime::event_bus::RuntimeEventBus;
    use app_lib::runtime::identity::IdentityMapping;
    use app_lib::runtime::ids::RunId;
    use app_lib::runtime::state::TurnState;
    use app_lib::runtime::tools::builtin::workspace::ListDirectoryRuntimeTool;
    use app_lib::runtime::tools::{AllowAllPermissionPipeline, ToolDispatcher};
    use std::sync::Arc;
    use tempfile::TempDir;

    let tmp = TempDir::new().unwrap();
    // Create a file so list_directory returns at least one entry
    std::fs::write(tmp.path().join("hello.txt"), b"world").unwrap();

    // Build a dispatcher with just the workspace tool and allow-all permissions
    let dispatcher = Arc::new(ToolDispatcher::new(Arc::new(AllowAllPermissionPipeline)));
    dispatcher.register(Arc::new(ListDirectoryRuntimeTool));

    // Build engine WITH workspace_path injected (the fix under test)
    let engine =
        QueryEngine::with_dispatcher(dispatcher).with_workspace_path(tmp.path().to_path_buf());

    let mapping = IdentityMapping::from_legacy_conversation_id("conv-ws".to_string());
    let turn = TurnState::new(mapping, RunId::new("run-ws"), "list workspace".to_string());
    let bus = RuntimeEventBus::new();

    // run_tool_with_bus should succeed — capability context is injected so
    // require_workspace_root() resolves to tmp.path() instead of returning PermissionDenied.
    let result = engine
        .run_tool_with_bus(&turn, &bus, "list_directory")
        .await;
    assert!(
        result.is_ok(),
        "list_directory should succeed when QueryEngine has workspace_path set: {:?}",
        result
    );
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
        fn definition(&self) -> ToolDefinition {
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
