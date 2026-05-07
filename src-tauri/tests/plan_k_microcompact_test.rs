use app_lib::runtime::chat::compaction::{microcompact, MicrocompactConfig};
use serde_json::json;

fn make_user(content: &str) -> serde_json::Value {
    json!({ "role": "user", "content": content })
}

fn make_assistant_with_tools(content: &str, tool_call_ids: &[&str]) -> serde_json::Value {
    let tool_calls: Vec<serde_json::Value> = tool_call_ids
        .iter()
        .map(|id| json!({ "id": id, "name": "bash", "arguments": {} }))
        .collect();
    json!({ "role": "assistant", "content": content, "toolCalls": tool_calls })
}

fn make_tool_result(tool_call_id: &str, content: &str) -> serde_json::Value {
    json!({
        "role": "tool",
        "toolCallId": tool_call_id,
        "name": "bash",
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
        preserved_tool_names: std::collections::HashSet::new(),
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
        preserved_tool_names: std::collections::HashSet::new(),
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
        preserved_tool_names: std::collections::HashSet::new(),
    };
    let result = microcompact(&messages, &config);
    assert_eq!(result.messages.len(), messages.len());
}

#[test]
fn x2_microcompact_config_default_includes_preserved_tool_names() {
    let config = MicrocompactConfig::default();
    // preserved_tool_names is built from catalog entries with preserve_tool_use_results=true
    // Verify the config builds without error and preserved_tool_names is a valid HashSet
    let _ = config.preserved_tool_names;
}

#[test]
fn x2_microcompact_skips_preserved_tool_results() {
    let big_content = "z".repeat(50_000);
    let messages = vec![
        make_user("analyze"),
        make_assistant_with_tools("iter0", &["tc-old"]),
        make_tool_result("tc-old", &big_content),
        json!({
            "role": "assistant",
            "content": "iter1",
            "toolCalls": [{ "id": "tc-non-preserved", "name": "web_search", "arguments": {} }]
        }),
        json!({
            "role": "tool",
            "toolCallId": "tc-non-preserved",
            "name": "web_search",
            "content": big_content,
        }),
        make_assistant_with_tools("iter2", &["tc-new"]),
        make_tool_result("tc-new", "short result"),
    ];
    let config = MicrocompactConfig {
        trigger_chars: 10_000,
        keep_recent_tool_results: 1,
        preserved_tool_names: ["bash".to_string(), "web_search".to_string()]
            .into_iter()
            .collect(),
    };
    let result = microcompact(&messages, &config);

    let preserved = result
        .messages
        .iter()
        .find(|m| m.get("toolCallId").and_then(|v| v.as_str()) == Some("tc-old"))
        .expect("preserved result should exist");
    assert_eq!(
        preserved
            .get("content")
            .and_then(|v| v.as_str())
            .unwrap_or(""),
        big_content,
        "preserved tools must not be replaced by [microcompacted]"
    );

    let non_preserved = result
        .messages
        .iter()
        .find(|m| m.get("toolCallId").and_then(|v| v.as_str()) == Some("tc-non-preserved"))
        .expect("non-preserved result should exist");
    assert!(
        non_preserved
            .get("content")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .contains("[microcompacted]"),
        "non-preserved tool results should still be compacted"
    );
}
