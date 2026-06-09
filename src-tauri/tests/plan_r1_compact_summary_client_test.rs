use app_lib::runtime::chat::compaction::truncate_messages_for_ptl_retry;
use app_lib::runtime::chat::preprocess::strip_images_from_messages;
use serde_json::json;

#[test]
fn r1_strip_images_replaces_user_type_image_blocks_only() {
    let messages = vec![
        json!({
            "role": "user",
            "content": [
                {"type": "text", "text": "look"},
                {"type": "image", "source": {"type": "base64", "data": "abc"}}
            ]
        }),
        json!({
            "role": "assistant",
            "content": [
                {"type": "image", "source": {"type": "base64", "data": "assistant-image"}}
            ]
        }),
    ];

    let (stripped, did_strip) = strip_images_from_messages(&messages);

    assert!(did_strip);
    assert_eq!(
        stripped[0]["content"][0],
        json!({"type": "text", "text": "look"})
    );
    assert_eq!(
        stripped[0]["content"][1],
        json!({"type": "text", "text": "[image]"})
    );
    assert_eq!(
        stripped[1]["content"][0],
        json!({"type": "image", "source": {"type": "base64", "data": "assistant-image"}}),
        "non-user image blocks must not be rewritten"
    );
}

#[test]
fn r1_strip_images_replaces_user_image_url_and_data_url_text() {
    let base64_like =
        "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/=".repeat(120);
    let messages = vec![json!({
        "role": "user",
        "content": [
            {"type": "image_url", "image_url": {"url": "https://example.test/a.png"}},
            {"type": "input_image", "source": {"type": "base64", "media_type": "image/png", "data": base64_like}},
            {"type": "text", "text": format!("data:image/png;base64,{}", base64_like)}
        ]
    })];

    let (stripped, did_strip) = strip_images_from_messages(&messages);

    assert!(did_strip);
    assert_eq!(
        stripped[0]["content"],
        json!([
            {"type": "text", "text": "[image]"},
            {"type": "text", "text": "[image]"},
            {"type": "text", "text": "[image]"}
        ])
    );
}

#[test]
fn r3_ptl_truncate_drops_oldest_round_and_preserves_system_context() {
    let messages = vec![
        json!({"role": "system", "content": "protocol_path_anthropic"}),
        json!({"role": "user", "id": "u1", "content": "q1"}),
        json!({"role": "assistant", "id": "a1", "content": "a1"}),
        json!({"role": "user", "id": "u2", "content": "q2"}),
        json!({"role": "assistant", "id": "a2", "content": "a2"}),
        json!({"role": "user", "id": "u3", "content": "q3"}),
        json!({"role": "assistant", "id": "a3", "content": "a3"}),
        json!({"role": "user", "id": "u4", "content": "q4"}),
        json!({"role": "assistant", "id": "a4", "content": "a4"}),
        json!({"role": "user", "id": "u5", "content": "q5"}),
        json!({"role": "assistant", "id": "a5", "content": "a5"}),
    ];

    let truncated = truncate_messages_for_ptl_retry(&messages);

    assert_eq!(truncated[0]["role"], "system");
    assert!(
        truncated
            .iter()
            .all(|m| m.get("id").and_then(|v| v.as_str()) != Some("u1")),
        "oldest user round should be removed"
    );
    assert!(
        truncated
            .iter()
            .any(|m| m.get("id").and_then(|v| v.as_str()) == Some("u5")),
        "newest round should be preserved"
    );
    assert!(truncated.len() < messages.len());
}

#[test]
fn r3_ptl_truncate_single_round_shortens_content_without_breaking_tool_pair() {
    let messages = vec![
        json!({"role": "system", "content": "protocol_path_anthropic"}),
        json!({"role": "user", "id": "u1", "content": "large prompt ".repeat(200)}),
        json!({
            "role": "assistant",
            "id": "a1",
            "content": "",
            "toolCalls": [{"id": "tc-1", "name": "Read", "arguments": {}}]
        }),
        json!({"role": "tool", "id": "t1", "toolCallId": "tc-1", "name": "Read", "content": "result"}),
    ];

    let truncated = truncate_messages_for_ptl_retry(&messages);

    assert_eq!(truncated.len(), messages.len());
    assert_eq!(truncated[0]["role"], "system");
    assert_eq!(truncated[1]["role"], "user");
    assert_eq!(truncated[2]["role"], "assistant");
    assert_eq!(truncated[3]["role"], "tool");
    assert_eq!(truncated[2]["toolCalls"][0]["id"], "tc-1");
    assert_eq!(truncated[3]["toolCallId"], "tc-1");
    assert!(
        truncated[1]["content"].as_str().unwrap().len()
            < messages[1]["content"].as_str().unwrap().len()
    );
}
