// src-tauri/tests/review_stream_error_terminal_events.rs
//! Architecture review: when `RuntimeLlmExecutor::run_llm_step` returns
//! `Err(TurnError::*)`, `RuntimeChatTurnDriver::run_chat_turn` MUST still
//! emit `MessagePersisted + StreamDone + AgentIdle` so the frontend chat
//! area renders an assistant bubble (instead of going white).
//!
//! Bug background (2026-05-28 客户白屏):
//!   `chat_turn_driver.rs:2071-2078` 的 `Err(err)` 分支直接 `return`，
//!   跳过 Step 6-8 → 前端 chatStore 没 assistant message → 白屏。
//!
//! See: docs/superpowers/specs/2026-05-28-streaming-error-handling-design.md

use app_lib::runtime::cancellation::CancellationToken;
use app_lib::runtime::chat::turn_config::{LlmStepInput, LlmStepResult, TurnError};
use app_lib::runtime::chat::{
    ChatTurnRequest, RuntimeChatTurnDriver, RuntimeLlmExecutor,
};
use app_lib::runtime::event_bus::RuntimeEventBus;
use app_lib::runtime::events::RuntimeEventKind;
use app_lib::runtime::identity::IdentityMapping;
use app_lib::runtime::query_engine::QueryEngine;
use app_lib::runtime::state::TurnState;
use async_trait::async_trait;
use std::sync::Arc;

/// Mock executor that always returns `Err(TurnError::LlmError(...))`,
/// simulating a chunk-timeout / network error after retries are exhausted.
struct ErrLlmExecutor {
    error_message: String,
}

#[async_trait]
impl RuntimeLlmExecutor for ErrLlmExecutor {
    async fn run_llm_step(
        &self,
        _input: &LlmStepInput<'_>,
        _bus: &RuntimeEventBus,
        _cancel: &CancellationToken,
    ) -> Result<LlmStepResult, TurnError> {
        Err(TurnError::LlmError(self.error_message.clone()))
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
        Ok("mock-error-msg-id".to_string())
    }

    async fn get_tool_defs(&self) -> Result<Vec<serde_json::Value>, TurnError> {
        Ok(vec![])
    }
}

fn make_test_turn(conversation_id: &str) -> TurnState {
    let mapping = IdentityMapping::from_legacy_conversation_id(conversation_id);
    TurnState::new(
        mapping,
        app_lib::runtime::ids::RunId::new("test-run-error"),
        "hi".to_string(),
    )
}

#[tokio::test]
async fn driver_emits_message_persisted_when_run_llm_step_errors() {
    // 模拟 chunk timeout / network error 经 MAX_STREAM_RETRIES 耗尽后,
    // run_llm_step 返回 Err(TurnError::LlmError(...)).
    let executor = Arc::new(ErrLlmExecutor {
        error_message: "Chunk timeout (90s) after 10 retries".to_string(),
    });
    let bus = RuntimeEventBus::new();
    let qe = QueryEngine::default();
    let driver = RuntimeChatTurnDriver::with_llm_executor(qe, bus.clone(), executor);

    let mut turn = make_test_turn("conv-stream-err");
    let request = ChatTurnRequest::new("conv-stream-err", "hello", vec![]);

    // Driver 当前会返回 Err（错误向上传播是合理的，但事件必须先 emit）
    let _result = driver.run_chat_turn(&mut turn, &request).await;

    let events = bus.recorded();

    // 关键不变式：不论 driver 返回 Ok 还是 Err，三件套必须已发出
    assert!(
        events.iter().any(|e| matches!(
            &e.kind,
            RuntimeEventKind::MessagePersisted { .. }
        )),
        "missing MessagePersisted on stream error — frontend will see white screen"
    );
    assert!(
        events.iter().any(|e| matches!(
            e.kind,
            RuntimeEventKind::StreamDone
        )),
        "missing StreamDone on stream error"
    );
    assert!(
        events.iter().any(|e| matches!(
            &e.kind,
            RuntimeEventKind::AgentIdle { .. }
        )),
        "missing AgentIdle on stream error — agent will appear stuck"
    );
}

#[tokio::test]
async fn message_persisted_payload_contains_error_text_on_stream_error() {
    // 错误 message 的 content.text 应包含错误文案占位（PR1 用纯字符串，
    // PR2 改为结构化 error 字段）.
    let executor = Arc::new(ErrLlmExecutor {
        error_message: "Chunk timeout (90s) after 10 retries".to_string(),
    });
    let bus = RuntimeEventBus::new();
    let qe = QueryEngine::default();
    let driver = RuntimeChatTurnDriver::with_llm_executor(qe, bus.clone(), executor);

    let mut turn = make_test_turn("conv-stream-err-text");
    let request = ChatTurnRequest::new("conv-stream-err-text", "hello", vec![]);

    let _result = driver.run_chat_turn(&mut turn, &request).await;

    let events = bus.recorded();
    // Look specifically for an *assistant* MessagePersisted — the user message
    // MessagePersisted is always emitted (before the LLM step), so we must
    // filter by role to verify that an assistant error bubble was persisted.
    let persisted = events
        .iter()
        .find_map(|e| match &e.kind {
            RuntimeEventKind::MessagePersisted { role, content, .. }
                if role == "assistant" =>
            {
                Some(content)
            }
            _ => None,
        })
        .expect("assistant MessagePersisted must be emitted on stream error");

    let text = persisted
        .get("text")
        .and_then(|v| v.as_str())
        .unwrap_or_default();

    assert!(
        !text.is_empty(),
        "MessagePersisted.content.text must NOT be empty on stream error \
         (else frontend renders empty bubble) — got: {:?}",
        persisted
    );
}

struct PromptTooLongExecutor;

#[async_trait]
impl RuntimeLlmExecutor for PromptTooLongExecutor {
    async fn run_llm_step(
        &self,
        _input: &LlmStepInput<'_>,
        _bus: &RuntimeEventBus,
        _cancel: &CancellationToken,
    ) -> Result<LlmStepResult, TurnError> {
        Err(TurnError::PromptTooLong(
            "Context too long: 250000 / 200000 tokens".to_string(),
        ))
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
        Ok("mock-ptl-msg-id".to_string())
    }

    async fn get_tool_defs(&self) -> Result<Vec<serde_json::Value>, TurnError> {
        Ok(vec![])
    }
}

#[tokio::test]
async fn driver_emits_message_persisted_on_prompt_too_long() {
    // PromptTooLong 触发 reactive compact 链路；最终 compact 也救不回来时，
    // driver 必须 emit 三件套，让前端 chat 区显示"上下文超限"占位气泡而不是白屏.
    let executor = Arc::new(PromptTooLongExecutor);
    let bus = RuntimeEventBus::new();
    let qe = QueryEngine::default();
    let driver = RuntimeChatTurnDriver::with_llm_executor(qe, bus.clone(), executor);

    let mut turn = make_test_turn("conv-prompt-too-long");
    let request = ChatTurnRequest::new("conv-prompt-too-long", "long input", vec![]);

    let _result = driver.run_chat_turn(&mut turn, &request).await;

    let events = bus.recorded();

    // PromptTooLong 场景应该既 emit StreamError（已有，区分错误类型）也 emit 三件套（PR1 新增）
    assert!(
        events.iter().any(|e| matches!(
            &e.kind,
            RuntimeEventKind::StreamError { raw_error, .. } if raw_error.as_deref() == Some("prompt_too_long")
        )),
        "PromptTooLong should still emit StreamError with raw_error=prompt_too_long"
    );
    assert!(
        events.iter().any(|e| matches!(
            &e.kind,
            RuntimeEventKind::MessagePersisted { .. }
        )),
        "PromptTooLong should emit MessagePersisted (PR1 fix)"
    );
    assert!(
        events.iter().any(|e| matches!(
            e.kind,
            RuntimeEventKind::StreamDone
        )),
        "PromptTooLong should emit StreamDone (PR1 fix)"
    );
    assert!(
        events.iter().any(|e| matches!(
            &e.kind,
            RuntimeEventKind::AgentIdle { .. }
        )),
        "PromptTooLong should emit AgentIdle (PR1 fix)"
    );
}
