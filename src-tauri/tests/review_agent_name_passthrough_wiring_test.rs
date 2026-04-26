#[test]
fn review_commands_send_message_accepts_agent_name() {
    let source = include_str!("../src/commands/chat.rs");
    assert!(
        source.contains("agent_name: Option<String>")
            && source.contains(
                ".send_message(conversation_id, content, file_ids, permission_mode, agent_name)"
            ),
        "commands/chat.rs send_message must accept agent_name and forward it to adapter"
    );
}

#[test]
fn review_transport_send_message_preserves_agent_name_on_request() {
    let source = include_str!("../src/transport/tauri_commands/chat.rs");
    assert!(
        source.contains("agent_name: Option<String>")
            && source.contains("request.agent_name = agent_name;"),
        "transport send_message must write agent_name into ChatTurnRequest"
    );
}
