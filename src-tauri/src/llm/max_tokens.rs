//! Model-aware default `max_tokens` (output budget).
//!
//! Different providers cap `max_tokens` at very different values: Qwen / DeepSeek
//! V3 / Doubao around 8k; Claude Opus/Sonnet 64k; GPT-5 128k; DeepSeek V4 with
//! 1M context up to 384k. Sending a too-large value to a strict provider
//! returns `400 InvalidParameter Range of max_tokens` and the chat just fails.
//!
//! This is a name-based heuristic so we can pick a reasonable per-model default
//! without forcing users to configure anything. Unknown models fall back to a
//! safe 8192 cap.

/// Return a safe default for a model name. Heuristic: longest-prefix-wins via
/// ordered checks; unknown → 8192.
pub fn default_max_tokens_for_model(model: &str) -> u32 {
    let m = model.to_lowercase();

    // Anthropic Claude
    if m.contains("claude-opus") || m.contains("claude-sonnet") {
        return 64_000;
    }
    if m.contains("claude") {
        return 8_192;
    }

    // OpenAI
    if m.contains("gpt-5") {
        return 128_000;
    }
    if m.contains("gpt-4") || m.contains("o1") || m.contains("o3") {
        return 32_768;
    }

    // 智谱 GLM
    if m.contains("glm-4.5") || m.contains("glm-4.6") {
        return 96_000;
    }

    // Google Gemini
    if m.contains("gemini-2") {
        return 64_000;
    }

    // DeepSeek (V4 = 1M ctx, 384k output; V3 / R1 = 8k)
    if m.contains("deepseek-v4") || m.contains("deepseek-chat-v4") {
        return 384_000;
    }
    if m.contains("deepseek") {
        return 8_192;
    }

    // 阿里通义千问
    if m.contains("qwen-max-longcontext") {
        return 30_000;
    }
    if m.contains("qwen3") || m.contains("qwen-max-2025") {
        return 16_384;
    }
    if m.contains("qwen") {
        return 8_192;
    }

    // 字节豆包
    if m.contains("doubao-1.5") {
        return 12_288;
    }
    if m.contains("doubao") {
        return 8_192;
    }

    // Moonshot Kimi
    if m.contains("kimi") || m.contains("moonshot") {
        return 8_192;
    }

    // Unknown — safe fallback
    8_192
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn picks_64k_for_claude_sonnet_opus() {
        assert_eq!(default_max_tokens_for_model("claude-sonnet-4-6"), 64_000);
        assert_eq!(default_max_tokens_for_model("claude-opus-4-7"), 64_000);
    }

    #[test]
    fn picks_128k_for_gpt5() {
        assert_eq!(default_max_tokens_for_model("gpt-5.2-codex-high"), 128_000);
    }

    #[test]
    fn deepseek_v4_distinct_from_v3() {
        assert_eq!(default_max_tokens_for_model("deepseek-v4"), 384_000);
        assert_eq!(default_max_tokens_for_model("deepseek-chat-v4-pro"), 384_000);
        assert_eq!(default_max_tokens_for_model("deepseek-chat"), 8_192);
        assert_eq!(default_max_tokens_for_model("deepseek-reasoner"), 8_192);
    }

    #[test]
    fn qwen_family() {
        assert_eq!(default_max_tokens_for_model("qwen-plus"), 8_192);
        assert_eq!(default_max_tokens_for_model("qwen-max-longcontext"), 30_000);
        assert_eq!(default_max_tokens_for_model("qwen3-235b"), 16_384);
    }

    #[test]
    fn unknown_safe_default() {
        assert_eq!(default_max_tokens_for_model("some-future-model"), 8_192);
        assert_eq!(default_max_tokens_for_model(""), 8_192);
    }

    #[test]
    fn case_insensitive() {
        assert_eq!(default_max_tokens_for_model("Claude-Sonnet-4.5"), 64_000);
        assert_eq!(default_max_tokens_for_model("DeepSeek-V4"), 384_000);
    }
}
