# Auto Title Generation Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** After the first AI reply completes, automatically generate a concise 6-12 character Chinese title for the conversation using a lightweight non-streaming LLM call.

**Architecture:** Add `sanitize_title`, `should_auto_title`, and `generate_and_set_title` to `conversation_service.rs`. After `run_chat_request` succeeds in `TauriChatCommandAdapter::send_message`, load the current LLM settings, then `tauri::async_runtime::spawn` the title generation. On success, reuse existing `rename_conversation` + `conversation:title-updated` infrastructure so the frontend sidebar updates automatically.

**Tech Stack:** Rust async (tokio), `LlmGateway::send_message` (non-streaming), `ConversationStore::rename_conversation`, `tauri::AppHandle::emit`, existing `ChatMessage::text` constructor, `build_gateway_settings` + `load_llm_settings` for real user config.

---

### Task 1: Add helpers to `conversation_service.rs`

**Files:**
- Modify: `src-tauri/src/runtime/conversation_service.rs`

- [ ] **Step 1: Write the failing unit tests**

Add this test module at the bottom of `src-tauri/src/runtime/conversation_service.rs`:

```rust
#[cfg(test)]
mod title_tests {
    use super::*;
    use crate::runtime::store::conversation_store::{ConversationStore, InMemoryConversationStore};
    use std::sync::Arc;

    /// Minimal mock that extends InMemoryConversationStore with a seeded message list.
    struct StoreWithMessages {
        inner: InMemoryConversationStore,
        messages: std::sync::Mutex<Vec<serde_json::Value>>,
        conversation_id: String,
    }

    impl StoreWithMessages {
        fn new(title: &str, conversation_id: &str, msgs: Vec<serde_json::Value>) -> Arc<Self> {
            let inner = InMemoryConversationStore::new();
            inner.create_conversation(conversation_id, title).unwrap();
            Arc::new(Self {
                inner,
                messages: std::sync::Mutex::new(msgs),
                conversation_id: conversation_id.to_string(),
            })
        }
    }

    impl ConversationStore for StoreWithMessages {
        fn create_conversation(&self, id: &str, title: &str) -> anyhow::Result<()> {
            self.inner.create_conversation(id, title)
        }
        fn list_conversation_ids(&self) -> anyhow::Result<Vec<String>> {
            self.inner.list_conversation_ids()
        }
        fn get_conversations(&self) -> anyhow::Result<Vec<serde_json::Value>> {
            self.inner.get_conversations()
        }
        fn delete_conversation(&self, id: &str) -> anyhow::Result<()> {
            self.inner.delete_conversation(id)
        }
        fn rename_conversation(&self, id: &str, new_title: &str) -> anyhow::Result<()> {
            self.inner.rename_conversation(id, new_title)
        }
        fn insert_active_task(&self, id: &str) -> anyhow::Result<()> {
            self.inner.insert_active_task(id)
        }
        fn remove_active_task(&self, id: &str) -> anyhow::Result<()> {
            self.inner.remove_active_task(id)
        }
        fn get_messages(&self, conversation_id: &str) -> anyhow::Result<Vec<serde_json::Value>> {
            if conversation_id == self.conversation_id {
                Ok(self.messages.lock().unwrap().clone())
            } else {
                Ok(vec![])
            }
        }
        fn append_compact_boundary(
            &self,
            record: crate::runtime::chat::compaction::CompactBoundaryRecord,
        ) -> anyhow::Result<()> {
            self.inner.append_compact_boundary(record)
        }
        fn list_compact_boundaries(
            &self,
            id: &str,
        ) -> anyhow::Result<Vec<crate::runtime::chat::compaction::CompactBoundaryRecord>> {
            self.inner.list_compact_boundaries(id)
        }
        fn get_conversation_model_override(&self, id: &str) -> anyhow::Result<Option<String>> {
            self.inner.get_conversation_model_override(id)
        }
        fn set_conversation_model_override(
            &self,
            id: &str,
            v: Option<String>,
        ) -> anyhow::Result<()> {
            self.inner.set_conversation_model_override(id, v)
        }
        fn archive_conversation(&self, id: &str) -> anyhow::Result<()> {
            self.inner.archive_conversation(id)
        }
        fn get_archived_conversations(&self) -> anyhow::Result<Vec<serde_json::Value>> {
            self.inner.get_archived_conversations()
        }
    }

    #[test]
    fn sanitize_title_strips_quotes_and_whitespace() {
        assert_eq!(sanitize_title("  「标题」  "), "标题");
        assert_eq!(sanitize_title("\"测试标题\""), "测试标题");
        assert_eq!(sanitize_title("'hello'\nworld"), "hello");
        assert_eq!(sanitize_title(""), "");
    }

    #[test]
    fn sanitize_title_truncates_long_output() {
        let long = "一二三四五六七八九十一二三四五六七八九十一二三四五六七八九十X";
        let result = sanitize_title(long);
        assert!(result.chars().count() <= 30);
    }

    #[test]
    fn should_auto_title_returns_false_when_already_titled() {
        let store = StoreWithMessages::new(
            "已有标题",
            "conv1",
            vec![
                serde_json::json!({"role": "user",      "content": {"text": "hello"}}),
                serde_json::json!({"role": "assistant",  "content": {"text": "hi"}}),
            ],
        );
        assert!(!should_auto_title(store.as_ref() as &dyn ConversationStore, "conv1").unwrap());
    }

    #[test]
    fn should_auto_title_returns_false_when_no_assistant_message() {
        let store = StoreWithMessages::new(
            "New Conversation",
            "conv1",
            vec![serde_json::json!({"role": "user", "content": {"text": "hello"}})],
        );
        assert!(!should_auto_title(store.as_ref() as &dyn ConversationStore, "conv1").unwrap());
    }

    #[test]
    fn should_auto_title_returns_true_when_conditions_met() {
        let store = StoreWithMessages::new(
            "New Conversation",
            "conv1",
            vec![
                serde_json::json!({"role": "user",      "content": {"text": "hello"}}),
                serde_json::json!({"role": "assistant",  "content": {"text": "hi"}}),
            ],
        );
        assert!(should_auto_title(store.as_ref() as &dyn ConversationStore, "conv1").unwrap());
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

```bash
cd src-tauri && cargo test title_tests -- --nocapture 2>&1 | tail -20
```

Expected: compile error — `sanitize_title` and `should_auto_title` not defined yet.

- [ ] **Step 3: Add required imports to `conversation_service.rs`**

Add these lines at the top of the file, after the existing `use` block:

```rust
use crate::llm::masking::MaskingLevel;
use crate::llm::streaming::ChatMessage;
use crate::models::settings::AppSettings;
use tauri::AppHandle;
```

- [ ] **Step 4: Add `sanitize_title`**

Add after the existing `rename_conversation` function:

```rust
pub fn sanitize_title(raw: &str) -> String {
    let line = raw.lines().next().unwrap_or("").trim();
    let trimmed = line.trim_matches(|c: char| {
        matches!(
            c,
            '"' | '\''
                | '\u{201C}'
                | '\u{201D}'
                | '\u{2018}'
                | '\u{2019}'
                | '「'
                | '」'
                | '『'
                | '』'
        )
    });
    trimmed.trim().chars().take(30).collect()
}
```

- [ ] **Step 5: Add `should_auto_title`**

Add after `sanitize_title`:

```rust
pub fn should_auto_title(
    db: &dyn crate::runtime::store::conversation_store::ConversationStore,
    conversation_id: &str,
) -> anyhow::Result<bool> {
    let convs = db.get_conversations()?;
    let title = convs
        .iter()
        .find(|c| c["id"].as_str() == Some(conversation_id))
        .and_then(|c| c["title"].as_str().map(|s| s.to_string()))
        .unwrap_or_default();

    if title != "New Conversation" {
        return Ok(false);
    }

    let messages = db.get_messages(conversation_id)?;
    let has_user = messages.iter().any(|m| m["role"].as_str() == Some("user"));
    let has_assistant = messages
        .iter()
        .any(|m| m["role"].as_str() == Some("assistant"));
    Ok(has_user && has_assistant)
}
```

- [ ] **Step 6: Add `generate_and_set_title`**

Add after `should_auto_title`. Note: `settings` is passed in from the call site so it carries the real user API key and model config.

```rust
/// Fire-and-forget entry point. All errors are logged and swallowed.
pub async fn generate_and_set_title(
    db: Arc<dyn crate::runtime::store::conversation_store::ConversationStore>,
    gateway: Arc<LlmGateway>,
    app: AppHandle,
    conversation_id: String,
    settings: AppSettings,
) {
    match generate_and_set_title_inner(db, gateway, app, conversation_id, settings).await {
        Ok(()) => {}
        Err(e) => log::warn!("[auto-title] failed: {}", e),
    }
}

async fn generate_and_set_title_inner(
    db: Arc<dyn crate::runtime::store::conversation_store::ConversationStore>,
    gateway: Arc<LlmGateway>,
    app: AppHandle,
    conversation_id: String,
    settings: AppSettings,
) -> anyhow::Result<()> {
    if !should_auto_title(db.as_ref(), &conversation_id)? {
        return Ok(());
    }

    let messages = db.get_messages(&conversation_id)?;

    let extract_text = |m: &serde_json::Value| -> String {
        m["content"]["text"]
            .as_str()
            .or_else(|| m["content"].as_str())
            .unwrap_or("")
            .chars()
            .take(500)
            .collect()
    };

    let first_user: String = messages
        .iter()
        .find(|m| m["role"].as_str() == Some("user"))
        .map(extract_text)
        .unwrap_or_default();

    let first_assistant: String = messages
        .iter()
        .find(|m| m["role"].as_str() == Some("assistant"))
        .map(extract_text)
        .unwrap_or_default();

    if first_user.is_empty() {
        anyhow::bail!("no user message content found");
    }

    let llm_messages = vec![
        ChatMessage::text("user", &first_user),
        ChatMessage::text("assistant", &first_assistant),
    ];

    let system_prompt =
        "你是一个对话标题生成器。根据下面的对话内容，用 6 到 12 个中文字生成一个简洁的标题，\
         直接输出标题文字，不加引号、不加标点、不加解释。";

    let response = gateway
        .send_message(
            &settings,
            llm_messages,
            MaskingLevel::None,
            Some(system_prompt),
            None,
            Some(vec![]),
        )
        .await?;

    let title = sanitize_title(&response.content);
    if title.is_empty() {
        anyhow::bail!("sanitized title is empty");
    }

    let outcome = rename_conversation(db, conversation_id, title).await?;

    let _ = app.emit(
        "conversation:title-updated",
        serde_json::json!({
            "conversationId": outcome.conversation_id,
            "title": outcome.new_title,
        }),
    );

    log::info!("[auto-title] set title: {}", outcome.new_title);
    Ok(())
}
```

- [ ] **Step 7: Run tests to verify they pass**

```bash
cd src-tauri && cargo test title_tests -- --nocapture 2>&1 | tail -20
```

Expected: 5 tests pass.

- [ ] **Step 8: Commit**

```bash
git add src-tauri/src/runtime/conversation_service.rs
git commit -m "feat(auto-title): add generate_and_set_title to conversation_service"
```

---

### Task 2: Wire into `send_message` in `chat.rs`

**Files:**
- Modify: `src-tauri/src/transport/tauri_commands/chat.rs` (around line 1654)

- [ ] **Step 1: Modify `send_message`**

Locate `pub async fn send_message` at line ~1654 in `chat.rs`. Replace the whole function:

```rust
pub async fn send_message(
    &self,
    conversation_id: String,
    content: String,
    file_ids: Vec<String>,
    agent_name: Option<String>,
) -> Result<(), String> {
    let mut request = ChatTurnRequest::new(conversation_id.clone(), content, file_ids);
    if let Some(agent_name) = agent_name {
        request = request.with_agent_name(agent_name);
    }
    let result = self.runtime.run_chat_request(request).await;

    if result.is_ok() {
        // Load settings now (in the async context where self is available) so
        // the spawned task carries the real user model/key config.
        if let Ok(resolved) = self.load_llm_settings().await {
            let db = self.services.db.clone() as Arc<dyn ConversationStore>;
            let gateway = self.services.gateway.clone();
            let app = self.services.app.clone();
            let conv_id = conversation_id.clone();
            let settings = build_gateway_settings(&resolved);
            tauri::async_runtime::spawn(async move {
                conversation_service::generate_and_set_title(
                    db, gateway, app, conv_id, settings,
                )
                .await;
            });
        }
    }

    result
}
```

- [ ] **Step 2: Verify it compiles**

```bash
cd src-tauri && cargo check 2>&1 | grep "^error" | head -20
```

Expected: no errors.

- [ ] **Step 3: Run regression tests**

```bash
cd src-tauri && cargo test review_ --tests --no-fail-fast 2>&1 | tail -20
```

Expected: all pass.

- [ ] **Step 4: Commit**

```bash
git add src-tauri/src/transport/tauri_commands/chat.rs
git commit -m "feat(auto-title): spawn title generation after first successful turn"
```

---

### Task 3: Manual smoke test

- [ ] **Step 1: Start the dev server**

```bash
pnpm tauri:dev
```

- [ ] **Step 2: Create a new conversation and send a message**

Open a new conversation (sidebar shows "New Conversation"), send any message, wait for the AI reply to finish.

- [ ] **Step 3: Verify the sidebar title updates**

Within a few seconds of the reply finishing, the sidebar title should change from "New Conversation" to a 6-12 character Chinese title. Confirm no error toast appears.

- [ ] **Step 4: Verify subsequent turns do not change the title**

Send a second message in the same conversation. Confirm the title remains unchanged.

- [ ] **Step 5: Verify manually renamed title is not overwritten**

Create a new conversation, right-click → rename to "我的测试对话", then send a message. Confirm the title stays "我的测试对话" after the AI replies.

- [ ] **Step 6: Commit smoke test sign-off**

```bash
git commit --allow-empty -m "test(auto-title): manual smoke test passed"
```
