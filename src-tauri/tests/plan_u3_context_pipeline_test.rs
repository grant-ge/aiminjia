use std::collections::HashSet;
use std::sync::{Arc, Mutex};

use app_lib::runtime::cancellation::CancellationToken;
use app_lib::runtime::chat::compaction::{AutoCompactConfig, AutoCompactState, MicrocompactConfig};
use app_lib::runtime::chat::preprocess::{
    apply_tool_result_budget, prepare_messages_for_llm, CollapseConfig, PreprocessConfig,
    PreprocessRetryAction, PreprocessRuntimeState, PreprocessStage, PreprocessTrigger,
    ToolResultBudgetConfig,
};
use app_lib::runtime::chat::turn_config::{LlmStepInput, LlmStepResult, TurnError};
use app_lib::runtime::chat::{ChatTurnRequest, RuntimeChatTurnDriver, RuntimeLlmExecutor};
use app_lib::runtime::event_bus::RuntimeEventBus;
use app_lib::runtime::identity::IdentityMapping;
use app_lib::runtime::ids::RunId;
use app_lib::runtime::query_engine::QueryEngine;
use app_lib::runtime::state::TurnState;
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

fn normalize_created_at(messages: &[Value]) -> Vec<Value> {
    messages
        .iter()
        .map(|message| {
            let mut message = message.clone();
            if let Some(object) = message.as_object_mut() {
                if object.get("subtype").and_then(Value::as_str) == Some("compact_boundary") {
                    object.insert(
                        "createdAt".to_string(),
                        Value::String("<normalized>".to_string()),
                    );
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
        assistant_tool_call("tc-error", "search_files"),
        json!({
            "role": "tool",
            "toolCallId": "tc-error",
            "name": "search_files",
            "content": "command failed".repeat(30),
            "isError": true
        }),
        assistant_tool_call("tc-file", "generate_report"),
        tool_message(
            "tc-file",
            "generate_report",
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
        },
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
        },
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
        },
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

struct PromptTooLongRecoveryExecutor {
    history: Vec<Value>,
    results: Mutex<Vec<Result<LlmStepResult, TurnError>>>,
    received_messages: Mutex<Vec<Vec<Value>>>,
    compact_summary_calls: Mutex<usize>,
}

impl PromptTooLongRecoveryExecutor {
    fn new(history: Vec<Value>, results: Vec<Result<LlmStepResult, TurnError>>) -> Self {
        Self {
            history,
            results: Mutex::new(results),
            received_messages: Mutex::new(Vec::new()),
            compact_summary_calls: Mutex::new(0),
        }
    }

    fn all_messages(&self) -> Vec<Vec<Value>> {
        self.received_messages.lock().unwrap().clone()
    }

    fn compact_summary_calls(&self) -> usize {
        *self.compact_summary_calls.lock().unwrap()
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
        self.results.lock().unwrap().remove(0)
    }

    async fn load_history(&self, _conversation_id: &str) -> Result<Vec<Value>, TurnError> {
        Ok(self.history.clone())
    }

    async fn compact_summary(
        &self,
        _conversation_id: &str,
        _messages: &[Value],
    ) -> Result<String, TurnError> {
        *self.compact_summary_calls.lock().unwrap() += 1;
        Ok("reactive compact summary".to_string())
    }

    async fn persist_assistant_message(
        &self,
        _conversation_id: &str,
        _content: &str,
        _generated_file_ids: &[String],
        _file_metas: &[Value],
    ) -> Result<String, TurnError> {
        Ok("assistant-msg".to_string())
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
                stop_reason: Some("end_turn".to_string()),
            }),
        ],
    ));
    let bus = RuntimeEventBus::new();
    let driver =
        RuntimeChatTurnDriver::with_llm_executor(QueryEngine::default(), bus, executor.clone());
    let mut turn = make_test_turn("conv-u3-recovery");
    let request = ChatTurnRequest::new("conv-u3-recovery", "latest question", vec![]);

    driver.run_chat_turn(&mut turn, &request).await.unwrap();

    let calls = executor.all_messages();
    assert_eq!(calls.len(), 2, "prompt_too_long should trigger one retry");
    assert_eq!(executor.compact_summary_calls(), 1);
    assert!(
        calls[1]
            .iter()
            .any(|msg| msg.get("isCompactSummary").and_then(|v| v.as_bool()) == Some(true)),
        "retry call must use compacted summary view"
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
    let bus = RuntimeEventBus::new();
    let driver =
        RuntimeChatTurnDriver::with_llm_executor(QueryEngine::default(), bus, executor.clone());
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
    assert_eq!(executor.compact_summary_calls(), 1);
}
