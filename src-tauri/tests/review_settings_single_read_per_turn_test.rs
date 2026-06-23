use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use app_lib::models::settings::CloudGatewayMode;
use app_lib::runtime::cancellation::CancellationToken;
use app_lib::runtime::chat::{
    ChatTurnRequest, LlmStepInput, LlmStepResult, ResolvedLlmSettings, RuntimeChatTurnDriver,
    RuntimeLlmExecutor, TurnError,
};
use app_lib::runtime::event_bus::RuntimeEventBus;
use app_lib::runtime::identity::IdentityMapping;
use app_lib::runtime::ids::RunId;
use app_lib::runtime::query_engine::QueryEngine;
use app_lib::runtime::state::TurnState;
use async_trait::async_trait;

struct CountingSettingsExecutor {
    responses: Mutex<Vec<LlmStepResult>>,
    load_calls: AtomicUsize,
    seen_api_keys: Mutex<Vec<String>>,
}

impl CountingSettingsExecutor {
    fn new(responses: Vec<LlmStepResult>) -> Self {
        Self {
            responses: Mutex::new(responses),
            load_calls: AtomicUsize::new(0),
            seen_api_keys: Mutex::new(Vec::new()),
        }
    }
}

#[async_trait]
impl RuntimeLlmExecutor for CountingSettingsExecutor {
    async fn load_llm_settings(&self) -> Result<ResolvedLlmSettings, TurnError> {
        self.load_calls.fetch_add(1, Ordering::SeqCst);
        Ok(ResolvedLlmSettings {
            primary_model: "deepseek-v3".to_string(),
            primary_api_key: "pk-turn-once".to_string(),
            auto_model_routing: true,
            custom_model_endpoint: String::new(),
            custom_model_name: String::new(),
            cloud_model: String::new(),
            cloud_model_type: String::new(),
            cloud_gateway_mode: CloudGatewayMode::V2,
            thinking_type: "disabled".to_string(),
            thinking_budget_tokens: 8000,
            context_window: None,
        })
    }

    async fn run_llm_step(
        &self,
        input: &LlmStepInput<'_>,
        _bus: &RuntimeEventBus,
        _cancel: &CancellationToken,
    ) -> Result<LlmStepResult, TurnError> {
        self.seen_api_keys
            .lock()
            .unwrap()
            .push(input.llm_settings.primary_api_key.clone());

        Ok(self.responses.lock().unwrap().remove(0))
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
        Ok("msg-settings".to_string())
    }

    async fn get_tool_defs(&self) -> Result<Vec<serde_json::Value>, TurnError> {
        Ok(vec![]) // 显式声明此 mock 不关心 tool_defs
    }
}

#[tokio::test]
async fn review_driver_loads_llm_settings_once_per_turn_and_reuses_them_each_step() {
    let executor = Arc::new(CountingSettingsExecutor::new(vec![
        LlmStepResult::ToolCalls {
            assistant_content: "thinking".to_string(),
            tool_calls: vec![],
            tokens_in: 10,
            tokens_out: 5,
            cache_creation_input_tokens: 0,
            cache_read_input_tokens: 0,
            thinking_blocks: Vec::new(),
        },
        LlmStepResult::ContentComplete {
            content: "done".to_string(),
            tokens_in: 4,
            tokens_out: 2,
            cache_creation_input_tokens: 0,
            cache_read_input_tokens: 0,
            thinking_blocks: Vec::new(),
            stop_reason: Some("end_turn".to_string()),
        },
    ]));
    let driver = RuntimeChatTurnDriver::with_llm_executor(
        QueryEngine::default(),
        RuntimeEventBus::new(),
        executor.clone(),
    );
    let mut turn = TurnState::new(
        IdentityMapping::from_legacy_conversation_id("conv-settings"),
        RunId::new("run-settings"),
        "hello".to_string(),
    );
    let request = ChatTurnRequest::new("conv-settings", "hello", vec![]);

    driver
        .run_chat_turn(&mut turn, &request)
        .await
        .expect("turn should succeed");

    assert_eq!(
        executor.load_calls.load(Ordering::SeqCst),
        1,
        "LLM settings should be resolved exactly once per turn"
    );
    assert_eq!(
        executor.seen_api_keys.lock().unwrap().as_slice(),
        &["pk-turn-once".to_string(), "pk-turn-once".to_string()],
        "each step should receive the same resolved settings snapshot"
    );
}
