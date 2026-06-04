use std::collections::HashSet;

use app_lib::llm::context_decay::estimate_tokens_from_json;
use app_lib::runtime::chat::compaction::{
    AutoCompactConfig, AutoCompactState, CompactBoundaryRecord, CompactTrigger, MicrocompactConfig,
    PreservedSegment,
};
use app_lib::runtime::chat::preprocess::{
    prepare_messages_for_llm, CollapseConfig, PreprocessConfig, PreprocessRuntimeState,
    PreprocessTrigger, ToolResultBudgetConfig,
};
use serde_json::{json, Value};

#[test]
fn r3_old_boundary_deserializes_without_preserved_segment() {
    let raw = r#"{
        "id": "b1",
        "conversation_id": "conv-1",
        "trigger": "Auto",
        "pre_tokens": 1000,
        "post_tokens": 300,
        "messages_summarized": 7,
        "created_at": "2026-06-02T00:00:00Z",
        "summary_text": "summary",
        "tail_message_id": "tail-1"
    }"#;

    let record: CompactBoundaryRecord = serde_json::from_str(raw).expect("legacy boundary");

    assert_eq!(record.id, "b1");
    assert_eq!(record.tail_message_id.as_deref(), Some("tail-1"));
    assert!(record.preserved_segment.is_none());
}

#[test]
fn r3_preserved_segment_serializes_with_boundary_record() {
    let record = CompactBoundaryRecord {
        id: "b1".to_string(),
        conversation_id: "conv-1".to_string(),
        trigger: CompactTrigger::Auto,
        pre_tokens: 1000,
        post_tokens: 300,
        messages_summarized: 7,
        created_at: "2026-06-02T00:00:00Z".to_string(),
        summary_text: "summary".to_string(),
        tail_message_id: Some("tail-1".to_string()),
        preserved_segment: Some(PreservedSegment {
            first_preserved_message_id: "head-1".to_string(),
            anchor_message_id: "anchor-1".to_string(),
            tail_message_id: "tail-1".to_string(),
            preserved_token_count: 42,
        }),
    };

    let value = serde_json::to_value(record).expect("serialize boundary");

    assert_eq!(
        value["preserved_segment"]["first_preserved_message_id"],
        "head-1"
    );
    assert_eq!(value["preserved_segment"]["anchor_message_id"], "anchor-1");
    assert_eq!(value["preserved_segment"]["tail_message_id"], "tail-1");
    assert_eq!(value["preserved_segment"]["preserved_token_count"], 42);
}

#[tokio::test]
async fn r3_preserved_segment_token_count_uses_json_token_estimate_once() {
    let tail = json!({"role": "user", "id": "tail-1", "content": "latest question ".repeat(80)});
    let messages = vec![
        json!({"role": "user", "id": "old-1", "content": "old question ".repeat(200)}),
        json!({"role": "assistant", "id": "old-a", "content": "old answer"}),
        tail.clone(),
    ];
    let config = PreprocessConfig {
        budget: ToolResultBudgetConfig {
            aggregate_char_budget: 10_000,
            keep_recent_tool_results: 1,
            preserved_tool_names: HashSet::new(),
            replacement_preview_chars: 32,
        },
        microcompact: MicrocompactConfig {
            trigger_chars: 10_000,
            keep_recent_tool_results: 1,
            preserved_tool_names: HashSet::new(),
        },
        collapse: CollapseConfig {
            long_result_chars: 10_000,
            keep_recent_tool_results: 1,
            replacement_preview_chars: 32,
        },
        auto_compact: AutoCompactConfig {
            threshold_chars: 1,
            max_output_chars: 80_000,
            consecutive_failure_limit: 3,
            custom_context_window: None,
        },
        context_window: 64_000,
        compact_boundary: None,
        project_instruction_content: None,
    };

    let mut compact_state = AutoCompactState::new();
    let mut runtime_state = PreprocessRuntimeState::default();
    let prepared = prepare_messages_for_llm(
        messages,
        "conv-r3",
        PreprocessTrigger::Normal,
        &config,
        &mut compact_state,
        &mut runtime_state,
        false,
        |_messages: Vec<Value>| async { Ok("summary".to_string()) },
    )
    .await
    .expect("prepare should compact");

    let preserved = prepared
        .compact_boundary
        .and_then(|record| record.preserved_segment)
        .expect("preserved segment should be recorded");
    let expected = estimate_tokens_from_json(&[tail]) as u64;
    let summary_id = prepared
        .messages
        .iter()
        .find(|message| {
            message
                .get("isCompactSummary")
                .and_then(|value| value.as_bool())
                == Some(true)
        })
        .and_then(|message| message.get("id").and_then(|value| value.as_str()))
        .expect("summary message should have an id")
        .to_string();

    assert_eq!(preserved.first_preserved_message_id, "tail-1");
    assert_eq!(preserved.anchor_message_id, summary_id);
    assert_eq!(preserved.tail_message_id, "tail-1");
    assert_eq!(preserved.preserved_token_count, expected);
    assert!(expected > 0);
}

#[tokio::test]
async fn r3_manual_compact_runs_below_auto_threshold_and_marks_manual_trigger() {
    let messages = vec![
        json!({"role": "user", "id": "u1", "content": "short manual compact request"}),
        json!({"role": "assistant", "id": "a1", "content": "short answer"}),
    ];
    let config = PreprocessConfig {
        budget: ToolResultBudgetConfig {
            aggregate_char_budget: 10_000,
            keep_recent_tool_results: 1,
            preserved_tool_names: HashSet::new(),
            replacement_preview_chars: 32,
        },
        microcompact: MicrocompactConfig {
            trigger_chars: 10_000,
            keep_recent_tool_results: 1,
            preserved_tool_names: HashSet::new(),
        },
        collapse: CollapseConfig {
            long_result_chars: 10_000,
            keep_recent_tool_results: 1,
            replacement_preview_chars: 32,
        },
        auto_compact: AutoCompactConfig {
            threshold_chars: usize::MAX,
            max_output_chars: 80_000,
            consecutive_failure_limit: 3,
            custom_context_window: None,
        },
        context_window: 64_000,
        compact_boundary: None,
        project_instruction_content: None,
    };

    let mut compact_state = AutoCompactState::new();
    let mut runtime_state = PreprocessRuntimeState::default();
    let prepared = prepare_messages_for_llm(
        messages,
        "conv-manual",
        PreprocessTrigger::ManualCompact,
        &config,
        &mut compact_state,
        &mut runtime_state,
        false,
        |_messages: Vec<Value>| async { Ok("manual summary".to_string()) },
    )
    .await
    .expect("manual prepare should compact");

    let boundary = prepared
        .compact_boundary
        .expect("manual compact should create boundary");
    assert_eq!(boundary.trigger, CompactTrigger::Manual);
    assert!(prepared.messages.iter().any(|message| {
        message.get("role").and_then(Value::as_str) == Some("system")
            && message.get("subtype").and_then(Value::as_str) == Some("compact_boundary")
            && message
                .get("compactMetadata")
                .and_then(|metadata| metadata.get("trigger"))
                .and_then(Value::as_str)
                == Some("manual")
    }));
    assert!(prepared.messages.iter().any(|message| {
        message.get("role").and_then(Value::as_str) == Some("user")
            && message.get("isCompactSummary").and_then(Value::as_bool) == Some(true)
    }));
}

#[tokio::test]
async fn r3_manual_compact_surfaces_summary_errors() {
    let messages = vec![json!({"role": "user", "id": "u1", "content": "manual compact"})];
    let config = PreprocessConfig {
        budget: ToolResultBudgetConfig {
            aggregate_char_budget: 10_000,
            keep_recent_tool_results: 1,
            preserved_tool_names: HashSet::new(),
            replacement_preview_chars: 32,
        },
        microcompact: MicrocompactConfig {
            trigger_chars: 10_000,
            keep_recent_tool_results: 1,
            preserved_tool_names: HashSet::new(),
        },
        collapse: CollapseConfig {
            long_result_chars: 10_000,
            keep_recent_tool_results: 1,
            replacement_preview_chars: 32,
        },
        auto_compact: AutoCompactConfig {
            threshold_chars: usize::MAX,
            max_output_chars: 80_000,
            consecutive_failure_limit: 3,
            custom_context_window: None,
        },
        context_window: 64_000,
        compact_boundary: None,
        project_instruction_content: None,
    };

    let mut compact_state = AutoCompactState::new();
    let mut runtime_state = PreprocessRuntimeState::default();
    let err = prepare_messages_for_llm(
        messages,
        "conv-manual-error",
        PreprocessTrigger::ManualCompact,
        &config,
        &mut compact_state,
        &mut runtime_state,
        false,
        |_messages: Vec<Value>| async {
            Err(app_lib::runtime::chat::turn_config::TurnError::LlmError(
                "manual summary failed".to_string(),
            ))
        },
    )
    .await
    .expect_err("manual compact should surface summary errors");

    assert!(err.to_string().contains("manual summary failed"));
}

#[tokio::test]
async fn r3_manual_compact_surfaces_empty_summary() {
    let messages = vec![json!({"role": "user", "id": "u1", "content": "manual compact"})];
    let config = PreprocessConfig {
        budget: ToolResultBudgetConfig {
            aggregate_char_budget: 10_000,
            keep_recent_tool_results: 1,
            preserved_tool_names: HashSet::new(),
            replacement_preview_chars: 32,
        },
        microcompact: MicrocompactConfig {
            trigger_chars: 10_000,
            keep_recent_tool_results: 1,
            preserved_tool_names: HashSet::new(),
        },
        collapse: CollapseConfig {
            long_result_chars: 10_000,
            keep_recent_tool_results: 1,
            replacement_preview_chars: 32,
        },
        auto_compact: AutoCompactConfig {
            threshold_chars: usize::MAX,
            max_output_chars: 80_000,
            consecutive_failure_limit: 3,
            custom_context_window: None,
        },
        context_window: 64_000,
        compact_boundary: None,
        project_instruction_content: None,
    };

    let mut compact_state = AutoCompactState::new();
    let mut runtime_state = PreprocessRuntimeState::default();
    let err = prepare_messages_for_llm(
        messages,
        "conv-manual-empty",
        PreprocessTrigger::ManualCompact,
        &config,
        &mut compact_state,
        &mut runtime_state,
        false,
        |_messages: Vec<Value>| async { Ok("   ".to_string()) },
    )
    .await
    .expect_err("manual compact should surface empty summaries");

    assert!(err.to_string().contains("manual compact summary was empty"));
}
