use app_lib::runtime::cancellation::CancellationToken;
use app_lib::runtime::chat::{
    ChatTurnRequest, LlmStepInput, LlmStepResult, RuntimeChatTurnDriver, RuntimeLlmExecutor,
    TurnError,
};
use app_lib::runtime::event_bus::RuntimeEventBus;
use app_lib::runtime::identity::IdentityMapping;
use app_lib::runtime::ids::RunId;
use app_lib::runtime::query_engine::QueryEngine;
use app_lib::runtime::state::TurnState;
use async_trait::async_trait;
use std::sync::Arc;

#[test]
fn review_session_state_b2_total_usage_initially_zero() {
    let engine = QueryEngine::new();

    let usage = engine.get_total_usage();
    assert_eq!(usage.tokens_in, 0);
    assert_eq!(usage.tokens_out, 0);
}

#[test]
fn review_session_state_b2_total_usage_accumulates_single_update() {
    let engine = QueryEngine::new();

    engine.accumulate_usage(11, 7);

    let usage = engine.get_total_usage();
    assert_eq!(usage.tokens_in, 11);
    assert_eq!(usage.tokens_out, 7);
}

#[test]
fn review_session_state_b2_total_usage_accumulates_multiple_updates() {
    let engine = QueryEngine::new();

    engine.accumulate_usage(3, 5);
    engine.accumulate_usage(17, 19);
    engine.accumulate_usage(0, 2);

    let usage = engine.get_total_usage();
    assert_eq!(usage.tokens_in, 20);
    assert_eq!(usage.tokens_out, 26);
}

struct SingleStepExecutor {
    response: std::sync::Mutex<Option<LlmStepResult>>,
}

#[async_trait]
impl RuntimeLlmExecutor for SingleStepExecutor {
    async fn run_llm_step(
        &self,
        _input: &LlmStepInput<'_>,
        _bus: &RuntimeEventBus,
        _cancel: &CancellationToken,
    ) -> Result<LlmStepResult, TurnError> {
        Ok(self
            .response
            .lock()
            .unwrap()
            .take()
            .expect("single response should exist"))
    }

    async fn persist_assistant_message(
        &self,
        _conversation_id: &str,
        _content: &str,
        _tool_calls: &[serde_json::Value],
        _generated_file_ids: &[String],
        _file_metas: &[serde_json::Value],
    ) -> Result<String, TurnError> {
        Ok("msg-b2".to_string())
    }

    async fn get_tool_defs(&self) -> Result<Vec<serde_json::Value>, TurnError> {
        Ok(vec![])  // 显式声明此 mock 不关心 tool_defs
    }
}

#[tokio::test]
async fn review_session_state_b2_driver_turn_accumulates_step_tokens_into_query_engine_total() {
    let query_engine = QueryEngine::new();
    let query_engine_probe = query_engine.clone();
    let bus = RuntimeEventBus::new();
    let executor = Arc::new(SingleStepExecutor {
        response: std::sync::Mutex::new(Some(LlmStepResult::ContentComplete {
            content: "b2 done".to_string(),
            tokens_in: 13,
            tokens_out: 21,
            cache_creation_input_tokens: 0,
            cache_read_input_tokens: 0,
            stop_reason: Some("end_turn".to_string()),
        })),
    });
    let driver = RuntimeChatTurnDriver::with_llm_executor(query_engine, bus, executor);
    let mut turn = TurnState::new(
        IdentityMapping::from_legacy_conversation_id("conv-b2-driver"),
        RunId::new("run-b2-driver"),
        "hello".to_string(),
    );
    let request = ChatTurnRequest::new("conv-b2-driver", "hello", vec![]);

    driver
        .run_chat_turn(&mut turn, &request)
        .await
        .expect("driver run should succeed");

    let usage = query_engine_probe.get_total_usage();
    assert_eq!(usage.tokens_in, 13);
    assert_eq!(usage.tokens_out, 21);
}
