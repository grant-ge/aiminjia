//! 验证 P0 修复：driver 不再用 DAILY_BASE_PROMPT 覆盖 executor 提供的 system_prompt，
//! executor 产出的 system prompt 真正进入 LLM 请求。

use std::path::PathBuf;
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

const SENTINEL: &str = "SENTINEL_FROM_BUILD_SYSTEM_PROMPT_xJ7K9";

struct CapturingExecutor {
    captured_system_prompt: Mutex<Option<String>>,
}

#[async_trait]
impl RuntimeLlmExecutor for CapturingExecutor {
    async fn run_llm_step(
        &self,
        input: &LlmStepInput<'_>,
        _bus: &RuntimeEventBus,
        _cancel: &CancellationToken,
    ) -> Result<LlmStepResult, TurnError> {
        *self.captured_system_prompt.lock().unwrap() = Some(input.system_prompt.to_string());
        Ok(LlmStepResult::ContentComplete {
            content: "ok".into(),
            tokens_in: 0,
            tokens_out: 0,
            cache_creation_input_tokens: 0,
            cache_read_input_tokens: 0,
            stop_reason: Some("end_turn".into()),
        })
    }

    async fn load_workspace_path(&self) -> Result<PathBuf, TurnError> {
        Ok(PathBuf::from("/tmp/test-workspace"))
    }

    async fn get_tool_defs(&self) -> Result<Vec<serde_json::Value>, TurnError> {
        Ok(vec![])
    }

    /// 关键 mock：返回一个 sentinel 字符串，用于断言其确实进入 LLM 请求。
    /// P0 修复前 driver 会把这个值替换成 DAILY_BASE_PROMPT；修复后保留。
    async fn build_system_prompt(&self, _request: &ChatTurnRequest) -> Result<String, TurnError> {
        Ok(SENTINEL.to_string())
    }

    async fn persist_assistant_message(
        &self,
        _conversation_id: &str,
        _content: &str,
        _tool_calls: &[serde_json::Value],
        _generated_file_ids: &[String],
        _file_metas: &[serde_json::Value],
    ) -> Result<String, TurnError> {
        Ok("mock-msg".into())
    }

    // load_turn_config_overrides default returns TurnConfigOverrides::default()
    // → system_prompt: None → driver uses prompt_snapshot / build_system_prompt
}

#[tokio::test]
async fn p0_fix_executor_system_prompt_reaches_llm_step_input() {
    let executor = Arc::new(CapturingExecutor {
        captured_system_prompt: Mutex::new(None),
    });

    let bus = RuntimeEventBus::new();
    let qe = QueryEngine::new();
    let driver = RuntimeChatTurnDriver::with_llm_executor(qe, bus, executor.clone());

    let mapping = IdentityMapping::from_legacy_conversation_id("conv-p0");
    let mut turn = TurnState::new(mapping, RunId::new("test-run"), "hello".into());
    let request = ChatTurnRequest::new("conv-p0", "hello", vec![]);

    driver
        .run_chat_turn(&mut turn, &request)
        .await
        .expect("turn must succeed");

    let captured = executor
        .captured_system_prompt
        .lock()
        .unwrap()
        .clone()
        .expect("system_prompt must be captured");

    assert!(
        captured.contains(SENTINEL),
        "executor's build_system_prompt sentinel must reach LlmStepInput.system_prompt; \
         got {} chars: {:?}",
        captured.len(),
        &captured.chars().take(200).collect::<String>()
    );

    // Negative assertion: ensure DAILY_BASE_PROMPT did NOT clobber it.
    // (DAILY_BASE_PROMPT ~ 100 chars; sentinel is in a real string, not just an empty default.)
    assert!(
        !captured.is_empty(),
        "captured system_prompt must not be empty"
    );
}
