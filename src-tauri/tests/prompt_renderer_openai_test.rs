use app_lib::runtime::chat::prompt::{
    ChatPromptRenderer, PromptAssembly, PromptBlock, PromptSectionId,
};

#[test]
fn render_emits_content_array_with_static_block_cache_control() {
    let assembly = PromptAssembly::new(vec![
        PromptBlock::static_block(PromptSectionId::new("base"), "static content"),
        PromptBlock::dynamic_block(PromptSectionId::new("persona"), "dynamic content"),
        PromptBlock::volatile_block(
            PromptSectionId::new("env"),
            "volatile content",
            "test",
        ),
    ]);

    let msg = ChatPromptRenderer::render_system_message(&assembly)
        .expect("render must produce something");

    assert_eq!(msg["role"], "system");
    let content = msg["content"].as_array().expect("content should be array");
    assert!(content.len() >= 2, "should have at least 2 content blocks");
    // 第一个块应当是 static，带 cache_control
    let first = &content[0];
    assert_eq!(first["type"], "text");
    assert!(first["text"].as_str().unwrap().contains("static content"));
    assert_eq!(first["cache_control"]["type"], "ephemeral");
    // 找到包含 volatile 内容的块，应不带 cache_control
    let volatile_block = content
        .iter()
        .find(|b| b["text"].as_str().unwrap_or("").contains("volatile"))
        .expect("should find volatile block");
    assert!(volatile_block.get("cache_control").is_none(),
        "volatile block must NOT have cache_control");
}

#[test]
fn render_returns_none_for_empty_assembly() {
    let assembly = PromptAssembly::new(vec![]);
    let msg = ChatPromptRenderer::render_system_message(&assembly);
    assert!(msg.is_none());
}
