// src-tauri/tests/s4_driver_loop_test.rs

use app_lib::runtime::chat::turn_config::*;
use std::collections::HashSet;

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

use app_lib::runtime::cancellation::CancellationToken;
use app_lib::runtime::chat::RuntimeLlmExecutor;
use app_lib::runtime::event_bus::RuntimeEventBus;
use async_trait::async_trait;
use std::sync::Arc;

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
                stop_reason: Some("end_turn".to_string()),
            })
        } else {
            Ok(responses.remove(0))
        }
    }

    async fn persist_assistant_message(
        &self,
        _conversation_id: &str,
        _content: &str,
        _tool_calls: &[serde_json::Value],
        _generated_file_ids: &[String],
        _file_metas: &[serde_json::Value],
    ) -> Result<String, TurnError> {
        Ok("mock-msg-id".to_string())
    }
}

#[test]
fn mock_executor_implements_trait() {
    let executor = MockLlmExecutor::new(vec![LlmStepResult::ContentComplete {
        content: "hello".to_string(),
        tokens_in: 10,
        tokens_out: 5,
        stop_reason: Some("end_turn".to_string()),
    }]);
    let _arc: Arc<dyn RuntimeLlmExecutor> = Arc::new(executor);
    // 编译通过即为成功
}

// ── S4-T6: safeguard 模块 ──────────────────────────────────────────────────

use app_lib::runtime::chat::safeguard::{check_iteration, SafeguardAction};

#[test]
fn safeguard_continues_when_not_near_limit() {
    let action = check_iteration(0, 10, "some content");
    assert!(matches!(action, SafeguardAction::Continue));
}

#[test]
fn safeguard_daily_injects_when_near_limit_no_content() {
    let action = check_iteration(7, 10, "");
    assert!(matches!(
        action,
        SafeguardAction::InjectPromptAndContinue(_)
    ));
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
use app_lib::runtime::chat::tool_round_driver::ToolRoundResult;
use app_lib::runtime::chat::tool_round_types::RuntimeToolCallOutcome;

#[test]
fn collect_results_counts_success_and_error() {
    let results = vec![
        ToolRoundResult::Ok(RuntimeToolCallOutcome::Completed {
            tool_call_id: "tc1".to_string(),
            tool_name: "search".to_string(),
            content: "found it".to_string(),
            is_error: false,
            msg_id: format!("tool-{}", uuid::Uuid::new_v4()),
            file_meta: None,
            is_degraded: false,
            degradation_notice: None,
            max_result_size_chars: 8_000,
            context_modifier_message: None,
            skill_runtime_patch: None,
        }),
        ToolRoundResult::Ok(RuntimeToolCallOutcome::Completed {
            tool_call_id: "tc2".to_string(),
            tool_name: "load".to_string(),
            content: "error loading".to_string(),
            is_error: true,
            msg_id: format!("tool-{}", uuid::Uuid::new_v4()),
            file_meta: None,
            is_degraded: false,
            degradation_notice: None,
            max_result_size_chars: 8_000,
            context_modifier_message: None,
            skill_runtime_patch: None,
        }),
    ];
    let collected = collect_results(results);
    assert_eq!(collected.success_count, 1);
    assert_eq!(collected.error_count, 1);
    assert_eq!(collected.tool_result_messages.len(), 2);
}

#[test]
fn runtime_tool_call_outcome_exposes_declared_max_result_size_chars() {
    let outcome = RuntimeToolCallOutcome::Completed {
        tool_call_id: "tc1".to_string(),
        tool_name: "echo".to_string(),
        content: "ok".to_string(),
        is_error: false,
        msg_id: format!("tool-{}", uuid::Uuid::new_v4()),
        file_meta: None,
        is_degraded: false,
        degradation_notice: None,
        max_result_size_chars: 12_345,
        context_modifier_message: None,
        skill_runtime_patch: None,
    };

    assert_eq!(outcome.max_result_size_chars(), 12_345);
}

#[test]
fn collect_results_truncation_message_includes_guidance() {
    let long = "x".repeat(10_000);
    let results = vec![ToolRoundResult::Ok(RuntimeToolCallOutcome::Completed {
        tool_call_id: "tc1".to_string(),
        tool_name: "search_files".to_string(),
        content: long,
        is_error: false,
        msg_id: format!("tool-{}", uuid::Uuid::new_v4()),
        file_meta: None,
        is_degraded: false,
        degradation_notice: None,
        max_result_size_chars: 4_000,
        context_modifier_message: None,
        skill_runtime_patch: None,
    })];

    let out = collect_results(results);
    let content = out.tool_result_messages[0]["content"].as_str().unwrap();
    assert!(content.contains("Use a more specific query"));
    assert!(content.contains("[Output truncated:"));
}

#[test]
fn collect_results_uses_per_result_limit_not_global_default() {
    let content_6k = "d".repeat(6_000);
    let results = vec![ToolRoundResult::Ok(RuntimeToolCallOutcome::Completed {
        tool_call_id: "tc1".to_string(),
        tool_name: "list_directory".to_string(),
        content: content_6k,
        is_error: false,
        msg_id: format!("tool-{}", uuid::Uuid::new_v4()),
        file_meta: None,
        is_degraded: false,
        degradation_notice: None,
        max_result_size_chars: 4_000,
        context_modifier_message: None,
        skill_runtime_patch: None,
    })];

    let out = collect_results(results);
    let content = out.tool_result_messages[0]["content"].as_str().unwrap();
    assert!(content.contains("[Output truncated:"));
}

#[test]
fn collect_results_keeps_content_within_declared_limit() {
    let content_5k = "p".repeat(5_000);
    let results = vec![ToolRoundResult::Ok(RuntimeToolCallOutcome::Completed {
        tool_call_id: "tc1".to_string(),
        tool_name: "execute_python".to_string(),
        content: content_5k,
        is_error: false,
        msg_id: format!("tool-{}", uuid::Uuid::new_v4()),
        file_meta: None,
        is_degraded: false,
        degradation_notice: None,
        max_result_size_chars: 32_000,
        context_modifier_message: None,
        skill_runtime_patch: None,
    })];

    let out = collect_results(results);
    let content = out.tool_result_messages[0]["content"].as_str().unwrap();
    assert_eq!(content.len(), 5_000);
    assert!(!content.contains("[Output truncated:"));
}

// ── S4-T13: driver_s4 core loop ──────────────────────────────────────────────

use app_lib::runtime::chat::chat_turn_driver::ChatAttachmentRef;
use app_lib::runtime::chat::{ChatTurnRequest, RuntimeChatTurnDriver};
use app_lib::runtime::identity::IdentityMapping;
use app_lib::runtime::query_engine::QueryEngine;
use app_lib::runtime::state::TurnState;

fn make_test_turn(conversation_id: &str) -> TurnState {
    let mapping = IdentityMapping::from_legacy_conversation_id(conversation_id);
    TurnState::new(
        mapping,
        app_lib::runtime::ids::RunId::new("test-run"),
        "hi".to_string(),
    )
}

#[tokio::test]
async fn driver_s4_loop_content_complete() {
    // Single ContentComplete response: driver should emit StreamStarted,
    // MessagePersisted, StreamDone, AgentIdle.
    let executor = Arc::new(MockLlmExecutor::new(vec![LlmStepResult::ContentComplete {
        content: "Hello world".to_string(),
        tokens_in: 10,
        tokens_out: 5,
        stop_reason: Some("end_turn".to_string()),
    }]));

    let bus = RuntimeEventBus::new();
    let qe = QueryEngine::default();
    let driver = RuntimeChatTurnDriver::with_llm_executor(qe, bus.clone(), executor);

    let mut turn = make_test_turn("conv-content-complete");
    let request = ChatTurnRequest::new("conv-content-complete", "hi", vec![]);

    let result = driver.run_chat_turn(&mut turn, &request).await;
    assert!(result.is_ok(), "run_chat_turn returned error: {:?}", result);

    let events = bus.recorded();
    assert!(
        events.iter().any(|e| matches!(
            e.kind,
            app_lib::runtime::events::RuntimeEventKind::StreamStarted
        )),
        "missing StreamStarted"
    );
    assert!(
        events.iter().any(|e| matches!(
            e.kind,
            app_lib::runtime::events::RuntimeEventKind::StreamDone
        )),
        "missing StreamDone"
    );
    assert!(
        events.iter().any(|e| matches!(
            &e.kind,
            app_lib::runtime::events::RuntimeEventKind::MessagePersisted { .. }
        )),
        "missing MessagePersisted"
    );
    assert!(
        events.iter().any(|e| matches!(
            &e.kind,
            app_lib::runtime::events::RuntimeEventKind::AgentIdle { .. }
        )),
        "missing AgentIdle"
    );
    assert!(
        events.iter().any(|e| matches!(
            &e.kind,
            app_lib::runtime::events::RuntimeEventKind::TurnCompleted { .. }
        )),
        "missing TurnCompleted"
    );
}

#[tokio::test]
async fn driver_s4_loop_cancelled() {
    // Cancelled result: driver should still emit StreamStarted and StreamDone.
    let executor = Arc::new(MockLlmExecutor::new(vec![LlmStepResult::Cancelled]));

    let bus = RuntimeEventBus::new();
    let qe = QueryEngine::default();
    let driver = RuntimeChatTurnDriver::with_llm_executor(qe, bus.clone(), executor);

    let mut turn = make_test_turn("conv-cancelled");
    let request = ChatTurnRequest::new("conv-cancelled", "cancel me", vec![]);

    let result = driver.run_chat_turn(&mut turn, &request).await;
    assert!(result.is_ok(), "run_chat_turn returned error: {:?}", result);

    let events = bus.recorded();
    assert!(
        events.iter().any(|e| matches!(
            e.kind,
            app_lib::runtime::events::RuntimeEventKind::StreamStarted
        )),
        "missing StreamStarted on cancel"
    );
    assert!(
        events.iter().any(|e| matches!(
            e.kind,
            app_lib::runtime::events::RuntimeEventKind::StreamDone
        )),
        "missing StreamDone on cancel"
    );
    assert!(
        events.iter().any(|e| matches!(
            &e.kind,
            app_lib::runtime::events::RuntimeEventKind::TurnCompleted {
                outcome: app_lib::runtime::chat::ChatTurnOutcome::Cancelled,
                ..
            }
        )),
        "missing cancelled TurnCompleted"
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
            tool_calls: vec![], // empty: no real dispatcher needed
            tokens_in: 20,
            tokens_out: 10,
        },
        LlmStepResult::ContentComplete {
            content: "Done.".to_string(),
            tokens_in: 5,
            tokens_out: 3,
            stop_reason: Some("end_turn".to_string()),
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
        events.iter().any(|e| matches!(
            e.kind,
            app_lib::runtime::events::RuntimeEventKind::StreamDone
        )),
        "missing StreamDone in tool-then-content path"
    );
    assert!(
        events.iter().any(|e| matches!(
            &e.kind,
            app_lib::runtime::events::RuntimeEventKind::MessagePersisted { .. }
        )),
        "missing MessagePersisted in tool-then-content path"
    );
}

#[tokio::test]
async fn driver_s4_message_persisted_carries_content() {
    // Verify the MessagePersisted event contains the LLM's text.
    let executor = Arc::new(MockLlmExecutor::new(vec![LlmStepResult::ContentComplete {
        content: "The answer is 42.".to_string(),
        tokens_in: 8,
        tokens_out: 4,
        stop_reason: Some("end_turn".to_string()),
    }]));

    let bus = RuntimeEventBus::new();
    let qe = QueryEngine::default();
    let driver = RuntimeChatTurnDriver::with_llm_executor(qe, bus.clone(), executor);

    let mut turn = make_test_turn("conv-content-check");
    let request = ChatTurnRequest::new("conv-content-check", "what is the answer?", vec![]);

    driver.run_chat_turn(&mut turn, &request).await.unwrap();

    let events = bus.recorded();
    let persisted = events.iter().find(|e| {
        matches!(
            &e.kind,
            app_lib::runtime::events::RuntimeEventKind::MessagePersisted { role, .. } if role == "assistant"
        )
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
        self.received_messages
            .lock()
            .unwrap()
            .push(input.messages.clone());
        let mut responses = self.responses.lock().unwrap();
        if responses.is_empty() {
            Ok(LlmStepResult::ContentComplete {
                content: "done".to_string(),
                tokens_in: 0,
                tokens_out: 0,
                stop_reason: Some("end_turn".to_string()),
            })
        } else {
            Ok(responses.remove(0))
        }
    }

    async fn persist_assistant_message(
        &self,
        _conversation_id: &str,
        _content: &str,
        _tool_calls: &[serde_json::Value],
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
            stop_reason: Some("end_turn".to_string()),
        },
    ]));

    let bus = RuntimeEventBus::new();
    let qe = QueryEngine::default();
    let driver = RuntimeChatTurnDriver::with_llm_executor(qe, bus.clone(), executor.clone());
    let mut turn = make_test_turn("conv-reminder");
    let request = ChatTurnRequest::new("conv-reminder", "hello", vec![]);

    driver.run_chat_turn(&mut turn, &request).await.unwrap();

    let all_messages = executor.all_messages();
    assert!(
        !all_messages.is_empty(),
        "executor must have received messages"
    );
    let first_call_messages = &all_messages[0];

    let first_msg = &first_call_messages[0];
    assert_eq!(first_msg["role"], "user", "first message must be user role");
    let content = first_msg["content"].as_str().unwrap_or("");
    assert!(
        content.contains("<system-reminder>"),
        "first user message must contain <system-reminder> tag, got: {}",
        content
    );
    assert!(
        content.contains("今天是"),
        "system-reminder must contain date info, got: {}",
        content
    );
    assert!(
        content.contains("</system-reminder>"),
        "system-reminder must have closing tag, got: {}",
        content
    );
}

#[tokio::test]
async fn driver_s4_system_reminder_precedes_user_content_message() {
    let executor = Arc::new(RecordingMockExecutor::new(vec![
        LlmStepResult::ContentComplete {
            content: "ok".to_string(),
            tokens_in: 0,
            tokens_out: 0,
            stop_reason: Some("end_turn".to_string()),
        },
    ]));

    let bus = RuntimeEventBus::new();
    let qe = QueryEngine::default();
    let driver = RuntimeChatTurnDriver::with_llm_executor(qe, bus.clone(), executor.clone());
    let mut turn = make_test_turn("conv-reminder-order");
    let request = ChatTurnRequest::new("conv-reminder-order", "what is today?", vec![]);

    driver.run_chat_turn(&mut turn, &request).await.unwrap();

    let first_call_messages = &executor.all_messages()[0];
    assert!(
        first_call_messages.len() >= 2,
        "must have at least system-reminder + user content"
    );

    let first = &first_call_messages[0];
    let second = &first_call_messages[1];

    assert!(
        first["content"]
            .as_str()
            .unwrap_or("")
            .contains("<system-reminder>"),
        "index 0 must be system-reminder"
    );
    assert_eq!(
        second["content"], "what is today?",
        "index 1 must be the actual user content"
    );
}

struct EnrichedUserMessageExecutor {
    received_messages: std::sync::Mutex<Vec<Vec<serde_json::Value>>>,
}

impl EnrichedUserMessageExecutor {
    fn new() -> Self {
        Self {
            received_messages: std::sync::Mutex::new(Vec::new()),
        }
    }

    fn all_messages(&self) -> Vec<Vec<serde_json::Value>> {
        self.received_messages.lock().unwrap().clone()
    }
}

#[async_trait]
impl RuntimeLlmExecutor for EnrichedUserMessageExecutor {
    async fn run_llm_step(
        &self,
        input: &LlmStepInput<'_>,
        _bus: &RuntimeEventBus,
        _cancel: &CancellationToken,
    ) -> Result<LlmStepResult, TurnError> {
        self.received_messages
            .lock()
            .unwrap()
            .push(input.messages.clone());
        Ok(LlmStepResult::ContentComplete {
            content: "ok".to_string(),
            tokens_in: 0,
            tokens_out: 0,
            stop_reason: Some("end_turn".to_string()),
        })
    }

    async fn build_user_message_content(
        &self,
        _conversation_id: &str,
        content: &str,
        attachments: &[ChatAttachmentRef],
    ) -> Result<String, TurnError> {
        assert_eq!(attachments.len(), 1);
        assert_eq!(attachments[0].file_path, "/tmp/demo.csv");
        Ok(format!(
            "{}\n\n[当前消息附件]\n- demo.csv (path: \"/tmp/demo.csv\", 类型: csv)",
            content
        ))
    }

    async fn persist_assistant_message(
        &self,
        _conversation_id: &str,
        _content: &str,
        _tool_calls: &[serde_json::Value],
        _generated_file_ids: &[String],
        _file_metas: &[serde_json::Value],
    ) -> Result<String, TurnError> {
        Ok("mock-msg-id".to_string())
    }
}

#[tokio::test]
async fn driver_s4_uses_enriched_user_message_content_for_uploaded_files() {
    let executor = Arc::new(EnrichedUserMessageExecutor::new());
    let bus = RuntimeEventBus::new();
    let qe = QueryEngine::default();
    let driver = RuntimeChatTurnDriver::with_llm_executor(qe, bus.clone(), executor.clone());
    let mut turn = make_test_turn("conv-upload");
    let request = ChatTurnRequest::new("conv-upload", "请分析这个文件", vec![ChatAttachmentRef {
        id: "attachment-1".to_string(),
        file_name: "demo.csv".to_string(),
        file_path: "/tmp/demo.csv".to_string(),
        kind: "file".to_string(),
        file_size: 0,
        file_type: "csv".to_string(),
        mime_type: Some("text/csv".to_string()),
    }]);

    driver.run_chat_turn(&mut turn, &request).await.unwrap();

    let first_call_messages = &executor.all_messages()[0];
    let last = first_call_messages.last().unwrap();
    let content = last["content"].as_str().unwrap_or("");
    assert_eq!(last["role"], "user");
    assert!(
        content.contains("[当前消息附件]"),
        "user content sent to LLM must include attachment hints, got: {}",
        content
    );
    assert!(
        content.contains("/tmp/demo.csv"),
        "user content sent to LLM must include attachment path, got: {}",
        content
    );
}

// ── S4-T3-Task3: 统一 system prompt 传递测试 ───────────────────────────────

struct CapturingMockExecutor {
    responses: std::sync::Mutex<Vec<LlmStepResult>>,
    received_system_prompts: std::sync::Mutex<Vec<String>>,
}

impl CapturingMockExecutor {
    fn new() -> Self {
        Self {
            responses: std::sync::Mutex::new(vec![LlmStepResult::ContentComplete {
                content: "ok".to_string(),
                tokens_in: 0,
                tokens_out: 0,
                stop_reason: Some("end_turn".to_string()),
            }]),
            received_system_prompts: std::sync::Mutex::new(Vec::new()),
        }
    }
}

#[async_trait]
impl RuntimeLlmExecutor for CapturingMockExecutor {
    async fn run_llm_step(
        &self,
        input: &LlmStepInput<'_>,
        _bus: &RuntimeEventBus,
        _cancel: &CancellationToken,
    ) -> Result<LlmStepResult, TurnError> {
        self.received_system_prompts
            .lock()
            .unwrap()
            .push(input.system_prompt.to_string());
        let mut responses = self.responses.lock().unwrap();
        if responses.is_empty() {
            Ok(LlmStepResult::ContentComplete {
                content: "done".to_string(),
                tokens_in: 0,
                tokens_out: 0,
                stop_reason: Some("end_turn".to_string()),
            })
        } else {
            Ok(responses.remove(0))
        }
    }

    async fn build_system_prompt(&self, _conversation_id: &str) -> Result<String, TurnError> {
        Ok("[UNIFIED-SYSTEM-PROMPT]".to_string())
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
async fn driver_s4_uses_unified_system_prompt() {
    let executor = Arc::new(CapturingMockExecutor::new());
    let bus = RuntimeEventBus::new();
    let qe = QueryEngine::default();
    let driver = RuntimeChatTurnDriver::with_llm_executor(qe, bus.clone(), executor.clone());
    let mut turn = make_test_turn("conv-analysis");
    let request = ChatTurnRequest::new("conv-analysis", "analyze data", vec![]);

    driver.run_chat_turn(&mut turn, &request).await.unwrap();

    let prompts = executor.received_system_prompts.lock().unwrap();
    assert!(!prompts.is_empty());
    assert_eq!(
        prompts[0], "[UNIFIED-SYSTEM-PROMPT]",
        "driver must use unified system prompt, got: {}",
        prompts[0]
    );
}

// ── S4-T4: tool_defs 精确传递测试 ─────────────────────────────────────────────

struct ToolDefsCapturingExecutor {
    captured_tool_defs: std::sync::Mutex<Vec<Vec<serde_json::Value>>>,
}

impl ToolDefsCapturingExecutor {
    fn new() -> Self {
        Self {
            captured_tool_defs: std::sync::Mutex::new(Vec::new()),
        }
    }
}

#[async_trait]
impl RuntimeLlmExecutor for ToolDefsCapturingExecutor {
    async fn run_llm_step(
        &self,
        input: &LlmStepInput<'_>,
        _bus: &RuntimeEventBus,
        _cancel: &CancellationToken,
    ) -> Result<LlmStepResult, TurnError> {
        self.captured_tool_defs
            .lock()
            .unwrap()
            .push(input.tool_defs.to_vec());
        Ok(LlmStepResult::ContentComplete {
            content: "ok".to_string(),
            tokens_in: 0,
            tokens_out: 0,
            stop_reason: Some("end_turn".to_string()),
        })
    }

    async fn get_tool_defs(&self) -> Result<Vec<serde_json::Value>, TurnError> {
        use app_lib::runtime::tools::catalog::DAILY_ALLOWED_TOOLS;
        let names: Vec<String> = DAILY_ALLOWED_TOOLS.iter().map(|s| s.to_string()).collect();
        Ok(names
            .iter()
            .map(|n| serde_json::json!({"name": n, "description": ""}))
            .collect())
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
async fn driver_s4_tool_defs_non_empty_in_daily_mode() {
    let executor = Arc::new(ToolDefsCapturingExecutor::new());
    let bus = RuntimeEventBus::new();
    let qe = QueryEngine::default();
    let driver = RuntimeChatTurnDriver::with_llm_executor(qe, bus.clone(), executor.clone());
    let mut turn = make_test_turn("conv-tool-defs-daily");
    let request = ChatTurnRequest::new("conv-tool-defs-daily", "hello", vec![]);

    driver.run_chat_turn(&mut turn, &request).await.unwrap();

    let captured = executor.captured_tool_defs.lock().unwrap();
    assert!(!captured.is_empty(), "must have captured tool defs");
    assert!(
        !captured[0].is_empty(),
        "tool_defs must be non-empty for daily mode (was vec![] before fix)"
    );
}

#[tokio::test]
async fn driver_s4_daily_tool_defs_match_whitelist() {
    use app_lib::runtime::tools::catalog::DAILY_ALLOWED_TOOLS;
    let executor = Arc::new(ToolDefsCapturingExecutor::new());
    let bus = RuntimeEventBus::new();
    let qe = QueryEngine::default();
    let driver = RuntimeChatTurnDriver::with_llm_executor(qe, bus.clone(), executor.clone());
    let mut turn = make_test_turn("conv-tool-defs-whitelist");
    let request = ChatTurnRequest::new("conv-tool-defs-whitelist", "hello", vec![]);

    driver.run_chat_turn(&mut turn, &request).await.unwrap();

    let captured = executor.captured_tool_defs.lock().unwrap();
    let received_names: std::collections::HashSet<String> = captured[0]
        .iter()
        .filter_map(|v| v["name"].as_str())
        .map(|s| s.to_string())
        .collect();

    let expected_names: std::collections::HashSet<String> =
        DAILY_ALLOWED_TOOLS.iter().map(|s| s.to_string()).collect();
    assert_eq!(
        received_names, expected_names,
        "daily tool_defs must exactly match whitelist"
    );
    for allowed in DAILY_ALLOWED_TOOLS {
        assert!(
            received_names.contains(*allowed),
            "daily whitelist tool '{}' must be in tool_defs",
            allowed
        );
    }
}

struct TurnConfigOverrideExecutor {
    captured_system_prompts: std::sync::Mutex<Vec<String>>,
    captured_tool_defs: std::sync::Mutex<Vec<Vec<serde_json::Value>>>,
    captured_messages: std::sync::Mutex<Vec<Vec<serde_json::Value>>>,
    responses: std::sync::Mutex<Vec<LlmStepResult>>,
    overrides: TurnConfigOverrides,
}

impl TurnConfigOverrideExecutor {
    fn new(overrides: TurnConfigOverrides, responses: Vec<LlmStepResult>) -> Self {
        Self {
            captured_system_prompts: std::sync::Mutex::new(Vec::new()),
            captured_tool_defs: std::sync::Mutex::new(Vec::new()),
            captured_messages: std::sync::Mutex::new(Vec::new()),
            responses: std::sync::Mutex::new(responses),
            overrides,
        }
    }
}

#[async_trait]
impl RuntimeLlmExecutor for TurnConfigOverrideExecutor {
    async fn run_llm_step(
        &self,
        input: &LlmStepInput<'_>,
        _bus: &RuntimeEventBus,
        _cancel: &CancellationToken,
    ) -> Result<LlmStepResult, TurnError> {
        self.captured_system_prompts
            .lock()
            .unwrap()
            .push(input.system_prompt.to_string());
        self.captured_tool_defs
            .lock()
            .unwrap()
            .push(input.tool_defs.to_vec());
        self.captured_messages
            .lock()
            .unwrap()
            .push(input.messages.clone());

        let mut responses = self.responses.lock().unwrap();
        if responses.is_empty() {
            Ok(LlmStepResult::ContentComplete {
                content: "done".to_string(),
                tokens_in: 0,
                tokens_out: 0,
                stop_reason: Some("end_turn".to_string()),
            })
        } else {
            Ok(responses.remove(0))
        }
    }

    async fn build_system_prompt(&self, _conversation_id: &str) -> Result<String, TurnError> {
        Ok("[BASE-SYSTEM-PROMPT]".to_string())
    }

    async fn get_tool_defs(&self) -> Result<Vec<serde_json::Value>, TurnError> {
        Ok(vec![serde_json::json!({
            "name": "base_tool",
            "description": "base"
        })])
    }

    async fn load_turn_config_overrides(
        &self,
        _request: &app_lib::runtime::chat::ChatTurnRequest,
    ) -> Result<TurnConfigOverrides, TurnError> {
        Ok(self.overrides.clone())
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

    async fn persist_user_message(
        &self,
        _conversation_id: &str,
        _content: &str,
        _attachments: &[ChatAttachmentRef],
        _client_message_id: Option<&str>,
        _selected_skill_id: Option<&str>,
        _selected_skill_label: Option<&str>,
    ) -> Result<String, TurnError> {
        Ok("user-id".to_string())
    }
}

#[tokio::test]
async fn driver_s4_prefers_turn_override_system_prompt_and_tool_defs() {
    let executor = Arc::new(TurnConfigOverrideExecutor::new(
        TurnConfigOverrides {
            system_prompt: Some("[SKILL-PROMPT]".to_string()),
            tool_defs: Some(vec![serde_json::json!({
                "name": "skill_only_tool",
                "description": "skill"
            })]),
            ..TurnConfigOverrides::default()
        },
        vec![LlmStepResult::ContentComplete {
            content: "ok".to_string(),
            tokens_in: 0,
            tokens_out: 0,
            stop_reason: Some("end_turn".to_string()),
        }],
    ));
    let bus = RuntimeEventBus::new();
    let qe = QueryEngine::default();
    let driver = RuntimeChatTurnDriver::with_llm_executor(qe, bus, executor.clone());
    let mut turn = make_test_turn("conv-skill-overrides");
    let request = ChatTurnRequest::new("conv-skill-overrides", "hello", vec![]);

    driver.run_chat_turn(&mut turn, &request).await.unwrap();

    let prompts = executor.captured_system_prompts.lock().unwrap();
    assert_eq!(prompts.as_slice(), ["[SKILL-PROMPT]"]);

    let tool_defs = executor.captured_tool_defs.lock().unwrap();
    let tool_names: Vec<&str> = tool_defs[0]
        .iter()
        .filter_map(|value| value.get("name").and_then(|v| v.as_str()))
        .collect();
    assert_eq!(tool_names, vec!["skill_only_tool"]);
}

#[tokio::test]
async fn driver_s4_turn_override_allowed_tools_blocks_runtime_execution() {
    let executor = Arc::new(TurnConfigOverrideExecutor::new(
        TurnConfigOverrides {
            allowed_tools: Some(HashSet::from(["allowed_tool".to_string()])),
            ..TurnConfigOverrides::default()
        },
        vec![
            LlmStepResult::ToolCalls {
                assistant_content: String::new(),
                tool_calls: vec![app_lib::runtime::chat::RuntimeToolCallRequest {
                    tool_call_id: "tc-blocked".to_string(),
                    tool_name: "blocked_tool".to_string(),
                    args: serde_json::json!({}),
                    purpose: None,
                }],
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
    let mut turn = make_test_turn("conv-allowed-tools");
    let request = ChatTurnRequest::new("conv-allowed-tools", "hello", vec![]);

    driver.run_chat_turn(&mut turn, &request).await.unwrap();

    let captured_messages = executor.captured_messages.lock().unwrap();
    let second_iteration = captured_messages
        .get(1)
        .expect("second iteration should include blocked tool feedback");
    let blocked_feedback = second_iteration
        .iter()
        .filter_map(|value| value.get("content").and_then(|v| v.as_str()))
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        blocked_feedback.contains("blocked_tool"),
        "blocked tool feedback must mention the blocked tool, got: {}",
        blocked_feedback
    );
    assert!(
        blocked_feedback.contains("allowed_tool"),
        "blocked tool feedback must mention the allowed set, got: {}",
        blocked_feedback
    );
}

// ── S4-T5: 多轮历史加载测试 ──────────────────────────────────────────────────

struct HistoryAwareMockExecutor {
    history: Vec<serde_json::Value>,
    captured_initial_messages: std::sync::Mutex<Vec<serde_json::Value>>,
}

impl HistoryAwareMockExecutor {
    fn new(history: Vec<serde_json::Value>) -> Self {
        Self {
            history,
            captured_initial_messages: std::sync::Mutex::new(Vec::new()),
        }
    }
}

#[async_trait]
impl RuntimeLlmExecutor for HistoryAwareMockExecutor {
    async fn run_llm_step(
        &self,
        input: &LlmStepInput<'_>,
        _bus: &RuntimeEventBus,
        _cancel: &CancellationToken,
    ) -> Result<LlmStepResult, TurnError> {
        let mut captured = self.captured_initial_messages.lock().unwrap();
        if captured.is_empty() {
            *captured = input.messages.clone();
        }
        Ok(LlmStepResult::ContentComplete {
            content: "response".to_string(),
            tokens_in: 0,
            tokens_out: 0,
            stop_reason: Some("end_turn".to_string()),
        })
    }

    async fn load_history(
        &self,
        _conversation_id: &str,
    ) -> Result<Vec<serde_json::Value>, TurnError> {
        Ok(self.history.clone())
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
async fn driver_s4_loads_history_into_messages() {
    let history = vec![
        serde_json::json!({"role": "user", "content": "previous question"}),
        serde_json::json!({"role": "assistant", "content": "previous answer"}),
    ];
    let executor = Arc::new(HistoryAwareMockExecutor::new(history.clone()));
    let bus = RuntimeEventBus::new();
    let qe = QueryEngine::default();
    let driver = RuntimeChatTurnDriver::with_llm_executor(qe, bus.clone(), executor.clone());
    let mut turn = make_test_turn("conv-history");
    let request = ChatTurnRequest::new("conv-history", "current question", vec![]);

    driver.run_chat_turn(&mut turn, &request).await.unwrap();

    let captured = executor.captured_initial_messages.lock().unwrap();
    assert!(!captured.is_empty(), "must have captured messages");

    let has_prev_question = captured
        .iter()
        .any(|m| m["content"].as_str() == Some("previous question"));
    let has_prev_answer = captured
        .iter()
        .any(|m| m["content"].as_str() == Some("previous answer"));
    assert!(
        has_prev_question,
        "history: 'previous question' must be in messages"
    );
    assert!(
        has_prev_answer,
        "history: 'previous answer' must be in messages"
    );

    let has_current = captured
        .iter()
        .any(|m| m["content"].as_str() == Some("current question"));
    assert!(has_current, "current user content must be in messages");
}

#[tokio::test]
async fn driver_s4_message_order_is_reminder_history_current() {
    let history = vec![
        serde_json::json!({"role": "user", "content": "past user msg"}),
        serde_json::json!({"role": "assistant", "content": "past assistant msg"}),
    ];
    let executor = Arc::new(HistoryAwareMockExecutor::new(history));
    let bus = RuntimeEventBus::new();
    let qe = QueryEngine::default();
    let driver = RuntimeChatTurnDriver::with_llm_executor(qe, bus.clone(), executor.clone());
    let mut turn = make_test_turn("conv-order");
    let request = ChatTurnRequest::new("conv-order", "new msg", vec![]);

    driver.run_chat_turn(&mut turn, &request).await.unwrap();

    let captured = executor.captured_initial_messages.lock().unwrap();
    assert!(
        captured[0]["content"]
            .as_str()
            .unwrap_or("")
            .contains("<system-reminder>"),
        "messages[0] must be system-reminder, got: {:?}",
        captured[0]
    );
    let last = captured.last().unwrap();
    assert_eq!(
        last["content"], "new msg",
        "last message must be current user content"
    );

    let middle_contents: Vec<&str> = captured[1..captured.len() - 1]
        .iter()
        .filter_map(|m| m["content"].as_str())
        .collect();
    assert!(
        middle_contents.contains(&"past user msg"),
        "history user msg must be in middle"
    );
    assert!(
        middle_contents.contains(&"past assistant msg"),
        "history assistant msg must be in middle"
    );
}

#[tokio::test]
async fn driver_s4_empty_history_works_normally() {
    let executor = Arc::new(HistoryAwareMockExecutor::new(vec![]));
    let bus = RuntimeEventBus::new();
    let qe = QueryEngine::default();
    let driver = RuntimeChatTurnDriver::with_llm_executor(qe, bus.clone(), executor.clone());
    let mut turn = make_test_turn("conv-no-history");
    let request = ChatTurnRequest::new("conv-no-history", "first message", vec![]);

    let result = driver.run_chat_turn(&mut turn, &request).await;
    assert!(result.is_ok(), "must work without history");

    let captured = executor.captured_initial_messages.lock().unwrap();
    assert_eq!(
        captured.len(),
        2,
        "without history: messages must be [system-reminder, user-content]"
    );
}

struct FailingHistoryExecutor;

#[async_trait]
impl RuntimeLlmExecutor for FailingHistoryExecutor {
    async fn run_llm_step(
        &self,
        _input: &LlmStepInput<'_>,
        _bus: &RuntimeEventBus,
        _cancel: &CancellationToken,
    ) -> Result<LlmStepResult, TurnError> {
        panic!("run_llm_step must not be called when history loading fails");
    }

    async fn load_history(
        &self,
        _conversation_id: &str,
    ) -> Result<Vec<serde_json::Value>, TurnError> {
        Err(TurnError::PersistenceError(
            "history backend unavailable".to_string(),
        ))
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
async fn driver_s4_returns_error_when_history_loading_fails() {
    let executor = Arc::new(FailingHistoryExecutor);
    let bus = RuntimeEventBus::new();
    let qe = QueryEngine::default();
    let driver = RuntimeChatTurnDriver::with_llm_executor(qe, bus.clone(), executor);
    let mut turn = make_test_turn("conv-history-fail");
    let request = ChatTurnRequest::new("conv-history-fail", "hello", vec![]);

    let result = driver.run_chat_turn(&mut turn, &request).await;
    assert!(result.is_err(), "history loading failure must be surfaced");
    let err_text = format!("{:?}", result.err().unwrap());
    assert!(
        err_text.contains("history backend unavailable"),
        "error should mention history loading failure, got: {}",
        err_text
    );
}

struct EnvInfoCapturingExecutor {
    env_info: String,
    captured_dynamic_contexts: std::sync::Mutex<Vec<String>>,
}

impl EnvInfoCapturingExecutor {
    fn new(env_info: impl Into<String>) -> Self {
        Self {
            env_info: env_info.into(),
            captured_dynamic_contexts: std::sync::Mutex::new(Vec::new()),
        }
    }
}

#[async_trait]
impl RuntimeLlmExecutor for EnvInfoCapturingExecutor {
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
        Ok(LlmStepResult::ContentComplete {
            content: "ok".to_string(),
            tokens_in: 0,
            tokens_out: 0,
            stop_reason: Some("end_turn".to_string()),
        })
    }

    async fn get_env_info(&self, _conversation_id: &str) -> Result<String, TurnError> {
        Ok(self.env_info.clone())
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
async fn driver_s4_env_info_appears_in_dynamic_context() {
    let executor = Arc::new(EnvInfoCapturingExecutor::new(
        "\n\n[当前环境]\n工作目录: /tmp/test\nPlatform: darwin",
    ));
    let bus = RuntimeEventBus::new();
    let qe = QueryEngine::default();
    let driver = RuntimeChatTurnDriver::with_llm_executor(qe, bus.clone(), executor.clone());
    let mut turn = make_test_turn("conv-env-info");
    let request = ChatTurnRequest::new("conv-env-info", "hello", vec![]);

    driver.run_chat_turn(&mut turn, &request).await.unwrap();

    let captured = executor.captured_dynamic_contexts.lock().unwrap();
    assert!(!captured.is_empty(), "must have captured dynamic_context");
    assert!(
        captured[0].contains("[当前环境]"),
        "dynamic_context must contain env info, got: {}",
        captured[0]
    );
    assert!(
        captured[0].contains("工作目录: /tmp/test"),
        "dynamic_context must contain working dir, got: {}",
        captured[0]
    );
}

#[tokio::test]
async fn driver_s4_empty_env_info_does_not_break_context() {
    let executor = Arc::new(EnvInfoCapturingExecutor::new(""));
    let bus = RuntimeEventBus::new();
    let qe = QueryEngine::default();
    let driver = RuntimeChatTurnDriver::with_llm_executor(qe, bus.clone(), executor.clone());
    let mut turn = make_test_turn("conv-env-info-empty");
    let request = ChatTurnRequest::new("conv-env-info-empty", "hello", vec![]);

    let result = driver.run_chat_turn(&mut turn, &request).await;
    assert!(result.is_ok(), "must work when env_info is empty");
}

// ── S4-Task3: CountingEnvInfoExecutor — 次数、顺序、错误回退 ─────────────────

struct CountingEnvInfoExecutor {
    env_info: Result<String, ()>,
    get_env_info_calls: std::sync::Mutex<u32>,
    captured_dynamic_contexts: std::sync::Mutex<Vec<String>>,
}

impl CountingEnvInfoExecutor {
    fn ok(env_info: impl Into<String>) -> Self {
        Self {
            env_info: Ok(env_info.into()),
            get_env_info_calls: std::sync::Mutex::new(0),
            captured_dynamic_contexts: std::sync::Mutex::new(Vec::new()),
        }
    }

    fn err() -> Self {
        Self {
            env_info: Err(()),
            get_env_info_calls: std::sync::Mutex::new(0),
            captured_dynamic_contexts: std::sync::Mutex::new(Vec::new()),
        }
    }
}

#[async_trait]
impl RuntimeLlmExecutor for CountingEnvInfoExecutor {
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
        Ok(LlmStepResult::ContentComplete {
            content: "ok".to_string(),
            tokens_in: 0,
            tokens_out: 0,
            stop_reason: Some("end_turn".to_string()),
        })
    }

    async fn get_env_info(&self, _conversation_id: &str) -> Result<String, TurnError> {
        *self.get_env_info_calls.lock().unwrap() += 1;
        match &self.env_info {
            Ok(v) => Ok(v.clone()),
            Err(_) => Err(TurnError::LlmError("boom".to_string())),
        }
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
async fn driver_s4_env_info_precedes_precompute_result_in_dynamic_context() {
    let executor = Arc::new(CountingEnvInfoExecutor::ok(
        "\n\n[当前环境]\n工作目录: /tmp/test\nPlatform: darwin",
    ));
    let bus = RuntimeEventBus::new();
    let qe = QueryEngine::default();
    let driver = RuntimeChatTurnDriver::with_llm_executor(qe, bus.clone(), executor.clone());
    let mut turn = make_test_turn("conv-env-precompute");
    let request = ChatTurnRequest::new("conv-env-precompute", "hello", vec![]);

    driver.run_chat_turn(&mut turn, &request).await.unwrap();

    let captured = executor.captured_dynamic_contexts.lock().unwrap();
    assert!(!captured.is_empty(), "must capture dynamic_context");
    let ctx = &captured[0];
    let env_pos = ctx.find("[当前环境]").expect("missing env info");
    if let Some(pre_pos) = ctx.find("[precompute_result]") {
        assert!(env_pos < pre_pos, "env_info must precede precompute_result");
    }
}

#[tokio::test]
async fn driver_s4_get_env_info_called_once_per_turn() {
    let executor = Arc::new(CountingEnvInfoExecutor::ok(
        "\n\n[当前环境]\n工作目录: /tmp/test\nPlatform: darwin",
    ));
    let bus = RuntimeEventBus::new();
    let qe = QueryEngine::default();
    let driver = RuntimeChatTurnDriver::with_llm_executor(qe, bus.clone(), executor.clone());
    let mut turn = make_test_turn("conv-env-once");
    let request = ChatTurnRequest::new("conv-env-once", "hello", vec![]);

    driver.run_chat_turn(&mut turn, &request).await.unwrap();

    assert_eq!(
        *executor.get_env_info_calls.lock().unwrap(),
        1,
        "get_env_info must be called once per turn"
    );
}

#[tokio::test]
async fn driver_s4_get_env_info_error_falls_back_to_empty_string() {
    let executor = Arc::new(CountingEnvInfoExecutor::err());
    let bus = RuntimeEventBus::new();
    let qe = QueryEngine::default();
    let driver = RuntimeChatTurnDriver::with_llm_executor(qe, bus.clone(), executor.clone());
    let mut turn = make_test_turn("conv-env-err");
    let request = ChatTurnRequest::new("conv-env-err", "hello", vec![]);

    let result = driver.run_chat_turn(&mut turn, &request).await;
    assert!(result.is_ok(), "turn must continue when get_env_info fails");

    let captured = executor.captured_dynamic_contexts.lock().unwrap();
    assert!(!captured.is_empty(), "must capture dynamic_context");
    assert!(
        !captured[0].contains("[当前环境]"),
        "failed get_env_info should fall back to empty string, got: {}",
        captured[0]
    );
}

#[test]
fn review_chat_adapter_new_has_no_block_on() {
    // TauriChatCommandAdapter::new() is a sync function called in Tauri's setup closure,
    // which already runs inside the tokio runtime. Calling block_on() there panics with
    // "Cannot start a runtime from within a runtime".
    let content = std::fs::read_to_string("src/transport/tauri_commands/chat.rs")
        .expect("chat.rs must exist");

    // Find the new() fn body and check it has no block_on
    // Simple heuristic: extract text between "pub fn new(" and the matching "-> Self {"
    // then verify no block_on before the first "pub async fn" that follows
    let new_fn_start = content
        .find("pub fn new(")
        .expect("new() must exist in chat.rs");
    let after_new = &content[new_fn_start..];
    // The new() body ends at "Self { runtime, services }" — find next pub fn after new()
    let next_pub_fn = after_new[10..]
        .find("\n    pub ")
        .unwrap_or(after_new.len());
    let new_fn_body = &after_new[..next_pub_fn + 10];

    assert!(
        !new_fn_body.contains("block_on("),
        "TauriChatCommandAdapter::new() must not call block_on() — it runs inside \
         Tauri's tokio runtime and nested block_on panics at startup. \
         Move async initialization into send_message() or use spawn."
    );
}
