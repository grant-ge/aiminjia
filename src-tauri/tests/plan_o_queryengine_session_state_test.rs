//! Plan-O: QueryEngine cross-turn session state and turn outcomes.

use std::sync::Arc;

use async_trait::async_trait;
use serde_json::{json, Value};

use app_lib::runtime::cancellation::CancellationToken;
use app_lib::runtime::chat::tool_round_types::RuntimeToolCallRequest;
use app_lib::runtime::chat::turn_config::{LlmStepInput, LlmStepResult, TurnError};
use app_lib::runtime::chat::{ChatTurnRequest, RuntimeChatTurnDriver, RuntimeLlmExecutor};
use app_lib::runtime::event_bus::RuntimeEventBus;
use app_lib::runtime::events::RuntimeEventKind;
use app_lib::runtime::identity::IdentityMapping;
use app_lib::runtime::ids::RunId;
use app_lib::runtime::query_engine::QueryEngine;
use app_lib::runtime::state::TurnState;
use app_lib::runtime::tools::context::ToolExecutionContext;
use app_lib::runtime::tools::definition::ToolDefinition;
use app_lib::runtime::tools::description_context::ToolDescriptionContext;
use app_lib::runtime::tools::executor::{ToolError, ToolResult};
use app_lib::runtime::tools::permission::{
    AllowAllPermissionPipeline, PermissionDecision, PermissionReason,
};
use app_lib::runtime::tools::{RuntimeTool, ToolDispatcher};

#[test]
fn o1_chat_turn_outcome_variants_compile() {
    use app_lib::runtime::chat::{ChatTurnOutcome, PermissionDenialRecord};

    let success = ChatTurnOutcome::Success;
    let cancelled = ChatTurnOutcome::Cancelled;
    let max_iter = ChatTurnOutcome::MaxIterationsReached { iterations: 30 };
    let budget = ChatTurnOutcome::BudgetExceeded {
        reason: "Reached maximum budget ($1.00)".to_string(),
        total_cost_usd: 1.05,
    };
    let exec_err = ChatTurnOutcome::ExecutionError {
        message: "LLM gateway timeout".to_string(),
    };

    assert!(matches!(success, ChatTurnOutcome::Success));
    assert!(matches!(cancelled, ChatTurnOutcome::Cancelled));
    assert!(matches!(
        max_iter,
        ChatTurnOutcome::MaxIterationsReached { iterations: 30 }
    ));
    assert!(matches!(budget, ChatTurnOutcome::BudgetExceeded { .. }));
    assert!(matches!(exec_err, ChatTurnOutcome::ExecutionError { .. }));

    let record = PermissionDenialRecord {
        tool_name: "Bash".to_string(),
        tool_call_id: "tc-001".to_string(),
        reason: "dangerous_pattern".to_string(),
    };
    assert_eq!(record.tool_name, "Bash");
}

#[test]
fn o1_chat_turn_outcome_is_error_helper() {
    use app_lib::runtime::chat::ChatTurnOutcome;

    assert!(!ChatTurnOutcome::Success.is_error());
    assert!(!ChatTurnOutcome::Cancelled.is_error());
    assert!(ChatTurnOutcome::MaxIterationsReached { iterations: 30 }.is_error());
    assert!(ChatTurnOutcome::BudgetExceeded {
        reason: "over budget".to_string(),
        total_cost_usd: 2.0,
    }
    .is_error());
    assert!(ChatTurnOutcome::ExecutionError {
        message: "boom".to_string(),
    }
    .is_error());
}

struct AlwaysDeniedTool;

#[async_trait]
impl RuntimeTool for AlwaysDeniedTool {
    fn id(&self) -> &str {
        "always_denied"
    }

    async fn definition(&self, _ctx: &ToolDescriptionContext) -> ToolDefinition {
        ToolDefinition::new("always_denied", "always permission denied")
    }

    async fn check_permissions(
        &self,
        _input: &Value,
        _ctx: &ToolExecutionContext,
    ) -> Option<PermissionDecision> {
        Some(PermissionDecision::Deny {
            message: "not allowed".to_string(),
            reason: PermissionReason::Other("test_deny".to_string()),
        })
    }

    async fn execute(
        &self,
        _input: Value,
        _ctx: ToolExecutionContext,
    ) -> Result<ToolResult, ToolError> {
        unreachable!("must not execute denied tool")
    }
}

#[tokio::test]
async fn o2_permission_denial_is_recorded_after_dispatch() {
    let dispatcher = Arc::new(ToolDispatcher::new(Arc::new(AllowAllPermissionPipeline)));
    dispatcher.register(Arc::new(AlwaysDeniedTool));

    let engine = QueryEngine::for_test(dispatcher);
    let bus = RuntimeEventBus::new();
    let mapping = IdentityMapping::from_legacy_conversation_id("sess-o2");
    let turn = TurnState::new(mapping, RunId::new("run-o2"), "test".to_string());

    let call = RuntimeToolCallRequest {
        tool_call_id: "tc-o2".to_string(),
        tool_name: "always_denied".to_string(),
        args: json!({}),
        purpose: None,
    };

    let _outcome = engine
        .run_tool_call_with_bus(&turn, &bus, call)
        .await
        .expect("run_tool_call_with_bus should not Err on permission denied");

    let denials = engine.get_permission_denials();
    assert_eq!(denials.len(), 1);
    assert_eq!(denials[0].tool_name, "always_denied");
    assert_eq!(denials[0].tool_call_id, "tc-o2");
}

#[tokio::test]
async fn o2_permission_denials_accumulate_across_calls() {
    let dispatcher = Arc::new(ToolDispatcher::new(Arc::new(AllowAllPermissionPipeline)));
    dispatcher.register(Arc::new(AlwaysDeniedTool));

    let engine = QueryEngine::for_test(dispatcher);
    let bus = RuntimeEventBus::new();
    let mapping = IdentityMapping::from_legacy_conversation_id("sess-o2b");
    let turn = TurnState::new(mapping, RunId::new("run-o2b"), "test".to_string());

    for i in 0..3 {
        let call = RuntimeToolCallRequest {
            tool_call_id: format!("tc-o2b-{i}"),
            tool_name: "always_denied".to_string(),
            args: json!({}),
            purpose: None,
        };
        let _ = engine.run_tool_call_with_bus(&turn, &bus, call).await;
    }

    assert_eq!(engine.get_permission_denials().len(), 3);
}

#[test]
fn o4_budget_not_exceeded_when_below_limit() {
    let engine = QueryEngine::new()
        .with_max_budget_usd(1.0)
        .with_cost_per_1k_tokens(0.001);
    engine.accumulate_usage(50_000, 50_000);
    assert!(!engine.is_budget_exceeded());
}

#[test]
fn o4_budget_exceeded_when_over_limit() {
    let engine = QueryEngine::new()
        .with_max_budget_usd(0.05)
        .with_cost_per_1k_tokens(0.001);
    engine.accumulate_usage(100_000, 100_000);
    assert!(engine.is_budget_exceeded());
}

#[test]
fn o4_no_budget_limit_never_exceeded() {
    let engine = QueryEngine::new();
    engine.accumulate_usage(1_000_000, 1_000_000);
    assert!(!engine.is_budget_exceeded());
}

#[test]
fn o4_estimated_cost_usd_calculation() {
    let engine = QueryEngine::new()
        .with_max_budget_usd(10.0)
        .with_cost_per_1k_tokens(0.002);
    engine.accumulate_usage(10_000, 5_000);
    let cost = engine.estimated_cost_usd();
    let expected = 15.0 * 0.002;
    assert!((cost - expected).abs() < 1e-9);
}

#[test]
fn o4_estimated_cost_usd_anthropic_cache_weighting() {
    // Anthropic pricing: cache_creation = 1.25x input, cache_read = 0.1x input
    let engine = QueryEngine::new().with_cost_per_1k_tokens(0.001);
    // 1000 plain input + 1000 output + 1000 cache_creation + 1000 cache_read
    // weighted = 1000 + 1000 + 1000*1.25 + 1000*0.1 = 3350 tokens
    // cost = 3350/1000 * 0.001 = 0.00335
    engine.accumulate_usage(1000, 1000);
    engine.accumulate_cache_usage(1000, 1000);
    let cost = engine.estimated_cost_usd();
    let expected = 3.350 * 0.001;
    assert!(
        (cost - expected).abs() < 1e-9,
        "got {} expected {}",
        cost,
        expected
    );
}

struct ImmediateContentExecutor;

#[async_trait]
impl RuntimeLlmExecutor for ImmediateContentExecutor {
    async fn run_llm_step(
        &self,
        _input: &LlmStepInput<'_>,
        _bus: &RuntimeEventBus,
        _cancel: &CancellationToken,
    ) -> Result<LlmStepResult, TurnError> {
        Ok(LlmStepResult::ContentComplete {
            content: "done".to_string(),
            tokens_in: 500_000,
            tokens_out: 500_000,
            cache_creation_input_tokens: 0,
            cache_read_input_tokens: 0,
            thinking_blocks: Vec::new(),
            stop_reason: Some("end_turn".to_string()),
        })
    }

    async fn persist_assistant_message(
        &self,
        _conversation_id: &str,
        _content: &str,
        _tool_calls: &[serde_json::Value],
        _generated_file_ids: &[String],
        _file_metas: &[serde_json::Value],
        _thinking_blocks: &[serde_json::Value],
        _error: Option<&app_lib::storage::file_store::types::MessageError>,
    ) -> Result<String, TurnError> {
        Ok("msg-test".to_string())
    }

    async fn get_tool_defs(&self) -> Result<Vec<serde_json::Value>, TurnError> {
        Ok(vec![]) // 显式声明此 mock 不关心 tool_defs
    }
}

#[tokio::test]
async fn o5_budget_exceeded_emits_turn_completed_event() {
    let dispatcher = Arc::new(ToolDispatcher::new(Arc::new(AllowAllPermissionPipeline)));
    let engine = QueryEngine::for_test(dispatcher)
        .with_max_budget_usd(0.50)
        .with_cost_per_1k_tokens(0.001);

    let bus = RuntimeEventBus::new();
    let executor = Arc::new(ImmediateContentExecutor);
    let driver = RuntimeChatTurnDriver::with_llm_executor(engine, bus.clone(), executor);

    let request = ChatTurnRequest::new("sess-o5", "hello", vec![]);
    let mapping = IdentityMapping::from_legacy_conversation_id("sess-o5");
    let mut turn = TurnState::new(
        mapping,
        RunId::new(request.run_id.as_str()),
        "hello".to_string(),
    );

    driver
        .run_chat_turn(&mut turn, &request)
        .await
        .expect("run_chat_turn should not Err");

    let event = bus
        .recorded()
        .into_iter()
        .find(|event| matches!(event.kind, RuntimeEventKind::TurnCompleted { .. }))
        .expect("turn should emit TurnCompleted");

    match event.kind {
        RuntimeEventKind::TurnCompleted {
            outcome,
            total_input_tokens,
            total_output_tokens,
            total_cost_usd,
            ..
        } => {
            assert!(matches!(
                outcome,
                app_lib::runtime::chat::ChatTurnOutcome::BudgetExceeded { .. }
            ));
            assert_eq!(total_input_tokens, 500_000);
            assert_eq!(total_output_tokens, 500_000);
            assert_eq!(total_cost_usd, Some(1.0));
        }
        other => panic!("expected TurnCompleted, got {:?}", other),
    }
}
