use app_lib::runtime::chat::compaction::{microcompact, MicrocompactConfig};
use serde_json::json;

fn make_user(content: &str) -> serde_json::Value {
    json!({ "role": "user", "content": content })
}

fn make_assistant_with_tools(content: &str, tool_call_ids: &[&str]) -> serde_json::Value {
    let tool_calls: Vec<serde_json::Value> = tool_call_ids
        .iter()
        .map(|id| json!({ "id": id, "name": "execute_python", "arguments": {} }))
        .collect();
    json!({ "role": "assistant", "content": content, "toolCalls": tool_calls })
}

fn make_tool_result(tool_call_id: &str, content: &str) -> serde_json::Value {
    json!({
        "role": "tool",
        "toolCallId": tool_call_id,
        "name": "execute_python",
        "content": content,
    })
}

#[test]
fn k2_microcompact_noop_when_below_threshold() {
    let messages = vec![
        make_user("hello"),
        make_assistant_with_tools("running", &["tc-1"]),
        make_tool_result("tc-1", "short"),
    ];
    let config = MicrocompactConfig {
        trigger_chars: 100_000,
        keep_recent_tool_results: 2,
    };
    let result = microcompact(&messages, &config);
    assert!(!result.executed);
    assert_eq!(result.tokens_freed_estimate, 0);
    assert_eq!(result.messages.len(), messages.len());
    assert_eq!(result.messages, messages);
}

#[test]
fn k2_microcompact_clears_old_tool_results_above_threshold() {
    let big_content = "x".repeat(50_000);
    let messages = vec![
        make_user("analyze"),
        make_assistant_with_tools("iter0", &["tc-old"]),
        make_tool_result("tc-old", &big_content),
        make_assistant_with_tools("iter1", &["tc-new"]),
        make_tool_result("tc-new", "short result"),
    ];
    let config = MicrocompactConfig {
        trigger_chars: 10_000,
        keep_recent_tool_results: 1,
    };
    let result = microcompact(&messages, &config);
    assert!(result.executed);
    assert!(result.tokens_freed_estimate > 0);

    let old_result = result
        .messages
        .iter()
        .find(|m| m.get("toolCallId").and_then(|v| v.as_str()) == Some("tc-old"))
        .expect("old tool result should still exist");
    let content = old_result
        .get("content")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    assert!(content.contains("[microcompacted]"));

    let new_result = result
        .messages
        .iter()
        .find(|m| m.get("toolCallId").and_then(|v| v.as_str()) == Some("tc-new"))
        .expect("new tool result should exist");
    let new_content = new_result
        .get("content")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    assert_eq!(new_content, "short result");
}

#[test]
fn k2_microcompact_preserves_message_count() {
    let big_content = "y".repeat(30_000);
    let messages = vec![
        make_user("start"),
        make_assistant_with_tools("a", &["tc-a"]),
        make_tool_result("tc-a", &big_content),
        make_assistant_with_tools("b", &["tc-b"]),
        make_tool_result("tc-b", &big_content),
        make_assistant_with_tools("c", &["tc-c"]),
        make_tool_result("tc-c", "final"),
    ];
    let config = MicrocompactConfig {
        trigger_chars: 5_000,
        keep_recent_tool_results: 1,
    };
    let result = microcompact(&messages, &config);
    assert_eq!(result.messages.len(), messages.len());
}
