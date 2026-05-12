//! Client-side pre-merge of consecutive same-role messages.
//!
//! Anthropic's `/v1/messages` server auto-merges consecutive user/assistant
//! turns. Other providers (OpenAI, Qwen, DeepSeek, Volcano, Custom) have
//! varying/undefined behavior. For non-Anthropic paths, we merge here before
//! serializing the request body.
//!
//! See spec §6.2.

/// Minimal trait to describe the message shape this module operates on.
/// Providers wire their own type via an adapter at the dispatch site.
pub trait MergableMessage: Sized {
    /// Role string, e.g. "user" / "assistant" / "system".
    fn role(&self) -> &str;
    /// Mutable text content. If the message has rich content blocks, provider
    /// adapter must flatten to text for the merge.
    fn content_text(&self) -> String;
    /// Set the text content (used only when merging).
    fn set_content_text(&mut self, text: String);
}

/// Returns a new Vec where consecutive same-role messages are merged.
/// First message wins its metadata (role, non-text fields); text is joined by "\n".
pub fn merge_consecutive_same_role<M: MergableMessage + Clone>(messages: &[M]) -> Vec<M> {
    let mut out: Vec<M> = Vec::with_capacity(messages.len());
    for msg in messages {
        if let Some(last) = out.last_mut() {
            if last.role() == msg.role() {
                let mut combined = last.content_text();
                combined.push('\n');
                combined.push_str(&msg.content_text());
                last.set_content_text(combined);
                continue;
            }
        }
        out.push(msg.clone());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Clone, Debug, PartialEq, Eq)]
    struct FakeMsg {
        role: String,
        text: String,
    }

    impl MergableMessage for FakeMsg {
        fn role(&self) -> &str {
            &self.role
        }
        fn content_text(&self) -> String {
            self.text.clone()
        }
        fn set_content_text(&mut self, text: String) {
            self.text = text;
        }
    }

    fn msg(role: &str, text: &str) -> FakeMsg {
        FakeMsg {
            role: role.into(),
            text: text.into(),
        }
    }

    #[test]
    fn merges_two_consecutive_user_messages() {
        let input = vec![msg("user", "hello"), msg("user", "world")];
        let out = merge_consecutive_same_role(&input);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].text, "hello\nworld");
    }

    #[test]
    fn merges_three_consecutive_same_role() {
        let input = vec![msg("user", "a"), msg("user", "b"), msg("user", "c")];
        let out = merge_consecutive_same_role(&input);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].text, "a\nb\nc");
    }

    #[test]
    fn preserves_alternation() {
        let input = vec![
            msg("user", "q1"),
            msg("assistant", "a1"),
            msg("user", "q2"),
            msg("assistant", "a2"),
        ];
        let out = merge_consecutive_same_role(&input);
        assert_eq!(out.len(), 4);
        assert_eq!(out[0].text, "q1");
        assert_eq!(out[2].text, "q2");
    }

    #[test]
    fn merges_consecutive_assistant_messages() {
        let input = vec![msg("assistant", "part1"), msg("assistant", "part2")];
        let out = merge_consecutive_same_role(&input);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].text, "part1\npart2");
    }

    #[test]
    fn mixed_groups() {
        let input = vec![
            msg("user", "u1"),
            msg("user", "u2"),
            msg("assistant", "a1"),
            msg("user", "u3"),
        ];
        let out = merge_consecutive_same_role(&input);
        assert_eq!(out.len(), 3);
        assert_eq!(out[0].text, "u1\nu2");
        assert_eq!(out[1].text, "a1");
        assert_eq!(out[2].text, "u3");
    }

    #[test]
    fn empty_input_returns_empty() {
        let input: Vec<FakeMsg> = vec![];
        let out = merge_consecutive_same_role(&input);
        assert!(out.is_empty());
    }

    #[test]
    fn single_message_unchanged() {
        let input = vec![msg("user", "solo")];
        let out = merge_consecutive_same_role(&input);
        assert_eq!(out, input);
    }
}
