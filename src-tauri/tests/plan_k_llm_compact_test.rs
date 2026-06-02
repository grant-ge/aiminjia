use app_lib::runtime::chat::compaction::{
    compact_messages_via_llm, should_auto_compact, AutoCompactConfig, CompactLlmOutput,
};
use serde_json::json;

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
