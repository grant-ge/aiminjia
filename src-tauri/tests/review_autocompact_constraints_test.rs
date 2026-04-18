use app_lib::runtime::chat::compaction::{
    build_compact_boundary_record, compact_messages_via_llm, microcompact, AutoCompactConfig,
    AutoCompactState, CompactTrigger, MicrocompactConfig,
};
use serde_json::json;

#[test]
fn review_k_circuit_breaker_default_limit_is_3() {
    let config = AutoCompactConfig::default();
    assert_eq!(config.consecutive_failure_limit, 3);
}

#[test]
fn review_k_circuit_breaker_trips_at_limit_not_before() {
    let config = AutoCompactConfig::default();
    let mut state = AutoCompactState::new();

    state.record_failure();
    assert!(!state.is_circuit_broken(&config));
    state.record_failure();
    assert!(!state.is_circuit_broken(&config));
    state.record_failure();
    assert!(state.is_circuit_broken(&config));
}

#[test]
fn review_k_compact_boundary_subtype_is_compact_boundary() {
    let output = compact_messages_via_llm(
        vec![json!({ "role": "user", "content": "test" })],
        "summary text".to_string(),
    );
    let boundary = &output.new_messages[0];
    assert_eq!(
        boundary.get("subtype").and_then(|v| v.as_str()),
        Some("compact_boundary")
    );
    assert_eq!(boundary.get("role").and_then(|v| v.as_str()), Some("system"));
}

#[test]
fn review_k_compact_boundary_record_trigger_enum_stable() {
    let auto_str = serde_json::to_string(&CompactTrigger::Auto).unwrap();
    let manual_str = serde_json::to_string(&CompactTrigger::Manual).unwrap();
    assert_eq!(auto_str, "\"Auto\"");
    assert_eq!(manual_str, "\"Manual\"");

    let record = build_compact_boundary_record("conv-review", CompactTrigger::Auto, 100, 10, 2);
    assert_eq!(record.trigger, CompactTrigger::Auto);
}

#[test]
fn review_k_compaction_module_does_not_import_llm_gateway() {
    let compaction_src = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/src/runtime/chat/compaction.rs"
    ));
    assert!(!compaction_src.contains("use crate::llm::gateway"));
}

#[test]
fn review_k_compact_summary_message_has_is_compact_summary_flag() {
    let output = compact_messages_via_llm(
        vec![
            json!({ "role": "user", "content": "original" }),
            json!({ "role": "assistant", "content": "reply" }),
        ],
        "this is the summary".to_string(),
    );

    let summary_msg = output
        .new_messages
        .iter()
        .find(|m| m.get("isCompactSummary").and_then(|v| v.as_bool()) == Some(true));
    assert!(summary_msg.is_some());
}

#[test]
fn review_k_microcompact_never_deletes_messages() {
    let messages: Vec<serde_json::Value> = (0..6)
        .flat_map(|i| {
            vec![
                json!({
                    "role": "assistant",
                    "content": format!("step {}", i),
                    "toolCalls": [{ "id": format!("tc-{}", i), "name": "run", "arguments": {} }],
                }),
                json!({
                    "role": "tool",
                    "toolCallId": format!("tc-{}", i),
                    "name": "run",
                    "content": "z".repeat(10_000),
                }),
            ]
        })
        .collect();

    let original_len = messages.len();
    let result = microcompact(
        &messages,
        &MicrocompactConfig {
            trigger_chars: 1,
            keep_recent_tool_results: 1,
        },
    );
    assert_eq!(result.messages.len(), original_len);
}
