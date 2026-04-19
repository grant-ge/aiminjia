use std::sync::Arc;

use app_lib::runtime::cancellation::CancellationToken;
use app_lib::runtime::chat::turn_config::{
    LlmStepInput, LlmStepResult, TurnError, TurnIterationState,
};
use app_lib::runtime::chat::{ChatTurnRequest, RuntimeChatTurnDriver, RuntimeLlmExecutor};
use app_lib::runtime::event_bus::RuntimeEventBus;
use app_lib::runtime::events::RuntimeEventKind;
use app_lib::runtime::hooks::config::{HookConfig, HookEvent, HookRegistry};
use app_lib::runtime::hooks::runner::HookRunner;
use app_lib::runtime::identity::IdentityMapping;
use app_lib::runtime::ids::RunId;
use app_lib::runtime::query_engine::QueryEngine;
use app_lib::runtime::state::TurnState;
use async_trait::async_trait;

#[tokio::test]
async fn stop_hook_prevent_continuation() {
    let runner = HookRunner::new();
    let config = HookConfig {
        event: HookEvent::Stop,
        command: "echo '{\"behavior\":\"allow\",\"preventContinuation\":true,\"stopReason\":\"stop signal received\"}'"
            .to_string(),
        tool_filter: None,
        timeout_secs: Some(10),
    };
    let input =
        serde_json::json!({"stop_reason": "content_complete", "content": "Final response."});
    let result = runner.run_hook(&config, "__stop__", &input).await.unwrap();
    assert!(result.prevent_continuation);
    assert_eq!(result.stop_reason.as_deref(), Some("stop signal received"));
}

#[tokio::test]
async fn stop_hook_allow_no_prevent() {
    let runner = HookRunner::new();
    let config = HookConfig {
        event: HookEvent::Stop,
        command: "echo '{\"behavior\":\"allow\"}'".to_string(),
        tool_filter: None,
        timeout_secs: Some(10),
    };
    let input = serde_json::json!({"stop_reason": "content_complete"});
    let result = runner.run_hook(&config, "__stop__", &input).await.unwrap();
    assert!(!result.prevent_continuation);
}

#[test]
fn hook_registry_stop_hooks_filtering() {
    let mut registry = HookRegistry::new();
    registry.hooks.push(HookConfig {
        event: HookEvent::PreToolUse,
        command: "echo pre".to_string(),
        tool_filter: None,
        timeout_secs: None,
    });
    registry.hooks.push(HookConfig {
        event: HookEvent::Stop,
        command: "echo stop".to_string(),
        tool_filter: None,
        timeout_secs: None,
    });

    let stop_hooks = registry.hooks_for(HookEvent::Stop, "__stop__");
    assert_eq!(stop_hooks.len(), 1);
    assert_eq!(stop_hooks[0].command, "echo stop");
}

#[test]
fn turn_iteration_state_stop_hook_field() {
    let mut state = TurnIterationState::new(vec![]);
    assert!(!state.stop_hook_prevent_continuation);
    state.stop_hook_prevent_continuation = true;
    assert!(state.stop_hook_prevent_continuation);
}

fn make_test_turn(conversation_id: &str) -> TurnState {
    let mapping = IdentityMapping::from_legacy_conversation_id(conversation_id);
    TurnState::new(mapping, RunId::new("test-run"), "hi".to_string())
}

struct StopHookExecutor;

#[async_trait]
impl RuntimeLlmExecutor for StopHookExecutor {
    async fn run_llm_step(
        &self,
        _input: &LlmStepInput<'_>,
        _bus: &RuntimeEventBus,
        _cancel: &CancellationToken,
    ) -> Result<LlmStepResult, TurnError> {
        Ok(LlmStepResult::ContentComplete {
            content: "done".to_string(),
            tokens_in: 1,
            tokens_out: 1,
            stop_reason: Some("end_turn".to_string()),
        })
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

#[tokio::test]
async fn run_chat_turn_emits_stop_hook_event_when_prevented() {
    let bus = RuntimeEventBus::new();
    let driver = RuntimeChatTurnDriver::with_llm_executor(
        QueryEngine::default(),
        bus.clone(),
        Arc::new(StopHookExecutor),
    );

    let mut registry = HookRegistry::new();
    registry.hooks.push(HookConfig {
        event: HookEvent::Stop,
        command: "echo '{\"behavior\":\"allow\",\"preventContinuation\":true,\"stopReason\":\"stop by hook\"}'"
            .to_string(),
        tool_filter: None,
        timeout_secs: Some(10),
    });

    let mut request = ChatTurnRequest::new("conv-stop-hook", "hello", vec![]);
    request.hook_registry = Some(Arc::new(registry));
    let mut turn = make_test_turn("conv-stop-hook");

    driver.run_chat_turn(&mut turn, &request).await.unwrap();

    let events = bus.recorded();
    assert!(events.iter().any(|event| matches!(
        &event.kind,
        RuntimeEventKind::StopHookPreventedContinuation { reason }
        if reason.as_deref() == Some("stop by hook")
    )));
}
