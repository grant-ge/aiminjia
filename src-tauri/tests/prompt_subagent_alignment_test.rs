use app_lib::llm::streaming::ChatMessage;

#[test]
fn subagent_system_prompt_remains_openai_chat_system_message() {
    let message = ChatMessage::text("system", "parent static\n\nworker task");

    assert_eq!(message.role, "system");
    assert_eq!(message.content, "parent static\n\nworker task");
    assert!(message.tool_calls.is_none());
}
