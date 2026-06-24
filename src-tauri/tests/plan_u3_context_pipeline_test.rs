use std::collections::HashSet;
use std::sync::{Arc, Mutex};

use app_lib::runtime::cancellation::CancellationToken;
use app_lib::runtime::chat::compact_client::CompactSummaryClient;
use app_lib::runtime::chat::compaction::{AutoCompactConfig, AutoCompactState, MicrocompactConfig};
use app_lib::runtime::chat::preprocess::{
    apply_tool_result_budget, collapse_tool_results, prepare_messages_for_llm, CollapseConfig,
    PreprocessConfig, PreprocessRetryAction, PreprocessRuntimeState, PreprocessStage,
    PreprocessTrigger, ToolResultBudgetConfig,
};
use app_lib::runtime::chat::tool_result_artifact::{
    build_persisted_tool_result_message, persist_tool_result_artifact,
};
use app_lib::runtime::chat::turn_config::{LlmStepInput, LlmStepResult, TurnError};
use app_lib::runtime::chat::{ChatTurnRequest, RuntimeChatTurnDriver, RuntimeLlmExecutor};
use app_lib::runtime::event_bus::{RuntimeEventBus, RuntimeEventSubscriber};
use app_lib::runtime::identity::IdentityMapping;
use app_lib::runtime::ids::RunId;
use app_lib::runtime::query_engine::QueryEngine;
use app_lib::runtime::state::TurnState;
use app_lib::transport::tauri_event_adapter::TauriEventAdapter;
use app_lib::transport::testing::RecordingRuntimeHost;
use async_trait::async_trait;
use serde_json::{json, Value};

fn make_test_turn(conversation_id: &str) -> TurnState {
    let mapping = IdentityMapping::from_legacy_conversation_id(conversation_id);
    TurnState::new(mapping, RunId::new("test-run"), "hi".to_string())
}

fn assistant_tool_call(id: &str, name: &str) -> Value {
    json!({
        "role": "assistant",
        "content": "",
        "toolCalls": [
            {
                "id": id,
                "name": name,
                "arguments": {}
            }
        ]
    })
}

fn tool_message(id: &str, name: &str, content: String) -> Value {
    json!({
        "role": "tool",
        "toolCallId": id,
        "name": name,
        "content": content
    })
}

fn persisted_tool_result_ref(id: &str, name: &str) -> String {
    format!(
        concat!(
            "<persisted-tool-result tool_call_id=\"{}\" tool_name=\"{}\">\n",
            "Full output saved to: /tmp/{}.txt\n",
            "Original chars: 90000\n",
            "Sha256: {}\n",
            "Note: Preview is incomplete. If omitted output matters, read the saved file before relying on this result.\n",
            "Preview:\n",
            "{}\n",
            "</persisted-tool-result>"
        ),
        id,
        name,
        id,
        "b".repeat(64),
        "preview ".repeat(80)
    )
}

fn normalize_created_at(messages: &[Value]) -> Vec<Value> {
    messages
        .iter()
        .map(|message| {
            let mut message = message.clone();
            if let Some(object) = message.as_object_mut() {
                if object.get("subtype").and_then(Value::as_str) == Some("compact_boundary") {
                    object.insert("id".to_string(), Value::String("<boundary-id>".to_string()));
                    object.insert(
                        "createdAt".to_string(),
                        Value::String("<normalized>".to_string()),
                    );
                    if let Some(metadata) = object
                        .get_mut("compactMetadata")
                        .and_then(Value::as_object_mut)
                    {
                        if metadata.contains_key("tailMessageId") {
                            metadata.insert(
                                "tailMessageId".to_string(),
                                Value::String("<message-id>".to_string()),
                            );
                        }
                        for key in ["postTokens", "tokensSaved"] {
                            if metadata.contains_key(key) {
                                metadata.insert(key.to_string(), Value::Number(0.into()));
                            }
                        }
                        if let Some(preserved) = metadata
                            .get_mut("preservedSegment")
                            .and_then(Value::as_object_mut)
                        {
                            for key in [
                                "firstPreservedMessageId",
                                "headUuid",
                                "tailMessageId",
                                "tailUuid",
                            ] {
                                if preserved.contains_key(key) {
                                    preserved.insert(
                                        key.to_string(),
                                        Value::String("<message-id>".to_string()),
                                    );
                                }
                            }
                            for key in ["anchorMessageId", "anchorUuid"] {
                                if preserved.contains_key(key) {
                                    preserved.insert(
                                        key.to_string(),
                                        Value::String("<summary-id>".to_string()),
                                    );
                                }
                            }
                        }
                    }
                } else if object.get("isCompactSummary").and_then(Value::as_bool) == Some(true) {
                    object.insert("id".to_string(), Value::String("<summary-id>".to_string()));
                } else if object.contains_key("id") {
                    object.insert("id".to_string(), Value::String("<message-id>".to_string()));
                }
            }
            message
        })
        .collect()
}

fn user_message(content: &str) -> Value {
    json!({
        "role": "user",
        "content": content
    })
}

#[test]
fn u3_budget_preserves_recent_error_and_generated_file_results() {
    let messages = vec![
        assistant_tool_call("tc-old", "read_file"),
        tool_message("tc-old", "read_file", "A".repeat(240)),
        assistant_tool_call("tc-error", "Glob"),
        json!({
            "role": "tool",
            "toolCallId": "tc-error",
            "name": "Glob",
            "content": "command failed".repeat(30),
            "isError": true
        }),
        assistant_tool_call("tc-file", "Bash"),
        tool_message(
            "tc-file",
            "Bash",
            format!(
                "report generated\nfileId: {}\n{}",
                uuid::Uuid::new_v4(),
                "B".repeat(220)
            ),
        ),
        assistant_tool_call("tc-recent", "read_file"),
        tool_message("tc-recent", "read_file", "C".repeat(200)),
    ];

    let config = ToolResultBudgetConfig {
        aggregate_char_budget: 420,
        keep_recent_tool_results: 1,
        preserved_tool_names: HashSet::new(),
        replacement_preview_chars: 48,
    };

    let result = apply_tool_result_budget(&messages, &config);
    assert!(
        result.executed,
        "budget stage should trim low-value history"
    );

    let old_content = result.messages[1]["content"].as_str().unwrap_or_default();
    assert!(
        old_content.contains("[budget-trimmed]"),
        "old low-value result should be replaced, got: {old_content}"
    );

    let error_content = result.messages[3]["content"].as_str().unwrap_or_default();
    assert!(
        error_content.contains("command failed"),
        "error tool result must stay intact"
    );

    let file_content = result.messages[5]["content"].as_str().unwrap_or_default();
    assert!(
        file_content.contains("fileId:"),
        "generated file tool result must stay intact"
    );

    let recent_content = result.messages[7]["content"].as_str().unwrap_or_default();
    assert_eq!(
        recent_content.len(),
        200,
        "most recent result must be preserved"
    );
}

#[test]
fn u3_budget_preserves_persisted_tool_result_references() {
    let artifact_ref = persisted_tool_result_ref("tc-artifact", "Bash");
    let messages = vec![
        assistant_tool_call("tc-artifact", "Bash"),
        tool_message("tc-artifact", "Bash", artifact_ref.clone()),
        assistant_tool_call("tc-plain", "Bash"),
        tool_message("tc-plain", "Bash", "A".repeat(500)),
    ];
    let config = ToolResultBudgetConfig {
        aggregate_char_budget: 700,
        keep_recent_tool_results: 0,
        preserved_tool_names: HashSet::new(),
        replacement_preview_chars: 16,
    };

    let result = apply_tool_result_budget(&messages, &config);
    assert!(result.executed);

    let artifact_content = result.messages[1]["content"].as_str().unwrap_or_default();
    assert_eq!(
        artifact_content, artifact_ref,
        "persisted refs must retain path/hash metadata under tool budget pressure"
    );

    let plain_content = result.messages[3]["content"].as_str().unwrap_or_default();
    assert!(
        plain_content.starts_with("[budget-trimmed]"),
        "ordinary old tool result should still be trimmed"
    );
}

#[test]
fn u3_collapse_preserves_persisted_tool_result_references() {
    let artifact_ref = persisted_tool_result_ref("tc-artifact", "Bash");
    let messages = vec![
        assistant_tool_call("tc-artifact", "Bash"),
        tool_message("tc-artifact", "Bash", artifact_ref.clone()),
        assistant_tool_call("tc-plain", "Bash"),
        tool_message("tc-plain", "Bash", "A".repeat(500)),
    ];
    let config = CollapseConfig {
        long_result_chars: 80,
        keep_recent_tool_results: 0,
        replacement_preview_chars: 16,
    };

    let result = collapse_tool_results(&messages, &config);
    assert!(result.executed);

    let artifact_content = result.messages[1]["content"].as_str().unwrap_or_default();
    assert_eq!(
        artifact_content, artifact_ref,
        "collapse must not hide recoverable artifact metadata"
    );

    let plain_content = result.messages[3]["content"].as_str().unwrap_or_default();
    assert!(
        plain_content.starts_with("[collapsed]"),
        "ordinary long tool result should still be collapsed"
    );
}

#[tokio::test]
async fn u3_prepare_messages_orders_budget_microcompact_collapse_before_auto_compact() {
    let messages = vec![
        user_message("first question"),
        assistant_tool_call("tc-a", "read_file"),
        tool_message("tc-a", "read_file", "A".repeat(260)),
        assistant_tool_call("tc-b", "read_file"),
        tool_message("tc-b", "read_file", "B".repeat(260)),
        assistant_tool_call("tc-c", "read_file"),
        tool_message("tc-c", "read_file", "B".repeat(260)),
        user_message("latest question"),
    ];

    let config = PreprocessConfig {
        budget: ToolResultBudgetConfig {
            aggregate_char_budget: 500,
            keep_recent_tool_results: 1,
            preserved_tool_names: HashSet::new(),
            replacement_preview_chars: 32,
        },
        microcompact: MicrocompactConfig {
            trigger_chars: 300,
            keep_recent_tool_results: 1,
            preserved_tool_names: HashSet::new(),
        },
        collapse: CollapseConfig {
            long_result_chars: 80,
            keep_recent_tool_results: 1,
            replacement_preview_chars: 40,
        },
        auto_compact: AutoCompactConfig {
            threshold_chars: 40,
            max_output_chars: 80_000,
            consecutive_failure_limit: 3,
            custom_context_window: None,
        },
        context_window: 64_000,
        query_source: None,
        context_collapse_owns_context: false,
        compact_boundary: None,
        project_instruction_content: None,
    };

    let mut compact_state = AutoCompactState::new();
    let mut preprocess_state = PreprocessRuntimeState::default();
    let prepared = prepare_messages_for_llm(
        messages,
        "conv-u3-order",
        PreprocessTrigger::Normal,
        &config,
        &mut compact_state,
        &mut preprocess_state,
        false,
        |_messages: Vec<Value>| async { Ok("summary body".to_string()) },
    )
    .await
    .expect("prepare should succeed");

    assert_eq!(
        prepared.executed_stages,
        vec![
            PreprocessStage::ToolResultBudget,
            PreprocessStage::Microcompact,
            PreprocessStage::Collapse,
            PreprocessStage::AutoCompact,
        ]
    );
    assert!(prepared.compact_boundary.is_some());
    assert_eq!(prepared.retry, PreprocessRetryAction::None);
}

#[tokio::test]
async fn u3_auto_compact_summary_receives_expanded_tool_artifact_evidence() {
    let tmp = tempfile::tempdir().unwrap();
    let tail_fact = "TOOL-ARTIFACT-TAIL-DECISION=keep-remote-logs";
    let raw_content = format!("{}{}", "x".repeat(3_000), tail_fact);
    let record = persist_tool_result_artifact(
        tmp.path(),
        "tc-artifact",
        "Bash",
        &raw_content,
        "text/plain",
    )
    .expect("persist artifact");
    let persisted_ref = build_persisted_tool_result_message(&record);
    assert!(!persisted_ref.contains(tail_fact));

    let messages = vec![
        user_message("summarize the tool evidence later"),
        assistant_tool_call("tc-artifact", "Bash"),
        tool_message("tc-artifact", "Bash", persisted_ref.clone()),
        user_message("latest question"),
    ];
    let config = PreprocessConfig {
        budget: ToolResultBudgetConfig {
            aggregate_char_budget: usize::MAX,
            keep_recent_tool_results: 10,
            preserved_tool_names: HashSet::new(),
            replacement_preview_chars: 16,
        },
        microcompact: MicrocompactConfig {
            trigger_chars: usize::MAX,
            keep_recent_tool_results: 10,
            preserved_tool_names: HashSet::new(),
        },
        collapse: CollapseConfig {
            long_result_chars: usize::MAX,
            keep_recent_tool_results: 10,
            replacement_preview_chars: 16,
        },
        auto_compact: AutoCompactConfig {
            threshold_chars: 1,
            max_output_chars: 80_000,
            consecutive_failure_limit: 3,
            custom_context_window: None,
        },
        context_window: 64_000,
        query_source: None,
        context_collapse_owns_context: false,
        compact_boundary: None,
        project_instruction_content: None,
    };

    let mut compact_state = AutoCompactState::new();
    let mut runtime_state = PreprocessRuntimeState::default();
    let prepared = prepare_messages_for_llm(
        messages,
        "conv-u3-artifact-evidence",
        PreprocessTrigger::Normal,
        &config,
        &mut compact_state,
        &mut runtime_state,
        false,
        |summary_input: Vec<Value>| async move {
            let tool_content = summary_input
                .iter()
                .find(|message| message.get("role").and_then(Value::as_str) == Some("tool"))
                .and_then(|message| message.get("content").and_then(Value::as_str))
                .unwrap_or_default();
            assert!(
                tool_content.contains(tail_fact),
                "summary input must recover full artifact evidence"
            );
            Ok(format!("summary captured {tail_fact}"))
        },
    )
    .await
    .expect("prepare should compact");

    let summary = prepared
        .messages
        .iter()
        .find(|message| message.get("isCompactSummary").and_then(Value::as_bool) == Some(true))
        .and_then(|message| message.get("content").and_then(Value::as_str))
        .unwrap_or_default();
    assert!(summary.contains(tail_fact));

    let serialized_output = serde_json::to_string(&prepared.messages).unwrap();
    assert!(
        !serialized_output.contains("<persisted-tool-result-evidence>"),
        "expanded evidence is only for the summary call, not transcript storage"
    );
}

#[tokio::test]
async fn u3_auto_compact_summary_uses_pre_budget_evidence_snapshot() {
    let small_tool_fact = "SMALL-TOOL-FACT=manual-path-excluded";
    let messages = vec![
        user_message("remember tool evidence"),
        assistant_tool_call("tc-small", "Bash"),
        tool_message(
            "tc-small",
            "Bash",
            format!("{small_tool_fact} {}", "x".repeat(400)),
        ),
        user_message("latest question"),
    ];
    let config = PreprocessConfig {
        budget: ToolResultBudgetConfig {
            aggregate_char_budget: 120,
            keep_recent_tool_results: 0,
            preserved_tool_names: HashSet::new(),
            replacement_preview_chars: 8,
        },
        microcompact: MicrocompactConfig {
            trigger_chars: usize::MAX,
            keep_recent_tool_results: 10,
            preserved_tool_names: HashSet::new(),
        },
        collapse: CollapseConfig {
            long_result_chars: usize::MAX,
            keep_recent_tool_results: 10,
            replacement_preview_chars: 16,
        },
        auto_compact: AutoCompactConfig {
            threshold_chars: 1,
            max_output_chars: 80_000,
            consecutive_failure_limit: 3,
            custom_context_window: None,
        },
        context_window: 64_000,
        query_source: None,
        context_collapse_owns_context: false,
        compact_boundary: None,
        project_instruction_content: None,
    };

    let mut compact_state = AutoCompactState::new();
    let mut runtime_state = PreprocessRuntimeState::default();
    let prepared = prepare_messages_for_llm(
        messages,
        "conv-u3-pre-budget-evidence",
        PreprocessTrigger::Normal,
        &config,
        &mut compact_state,
        &mut runtime_state,
        false,
        |summary_input: Vec<Value>| async move {
            let serialized = serde_json::to_string(&summary_input).unwrap();
            assert!(serialized.contains(small_tool_fact));
            assert!(
                !serialized.contains("[budget-trimmed]"),
                "summary input must not be based on lossy budget projection"
            );
            Ok(format!("summary captured {small_tool_fact}"))
        },
    )
    .await
    .expect("prepare should compact");

    assert!(
        prepared
            .executed_stages
            .contains(&PreprocessStage::ToolResultBudget),
        "normal request projection still applies the tool budget"
    );
    let summary = prepared
        .messages
        .iter()
        .find(|message| message.get("isCompactSummary").and_then(Value::as_bool) == Some(true))
        .and_then(|message| message.get("content").and_then(Value::as_str))
        .unwrap_or_default();
    assert!(summary.contains(small_tool_fact));
}

#[tokio::test]
async fn u3_prepare_messages_reuses_same_shape_for_normal_and_prompt_too_long() {
    let messages = vec![
        user_message("first question"),
        assistant_tool_call("tc-a", "read_file"),
        tool_message("tc-a", "read_file", "A".repeat(240)),
        assistant_tool_call("tc-b", "read_file"),
        tool_message("tc-b", "read_file", "B".repeat(240)),
        assistant_tool_call("tc-c", "read_file"),
        tool_message("tc-c", "read_file", "B".repeat(240)),
        user_message("latest question"),
    ];

    let config = PreprocessConfig {
        budget: ToolResultBudgetConfig {
            aggregate_char_budget: 460,
            keep_recent_tool_results: 1,
            preserved_tool_names: HashSet::new(),
            replacement_preview_chars: 32,
        },
        microcompact: MicrocompactConfig {
            trigger_chars: 300,
            keep_recent_tool_results: 1,
            preserved_tool_names: HashSet::new(),
        },
        collapse: CollapseConfig {
            long_result_chars: 80,
            keep_recent_tool_results: 1,
            replacement_preview_chars: 40,
        },
        auto_compact: AutoCompactConfig {
            threshold_chars: 40,
            max_output_chars: 80_000,
            consecutive_failure_limit: 3,
            custom_context_window: None,
        },
        context_window: 64_000,
        query_source: None,
        context_collapse_owns_context: false,
        compact_boundary: None,
        project_instruction_content: None,
    };

    let mut normal_compact_state = AutoCompactState::new();
    let mut normal_runtime_state = PreprocessRuntimeState::default();
    let normal = prepare_messages_for_llm(
        messages.clone(),
        "conv-u3-shape",
        PreprocessTrigger::Normal,
        &config,
        &mut normal_compact_state,
        &mut normal_runtime_state,
        false,
        |_messages: Vec<Value>| async { Ok("summary body".to_string()) },
    )
    .await
    .expect("normal prepare should succeed");

    let mut recovery_compact_state = AutoCompactState::new();
    let mut recovery_runtime_state = PreprocessRuntimeState::default();
    let recovery = prepare_messages_for_llm(
        messages,
        "conv-u3-shape",
        PreprocessTrigger::PromptTooLongRecovery,
        &config,
        &mut recovery_compact_state,
        &mut recovery_runtime_state,
        false,
        |_messages: Vec<Value>| async { Ok("summary body".to_string()) },
    )
    .await
    .expect("recovery prepare should succeed");

    assert_eq!(normal.executed_stages, recovery.executed_stages);
    assert_eq!(
        normalize_created_at(&normal.messages),
        normalize_created_at(&recovery.messages)
    );
    assert_eq!(normal.retry, PreprocessRetryAction::None);
    assert_eq!(recovery.retry, PreprocessRetryAction::RetryTurn);
}

#[tokio::test]
async fn u3_prepare_messages_is_idempotent_after_compact_output() {
    let messages = vec![
        user_message("first question"),
        assistant_tool_call("tc-a", "read_file"),
        tool_message("tc-a", "read_file", "A".repeat(260)),
        assistant_tool_call("tc-b", "read_file"),
        tool_message("tc-b", "read_file", "B".repeat(260)),
        user_message("latest question"),
    ];

    let config = PreprocessConfig {
        budget: ToolResultBudgetConfig {
            aggregate_char_budget: 380,
            keep_recent_tool_results: 1,
            preserved_tool_names: HashSet::new(),
            replacement_preview_chars: 24,
        },
        microcompact: MicrocompactConfig {
            trigger_chars: 260,
            keep_recent_tool_results: 1,
            preserved_tool_names: HashSet::new(),
        },
        collapse: CollapseConfig {
            long_result_chars: 80,
            keep_recent_tool_results: 1,
            replacement_preview_chars: 36,
        },
        auto_compact: AutoCompactConfig {
            threshold_chars: 40,
            max_output_chars: 80_000,
            consecutive_failure_limit: 3,
            custom_context_window: None,
        },
        context_window: 64_000,
        query_source: None,
        context_collapse_owns_context: false,
        compact_boundary: None,
        project_instruction_content: None,
    };

    let mut compact_state = AutoCompactState::new();
    let mut runtime_state = PreprocessRuntimeState::default();
    let first = prepare_messages_for_llm(
        messages,
        "conv-u3-idempotent",
        PreprocessTrigger::Normal,
        &config,
        &mut compact_state,
        &mut runtime_state,
        false,
        |_messages: Vec<Value>| async { Ok("summary body".to_string()) },
    )
    .await
    .expect("first prepare should succeed");

    let second = prepare_messages_for_llm(
        first.messages.clone(),
        "conv-u3-idempotent",
        PreprocessTrigger::Normal,
        &config,
        &mut compact_state,
        &mut runtime_state,
        false,
        |_messages: Vec<Value>| async { Ok("summary body".to_string()) },
    )
    .await
    .expect("second prepare should succeed");

    assert_eq!(second.messages, first.messages);
    assert!(
        second.executed_stages.is_empty(),
        "prepared messages must not be rewritten again"
    );
    assert!(second.compact_boundary.is_none());
}

#[tokio::test]
async fn u3_prepare_messages_processes_new_messages_after_compact_artifacts() {
    let messages = vec![
        json!({
            "id": "boundary-1",
            "role": "system",
            "subtype": "compact_boundary",
            "content": "Conversation compacted",
            "compactMetadata": {
                "trigger": "auto",
                "preTokens": 1000,
                "postTokens": 300,
                "messagesSummarized": 5,
                "tailMessageId": "tail-user"
            }
        }),
        json!({
            "id": "summary-1",
            "role": "user",
            "content": "<context>\nsummary\n</context>",
            "isCompactSummary": true
        }),
        json!({"id": "tail-user", "role": "user", "content": "latest question"}),
        assistant_tool_call("tc-after", "read_file"),
        tool_message("tc-after", "read_file", "Z".repeat(10_000)),
    ];

    let config = PreprocessConfig {
        budget: ToolResultBudgetConfig {
            aggregate_char_budget: 500,
            keep_recent_tool_results: 0,
            preserved_tool_names: HashSet::new(),
            replacement_preview_chars: 16,
        },
        microcompact: MicrocompactConfig {
            trigger_chars: usize::MAX,
            keep_recent_tool_results: 0,
            preserved_tool_names: HashSet::new(),
        },
        collapse: CollapseConfig {
            long_result_chars: usize::MAX,
            keep_recent_tool_results: 0,
            replacement_preview_chars: 16,
        },
        auto_compact: AutoCompactConfig {
            threshold_chars: usize::MAX,
            max_output_chars: 80_000,
            consecutive_failure_limit: 3,
            custom_context_window: None,
        },
        context_window: 64_000,
        query_source: None,
        context_collapse_owns_context: false,
        compact_boundary: None,
        project_instruction_content: None,
    };

    let mut compact_state = AutoCompactState::new();
    let mut runtime_state = PreprocessRuntimeState::default();
    let prepared = prepare_messages_for_llm(
        messages,
        "conv-u3-after-compact",
        PreprocessTrigger::Normal,
        &config,
        &mut compact_state,
        &mut runtime_state,
        false,
        |_messages: Vec<Value>| async { Ok("should not compact".to_string()) },
    )
    .await
    .expect("prepare should process post-compact tail");

    assert!(
        prepared
            .executed_stages
            .contains(&PreprocessStage::ToolResultBudget),
        "new tool output after compact artifacts must still go through budget"
    );
    let tool_content = prepared
        .messages
        .iter()
        .find(|message| message.get("role").and_then(Value::as_str) == Some("tool"))
        .and_then(|message| message.get("content").and_then(Value::as_str))
        .unwrap_or_default();
    assert!(tool_content.starts_with("[budget-trimmed]"));
    assert!(prepared.compact_boundary.is_none());
}

struct PromptTooLongRecoveryExecutor {
    history: Vec<Value>,
    results: Mutex<Vec<Result<LlmStepResult, TurnError>>>,
    received_messages: Mutex<Vec<Vec<Value>>>,
}

impl PromptTooLongRecoveryExecutor {
    fn new(history: Vec<Value>, results: Vec<Result<LlmStepResult, TurnError>>) -> Self {
        Self {
            history,
            results: Mutex::new(results),
            received_messages: Mutex::new(Vec::new()),
        }
    }

    fn all_messages(&self) -> Vec<Vec<Value>> {
        self.received_messages.lock().unwrap().clone()
    }
}

/// A CompactSummaryClient that counts invocations and returns a fixed reactive
/// summary — used by PromptTooLongRecovery tests to assert compaction was
/// triggered exactly once.
struct CountingCompactSummaryClient {
    calls: Mutex<usize>,
}

impl CountingCompactSummaryClient {
    fn new() -> Self {
        Self {
            calls: Mutex::new(0),
        }
    }

    fn call_count(&self) -> usize {
        *self.calls.lock().unwrap()
    }
}

#[async_trait]
impl CompactSummaryClient for CountingCompactSummaryClient {
    async fn compact_summary(
        &self,
        _conversation_id: &str,
        _messages: &[serde_json::Value],
        _llm_settings: &app_lib::runtime::chat::turn_config::ResolvedLlmSettings,
        _trace_id: Option<&str>,
        _run_id: Option<&str>,
    ) -> Result<String, TurnError> {
        *self.calls.lock().unwrap() += 1;
        Ok("reactive compact summary".to_string())
    }
}

#[async_trait]
impl RuntimeLlmExecutor for PromptTooLongRecoveryExecutor {
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
        let mut results = self.results.lock().unwrap();
        if results.is_empty() {
            return Err(TurnError::PromptTooLong(
                "prompt too long after configured retries".to_string(),
            ));
        }
        results.remove(0)
    }

    async fn load_history(&self, _conversation_id: &str) -> Result<Vec<Value>, TurnError> {
        Ok(self.history.clone())
    }

    async fn persist_assistant_message(
        &self,
        _conversation_id: &str,
        _content: &str,
        _tool_calls: &[serde_json::Value],
        _generated_file_ids: &[String],
        _file_metas: &[Value],
        _thinking_blocks: &[Value],
        _error: Option<&app_lib::storage::file_store::types::MessageError>,
    ) -> Result<String, TurnError> {
        Ok("assistant-msg".to_string())
    }

    async fn get_tool_defs(&self) -> Result<Vec<serde_json::Value>, TurnError> {
        Ok(vec![]) // 显式声明此 mock 不关心 tool_defs
    }
}

#[tokio::test]
async fn u3_driver_prompt_too_long_retries_once_with_compacted_messages() {
    let history = vec![
        assistant_tool_call("tc-a", "read_file"),
        tool_message("tc-a", "read_file", "A".repeat(8_000)),
    ];
    let executor = Arc::new(PromptTooLongRecoveryExecutor::new(
        history,
        vec![
            Err(TurnError::PromptTooLong("prompt too long".to_string())),
            Ok(LlmStepResult::ContentComplete {
                content: "done".to_string(),
                tokens_in: 1,
                tokens_out: 1,
                cache_creation_input_tokens: 0,
                cache_read_input_tokens: 0,
                thinking_blocks: Vec::new(),
                stop_reason: Some("end_turn".to_string()),
            }),
        ],
    ));
    let compact_client = Arc::new(CountingCompactSummaryClient::new());
    let host = RecordingRuntimeHost::new();
    let bus = RuntimeEventBus::new();
    let adapter: Arc<dyn RuntimeEventSubscriber> = Arc::new(TauriEventAdapter::new(host.clone()));
    bus.subscribe(adapter.clone());
    let _adapter = adapter;
    let driver = RuntimeChatTurnDriver::with_llm_executor(
        QueryEngine::default(),
        bus.clone(),
        executor.clone(),
    )
    .with_compact_client(compact_client.clone());
    let mut turn = make_test_turn("conv-u3-recovery");
    let request = ChatTurnRequest::new("conv-u3-recovery", "latest question", vec![]);

    driver.run_chat_turn(&mut turn, &request).await.unwrap();

    let calls = executor.all_messages();
    assert_eq!(calls.len(), 2, "prompt_too_long should trigger one retry");
    assert_eq!(compact_client.call_count(), 1);
    assert!(
        calls[1]
            .iter()
            .any(|msg| msg.get("isCompactSummary").and_then(|v| v.as_bool()) == Some(true)),
        "retry call must use compacted summary view"
    );
    let trace = host.trace();
    let event = trace
        .events
        .iter()
        .find(|event| event.name == "compact:completed")
        .expect("PromptTooLong recovery compact should emit compact:completed");
    assert_eq!(
        event.payload["conversationId"].as_str(),
        Some("conv-u3-recovery")
    );
    assert!(
        event.payload["messagesSummarized"].as_u64().unwrap_or(0) >= 3,
        "history messages plus current user should be summarized"
    );
}

#[tokio::test]
async fn u3_driver_prompt_too_long_surfaces_after_single_compacted_retry() {
    let history = vec![
        assistant_tool_call("tc-a", "read_file"),
        tool_message("tc-a", "read_file", "A".repeat(8_000)),
    ];
    let executor = Arc::new(PromptTooLongRecoveryExecutor::new(
        history,
        vec![
            Err(TurnError::PromptTooLong("prompt too long".to_string())),
            Err(TurnError::PromptTooLong(
                "prompt too long again".to_string(),
            )),
        ],
    ));
    let compact_client = Arc::new(CountingCompactSummaryClient::new());
    let bus = RuntimeEventBus::new();
    let driver =
        RuntimeChatTurnDriver::with_llm_executor(QueryEngine::default(), bus, executor.clone())
            .with_compact_client(compact_client.clone());
    let mut turn = make_test_turn("conv-u3-no-loop");
    let request = ChatTurnRequest::new("conv-u3-no-loop", "latest question", vec![]);

    let result = driver.run_chat_turn(&mut turn, &request).await;
    assert!(result.is_err(), "second prompt_too_long should surface");

    let calls = executor.all_messages();
    assert_eq!(
        calls.len(),
        2,
        "driver must not enter an infinite retry loop"
    );
    assert_eq!(compact_client.call_count(), 1);
}
