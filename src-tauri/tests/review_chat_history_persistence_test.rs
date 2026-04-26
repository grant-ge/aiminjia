// src-tauri/tests/review_chat_history_persistence_test.rs
//
// 架构约束测试：聊天记录持久化 round-trip。
// 验证 toolCalls 和 tool 消息在写入/读取链路上不丢失。

use app_lib::storage::file_store::AppStorage;
use tempfile::TempDir;

fn make_storage() -> (AppStorage, TempDir) {
    let dir = TempDir::new().unwrap();
    let storage = AppStorage::new(dir.path()).unwrap();
    (storage, dir)
}

/// assistant 消息带 toolCalls 时，写入再读回应保留 toolCalls 字段。
#[test]
fn review_assistant_tool_calls_round_trip() {
    let (storage, _dir) = make_storage();
    let conv_id = "conv-tc-rt";
    storage.create_conversation(conv_id, "test").unwrap();

    let content_json = serde_json::json!({
        "text": "我来帮你打开页面",
        "toolCalls": [
            {"id": "tc-001", "name": "browse_navigate", "arguments": {"url": "https://example.com"}}
        ]
    })
    .to_string();

    storage
        .insert_message("msg-001", conv_id, "assistant", &content_json)
        .unwrap();

    let msgs = storage.get_recent_messages(conv_id, 10).unwrap();
    assert_eq!(msgs.len(), 1);
    let content = &msgs[0]["content"];
    assert_eq!(
        content["toolCalls"][0]["name"].as_str().unwrap(),
        "browse_navigate",
        "toolCalls must survive storage round-trip"
    );
    assert_eq!(
        content["toolCalls"][0]["arguments"]["url"]
            .as_str()
            .unwrap(),
        "https://example.com"
    );
}

/// tool result 消息写入再读回应保留 toolResult.toolCallId/name/content 字段。
#[test]
fn review_tool_message_round_trip() {
    let (storage, _dir) = make_storage();
    let conv_id = "conv-tool-rt";
    storage.create_conversation(conv_id, "test").unwrap();

    let content_json = serde_json::json!({
        "toolCallId": "tc-001",
        "name": "browse_navigate",
        "content": "Page ready: https://example.com"
    })
    .to_string();

    storage
        .insert_message("msg-tool-001", conv_id, "tool", &content_json)
        .unwrap();

    let msgs = storage.get_recent_messages(conv_id, 10).unwrap();
    let tool_msg = msgs
        .iter()
        .find(|m| m["role"].as_str() == Some("tool"))
        .expect("tool message must be stored");
    assert_eq!(
        tool_msg["toolResult"]["toolCallId"].as_str().unwrap(),
        "tc-001"
    );
    assert_eq!(
        tool_msg["toolResult"]["name"].as_str().unwrap(),
        "browse_navigate"
    );
    assert_eq!(
        tool_msg["toolResult"]["content"].as_str().unwrap(),
        "Page ready: https://example.com"
    );
}

/// build_history_from_compact_boundary 必须将磁盘 tool 消息还原为
/// {role:"tool", toolCallId, name, content} 格式供 LLM 消费。
#[test]
fn review_load_history_restores_tool_messages() {
    use app_lib::transport::tauri_commands::chat::build_history_from_compact_boundary;

    let raw = vec![
        serde_json::json!({
            "id": "m1",
            "role": "user",
            "content": {"text": "帮我打开百度"},
        }),
        serde_json::json!({
            "id": "m2",
            "role": "assistant",
            "content": {
                "text": "",
                "toolCalls": [{"id": "tc-1", "name": "browse_navigate", "arguments": {"url": "https://baidu.com"}}]
            },
        }),
        serde_json::json!({
            "id": "m3",
            "role": "tool",
            "content": {
                "toolCallId": "tc-1",
                "name": "browse_navigate",
                "content": "Page ready: https://baidu.com"
            },
        }),
    ];

    let history = build_history_from_compact_boundary(raw, None, false);

    assert_eq!(history.len(), 3);
    assert_eq!(history[0]["role"], "user");
    assert_eq!(history[0]["content"], "帮我打开百度");

    assert_eq!(history[1]["role"], "assistant");
    assert_eq!(history[1]["toolCalls"][0]["name"], "browse_navigate");

    assert_eq!(history[2]["role"], "tool");
    assert_eq!(history[2]["toolCallId"], "tc-1");
    assert_eq!(history[2]["name"], "browse_navigate");
    assert_eq!(history[2]["content"], "Page ready: https://baidu.com");
}

/// 旧磁盘格式（toolName/result 字段）必须被兼容还原。
#[test]
fn review_load_history_restores_legacy_tool_messages() {
    use app_lib::transport::tauri_commands::chat::build_history_from_compact_boundary;

    let raw = vec![serde_json::json!({
        "id": "m1",
        "role": "tool",
        "content": {
            "toolCallId": "tc-legacy",
            "toolName": "browse_navigate",
            "isError": false,
            "result": "Page ready: https://baidu.com",
        },
    })];

    let history = build_history_from_compact_boundary(raw, None, false);

    assert_eq!(history.len(), 1);
    assert_eq!(history[0]["role"], "tool");
    assert_eq!(history[0]["toolCallId"], "tc-legacy");
    assert_eq!(history[0]["name"], "browse_navigate");
    assert_eq!(history[0]["content"], "Page ready: https://baidu.com");
}
