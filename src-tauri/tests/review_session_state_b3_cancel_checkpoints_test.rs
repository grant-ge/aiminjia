use std::sync::{Arc, Mutex};

use app_lib::runtime::cancellation::CancellationToken;
use app_lib::runtime::chat::turn_config::{LlmStepInput, LlmStepResult, TurnError};
use app_lib::runtime::chat::{ChatTurnRequest, RuntimeChatTurnDriver, RuntimeLlmExecutor};
use app_lib::runtime::event_bus::RuntimeEventBus;
use app_lib::runtime::identity::IdentityMapping;
use app_lib::runtime::ids::RunId;
use app_lib::runtime::query_engine::QueryEngine;
use app_lib::runtime::state::TurnState;
use async_trait::async_trait;

fn make_test_turn(conversation_id: &str) -> TurnState {
    let mapping = IdentityMapping::from_legacy_conversation_id(conversation_id);
    TurnState::new(mapping, RunId::new("test-run"), "hi".to_string())
}

struct CountingExecutor {
    run_llm_step_call_count: Arc<Mutex<u32>>,
}

impl CountingExecutor {
    fn new(run_llm_step_call_count: Arc<Mutex<u32>>) -> Self {
        Self {
            run_llm_step_call_count,
        }
    }
}

#[async_trait]
impl RuntimeLlmExecutor for CountingExecutor {
    async fn run_llm_step(
        &self,
        _input: &LlmStepInput<'_>,
        _bus: &RuntimeEventBus,
        _cancel: &CancellationToken,
    ) -> Result<LlmStepResult, TurnError> {
        let mut count = self.run_llm_step_call_count.lock().unwrap();
        *count += 1;
        Ok(LlmStepResult::ContentComplete {
            content: "should-not-run".to_string(),
            tokens_in: 0,
            tokens_out: 0,
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
    ) -> Result<String, TurnError> {
        Ok("mock-msg-id".to_string())
    }

    async fn get_tool_defs(&self) -> Result<Vec<serde_json::Value>, TurnError> {
        Ok(vec![]) // 显式声明此 mock 不关心 tool_defs
    }
}

#[tokio::test]
async fn pre_cancelled_turn_should_not_call_run_llm_step() {
    let call_count = Arc::new(Mutex::new(0u32));
    let executor = Arc::new(CountingExecutor::new(call_count.clone()));

    let bus = RuntimeEventBus::new();
    let driver = RuntimeChatTurnDriver::with_llm_executor(QueryEngine::default(), bus, executor);

    let mut turn = make_test_turn("conv-b3-cp1");
    turn.cancellation().cancel();

    let request = ChatTurnRequest::new("conv-b3-cp1", "hi", vec![]);
    let result = driver.run_chat_turn(&mut turn, &request).await;
    assert!(result.is_ok(), "run_chat_turn returned error: {:?}", result);

    let count = *call_count.lock().unwrap();
    assert_eq!(
        count, 0,
        "run_llm_step should not be called when turn is already cancelled"
    );
}
