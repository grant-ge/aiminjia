//! S1 三态权限模型回归测试。
//!
//! 验证 PermissionDecision 三态（Allow / Deny / Ask）在各层的行为：
//! - StorePolicyPipeline 对 unknown scope + 无 stored policy 返回 Ask
//! - ToolDispatcher.dispatch() 收到 Ask 时返回 Ok(AskRequired)
//! - Ask 和 Deny 在 Dispatcher 层可区分（AskRequired ≠ Err(PermissionDenied)）
//! - legacy 工具经 registry.execute() 也经过统一 pipeline，unknown scope 不被放行
//! - QueryEngine.run_tool_call_with_bus() 对 Ask 返回 RuntimeToolCallOutcome::AskRequired

use std::sync::Arc;

use app_lib::runtime::store::permission_store::PermissionStore;
use app_lib::runtime::tools::description_context::ToolDescriptionContext;
use app_lib::runtime::tools::permission::{
    default_permission_ask, PermissionReason, StorePolicyPipeline,
};
use app_lib::runtime::tools::{
    PermissionDecision, PermissionPipeline, RuntimeTool, ToolDefinition, ToolDispatchOutcome,
    ToolDispatcher, ToolError, ToolExecutionContext, ToolResult,
};
use async_trait::async_trait;
use serde_json::{json, Value};

// ─── Helpers ─────────────────────────────────────────────────────────────────

fn make_ctx() -> ToolExecutionContext {
    ToolExecutionContext::for_test("test-conv", "test-run", "test-tc")
}

fn is_ask(d: &PermissionDecision) -> bool {
    matches!(d, PermissionDecision::Ask { .. })
}

// ─── Mock: 永远返回 Ask 的 PermissionPipeline ──────────────────────────────

struct AlwaysAskPermissionPipeline;

impl PermissionPipeline for AlwaysAskPermissionPipeline {
    fn authorize(
        &self,
        definition: &ToolDefinition,
        _input: &Value,
        _ctx: &ToolExecutionContext,
    ) -> PermissionDecision {
        PermissionDecision::Ask {
            message: format!("permission confirmation required for '{}'", definition.id),
            suggestions: vec!["Allow once".into(), "Always allow".into(), "Deny".into()],
            remember_options: default_permission_ask().0,
            default_destination: default_permission_ask().1,
            reason: PermissionReason::UnknownScope,
            path_auth_scope: None,
        }
    }
}

// ─── Mock: 简单的 RuntimeTool（用于 dispatcher 测试）──────────────────────

struct EchoTool;

#[async_trait]
impl RuntimeTool for EchoTool {
    fn id(&self) -> &str {
        "echo_tool"
    }

    async fn definition(&self, _ctx: &ToolDescriptionContext) -> ToolDefinition {
        // 使用 unknown scope 以触发 Ask
        ToolDefinition::new("echo_tool", "simple echo tool for testing")
            .with_capability_scope(["custom:test"])
    }

    async fn execute(
        &self,
        _input: Value,
        _ctx: ToolExecutionContext,
    ) -> Result<ToolResult, ToolError> {
        Ok(ToolResult::new("echo_tool", "echo_ok", None))
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// 测试 1：StorePolicyPipeline 对未存储 + unknown scope 返回 Ask
// ─────────────────────────────────────────────────────────────────────────────

#[test]
fn store_policy_pipeline_unknown_scope_no_policy_returns_ask() {
    let store = Arc::new(PermissionStore::in_memory());
    let pipeline = StorePolicyPipeline::new(store);

    let def = ToolDefinition::new("custom_tool", "tool with unknown scope")
        .with_capability_scope(["custom:test"]);
    let ctx = make_ctx();

    let result = pipeline.authorize(&def, &json!({}), &ctx);
    assert!(
        is_ask(&result),
        "StorePolicyPipeline: unknown scope with no stored policy must return Ask, got: {:?}",
        result
    );

    // 确认 Ask 携带了有意义的 message 和 suggestions
    if let PermissionDecision::Ask {
        message,
        suggestions,
        ..
    } = &result
    {
        assert!(
            message.contains("custom:test"),
            "Ask message should mention the unknown scope, got: {}",
            message
        );
        assert!(
            !suggestions.is_empty(),
            "Ask should include at least one suggestion"
        );
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// 测试 2：ToolDispatcher.dispatch() 收到 Ask 时返回 Ok(AskRequired)
// ─────────────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn dispatcher_ask_pipeline_returns_ok_ask_required() {
    let dispatcher = ToolDispatcher::new(Arc::new(AlwaysAskPermissionPipeline));
    dispatcher.register(Arc::new(EchoTool));

    let ctx = make_ctx();
    let outcome = dispatcher.dispatch("echo_tool", json!({}), ctx).await;

    assert!(
        outcome.is_ok(),
        "Ask should not be an error at the dispatcher level, got: {:?}",
        outcome.err()
    );
    assert!(
        matches!(outcome.unwrap(), ToolDispatchOutcome::AskRequired(_)),
        "dispatch() with Ask pipeline must return Ok(AskRequired)"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// 测试 3：AskRequired 与 Err(PermissionDenied) 在 Dispatcher 层可区分
// ─────────────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn dispatcher_ask_is_distinguishable_from_deny() {
    use app_lib::runtime::tools::permission::PermissionReason;

    struct DenyAllPipeline;
    impl PermissionPipeline for DenyAllPipeline {
        fn authorize(
            &self,
            _definition: &ToolDefinition,
            _input: &Value,
            _ctx: &ToolExecutionContext,
        ) -> PermissionDecision {
            PermissionDecision::Deny {
                message: "explicit deny".into(),
                reason: PermissionReason::Other("deny_all".into()),
            }
        }
    }

    // Dispatcher with Ask pipeline → Ok(AskRequired)
    let ask_dispatcher = ToolDispatcher::new(Arc::new(AlwaysAskPermissionPipeline));
    ask_dispatcher.register(Arc::new(EchoTool));
    let ask_ctx = make_ctx();
    let ask_outcome = ask_dispatcher
        .dispatch("echo_tool", json!({}), ask_ctx)
        .await;
    assert!(
        ask_outcome.is_ok(),
        "Ask must not be an Err at the dispatcher level"
    );
    assert!(
        matches!(ask_outcome.unwrap(), ToolDispatchOutcome::AskRequired(_)),
        "Ask must map to AskRequired outcome"
    );

    // Dispatcher with Deny pipeline → Err(PermissionDenied)
    let deny_dispatcher = ToolDispatcher::new(Arc::new(DenyAllPipeline));
    deny_dispatcher.register(Arc::new(EchoTool));
    let deny_ctx = make_ctx();
    let deny_outcome = deny_dispatcher
        .dispatch("echo_tool", json!({}), deny_ctx)
        .await;
    assert!(
        deny_outcome.is_err(),
        "Deny must produce an Err at the dispatcher level"
    );
    assert!(
        matches!(deny_outcome, Err(ToolError::PermissionDenied(_))),
        "Deny must map to ToolError::PermissionDenied"
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// 测试 4：registry.execute() 对 unknown scope 不被 allow_all 放行
//        ——验证 legacy 工具也经过统一 pipeline
// ─────────────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn registry_execute_unknown_scope_not_silently_allowed() {
    #![allow(deprecated)]

    use app_lib::plugin::registry::{RequestScopedRuntimeDeps, ToolRegistry};
    use app_lib::plugin::tool_trait::{ToolOutput, ToolPlugin};
    use std::sync::Arc;

    // A legacy ToolPlugin with a known name but unknown scope surfaced via
    // registry.execute() using StorePolicyPipeline (permission_store set).
    // We register it in the registry, set a permission_store, and confirm
    // that the unknown scope triggers AskRequired (not silent Allow).

    struct LegacyUnknownScopeTool;

    #[async_trait]
    impl ToolPlugin for LegacyUnknownScopeTool {
        fn name(&self) -> &str {
            "legacy_unknown_scope_tool"
        }
        fn description(&self) -> &str {
            "legacy tool with unknown scope"
        }
        fn input_schema(&self) -> serde_json::Value {
            json!({"type": "object", "properties": {}})
        }
        async fn execute(
            &self,
            _ctx: &app_lib::plugin::context::PluginContext,
            _input: serde_json::Value,
        ) -> Result<ToolOutput, app_lib::plugin::tool_trait::ToolError> {
            Ok(ToolOutput::success("legacy_ok"))
        }
    }

    // Register a RuntimeTool with an unknown scope for the same name,
    // so that registry.execute() routes through the runtime path with
    // StorePolicyPipeline and encounters the unknown scope.
    struct RuntimeUnknownScopeTool;

    #[async_trait]
    impl RuntimeTool for RuntimeUnknownScopeTool {
        fn id(&self) -> &str {
            "legacy_unknown_scope_tool"
        }

        async fn definition(&self, _ctx: &ToolDescriptionContext) -> ToolDefinition {
            ToolDefinition::new(
                "legacy_unknown_scope_tool",
                "runtime version with unknown scope",
            )
            .with_capability_scope(["custom:unknown_scope"])
        }
        async fn execute(
            &self,
            _input: Value,
            _ctx: ToolExecutionContext,
        ) -> Result<ToolResult, ToolError> {
            Ok(ToolResult::new(
                "legacy_unknown_scope_tool",
                "runtime_ok",
                None,
            ))
        }
    }

    let registry = ToolRegistry::new();
    registry
        .register(Arc::new(LegacyUnknownScopeTool), "builtin")
        .await;
    registry
        .register_runtime(Arc::new(RuntimeUnknownScopeTool))
        .await;

    // Set a PermissionStore with no policy for "custom:unknown_scope"
    let store = Arc::new(PermissionStore::in_memory());
    registry.set_permission_store(store.clone()).await;

    // Build a minimal PluginContext (same pattern as other tests)
    let tmp_dir = tempfile::TempDir::new().unwrap();
    let tmp = tmp_dir.path().to_path_buf();
    let app_storage = Arc::new(
        app_lib::storage::file_store::AppStorage::new(&tmp).expect("AppStorage::new failed"),
    );
    let file_manager = Arc::new(app_lib::storage::file_manager::FileManager::new(&tmp));

    #[allow(deprecated)]
    let ctx = app_lib::plugin::context::PluginContext {
        storage: app_storage,
        file_manager,
        workspace_path: tmp.clone(),
        conversation_id: "conv-unknown-scope".to_string(),
        session_id: app_lib::runtime::ids::SessionId::new("conv-unknown-scope"),
        run_id: None,
        agent_id: None,
        tavily_api_key: None,
        bocha_api_key: None,
        app_handle: None,
        auth_manager: None,
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
        permission_ctx: None,
        current_persona_id: None,
    };

    let result = registry
        .execute(
            "legacy_unknown_scope_tool",
            &RequestScopedRuntimeDeps::from_plugin_context(&ctx),
            json!({}),
            app_lib::runtime::cancellation::CancellationToken::new(),
        )
        .await;

    // With StorePolicyPipeline + unknown scope + no stored policy → AskRequired
    // This confirms the tool is NOT silently allowed (allow_all is cfg(test) only,
    // and even in tests the registry uses the real pipeline when permission_store is set).
    assert!(
        result.is_err(),
        "unknown scope must not be silently allowed through registry.execute()"
    );

    let err = result.unwrap_err();
    // The error should be AskRequired (not ExecutionFailed or a hard deny)
    assert!(
        matches!(err, app_lib::plugin::tool_trait::ToolError::AskRequired(_)),
        "unknown scope with no stored policy should surface as AskRequired, got: {:?}",
        err
    );
}

// ─────────────────────────────────────────────────────────────────────────────
// 测试 5：QueryEngine.run_tool_call_with_bus() 对 Ask 返回
//         RuntimeToolCallOutcome::AskRequired（结构化上浮至 TurnDriver 层）
// ─────────────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn query_engine_run_tool_call_with_bus_ask_returns_ask_required_outcome() {
    use app_lib::runtime::chat::tool_round_types::{
        RuntimeToolCallOutcome, RuntimeToolCallRequest,
    };
    use app_lib::runtime::event_bus::RuntimeEventBus;
    use app_lib::runtime::identity::IdentityMapping;
    use app_lib::runtime::ids::RunId;
    use app_lib::runtime::query_engine::QueryEngine;
    use app_lib::runtime::state::TurnState;

    // Dispatcher wired to AlwaysAskPermissionPipeline so any tool call returns Ask.
    let dispatcher = Arc::new(ToolDispatcher::new(Arc::new(AlwaysAskPermissionPipeline)));
    dispatcher.register(Arc::new(EchoTool));

    let engine = QueryEngine::with_dispatcher(dispatcher);
    let bus = RuntimeEventBus::new();

    let mapping = IdentityMapping::from_legacy_conversation_id("conv-ask-outcome");
    let turn = TurnState::new(
        mapping,
        RunId::new("run-ask-outcome"),
        "ask test".to_string(),
    );

    let outcome = engine
        .run_tool_call_with_bus(
            &turn,
            &bus,
            RuntimeToolCallRequest {
                tool_call_id: "tc-ask-001".to_string(),
                tool_name: "echo_tool".to_string(),
                args: json!({}),
                purpose: None,
            },
        )
        .await
        .expect("run_tool_call_with_bus must not return Err for Ask (Ask is not an infra error)");

    // The key assertion: Ask is now structurally surfaced as AskRequired,
    // not flattened into Completed(is_error=true).
    assert!(
        matches!(outcome, RuntimeToolCallOutcome::AskRequired { .. }),
        "QueryEngine.run_tool_call_with_bus() with an Ask pipeline must return \
         RuntimeToolCallOutcome::AskRequired, not Completed(is_error=true). \
         Got: {:?}",
        outcome
    );

    // Confirm the helper methods still work correctly for AskRequired.
    assert_eq!(outcome.tool_call_id(), "tc-ask-001");
    assert_eq!(outcome.tool_name(), "echo_tool");
    assert!(
        outcome.is_error(),
        "AskRequired must report is_error()=true so downstream guards continue to apply"
    );
    assert_eq!(
        outcome.content(),
        "",
        "AskRequired must not synthesize fallback tool_result content once pending permission routing owns the lifecycle"
    );
    assert_eq!(
        outcome.max_result_size_chars(),
        0,
        "AskRequired should not advertise tool_result truncation budget because no tool_result is produced before resolution"
    );
}
