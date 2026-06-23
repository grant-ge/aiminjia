use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use app_lib::runtime::cancellation::CancellationToken;
use app_lib::runtime::chat::chat_turn_driver::inject_synthetic_tool_results_for_missing_calls;
use app_lib::runtime::chat::compact_client::CompactSummaryClient;
use app_lib::runtime::chat::compaction::{
    build_compact_boundary_record, compact_transcript_path_for_conversation_dir,
    CompactBoundaryRecord, CompactTrigger,
};
use app_lib::runtime::chat::turn_config::{
    LlmStepInput, LlmStepResult, ResolvedLlmSettings, TurnError,
};
use app_lib::runtime::chat::{ChatTurnRequest, RuntimeChatTurnDriver, RuntimeLlmExecutor};
use app_lib::runtime::event_bus::RuntimeEventBus;
use app_lib::runtime::identity::IdentityMapping;
use app_lib::runtime::ids::RunId;
use app_lib::runtime::query_engine::QueryEngine;
use app_lib::runtime::state::TurnState;
use app_lib::storage::file_store::AppStorage;
use app_lib::transport::tauri_commands::chat::build_history_from_compact_boundary;
use async_trait::async_trait;
use serde_json::{json, Value as JsonValue};
use tempfile::TempDir;

fn make_test_turn(conversation_id: &str) -> TurnState {
    let mapping = IdentityMapping::from_legacy_conversation_id(conversation_id);
    TurnState::new(mapping, RunId::new("test-run"), "hi".to_string())
}

fn has_orphan_tool_calls(messages: &[JsonValue]) -> bool {
    use std::collections::HashSet;
    let mut tool_call_ids: HashSet<String> = HashSet::new();
    let mut tool_result_ids: HashSet<String> = HashSet::new();

    for msg in messages {
        let role = msg.get("role").and_then(|v| v.as_str()).unwrap_or_default();
        if role == "assistant" {
            if let Some(tcs) = msg.get("toolCalls").and_then(|v| v.as_array()) {
                for tc in tcs {
                    if let Some(id) = tc.get("id").and_then(|v| v.as_str()) {
                        tool_call_ids.insert(id.to_string());
                    }
                }
            }
        } else if role == "tool" {
            let id = msg
                .get("toolCallId")
                .or_else(|| msg.get("tool_call_id"))
                .and_then(|v| v.as_str());
            if let Some(id) = id {
                tool_result_ids.insert(id.to_string());
            }
        }
    }

    tool_call_ids.iter().any(|id| !tool_result_ids.contains(id))
}

struct ErrorAfterHistoryExecutor {
    history: Vec<JsonValue>,
}

#[async_trait]
impl RuntimeLlmExecutor for ErrorAfterHistoryExecutor {
    async fn run_llm_step(
        &self,
        _input: &LlmStepInput<'_>,
        _bus: &RuntimeEventBus,
        _cancel: &CancellationToken,
    ) -> Result<LlmStepResult, TurnError> {
        Err(TurnError::LlmError("boom".to_string()))
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
        Ok("assistant-msg".to_string())
    }

    async fn load_history(&self, _conversation_id: &str) -> Result<Vec<JsonValue>, TurnError> {
        Ok(self.history.clone())
    }

    async fn get_tool_defs(&self) -> Result<Vec<serde_json::Value>, TurnError> {
        Ok(vec![]) // 显式声明此 mock 不关心 tool_defs
    }
}

struct CompactingExecutor {
    boundaries: Mutex<Vec<CompactBoundaryRecord>>,
    compact_messages: Mutex<Vec<JsonValue>>,
    llm_calls: Mutex<usize>,
    user_message_ids: Mutex<Vec<String>>,
    conversation_dir: PathBuf,
}

/// A CompactSummaryClient that returns a fixed summary string — used by
/// CompactingExecutor tests that need a real compaction result.
struct StaticCompactSummaryClient {
    summary: String,
}

impl StaticCompactSummaryClient {
    fn new(summary: impl Into<String>) -> Self {
        Self {
            summary: summary.into(),
        }
    }
}

#[async_trait]
impl CompactSummaryClient for StaticCompactSummaryClient {
    async fn compact_summary(
        &self,
        _conversation_id: &str,
        _messages: &[serde_json::Value],
        _llm_settings: &app_lib::runtime::chat::turn_config::ResolvedLlmSettings,
        _trace_id: Option<&str>,
        _run_id: Option<&str>,
    ) -> Result<String, TurnError> {
        Ok(self.summary.clone())
    }
}

impl CompactingExecutor {
    fn new() -> Self {
        Self {
            boundaries: Mutex::new(Vec::new()),
            compact_messages: Mutex::new(Vec::new()),
            llm_calls: Mutex::new(0),
            user_message_ids: Mutex::new(Vec::new()),
            conversation_dir: std::env::temp_dir().join("aijia-compact-transcript-path-test"),
        }
    }

    fn saved_boundaries(&self) -> Vec<CompactBoundaryRecord> {
        self.boundaries.lock().unwrap().clone()
    }

    fn saved_compact_messages(&self) -> Vec<JsonValue> {
        self.compact_messages.lock().unwrap().clone()
    }

    fn persisted_user_ids(&self) -> Vec<String> {
        self.user_message_ids.lock().unwrap().clone()
    }
}

#[async_trait]
impl RuntimeLlmExecutor for CompactingExecutor {
    async fn load_llm_settings(&self) -> Result<ResolvedLlmSettings, TurnError> {
        Ok(ResolvedLlmSettings {
            context_window: Some(128_000),
            ..ResolvedLlmSettings::default()
        })
    }

    async fn run_llm_step(
        &self,
        _input: &LlmStepInput<'_>,
        _bus: &RuntimeEventBus,
        _cancel: &CancellationToken,
    ) -> Result<LlmStepResult, TurnError> {
        let mut calls = self.llm_calls.lock().unwrap();
        *calls += 1;
        Ok(LlmStepResult::ContentComplete {
            content: "done".to_string(),
            tokens_in: 1,
            tokens_out: 1,
            cache_creation_input_tokens: 0,
            cache_read_input_tokens: 0,
            thinking_blocks: Vec::new(),
            stop_reason: Some("end_turn".to_string()),
        })
    }

    fn conversation_dir(&self, _conversation_id: &str) -> Option<PathBuf> {
        Some(self.conversation_dir.clone())
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
        Ok("assistant-msg".to_string())
    }

    async fn load_history(&self, _conversation_id: &str) -> Result<Vec<JsonValue>, TurnError> {
        let mut history = vec![json!({
            "id": "old-1",
            "role": "user",
            "content": {"text": "x".repeat(520_000)},
            "createdAt": "2026-04-19T00:00:00Z"
        })];
        history.push(json!({
            "id": "tail-user",
            "role": "user",
            "content": {"text": "latest question before compact"},
            "createdAt": "2026-04-19T00:00:01Z"
        }));
        Ok(history)
    }

    async fn persist_user_message(
        &self,
        _conversation_id: &str,
        _content: &str,
        _attachments: &[app_lib::runtime::chat::chat_turn_driver::ChatAttachmentRef],
        _skill_command: Option<&app_lib::runtime::chat::chat_turn_driver::SkillCommandRef>,
        _reasoning_mode: Option<app_lib::runtime::chat::chat_turn_driver::ReasoningMode>,
        _client_message_id: Option<&str>,
    ) -> Result<String, TurnError> {
        let id = "current-user".to_string();
        self.user_message_ids.lock().unwrap().push(id.clone());
        Ok(id)
    }

    async fn save_compact_boundary(&self, record: CompactBoundaryRecord) -> Result<(), TurnError> {
        self.boundaries.lock().unwrap().push(record);
        Ok(())
    }

    async fn persist_compact_messages(
        &self,
        _conversation_id: &str,
        messages: &[JsonValue],
    ) -> Result<(), TurnError> {
        self.compact_messages
            .lock()
            .unwrap()
            .extend(messages.iter().cloned());
        Ok(())
    }

    async fn get_tool_defs(&self) -> Result<Vec<serde_json::Value>, TurnError> {
        Ok(vec![]) // 显式声明此 mock 不关心 tool_defs
    }
}

#[tokio::test]
async fn u2_turnerror_path_injects_synthetic_results_before_returning_error() {
    let history = vec![json!({
        "role": "assistant",
        "content": "",
        "toolCalls": [
            {"id": "call_abc", "name": "Bash", "arguments": {}}
        ]
    })];
    let executor = Arc::new(ErrorAfterHistoryExecutor { history });
    let bus = RuntimeEventBus::new();
    let driver = RuntimeChatTurnDriver::with_llm_executor(QueryEngine::default(), bus, executor);

    let mut turn = make_test_turn("conv-u2");
    let request = ChatTurnRequest::new("conv-u2", "trigger", vec![]);
    let result = driver.run_chat_turn(&mut turn, &request).await;
    let err = result.expect_err("llm error should surface");
    let text = format!("{err:#}");
    assert!(text.contains("boom"));
}

#[test]
fn u2_inject_synthetic_results_repairs_orphan_tool_calls() {
    let mut messages = vec![json!({
        "role": "assistant",
        "content": "",
        "toolCalls": [
            {"id": "call_abc", "name": "Bash", "arguments": {}},
            {"id": "call_def", "name": "read_file", "arguments": {}}
        ]
    })];

    let injected = inject_synthetic_tool_results_for_missing_calls(&mut messages, None);
    assert_eq!(injected, 2);
    assert!(!has_orphan_tool_calls(&messages));
}

#[tokio::test]
async fn u3_compact_success_persists_boundary_record_with_anchor() {
    let executor = Arc::new(CompactingExecutor::new());
    let bus = RuntimeEventBus::new();
    let compact_client = Arc::new(StaticCompactSummaryClient::new(
        "压缩摘要：保留最后一个 user 问题。",
    ));
    let driver =
        RuntimeChatTurnDriver::with_llm_executor(QueryEngine::default(), bus, executor.clone())
            .with_compact_client(compact_client);

    let mut turn = make_test_turn("conv-u3");
    let request = ChatTurnRequest::new("conv-u3", "current question", vec![]);
    driver.run_chat_turn(&mut turn, &request).await.unwrap();

    let boundaries = executor.saved_boundaries();
    assert_eq!(
        boundaries.len(),
        1,
        "compact success should persist one boundary"
    );
    let boundary = &boundaries[0];
    assert_eq!(boundary.conversation_id, "conv-u3");
    assert_eq!(boundary.trigger, CompactTrigger::Auto);
    assert!(boundary.pre_tokens > boundary.post_tokens);
    assert_eq!(boundary.tail_message_id.as_deref(), Some("current-user"));
    let preserved = boundary
        .preserved_segment
        .as_ref()
        .expect("compact should record the preserved tail segment");
    assert_eq!(preserved.first_preserved_message_id, "current-user");
    assert_eq!(preserved.tail_message_id, "current-user");
    assert!(
        preserved.anchor_message_id.starts_with("compact-summary-"),
        "suffix-preserving compact should anchor the kept segment after the summary message"
    );
    assert!(boundary.summary_text.contains("压缩摘要"));
    let expected_transcript_dir = executor.conversation_dir("conv-u3").unwrap();
    let expected_transcript_path =
        compact_transcript_path_for_conversation_dir(&expected_transcript_dir);
    assert!(
        PathBuf::from(&expected_transcript_path).is_absolute(),
        "compact boundary transcript hint should be an absolute path"
    );
    assert!(
        boundary.summary_text.contains(&expected_transcript_path),
        "compact boundary summary should include the original transcript path"
    );

    let compact_messages = executor.saved_compact_messages();
    let boundary_message = compact_messages
        .iter()
        .find(|message| {
            message.get("role").and_then(|value| value.as_str()) == Some("system")
                && message.get("subtype").and_then(|value| value.as_str())
                    == Some("compact_boundary")
        })
        .expect("compact success should persist a system compact_boundary message");
    assert_eq!(
        boundary_message["compactMetadata"]["tailMessageId"],
        "current-user"
    );
    let summary_message = compact_messages
        .iter()
        .find(|message| {
            message
                .get("isCompactSummary")
                .and_then(|value| value.as_bool())
                == Some(true)
        })
        .expect("compact success should persist the compact summary message");
    assert!(
        summary_message
            .get("content")
            .and_then(|value| value.as_str())
            .unwrap_or_default()
            .contains(&expected_transcript_path),
        "compact summary message should include the original transcript path"
    );
    assert_eq!(
        boundary_message["compactMetadata"]["preservedSegment"]["headUuid"],
        "current-user"
    );
    assert_eq!(
        boundary_message["compactMetadata"]["preservedSegment"]["anchorUuid"],
        preserved.anchor_message_id
    );
    assert_eq!(
        boundary_message["compactMetadata"]["preservedSegment"]["tailUuid"],
        "current-user"
    );
    assert_eq!(
        boundary_message["compactMetadata"]["postTokens"],
        boundary.post_tokens
    );
    assert_eq!(
        boundary_message["compactMetadata"]["tokensSaved"],
        boundary.pre_tokens.saturating_sub(boundary.post_tokens)
    );
    assert!(
        compact_messages.iter().any(|message| {
            message
                .get("isCompactSummary")
                .and_then(|value| value.as_bool())
                == Some(true)
        }),
        "compact success should persist the compact summary message into the transcript"
    );
    assert_eq!(executor.persisted_user_ids(), vec!["current-user"]);
}

#[test]
fn u4_history_builder_restarts_from_boundary_anchor_and_prepends_summary() {
    let raw_messages = vec![
        json!({
            "id": "old-1",
            "role": "user",
            "content": {"text": "old question"},
            "createdAt": "2026-04-19T00:00:00Z"
        }),
        json!({
            "id": "old-2",
            "role": "assistant",
            "content": {"text": "old answer"},
            "createdAt": "2026-04-19T00:00:01Z"
        }),
        json!({
            "id": "tail-user",
            "role": "user",
            "content": {"text": "tail question"},
            "createdAt": "2026-04-19T00:00:02Z"
        }),
        json!({
            "id": "tail-assistant",
            "role": "assistant",
            "content": {"text": "tail answer"},
            "createdAt": "2026-04-19T00:00:03Z"
        }),
    ];

    let mut boundary = build_compact_boundary_record("conv-u4", CompactTrigger::Auto, 100, 20, 3);
    boundary.summary_text = "summary body".to_string();
    boundary.tail_message_id = Some("tail-user".to_string());

    let rebuilt = build_history_from_compact_boundary(raw_messages, Some(&boundary), false);
    assert_eq!(rebuilt.len(), 3);
    assert_eq!(rebuilt[0]["role"], "user");
    assert!(rebuilt[0]["content"]
        .as_str()
        .unwrap()
        .contains("summary body"));
    assert_eq!(rebuilt[1]["content"], "tail question");
    assert_eq!(rebuilt[2]["content"], "tail answer");
}

#[test]
fn u4_history_builder_without_matching_anchor_falls_back_to_recent_history() {
    let raw_messages = vec![
        json!({
            "id": "msg-1",
            "role": "user",
            "content": {"text": "question"},
            "createdAt": "2026-04-19T00:00:00Z"
        }),
        json!({
            "id": "msg-2",
            "role": "assistant",
            "content": {"text": "answer"},
            "createdAt": "2026-04-19T00:00:01Z"
        }),
    ];

    let mut boundary = build_compact_boundary_record("conv-u4b", CompactTrigger::Auto, 100, 20, 2);
    boundary.summary_text = "summary body".to_string();
    boundary.tail_message_id = Some("missing-anchor".to_string());

    let rebuilt = build_history_from_compact_boundary(raw_messages, Some(&boundary), false);
    assert_eq!(rebuilt.len(), 3);
    assert_eq!(rebuilt[1]["content"], "question");
    assert_eq!(rebuilt[2]["content"], "answer");
}

#[test]
fn u3_app_storage_persists_boundary_summary_and_anchor_fields() {
    let dir = TempDir::new().unwrap();
    let storage = AppStorage::new(dir.path()).unwrap();
    storage
        .create_conversation("conv-u3-store", "Conv")
        .unwrap();

    let mut record =
        build_compact_boundary_record("conv-u3-store", CompactTrigger::Auto, 88, 22, 5);
    record.summary_text = "persisted summary".to_string();
    record.tail_message_id = Some("tail-user".to_string());
    storage.append_compact_boundary(&record).unwrap();

    let persisted = storage.list_compact_boundaries("conv-u3-store").unwrap();
    assert_eq!(persisted.len(), 1);
    assert_eq!(persisted[0].summary_text, "persisted summary");
    assert_eq!(persisted[0].tail_message_id.as_deref(), Some("tail-user"));
}
