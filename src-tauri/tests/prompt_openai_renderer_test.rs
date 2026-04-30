use app_lib::llm::streaming::ChatMessage;
use app_lib::runtime::chat::prompt::{
    OpenAiChatPromptRenderer, PromptAssembly, PromptBlock, PromptSectionId,
};

fn sample_prompt_assembly() -> PromptAssembly {
    PromptAssembly::new(vec![
        PromptBlock::static_block(PromptSectionId::new("base"), "base instructions"),
        PromptBlock::dynamic_block(PromptSectionId::new("persona"), "persona instructions"),
        PromptBlock::volatile_block(
            PromptSectionId::new("reminder"),
            "turn reminder",
            "changes each turn",
        ),
        PromptBlock::dynamic_block(PromptSectionId::new("empty"), "   "),
    ])
}

#[test]
fn renderer_flattens_prompt_assembly_into_single_system_message() {
    let rendered = OpenAiChatPromptRenderer::render_system_message(&sample_prompt_assembly())
        .expect("non-empty assembly should render");

    assert_eq!(rendered["role"], "system");
    assert_eq!(
        rendered["content"],
        "base instructions\n\npersona instructions\n\nturn reminder"
    );
}

#[test]
fn renderer_does_not_emit_anthropic_private_fields() {
    let rendered = OpenAiChatPromptRenderer::render_system_message(&sample_prompt_assembly())
        .expect("non-empty assembly should render");
    let object = rendered.as_object().expect("renderer must emit object");

    assert!(object.get("cache_control").is_none());
    assert!(object.get("type").is_none());
    assert!(object.get("blocks").is_none());
    assert_eq!(object.len(), 2);
    assert!(object.contains_key("role"));
    assert!(object.contains_key("content"));
}

#[test]
fn rendered_system_message_deserializes_to_chat_message() {
    let rendered = OpenAiChatPromptRenderer::render_system_message(&sample_prompt_assembly())
        .expect("non-empty assembly should render");
    let message: ChatMessage = serde_json::from_value(rendered).expect("valid ChatMessage");

    assert_eq!(message.role, "system");
    assert_eq!(
        message.content,
        "base instructions\n\npersona instructions\n\nturn reminder"
    );
    assert!(message.tool_calls.is_none());
    assert!(message.tool_call_id.is_none());
    assert!(message.name.is_none());
}
