use app_lib::runtime::chat::turn_config::TurnIterationState;
use serde_json::json;

#[test]
fn append_messages_batch_appends_assistant_and_tool_messages_in_order() {
    let mut state = TurnIterationState::new(vec![json!({
        "role": "user",
        "content": "hi",
    })]);

    state.append_messages_batch(vec![
        json!({
            "role": "assistant",
            "content": "thinking",
            "toolCalls": [{"id": "tc-b4-1", "name": "echo_tool", "arguments": {}}],
        }),
        json!({
            "role": "tool",
            "toolCallId": "tc-b4-1",
            "name": "echo_tool",
            "content": "ok",
        }),
    ]);

    assert_eq!(state.messages.len(), 3);
    assert_eq!(state.messages[1]["role"], "assistant");
    assert_eq!(state.messages[2]["role"], "tool");
}
