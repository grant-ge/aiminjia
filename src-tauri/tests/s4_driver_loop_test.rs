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

// ── S4-T13: driver_s4 core loop ──────────────────────────────────────────────

use app_lib::runtime::chat::{ChatTurnRequest, RuntimeChatTurnDriver};
use app_lib::runtime::identity::IdentityMapping;
use app_lib::runtime::state::TurnState;
use app_lib::runtime::query_engine::QueryEngine;

fn make_test_turn(conversation_id: &str) -> TurnState {
    let mapping = IdentityMapping::from_legacy_conversation_id(conversation_id);
    TurnState::new(mapping, app_lib::runtime::ids::RunId::new("test-run"), "hi".to_string())
}

#[tokio::test]
async fn driver_s4_loop_content_complete() {
    // Single ContentComplete response: driver should emit StreamStarted,
    // MessagePersisted, StreamDone, AgentIdle.
    let executor = Arc::new(MockLlmExecutor::new(vec![
        LlmStepResult::ContentComplete {
            content: "Hello world".to_string(),
            tokens_in: 10,
            tokens_out: 5,
        },
    ]));

    let bus = RuntimeEventBus::new();
    let qe = QueryEngine::default();
    let driver = RuntimeChatTurnDriver::with_llm_executor(qe, bus.clone(), executor);

    let mut turn = make_test_turn("conv-content-complete");
    let request = ChatTurnRequest::new("conv-content-complete", "hi", vec![]);

    let result = driver.run_chat_turn(&mut turn, &request).await;
    assert!(result.is_ok(), "run_chat_turn returned error: {:?}", result);

    let events = bus.recorded();
    assert!(
        events.iter().any(|e| matches!(e.kind, app_lib::runtime::events::RuntimeEventKind::StreamStarted)),
        "missing StreamStarted"
    );
    assert!(
        events.iter().any(|e| matches!(e.kind, app_lib::runtime::events::RuntimeEventKind::StreamDone)),
        "missing StreamDone"
    );
    assert!(
        events.iter().any(|e| matches!(&e.kind, app_lib::runtime::events::RuntimeEventKind::MessagePersisted { .. })),
        "missing MessagePersisted"
    );
    assert!(
        events.iter().any(|e| matches!(&e.kind, app_lib::runtime::events::RuntimeEventKind::AgentIdle { .. })),
        "missing AgentIdle"
    );
}

#[tokio::test]
async fn driver_s4_loop_cancelled() {
    // Cancelled result: driver should still emit StreamStarted and StreamDone.
    let executor = Arc::new(MockLlmExecutor::new(vec![
        LlmStepResult::Cancelled,
    ]));

    let bus = RuntimeEventBus::new();
    let qe = QueryEngine::default();
    let driver = RuntimeChatTurnDriver::with_llm_executor(qe, bus.clone(), executor);

    let mut turn = make_test_turn("conv-cancelled");
    let request = ChatTurnRequest::new("conv-cancelled", "cancel me", vec![]);

    let result = driver.run_chat_turn(&mut turn, &request).await;
    assert!(result.is_ok(), "run_chat_turn returned error: {:?}", result);

    let events = bus.recorded();
    assert!(
        events.iter().any(|e| matches!(e.kind, app_lib::runtime::events::RuntimeEventKind::StreamStarted)),
        "missing StreamStarted on cancel"
    );
    assert!(
        events.iter().any(|e| matches!(e.kind, app_lib::runtime::events::RuntimeEventKind::StreamDone)),
        "missing StreamDone on cancel"
    );
}

#[tokio::test]
async fn driver_s4_loop_tool_calls_then_content() {
    // First iteration: ToolCalls (no real tools registered, round returns empty).
    // Second iteration: ContentComplete.
    // This tests the multi-iteration path.
    let executor = Arc::new(MockLlmExecutor::new(vec![
        LlmStepResult::ToolCalls {
            assistant_content: "Let me check that.".to_string(),
            tool_calls: vec![],  // empty: no real dispatcher needed
            tokens_in: 20,
            tokens_out: 10,
        },
        LlmStepResult::ContentComplete {
            content: "Done.".to_string(),
            tokens_in: 5,
            tokens_out: 3,
        },
    ]));

    let bus = RuntimeEventBus::new();
    let qe = QueryEngine::default();
    let driver = RuntimeChatTurnDriver::with_llm_executor(qe, bus.clone(), executor);

    let mut turn = make_test_turn("conv-tool-then-content");
    let request = ChatTurnRequest::new("conv-tool-then-content", "do something", vec![]);

    let result = driver.run_chat_turn(&mut turn, &request).await;
    assert!(result.is_ok(), "run_chat_turn returned error: {:?}", result);

    let events = bus.recorded();
    assert!(
        events.iter().any(|e| matches!(e.kind, app_lib::runtime::events::RuntimeEventKind::StreamDone)),
        "missing StreamDone in tool-then-content path"
    );
    assert!(
        events.iter().any(|e| matches!(&e.kind, app_lib::runtime::events::RuntimeEventKind::MessagePersisted { .. })),
        "missing MessagePersisted in tool-then-content path"
    );
}

#[tokio::test]
async fn driver_s4_message_persisted_carries_content() {
    // Verify the MessagePersisted event contains the LLM's text.
    let executor = Arc::new(MockLlmExecutor::new(vec![
        LlmStepResult::ContentComplete {
            content: "The answer is 42.".to_string(),
            tokens_in: 8,
            tokens_out: 4,
        },
    ]));

    let bus = RuntimeEventBus::new();
    let qe = QueryEngine::default();
    let driver = RuntimeChatTurnDriver::with_llm_executor(qe, bus.clone(), executor);

    let mut turn = make_test_turn("conv-content-check");
    let request = ChatTurnRequest::new("conv-content-check", "what is the answer?", vec![]);

    driver.run_chat_turn(&mut turn, &request).await.unwrap();

    let events = bus.recorded();
    let persisted = events.iter().find(|e| {
        matches!(&e.kind, app_lib::runtime::events::RuntimeEventKind::MessagePersisted { .. })
    });
    assert!(persisted.is_some(), "no MessagePersisted event");
    if let app_lib::runtime::events::RuntimeEventKind::MessagePersisted { content, role, .. } =
        &persisted.unwrap().kind
    {
        assert_eq!(role, "assistant");
        // content must be a MessageContent object {"text": "..."} — not a raw string.
        // The frontend Message type requires content: MessageContent, so we always
        // emit {"text": full_content} to match the legacy finish_agent path.
        let text = content.get("text").and_then(|v| v.as_str()).unwrap_or("");
        assert_eq!(text, "The answer is 42.");
    }
}

// ============================================================================
// S4 架构约束测试：确保编排层隔离 claude-code-best 对齐
// ============================================================================

#[test]
fn review_s4_no_plugin_context_in_driver() {
    let driver_src = std::fs::read_to_string("src/runtime/chat/chat_turn_driver.rs")
        .expect("read chat_turn_driver.rs");
    assert!(
        !driver_src.contains("PluginContext"),
        "chat_turn_driver.rs must not reference PluginContext (编排层不持有工具层对象)"
    );
}

#[test]
fn review_s4_no_app_emit_in_runtime_chat() {
    let driver_src = std::fs::read_to_string("src/runtime/chat/chat_turn_driver.rs")
        .expect("read chat_turn_driver.rs");
    assert!(
        !driver_src.contains("app.emit("),
        "chat_turn_driver.rs must not directly call app.emit() (事件必须走 RuntimeEventBus)"
    );
}

#[test]
fn review_s4_runtime_has_no_tauri_use() {
    // Sanity check: ensure runtime/ modules do not import tauri::*
    for entry in walk_rust_files("src/runtime/") {
        let content = std::fs::read_to_string(&entry).unwrap_or_default();
        assert!(
            !content.contains("use tauri::"),
            "{} should not `use tauri::*` (runtime/ must be transport-neutral)",
            entry.display()
        );
    }
}

fn walk_rust_files(dir: &str) -> Vec<std::path::PathBuf> {
    let mut out = Vec::new();
    fn recurse(path: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
        if let Ok(entries) = std::fs::read_dir(path) {
            for entry in entries.flatten() {
                let p = entry.path();
                if p.is_dir() {
                    recurse(&p, out);
                } else if p.extension().map(|e| e == "rs").unwrap_or(false) {
                    out.push(p);
                }
            }
        }
    }
    recurse(std::path::Path::new(dir), &mut out);
    out
}

// ── S4-T2: system-reminder 日期注入测试 ──────────────────────────────────────

struct RecordingMockExecutor {
    responses: std::sync::Mutex<Vec<LlmStepResult>>,
    received_messages: std::sync::Mutex<Vec<Vec<serde_json::Value>>>,
}

impl RecordingMockExecutor {
    fn new(responses: Vec<LlmStepResult>) -> Self {
        Self {
            responses: std::sync::Mutex::new(responses),
            received_messages: std::sync::Mutex::new(Vec::new()),
        }
    }

    fn all_messages(&self) -> Vec<Vec<serde_json::Value>> {
        self.received_messages.lock().unwrap().clone()
    }
}

#[async_trait]
impl RuntimeLlmExecutor for RecordingMockExecutor {
    async fn run_llm_step(
        &self,
        input: &LlmStepInput<'_>,
        _bus: &RuntimeEventBus,
        _cancel: &CancellationToken,
    ) -> Result<LlmStepResult, TurnError> {
        self.received_messages.lock().unwrap().push(input.messages.clone());
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

#[tokio::test]
async fn driver_s4_injects_system_reminder_as_first_user_message() {
    let executor = Arc::new(RecordingMockExecutor::new(vec![
        LlmStepResult::ContentComplete {
            content: "ok".to_string(),
            tokens_in: 0,
            tokens_out: 0,
        },
    ]));

    let bus = RuntimeEventBus::new();
    let qe = QueryEngine::default();
    let driver = RuntimeChatTurnDriver::with_llm_executor(qe, bus.clone(), executor.clone());
    let mut turn = make_test_turn("conv-reminder");
    let request = ChatTurnRequest::new("conv-reminder", "hello", vec![]);

    driver.run_chat_turn(&mut turn, &request).await.unwrap();

    let all_messages = executor.all_messages();
    assert!(!all_messages.is_empty(), "executor must have received messages");
    let first_call_messages = &all_messages[0];

    let first_msg = &first_call_messages[0];
    assert_eq!(first_msg["role"], "user", "first message must be user role");
    let content = first_msg["content"].as_str().unwrap_or("");
    assert!(content.contains("<system-reminder>"),
        "first user message must contain <system-reminder> tag, got: {}", content);
    assert!(content.contains("今天是"),
        "system-reminder must contain date info, got: {}", content);
    assert!(content.contains("</system-reminder>"),
        "system-reminder must have closing tag, got: {}", content);
}

#[tokio::test]
async fn driver_s4_system_reminder_precedes_user_content_message() {
    let executor = Arc::new(RecordingMockExecutor::new(vec![
        LlmStepResult::ContentComplete {
            content: "ok".to_string(),
            tokens_in: 0,
            tokens_out: 0,
        },
    ]));

    let bus = RuntimeEventBus::new();
    let qe = QueryEngine::default();
    let driver = RuntimeChatTurnDriver::with_llm_executor(qe, bus.clone(), executor.clone());
    let mut turn = make_test_turn("conv-reminder-order");
    let request = ChatTurnRequest::new("conv-reminder-order", "what is today?", vec![]);

    driver.run_chat_turn(&mut turn, &request).await.unwrap();

    let first_call_messages = &executor.all_messages()[0];
    assert!(first_call_messages.len() >= 2,
        "must have at least system-reminder + user content");

    let first = &first_call_messages[0];
    let second = &first_call_messages[1];

    assert!(first["content"].as_str().unwrap_or("").contains("<system-reminder>"),
        "index 0 must be system-reminder");
    assert_eq!(second["content"], "what is today?",
        "index 1 must be the actual user content");
}

