// src-tauri/tests/s4_driver_loop_test.rs

use app_lib::runtime::chat::turn_config::*;

#[test]
fn turn_iteration_state_initializes_cleanly() {
    let state = TurnIterationState::new(vec![]);
    assert_eq!(state.iteration_count, 0);
    assert!(!state.stream_cancelled);
    assert!(state.full_content.is_empty());
    assert!(!state.force_no_tools);
}

use app_lib::runtime::events::{RuntimeEvent, RuntimeEventKind};
use app_lib::transport::tauri_event_adapter::map_runtime_event;

#[test]
fn stream_error_maps_to_legacy_event() {
    let event = RuntimeEvent::new(
        "test-session".into(),
        "test-run".into(),
        RuntimeEventKind::StreamError {
            error: "Connection timeout".to_string(),
            raw_error: Some("reqwest::Error".to_string()),
        },
    );
    let legacy = map_runtime_event(&event);
    assert!(legacy.is_some());
    let legacy = legacy.unwrap();
    assert_eq!(legacy.name, "streaming:error");
    assert_eq!(legacy.payload["error"], "Connection timeout");
}

// ── S4-T3: MockLlmExecutor ─────────────────────────────────────────────────

use std::sync::Arc;
use app_lib::runtime::cancellation::CancellationToken;
use app_lib::runtime::chat::RuntimeLlmExecutor;
use app_lib::runtime::event_bus::RuntimeEventBus;
use async_trait::async_trait;

struct MockLlmExecutor {
    responses: std::sync::Mutex<Vec<LlmStepResult>>,
}

impl MockLlmExecutor {
    fn new(responses: Vec<LlmStepResult>) -> Self {
        Self {
            responses: std::sync::Mutex::new(responses),
        }
    }
}

#[async_trait]
impl RuntimeLlmExecutor for MockLlmExecutor {
    async fn run_llm_step(
        &self,
        _input: &LlmStepInput<'_>,
        _bus: &RuntimeEventBus,
        _cancel: &CancellationToken,
    ) -> Result<LlmStepResult, TurnError> {
        let mut responses = self.responses.lock().unwrap();
        if responses.is_empty() {
            Ok(LlmStepResult::ContentComplete {
                content: "done".to_string(),
                tokens_in: 0,
                tokens_out: 0,
            })
        } else {
            Ok(responses.remove(0))
        }
    }

    async fn persist_assistant_message(
        &self,
        _conversation_id: &str,
        _content: &str,
        _generated_file_ids: &[String],
        _file_metas: &[serde_json::Value],
    ) -> Result<String, TurnError> {
        Ok("mock-msg-id".to_string())
    }
}

#[test]
fn mock_executor_implements_trait() {
    let executor = MockLlmExecutor::new(vec![
        LlmStepResult::ContentComplete {
            content: "hello".to_string(),
            tokens_in: 10,
            tokens_out: 5,
        },
    ]);
    let _arc: Arc<dyn RuntimeLlmExecutor> = Arc::new(executor);
    // 编译通过即为成功
}
