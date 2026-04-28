const BASE_PROMPT: &str = include_str!("../prompts/base.md");
const DAILY_PROMPT: &str = include_str!("../prompts/daily.md");

#[test]
fn base_prompt_discourages_decorative_emoji() {
    assert!(
        BASE_PROMPT.contains("不要使用 emoji")
            || BASE_PROMPT.contains("不要使用表情")
            || BASE_PROMPT.contains("不要使用装饰性图标"),
        "base prompt should explicitly discourage decorative emoji in assistant replies"
    );
}

#[test]
fn daily_prompt_does_not_seed_emoji_style_examples() {
    let seeded_emoji = ["📊", "📝", "📁", "🌐", "💼"];

    for emoji in seeded_emoji {
        assert!(
            !DAILY_PROMPT.contains(emoji),
            "daily prompt should not seed assistant style with emoji example {emoji}"
        );
    }
}
