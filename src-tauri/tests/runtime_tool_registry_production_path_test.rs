//! Verify that RuntimeTools registered via register_runtime() take precedence
//! in to_runtime_dispatcher() over legacy ToolPlugin adapters.

#![allow(deprecated)]

use app_lib::plugin::registry::{RequestScopedRuntimeDeps, ToolRegistry};
use app_lib::runtime::tools::{
    RuntimeTool, ToolDefinition, ToolError, ToolExecutionContext, ToolResult,
};
use async_trait::async_trait;
use serde_json::{json, Value};
use std::sync::Arc;

// ─── A minimal RuntimeTool for testing ───────────────────────────────────────

struct FakeRuntimeTool {
    id: &'static str,
}

#[async_trait]
impl RuntimeTool for FakeRuntimeTool {
    fn definition(&self) -> ToolDefinition {
        ToolDefinition::new(self.id, "fake runtime tool for testing")
    }
    async fn execute(
        &self,
        _input: Value,
        _ctx: ToolExecutionContext,
    ) -> Result<ToolResult, ToolError> {
        Ok(ToolResult::new(
            self.id,
            format!("runtime:{}", self.id),
            None,
        ))
    }
}

// ─── Helper: build a minimal PluginContext (for legacy fallback path) ─────────

#[allow(deprecated)]
fn make_test_plugin_ctx(conversation_id: &str) -> app_lib::plugin::context::PluginContext {
    use std::sync::Arc;
    let tmp_dir = tempfile::TempDir::new().expect("TempDir::new failed");
    let tmp = tmp_dir.path().to_path_buf();
    // Keep tmp_dir alive for the duration of the context
    let _ = &tmp_dir;
    let storage = Arc::new(
        app_lib::storage::file_store::AppStorage::new(&tmp).expect("AppStorage::new failed"),
    );
    let file_manager = Arc::new(app_lib::storage::file_manager::FileManager::new(&tmp));
    let session_manager = Arc::new(app_lib::python::session::PythonSessionManager::new(
        tmp.clone(),
        None,
    ));
    #[allow(deprecated)]
    app_lib::plugin::context::PluginContext {
        storage,
        file_manager,
        workspace_path: tmp.clone(),
        conversation_id: conversation_id.to_string(),
        session_id: app_lib::runtime::ids::SessionId::new(conversation_id.to_string()),
        run_id: None,
        agent_id: None,
        tavily_api_key: None,
        bocha_api_key: None,
        app_handle: None,
        session_manager,
        auth_manager: None,
        connector_engine: None,
        dingtalk_bridge: None,
        use_cloud: false,
        model: String::new(),
        gateway: None,
        tool_registry: None,
        app_settings: None,
        agent_runtime: None,
        event_bus: None,
        skill_registry: None,
        authorized_workspace: None,
        read_file_state: None,
        cancellation: None,
        permission_mode: app_lib::runtime::tools::permission::PermissionMode::Default,
            runtime_resolver: None,
    }
}

// ─── Test 1: register_runtime() makes tool dispatchable ──────────────────────

#[tokio::test]
async fn register_runtime_adds_to_registry() {
    let registry = ToolRegistry::new();
    let tool = Arc::new(FakeRuntimeTool { id: "test_tool" });
    registry.register_runtime(tool).await;

    let ctx = make_test_plugin_ctx("conv-test");
    let dispatcher = registry
        .to_runtime_dispatcher(RequestScopedRuntimeDeps::from_plugin_context(&ctx))
        .await;
    let exec_ctx = ToolExecutionContext::for_test("conv-test", "run-1", "tc-1");
    let outcome = dispatcher.dispatch("test_tool", json!({}), exec_ctx).await;
    assert!(outcome.is_ok(), "Runtime tool should be dispatchable");
    let content = match outcome.unwrap() {
        app_lib::runtime::tools::ToolDispatchOutcome::Completed { result, .. } => result.content,
        app_lib::runtime::tools::ToolDispatchOutcome::AskRequired(_) => {
            panic!("unexpected AskRequired for test_tool")
        }
        app_lib::runtime::tools::ToolDispatchOutcome::InteractionRequired(_) => {
            panic!("unexpected InteractionRequired for test_tool")
        }
    };
    assert_eq!(content, "runtime:test_tool");
}

// ─── Test 2: RuntimeTool wins over legacy ToolPlugin for same name ────────────

#[tokio::test]
async fn runtime_tool_takes_precedence_over_legacy_for_same_name() {
    use app_lib::plugin::tool_trait::{ToolOutput, ToolPlugin};

    struct LegacyDualTool;

    #[async_trait]
    impl ToolPlugin for LegacyDualTool {
        fn name(&self) -> &str {
            "dual_tool"
        }
        fn description(&self) -> &str {
            "legacy version"
        }
        fn input_schema(&self) -> Value {
            json!({"type": "object", "required": [], "properties": {}})
        }
        async fn execute(
            &self,
            _ctx: &app_lib::plugin::context::PluginContext,
            _input: Value,
        ) -> Result<ToolOutput, app_lib::plugin::tool_trait::ToolError> {
            Ok(ToolOutput::success("legacy:dual_tool"))
        }
    }

    let registry = ToolRegistry::new();
    // Register legacy version first
    registry.register(Arc::new(LegacyDualTool), "builtin").await;
    // Register runtime version — should win
    registry
        .register_runtime(Arc::new(FakeRuntimeTool { id: "dual_tool" }))
        .await;

    let ctx = make_test_plugin_ctx("conv-test2");
    let dispatcher = registry
        .to_runtime_dispatcher(RequestScopedRuntimeDeps::from_plugin_context(&ctx))
        .await;
    let exec_ctx = ToolExecutionContext::for_test("conv-test2", "run-2", "tc-2");
    let outcome = dispatcher
        .dispatch("dual_tool", json!({}), exec_ctx)
        .await
        .expect("dispatch should succeed");
    let content = match outcome {
        app_lib::runtime::tools::ToolDispatchOutcome::Completed { result, .. } => result.content,
        app_lib::runtime::tools::ToolDispatchOutcome::AskRequired(_) => {
            panic!("unexpected AskRequired for dual_tool")
        }
        app_lib::runtime::tools::ToolDispatchOutcome::InteractionRequired(_) => {
            panic!("unexpected InteractionRequired for dual_tool")
        }
    };
    assert_eq!(
        content, "runtime:dual_tool",
        "RuntimeTool should take precedence over legacy ToolPlugin"
    );
}

// ─── Test 3: get_all_schemas() includes runtime tool schema from catalog ──────

#[tokio::test]
async fn get_all_schemas_includes_runtime_tool_schema() {
    // Use list_directory which IS in TOOL_CATALOG
    use app_lib::runtime::tools::builtin::workspace::ListDirectoryRuntimeTool;

    let registry = ToolRegistry::new();
    registry
        .register_runtime(Arc::new(ListDirectoryRuntimeTool))
        .await;

    let schemas = registry.get_all_schemas().await;
    let names: Vec<&str> = schemas.iter().map(|s| s.name.as_str()).collect();
    assert!(
        names.contains(&"list_directory"),
        "Schema should include list_directory, got: {:?}",
        names
    );
}

// ─── Test 4: legacy tools not superseded still appear in schemas ──────────────

#[tokio::test]
async fn legacy_only_tool_still_appears_in_get_all_schemas() {
    use app_lib::plugin::tool_trait::{ToolOutput, ToolPlugin};

    struct LegacyOnlyTool;

    #[async_trait]
    impl ToolPlugin for LegacyOnlyTool {
        fn name(&self) -> &str {
            "legacy_only_tool"
        }
        fn description(&self) -> &str {
            "not yet migrated"
        }
        fn input_schema(&self) -> Value {
            json!({"type": "object"})
        }
        async fn execute(
            &self,
            _ctx: &app_lib::plugin::context::PluginContext,
            _input: Value,
        ) -> Result<ToolOutput, app_lib::plugin::tool_trait::ToolError> {
            Ok(ToolOutput::success("ok"))
        }
    }

    let registry = ToolRegistry::new();
    registry.register(Arc::new(LegacyOnlyTool), "builtin").await;

    let schemas = registry.get_all_schemas().await;
    let names: Vec<&str> = schemas.iter().map(|s| s.name.as_str()).collect();
    assert!(
        names.contains(&"legacy_only_tool"),
        "Legacy-only tool should still appear in schemas, got: {:?}",
        names
    );
}
