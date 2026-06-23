const SYSTEM_PROMPT: &str = include_str!("../prompts/system.md");

#[test]
fn system_prompt_discourages_decorative_emoji() {
    assert!(
        SYSTEM_PROMPT.contains("不要使用 emoji")
            || SYSTEM_PROMPT.contains("不要使用表情")
            || SYSTEM_PROMPT.contains("不要使用装饰性图标"),
        "system prompt should explicitly discourage decorative emoji in assistant replies"
    );
}

#[test]
fn system_prompt_does_not_seed_emoji_style_examples() {
    let seeded_emoji = ["📊", "📝", "📁", "🌐", "💼"];

    for emoji in seeded_emoji {
        assert!(
            !SYSTEM_PROMPT.contains(emoji),
            "system prompt should not seed assistant style with emoji example {emoji}"
        );
    }
}
