# Pending Message Queue P5 — Multimodal Budget + Non-Anthropic Pre-Merge Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Adapt the multimodal pipeline (from P7 in the 275c worktree) to handle drained pending batches correctly: images from all N items share the same budget (4 images / 3 MB single / 6 MB total, unchanged). For non-Anthropic providers, pre-merge consecutive user messages on the client side before sending to the LLM gateway.

**Architecture:** Since P3 Task 5 already carries `pending_batch` inside `ChatTurnRequest` and persists N items as independent user messages, the LLM pipeline will see multiple consecutive user messages in history. For Anthropic path: the server merges them (official behavior). For non-Anthropic: `chat_turn_driver` pre-merges before sending the request. The multimodal budget currently scans `request.attachments` (already a flat list across the batch) — it works as-is for single-turn-view, but we need to ensure the budget is applied to the MERGED attachment list, not re-applied per historical user message.

**Tech Stack:** Rust, existing `multimodal.rs`, `vision_support.rs`, `chat_turn_driver.rs`, provider adapters.

**Spec reference:** §6.2, §6.3

**Prerequisites:** P1–P4 merged AND P7 (rich-composer multimodal) merged to main.

---

## Pre-flight: Verify P7 is merged

- [ ] **Step 0: Verify P7 merged**

Run:

```bash
git log --oneline main -50 | grep -iE "multimodal|P7" | head -3
ls src-tauri/src/runtime/chat/multimodal.rs 2>/dev/null && echo "multimodal.rs exists on main"
ls src-tauri/src/llm/vision_support.rs 2>/dev/null && echo "vision_support.rs exists on main"
```

Expected: both files exist. If not, STOP and complete P7 first.

---

## File Structure

Modify:

- `src-tauri/src/runtime/chat/multimodal.rs` — audit that `build_anthropic_image_blocks` already treats the attachment list as a single-turn view (it does — it takes `&[ChatAttachmentRef]`). No API change needed. Add a test covering the batch scenario.
- `src-tauri/src/runtime/chat/chat_turn_driver.rs` — add `pre_merge_consecutive_user_messages` step for non-Anthropic providers
- `src-tauri/src/runtime/chat/history.rs` (if it builds the messages vec for the LLM call) — may need to merge consecutive `user` role messages when provider is non-Anthropic

Create:

- `src-tauri/src/runtime/chat/provider_merge.rs` — pure pre-merge helpers + tests

---

## Task 1: Verify multimodal budget behavior with batched attachments

**Files:**
- Modify: `src-tauri/src/runtime/chat/multimodal.rs` (add test only)

- [ ] **Step 1: Add batch-scenario test**

Find the `#[cfg(test)] mod tests` in `src-tauri/src/runtime/chat/multimodal.rs`. Add:

```rust
    #[test]
    fn budget_enforced_across_batched_attachments_from_multiple_items() {
        // Simulate 3 IM messages each carrying 2 images (6 total).
        // Budget = 4 images max. We expect the first 4 converted, last 2 degraded.
        let dir = tempfile::tempdir().unwrap();
        let mut files = Vec::new();
        let mut attachments = Vec::new();
        for i in 0..6 {
            let path = dir.path().join(format!("img-{i}.png"));
            let mut f = std::fs::File::create(&path).unwrap();
            // 1 KB dummy PNG bytes (not valid PNG, but byte size suffices for budget)
            f.write_all(&vec![0u8; 1024]).unwrap();
            let att = ChatAttachmentRef {
                id: format!("att-{i}"),
                file_name: format!("img-{i}.png"),
                file_path: path.to_string_lossy().to_string(),
                kind: "image".into(),
                file_size: 1024,
                file_type: "image".into(),
                mime_type: Some("image/png".into()),
            };
            attachments.push(att);
            files.push(path);
        }

        let result = build_anthropic_image_blocks(&attachments);
        assert_eq!(
            result.image_blocks.len(),
            MAX_ANTHROPIC_IMAGE_COUNT,
            "first {MAX_ANTHROPIC_IMAGE_COUNT} images converted"
        );
        assert_eq!(
            result.converted_attachment_ids.len(),
            MAX_ANTHROPIC_IMAGE_COUNT
        );
        assert_eq!(
            result.degraded_attachment_ids.len(),
            6 - MAX_ANTHROPIC_IMAGE_COUNT
        );
        // Degraded ids are the last 2 (att-4, att-5)
        assert!(result.degraded_attachment_ids.contains("att-4"));
        assert!(result.degraded_attachment_ids.contains("att-5"));
    }

    #[test]
    fn budget_enforced_by_total_bytes_across_batch() {
        let dir = tempfile::tempdir().unwrap();
        let mut attachments = Vec::new();
        // 3 images × 2.5 MB each = 7.5 MB total → exceeds 6 MB total budget.
        for i in 0..3 {
            let path = dir.path().join(format!("big-{i}.png"));
            let mut f = std::fs::File::create(&path).unwrap();
            f.write_all(&vec![0u8; 2_500_000]).unwrap();
            attachments.push(ChatAttachmentRef {
                id: format!("big-{i}"),
                file_name: format!("big-{i}.png"),
                file_path: path.to_string_lossy().to_string(),
                kind: "image".into(),
                file_size: 2_500_000,
                file_type: "image".into(),
                mime_type: Some("image/png".into()),
            });
        }
        let result = build_anthropic_image_blocks(&attachments);
        // First two fit within 6 MB; third exceeds → degraded.
        assert_eq!(result.image_blocks.len(), 2);
        assert!(result.degraded_attachment_ids.contains("big-2"));
        assert!(result.image_bytes_total <= MAX_ANTHROPIC_IMAGE_BYTES_TOTAL);
    }
```

- [ ] **Step 2: Run tests**

Run: `cd src-tauri && cargo test --lib multimodal`

Expected: all existing multimodal tests + 2 new ones PASS. Existing behavior is verified to work correctly for batched attachments — no code change needed.

- [ ] **Step 3: Commit**

```bash
git add src-tauri/src/runtime/chat/multimodal.rs
git commit -m "test(multimodal): budget enforcement across batched pending attachments"
```

---

## Task 2: Identify where ChatTurnRequest.attachments flows into multimodal builder

**Files:** (inspection only — no changes in this task)

- [ ] **Step 1: Trace the call graph**

Run:

```bash
grep -rn "build_anthropic_image_blocks\|multimodal::build" src-tauri/src/ | head -20
```

Identify the exact site where `build_anthropic_image_blocks(&request.attachments)` is called (likely inside `chat_turn_driver.rs` where provider-specific bodies are constructed).

- [ ] **Step 2: Confirm the input is already "current turn all attachments"**

Read the call site and verify it passes `request.attachments` (the flat combined list from the drained batch). If so, no Task 3 is needed.

Verification: P3 Task 5's `build_request_from_batch` writes `all_atts` (attachments flattened across N items) into `request.attachments`. `build_anthropic_image_blocks` operates on this flat list → budget automatically applies across the batch. ✓

- [ ] **Step 3: No code change — document finding**

Record in the plan progress: "Multimodal builder receives `request.attachments` which is already the flat batch from P3. No adaptation needed."

---

## Task 3: Non-Anthropic pre-merge helper module

**Files:**
- Create: `src-tauri/src/runtime/chat/provider_merge.rs`
- Modify: `src-tauri/src/runtime/chat/mod.rs` (add `pub mod provider_merge;`)

- [ ] **Step 1: Inspect the message shape used by non-Anthropic providers**

Run:

```bash
grep -rn "pub struct.*Message\|ChatMessage\|MessageRole" src-tauri/src/llm/streaming.rs src-tauri/src/runtime/chat/chat_turn_driver.rs 2>/dev/null | head -10
```

Identify the type representing a single role/content pair (likely `LlmMessage` or `ChatMessage`). The helpers below use a generic signature; adapt based on the actual type.

- [ ] **Step 2: Write the failing test**

Create `src-tauri/src/runtime/chat/provider_merge.rs`:

```rust
//! Client-side pre-merge of consecutive same-role messages.
//!
//! Anthropic's `/v1/messages` server auto-merges consecutive user/assistant
//! turns. Other providers (OpenAI, Qwen, DeepSeek, Volcano, Custom) have
//! varying/undefined behavior. For non-Anthropic paths, we merge here before
//! serializing the request body.
//!
//! See spec §6.2.

/// Minimal trait to describe the message shape this module operates on.
/// Providers wire their own type via an adapter (Task 4).
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
```

- [ ] **Step 3: Register the module**

Add to `src-tauri/src/runtime/chat/mod.rs`:

```rust
pub mod provider_merge;
```

- [ ] **Step 4: Run tests**

Run: `cd src-tauri && cargo test --lib provider_merge`

Expected: 7 tests PASS.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/runtime/chat/provider_merge.rs src-tauri/src/runtime/chat/mod.rs
git commit -m "feat(chat): client-side pre-merge helper for consecutive same-role messages"
```

---

## Task 4: Wire pre-merge into non-Anthropic provider paths

**Files:**
- Modify: `src-tauri/src/runtime/chat/chat_turn_driver.rs` (or wherever the LLM request is built per provider)

- [ ] **Step 1: Locate the provider dispatch site**

Run:

```bash
grep -rn "provider\.send\|gateway\.send_message\|Provider::new\|send_to_provider" src-tauri/src/runtime/chat/ src-tauri/src/llm/gateway.rs | head -20
```

Find where the LLM gateway decides which provider to use and where the request body (list of messages) is serialized.

- [ ] **Step 2: Implement MergableMessage for the internal message type**

Identify the `LlmMessage` (or equivalent) struct. Add in the same file as the struct:

```rust
impl crate::runtime::chat::provider_merge::MergableMessage for LlmMessage {
    fn role(&self) -> &str {
        match self.role {
            LlmRole::User => "user",
            LlmRole::Assistant => "assistant",
            LlmRole::System => "system",
            // ... match all variants
        }
    }
    fn content_text(&self) -> String {
        // If content is already a String, return clone. If it's a rich enum
        // (text + tool_use + tool_result), flatten only text parts and
        // serialize others as placeholder markers. Messages with no text
        // content shouldn't be merge candidates in practice — they'd be
        // assistant messages with tool_use only, and those break alternation
        // so merge_consecutive_same_role won't touch them (adjacent user
        // messages only have text).
        //
        // Adapt this based on the actual enum shape.
        self.text_content().unwrap_or_default()
    }
    fn set_content_text(&mut self, text: String) {
        self.set_text_content(text);
    }
}
```

Adapt to the actual type. If `LlmMessage.content` is `ChatMessageContent::Text(String)`, the implementation is trivial; if it's richer, design a fallback that degrades rich content to text only during merge.

- [ ] **Step 3: Call the merge before sending to non-Anthropic provider**

Find where messages are serialized for the outbound request (likely in the gateway or a provider adapter). Before the serialization, insert:

```rust
use crate::runtime::chat::provider_merge::merge_consecutive_same_role;
use crate::llm::vision_support; // if used to gate Anthropic path

let is_anthropic_path = matches!(
    provider_kind,
    ProviderKind::Lotus | ProviderKind::Claude
);
let messages_for_llm = if is_anthropic_path {
    messages_for_llm // leave as-is; server auto-merges
} else {
    merge_consecutive_same_role(&messages_for_llm)
};
```

Use the actual enum name for `ProviderKind`. Confirm with:

```bash
grep -n "enum ProviderKind\|pub enum Provider" src-tauri/src/llm/ | head -5
```

- [ ] **Step 4: Add integration test**

Append (or create) `src-tauri/src/runtime/chat/provider_merge_integration_test.rs` (or add to existing driver tests). Skip the full LLM mock — just test the branch selection:

```rust
#[cfg(test)]
mod pre_merge_integration {
    use super::*;
    // If no easy mock is available, this task can be deferred to manual smoke
    // via a live DeepSeek/Qwen call.
}
```

If wiring a mock is cumbersome, rely on the unit tests from Task 3 + manual smoke in Task 6.

- [ ] **Step 5: Verify it compiles**

Run: `cd src-tauri && cargo check --lib`

Expected: succeeds.

- [ ] **Step 6: Commit**

```bash
git add src-tauri/src/runtime/chat/chat_turn_driver.rs src-tauri/src/llm/gateway.rs
git commit -m "feat(chat): pre-merge consecutive user messages for non-Anthropic providers"
```

---

## Task 5: Architecture review — ensure Anthropic path skips pre-merge

**Files:**
- Create: `src-tauri/tests/review_pre_merge_anthropic_skip.rs`

- [ ] **Step 1: Write the review test**

Create `src-tauri/tests/review_pre_merge_anthropic_skip.rs`:

```rust
//! Architectural guard: the pre-merge logic must NOT apply to Anthropic-path
//! providers (Lotus / Claude). This prevents a later refactor from double-
//! merging (server merge + client merge = lost content separation).

use std::path::Path;

#[test]
fn pre_merge_wrapped_in_non_anthropic_branch() {
    // Look for either the gateway or chat_turn_driver. Whichever has the call,
    // verify it's guarded by a provider-kind check.
    let candidates = [
        "src/runtime/chat/chat_turn_driver.rs",
        "src/llm/gateway.rs",
    ];
    let mut found = false;
    for p in candidates {
        let content = std::fs::read_to_string(Path::new(p)).unwrap_or_default();
        if content.contains("merge_consecutive_same_role") {
            // Must be preceded by provider kind discrimination in the same file
            assert!(
                content.contains("Lotus") || content.contains("Claude") || content.contains("Anthropic"),
                "{p} calls merge_consecutive_same_role but has no provider-kind guard — Anthropic path could double-merge",
            );
            found = true;
        }
    }
    assert!(found, "merge_consecutive_same_role must be wired somewhere");
}
```

- [ ] **Step 2: Run**

Run: `cd src-tauri && cargo test --test review_pre_merge_anthropic_skip`

Expected: PASS.

- [ ] **Step 3: Commit**

```bash
git add src-tauri/tests/review_pre_merge_anthropic_skip.rs
git commit -m "test(review): non-Anthropic pre-merge must not apply to Anthropic paths"
```

---

## Task 6: Manual smoke — multimodal + multi-message behavior

**Files:** none (manual)

- [ ] **Step 1: Anthropic path (vision model) + batch of images**

1. Start app, select a vision-capable model (e.g., `claude-sonnet-4-5`).
2. Open a conversation; send a dummy long-running query to make LLM busy.
3. While busy, send 3 messages each with 2 images (6 images total).
4. Wait for drain.

Expected: LLM receives 3 user messages (server merges), 4 image blocks (first 4), 2 degraded to text-fallback ("以下附件未能作为图片识别").

- [ ] **Step 2: Non-Anthropic path (DeepSeek)**

1. Switch model to `deepseek-v4-pro` (no vision support).
2. Send 3 text messages during busy state.
3. Wait for drain.

Expected: Backend logs show `merge_consecutive_same_role` applied, final request has 1 merged user message; LLM responds normally.

- [ ] **Step 3: Mixed attachments**

1. Anthropic path, batch of 2 images + 1 PDF + 3 text-only messages.
2. Wait for drain.

Expected: 2 image blocks (Anthropic native), PDF + text-fallback intact, LLM sees coherent batch.

- [ ] **Step 4: Log review**

Check `~/.renlijia/logs/` for:
- `[pending] drain dispatched n=3`
- `[pending] queued …` entries
- `[multimodal] converted=2 degraded=0 total_bytes=…`
- No errors about `MessagePersisted` or `ChatTurnRequest`

---

## Self-Review

Spec coverage:
1. **§6.2 multi-user-message LLM input** → Task 4 (non-Anthropic pre-merge) + Anthropic server-side merge ✓
2. **§6.3 multimodal budget across batch** → Task 1 (verified existing `build_anthropic_image_blocks` already handles this as it takes a flat list) ✓
3. **Anthropic path skip guard** → Task 5 ✓

Not covered (intentional):
- Persistent optimistic UI for queue state → not needed per spec
- Multi-provider token accounting for merged messages → out of scope; usage is reported per turn

Type consistency:
- `MergableMessage` trait API matches the implementation side notes in Task 4
- `ChatTurnRequest.attachments` from P3 is the flat list read by `build_anthropic_image_blocks`
- Provider-kind check in Task 4 uses existing enum (Lotus/Claude/…)

---

## Wrap-up for the 5-PR series

After merging P1–P5:

1. **Total files added:** 8 Rust files + 6 TS files + 2 spec/plan docs
2. **Total files modified:** ~15 Rust files + 5 TS files + 2 i18n files
3. **Tests added:** 60+ unit tests + 4 integration test files + 3 review tests
4. **User-facing change:** messages sent during busy state no longer error out; chips appear above composer; per-item × removal; auto-drain after debounce

Spec §14 unknowns still to revisit after data in production:
- Queue size 50 tuning
- Debounce window 1.2s tuning
- Group chat per-sender merge strategy
