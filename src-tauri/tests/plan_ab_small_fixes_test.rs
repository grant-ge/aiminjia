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

struct CoreMemoryCapturingExecutor {
    responses: Mutex<Vec<LlmStepResult>>,
    core_memory: String,
    load_core_memory_calls: Mutex<u32>,
    captured_dynamic_contexts: Mutex<Vec<String>>,
}

impl CoreMemoryCapturingExecutor {
    fn new(core_memory: impl Into<String>, responses: Vec<LlmStepResult>) -> Self {
        Self {
            responses: Mutex::new(responses),
            core_memory: core_memory.into(),
            load_core_memory_calls: Mutex::new(0),
            captured_dynamic_contexts: Mutex::new(Vec::new()),
        }
    }
}

#[async_trait]
impl RuntimeLlmExecutor for CoreMemoryCapturingExecutor {
    async fn run_llm_step(
        &self,
        input: &LlmStepInput<'_>,
        _bus: &RuntimeEventBus,
        _cancel: &CancellationToken,
    ) -> Result<LlmStepResult, TurnError> {
        self.captured_dynamic_contexts
            .lock()
            .unwrap()
            .push(input.dynamic_context.to_string());
        let mut responses = self.responses.lock().unwrap();
        if responses.is_empty() {
            Ok(LlmStepResult::ContentComplete {
                content: "ok".to_string(),
                tokens_in: 0,
                tokens_out: 0,
                stop_reason: Some("end_turn".to_string()),
            })
        } else {
            Ok(responses.remove(0))
        }
    }

    async fn load_core_memory(&self, _conversation_id: &str) -> Result<String, TurnError> {
        *self.load_core_memory_calls.lock().unwrap() += 1;
        Ok(self.core_memory.clone())
    }

    async fn persist_assistant_message(
        &self,
        _conversation_id: &str,
        _content: &str,
        _tool_calls: &[serde_json::Value],
        _generated_file_ids: &[String],
        _file_metas: &[serde_json::Value],
    ) -> Result<String, TurnError> {
        Ok("mock-id".to_string())
    }
}

#[tokio::test]
async fn ab1_core_memory_appears_in_dynamic_context() {
    let executor = Arc::new(CoreMemoryCapturingExecutor::new(
        "test_core_memory_content",
        vec![LlmStepResult::ContentComplete {
            content: "done".to_string(),
            tokens_in: 0,
            tokens_out: 0,
            stop_reason: Some("end_turn".to_string()),
        }],
    ));
    let bus = RuntimeEventBus::new();
    let qe = QueryEngine::default();
    let driver = RuntimeChatTurnDriver::with_llm_executor(qe, bus, executor.clone());
    let mut turn = make_test_turn("conv-core-memory");
    let request = ChatTurnRequest::new("conv-core-memory", "hello", vec![]);

    driver.run_chat_turn(&mut turn, &request).await.unwrap();

    let captured = executor.captured_dynamic_contexts.lock().unwrap();
    assert!(!captured.is_empty(), "must capture dynamic_context");
    assert!(
        captured[0].contains("[核心记忆]"),
        "dynamic_context must contain core_memory label, got: {}",
        captured[0]
    );
    assert!(
        captured[0].contains("test_core_memory_content"),
        "dynamic_context must contain loaded core_memory, got: {}",
        captured[0]
    );
}

#[tokio::test]
async fn ab1_load_core_memory_called_once_per_turn() {
    let executor = Arc::new(CoreMemoryCapturingExecutor::new(
        "test_core_memory_content",
        vec![
            LlmStepResult::ToolCalls {
                assistant_content: String::new(),
                tool_calls: vec![],
                tokens_in: 0,
                tokens_out: 0,
            },
            LlmStepResult::ContentComplete {
                content: "done".to_string(),
                tokens_in: 0,
                tokens_out: 0,
                stop_reason: Some("end_turn".to_string()),
            },
        ],
    ));
    let bus = RuntimeEventBus::new();
    let qe = QueryEngine::default();
    let driver = RuntimeChatTurnDriver::with_llm_executor(qe, bus, executor.clone());
    let mut turn = make_test_turn("conv-core-memory-once");
    let request = ChatTurnRequest::new("conv-core-memory-once", "hello", vec![]);

    driver.run_chat_turn(&mut turn, &request).await.unwrap();

    assert_eq!(
        *executor.load_core_memory_calls.lock().unwrap(),
        1,
        "load_core_memory must be called once per turn"
    );
}

#[test]
fn ab3_worker_runtime_owns_runtime_tool_round_path() {
    let worker_runtime = include_str!("../src/runtime/agent/worker_runtime.rs");
    assert!(
        worker_runtime.contains("ToolRoundDriver"),
        "worker_runtime.rs must own runtime tool rounds through ToolRoundDriver"
    );
}

#[test]
fn ab3_sub_agent_is_only_a_delegating_entrypoint() {
    let source = include_str!("../src/llm/sub_agent.rs");
    assert!(
        source.contains("SubagentWorkerRuntime"),
        "sub_agent.rs must delegate to SubagentWorkerRuntime"
    );
    assert!(
        !source.contains("tool_registry\n                .execute(")
            && !source.contains("tool_registry.execute("),
        "sub_agent.rs must not keep the old ToolRegistry::execute() loop"
    );
}
