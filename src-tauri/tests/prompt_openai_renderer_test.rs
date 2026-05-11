use app_lib::llm::streaming::ChatMessage;
use app_lib::runtime::chat::prompt::{
    ChatPromptRenderer, PromptAssembly, PromptBlock, PromptSectionId,
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
fn renderer_emits_multi_block_content_array_with_cache_control() {
    let rendered = ChatPromptRenderer::render_system_message(&sample_prompt_assembly())
        .expect("non-empty assembly should render");

    assert_eq!(rendered["role"], "system");
    let content = rendered["content"]
        .as_array()
        .expect("content should be an array (multi-block prompt cache shape)");

    // 4 个 block 中 1 个 empty 被过滤，剩 3 个：base / persona / reminder
    assert_eq!(content.len(), 3, "empty block should be filtered out");

    // base: StaticPrefix → cache_control: ephemeral
    assert_eq!(content[0]["type"], "text");
    assert_eq!(content[0]["text"], "base instructions");
    assert_eq!(content[0]["cache_control"]["type"], "ephemeral");

    // persona: SessionDynamic → cache_control: ephemeral
    assert_eq!(content[1]["type"], "text");
    assert_eq!(content[1]["text"], "persona instructions");
    assert_eq!(content[1]["cache_control"]["type"], "ephemeral");

    // reminder: Volatile → no cache_control
    assert_eq!(content[2]["type"], "text");
    assert_eq!(content[2]["text"], "turn reminder");
    assert!(
        content[2].get("cache_control").is_none(),
        "Volatile block must NOT carry cache_control"
    );
}

#[test]
fn renderer_does_not_emit_anthropic_private_fields() {
    let rendered = ChatPromptRenderer::render_system_message(&sample_prompt_assembly())
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
fn flat_renderer_output_round_trips_into_chat_message() {
    // 降级 flat 路径用于不支持 content 数组的 OpenAI 兼容端点；
    // 输出 content 仍是单字符串，可以直接反序列化成 ChatMessage。
    let rendered = ChatPromptRenderer::render_system_message_flat(&sample_prompt_assembly())
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
