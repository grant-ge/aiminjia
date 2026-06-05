use app_lib::runtime::chat::compaction::{
    append_literal_anchor_hints, append_transcript_path_hint, compact_messages_via_llm,
    compact_transcript_path_for_conversation_dir, evaluate_auto_compact, should_auto_compact,
    AutoCompactConfig, AutoCompactDecisionInput, AutoCompactDecisionReason, AutoCompactState,
    AutoCompactTriggerKind, CompactLlmOutput,
};
use serde_json::json;
use std::path::PathBuf;

fn make_messages(n: usize, tool_result_chars: usize) -> Vec<serde_json::Value> {
    let mut msgs = vec![json!({ "role": "user", "content": "start" })];
    for i in 0..n {
        msgs.push(json!({
            "role": "assistant",
            "content": format!("step {}", i),
            "toolCalls": [{ "id": format!("tc-{}", i), "name": "run", "arguments": {} }],
        }));
        msgs.push(json!({
            "role": "tool",
            "toolCallId": format!("tc-{}", i),
            "name": "run",
            "content": "x".repeat(tool_result_chars),
        }));
    }
    msgs
}

fn decision_for(
    messages: &[serde_json::Value],
    config: &AutoCompactConfig,
    state: &AutoCompactState,
    freed_tokens_estimate: usize,
    trigger: AutoCompactTriggerKind,
    query_source: Option<&str>,
    stop_hook_active: bool,
    context_collapse_suppressed: bool,
) -> app_lib::runtime::chat::compaction::AutoCompactDecision {
    evaluate_auto_compact(AutoCompactDecisionInput {
        messages,
        config,
        state,
        trigger,
        query_source,
        freed_tokens_estimate,
        stop_hook_active,
        context_collapse_suppressed,
    })
}

#[test]
fn k3_should_auto_compact_false_below_threshold() {
    let messages = make_messages(2, 100);
    let config = AutoCompactConfig {
        threshold_chars: 200_000,
        max_output_chars: 80_000,
        consecutive_failure_limit: 3,
        custom_context_window: None,
    };
    assert!(!should_auto_compact(&messages, &config));
}

#[test]
fn k3_should_auto_compact_true_above_threshold() {
    let messages = make_messages(5, 50_000);
    let config = AutoCompactConfig {
        threshold_chars: 100_000,
        max_output_chars: 80_000,
        consecutive_failure_limit: 3,
        custom_context_window: None,
    };
    assert!(should_auto_compact(&messages, &config));
}

#[test]
fn k3_auto_compact_decision_records_above_threshold() {
    let messages = make_messages(5, 50_000);
    let config = AutoCompactConfig {
        threshold_chars: 100_000,
        max_output_chars: 80_000,
        consecutive_failure_limit: 3,
        custom_context_window: Some(64_000),
    };
    let state = AutoCompactState::new();

    let decision = decision_for(
        &messages,
        &config,
        &state,
        0,
        AutoCompactTriggerKind::Normal,
        Some("chat_turn"),
        false,
        false,
    );

    assert!(decision.should_run);
    assert_eq!(decision.reason, AutoCompactDecisionReason::AboveThreshold);
    assert!(decision.adjusted_tokens >= decision.threshold_tokens);
    assert_eq!(decision.query_source.as_deref(), Some("chat_turn"));
}

#[test]
fn k3_auto_compact_decision_predicts_turn_growth() {
    let messages = vec![json!({ "role": "user", "content": "x".repeat(130_000) })];
    let config = AutoCompactConfig {
        threshold_chars: usize::MAX,
        max_output_chars: 80_000,
        consecutive_failure_limit: 3,
        custom_context_window: Some(64_000),
    };
    let state = AutoCompactState::new();

    let decision = decision_for(
        &messages,
        &config,
        &state,
        0,
        AutoCompactTriggerKind::Normal,
        Some("chat_turn"),
        false,
        false,
    );

    assert!(decision.should_run);
    assert_eq!(decision.reason, AutoCompactDecisionReason::PredictiveGrowth);
    assert!(decision.adjusted_tokens > decision.predictive_threshold_tokens);
}

#[test]
fn k3_auto_compact_decision_uses_freed_token_compensation() {
    let messages = vec![json!({ "role": "user", "content": "x".repeat(20_000) })];
    let config = AutoCompactConfig {
        threshold_chars: 4_000,
        max_output_chars: 80_000,
        consecutive_failure_limit: 3,
        custom_context_window: Some(64_000),
    };
    let state = AutoCompactState::new();

    let decision = decision_for(
        &messages,
        &config,
        &state,
        10_000,
        AutoCompactTriggerKind::Normal,
        Some("chat_turn"),
        false,
        false,
    );

    assert!(!decision.should_run);
    assert_eq!(decision.reason, AutoCompactDecisionReason::BelowThreshold);
    assert_eq!(decision.adjusted_tokens, 0);
}

#[test]
fn k3_auto_compact_decision_respects_circuit_breaker() {
    let messages = make_messages(5, 50_000);
    let config = AutoCompactConfig {
        threshold_chars: 1,
        max_output_chars: 80_000,
        consecutive_failure_limit: 3,
        custom_context_window: Some(64_000),
    };
    let mut state = AutoCompactState::new();
    state.consecutive_failures = 3;

    let decision = decision_for(
        &messages,
        &config,
        &state,
        0,
        AutoCompactTriggerKind::Normal,
        Some("chat_turn"),
        false,
        false,
    );

    assert!(!decision.should_run);
    assert_eq!(decision.reason, AutoCompactDecisionReason::CircuitBroken);
    assert!(decision.circuit_broken);
}

#[test]
fn k3_auto_compact_decision_blocks_recursive_sources() {
    let messages = make_messages(5, 50_000);
    let config = AutoCompactConfig {
        threshold_chars: 1,
        max_output_chars: 80_000,
        consecutive_failure_limit: 3,
        custom_context_window: Some(64_000),
    };
    let state = AutoCompactState::new();

    let decision = decision_for(
        &messages,
        &config,
        &state,
        0,
        AutoCompactTriggerKind::Normal,
        Some("compact"),
        false,
        false,
    );

    assert!(!decision.should_run);
    assert_eq!(decision.reason, AutoCompactDecisionReason::RecursiveSource);
    assert!(decision.recursive_source_blocked);
}

#[test]
fn k3_auto_compact_decision_suppresses_when_context_collapse_owns_context() {
    let messages = make_messages(5, 50_000);
    let config = AutoCompactConfig {
        threshold_chars: 1,
        max_output_chars: 80_000,
        consecutive_failure_limit: 3,
        custom_context_window: Some(64_000),
    };
    let state = AutoCompactState::new();

    let decision = decision_for(
        &messages,
        &config,
        &state,
        0,
        AutoCompactTriggerKind::Normal,
        Some("chat_turn"),
        false,
        true,
    );

    assert!(!decision.should_run);
    assert_eq!(
        decision.reason,
        AutoCompactDecisionReason::ContextCollapseSuppressed
    );
    assert!(decision.context_collapse_suppressed);
}

#[test]
fn k3_compact_messages_via_llm_replaces_history() {
    let mut messages = make_messages(5, 1_000);
    messages.push(json!({ "role": "user", "content": "latest question" }));

    let output = compact_messages_via_llm_stub(
        messages.clone(),
        "之前的对话摘要：分析了 5 个步骤，结论是 X。".to_string(),
    );

    assert!(output.new_messages.len() <= 3);

    let first = &output.new_messages[0];
    assert_eq!(first.get("role").and_then(|v| v.as_str()), Some("system"));
    assert_eq!(
        first.get("subtype").and_then(|v| v.as_str()),
        Some("compact_boundary")
    );

    let summary_msg = output.new_messages.iter().find(|m| {
        m.get("role").and_then(|v| v.as_str()) == Some("user")
            && m.get("isCompactSummary")
                .and_then(|v| v.as_bool())
                .unwrap_or(false)
    });
    assert!(summary_msg.is_some());
    let summary_content = summary_msg
        .unwrap()
        .get("content")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    assert!(summary_content.contains("摘要"));

    let latest = output.new_messages.last().unwrap();
    assert_eq!(
        latest.get("content").and_then(|v| v.as_str()),
        Some("latest question")
    );

    assert!(output.pre_tokens > 0);
    assert!(output.post_tokens < output.pre_tokens);
}

fn compact_messages_via_llm_stub(
    messages: Vec<serde_json::Value>,
    summary_text: String,
) -> CompactLlmOutput {
    compact_messages_via_llm(messages, summary_text)
}

#[test]
fn compact_preserves_tail_tool_round() {
    let messages = vec![
        json!({ "role": "user", "content": "q1" }),
        json!({ "role": "assistant", "content": "a1" }),
        json!({ "role": "user", "content": "q2" }),
        json!({
            "role": "assistant",
            "content": "",
            "toolCalls": [{ "id": "tc_1", "name": "exec", "arguments": {} }],
        }),
        json!({
            "role": "tool",
            "toolCallId": "tc_1",
            "name": "exec",
            "content": "result",
        }),
    ];

    let output = compact_messages_via_llm_stub(messages, "摘要".to_string());
    assert_eq!(output.new_messages.len(), 5);
    assert_eq!(output.new_messages[2]["content"], "q2");
    assert!(output.new_messages[3]["toolCalls"].is_array());
    assert_eq!(output.new_messages[4]["role"], "tool");
    assert_eq!(output.new_messages[4]["toolCallId"], "tc_1");
}

#[test]
fn compact_messages_summarized_excludes_preserved_tail_round() {
    let messages = vec![
        json!({ "role": "user", "id": "u1", "content": "old question" }),
        json!({ "role": "assistant", "id": "a1", "content": "old answer" }),
        json!({ "role": "user", "id": "u2", "content": "latest question" }),
        json!({ "role": "assistant", "id": "a2", "content": "latest answer" }),
    ];

    let output = compact_messages_via_llm_stub(messages, "摘要".to_string());

    assert_eq!(
        output.messages_summarized, 2,
        "messagesSummarized should count messages replaced by the summary, not the preserved tail"
    );
    assert_eq!(
        output.new_messages[0]["compactMetadata"]["messagesSummarized"],
        2
    );
}

#[test]
fn compact_summary_hint_appends_transcript_path_once() {
    let transcript_path =
        r"C:\Users\Administrator\.renlijia\users\t_1__u_2\conversations\conv-1\messages.jsonl";
    let summary = append_transcript_path_hint("摘要正文".to_string(), Some(transcript_path));

    assert!(summary.starts_with("摘要正文"));
    assert!(summary.contains("完整的对话记录在"));
    assert!(summary.contains(transcript_path));

    let repeated = append_transcript_path_hint(summary.clone(), Some(transcript_path));
    assert_eq!(repeated, summary);
    assert_eq!(
        append_transcript_path_hint("摘要正文".to_string(), None),
        "摘要正文"
    );
}

#[test]
fn compact_literal_anchor_hint_preserves_key_user_markers() {
    let messages = vec![json!({
        "role": "user",
        "content": {
            "text": "关键事实=PREDICTIVE-COMPACT-FACT=下一步只允许走灰度发布。\n已排除误判=PREDICTIVE-COMPACT-EXCLUDE=压力文本不是用户需求。\nCTX-COMPACT-PREDICTIVE-PRESSURE filler"
        }
    })];

    let summary = append_literal_anchor_hints("以下是对话历史摘要：".to_string(), &messages);

    assert!(summary.contains("PREDICTIVE-COMPACT-FACT"));
    assert!(summary.contains("PREDICTIVE-COMPACT-EXCLUDE"));
    assert!(!summary.contains("CTX-COMPACT-PREDICTIVE-PRESSURE"));

    let repeated = append_literal_anchor_hints(summary.clone(), &messages);
    assert_eq!(repeated, summary);
}

#[test]
fn compact_transcript_path_hint_is_absolute() {
    let path =
        compact_transcript_path_for_conversation_dir(&PathBuf::from("relative-conversation-dir"));
    let path_buf = PathBuf::from(path);

    assert!(
        path_buf.is_absolute(),
        "compact transcript hint must use an absolute path"
    );
    assert!(path_buf.ends_with(PathBuf::from("relative-conversation-dir").join("messages.jsonl")));
}
