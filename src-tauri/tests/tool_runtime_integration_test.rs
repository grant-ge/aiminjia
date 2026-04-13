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
    let engine = QueryEngine::with_dispatcher(dispatcher)
        .with_workspace_path(tmp.path().to_path_buf());

    let mapping = IdentityMapping::from_legacy_conversation_id("conv-ws".to_string());
    let turn = TurnState::new(
        mapping,
        RunId::new("run-ws"),
        "list workspace".to_string(),
    );
    let bus = RuntimeEventBus::new();

    // run_tool_with_bus should succeed — capability context is injected so
    // require_workspace_root() resolves to tmp.path() instead of returning PermissionDenied.
    let result = engine.run_tool_with_bus(&turn, &bus, "list_directory").await;
    assert!(
        result.is_ok(),
        "list_directory should succeed when QueryEngine has workspace_path set: {:?}",
        result
    );
}

