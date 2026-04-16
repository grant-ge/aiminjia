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

// ── S4-T6: safeguard 模块 ──────────────────────────────────────────────────

use app_lib::runtime::chat::safeguard::{check_iteration, SafeguardAction};

#[test]
fn safeguard_continues_when_not_near_limit() {
    let mut injected = false;
    // iteration=0, max=10, has_saved_note=false, force_no_tools=false
    let action = check_iteration(0, 10, "some content", false, false, &mut injected, false);
    assert!(matches!(action, SafeguardAction::Continue));
}

#[test]
fn safeguard_daily_injects_when_near_limit_no_content() {
    let mut injected = false;
    // Daily mode: iteration 7 >= max_iterations(10) - 3 = 7, empty full_content
    let action = check_iteration(7, 10, "", false, false, &mut injected, false);
    assert!(matches!(action, SafeguardAction::InjectPromptAndContinue(_)));
}

#[test]
fn safeguard_analysis_forces_no_tools_at_final_phase() {
    let mut injected = true; // phase 1 already injected
    // iteration=12, max=15 → remaining = 15-13 = 2 (<= 3), no content, has_saved_note=true
    let action = check_iteration(12, 15, "", true, true, &mut injected, false);
    assert!(matches!(action, SafeguardAction::ForceNoToolsAndContinue(_)));
}

// ── S4-T7: post_process 模块 ──────────────────────────────────────────────

use app_lib::runtime::chat::post_process;

#[test]
fn finalize_adds_max_iter_notice_when_hit_limit() {
    let mut content = "partial result".to_string();
    post_process::finalize_content(&mut content, 10, 10, false);
    // 验证追加了 max iterations 通知文本
    assert!(content.contains("partial result")); // 原内容保留
    assert!(content.len() > "partial result".len()); // 有追加
}

#[test]
fn finalize_sets_fallback_when_content_empty() {
    let mut content = String::new();
    post_process::finalize_content(&mut content, 1, 10, false);
    assert!(!content.is_empty());
}

#[test]
fn finalize_no_change_for_normal_content() {
    let mut content = "normal response".to_string();
    post_process::finalize_content(&mut content, 3, 10, false);
    assert_eq!(content, "normal response");
}

// ── S4-T8: tool_result_collector 模块 ────────────────────────────────────────

use app_lib::runtime::chat::tool_result_collector::collect_results;
use app_lib::runtime::chat::tool_round_types::RuntimeToolCallOutcome;
use app_lib::runtime::chat::tool_round_driver::ToolRoundResult;

#[test]
fn collect_results_counts_success_and_error() {
    let results = vec![
        ToolRoundResult::Ok(RuntimeToolCallOutcome::Completed {
            tool_call_id: "tc1".to_string(),
            tool_name: "search".to_string(),
            content: "found it".to_string(),
            is_error: false,
            file_meta: None,
            is_degraded: false,
            degradation_notice: None,
        }),
        ToolRoundResult::Ok(RuntimeToolCallOutcome::Completed {
            tool_call_id: "tc2".to_string(),
            tool_name: "load".to_string(),
            content: "error loading".to_string(),
            is_error: true,
            file_meta: None,
            is_degraded: false,
            degradation_notice: None,
        }),
    ];
    let collected = collect_results(results, 8000);
    assert_eq!(collected.success_count, 1);
    assert_eq!(collected.error_count, 1);
    assert_eq!(collected.tool_result_messages.len(), 2);
}
