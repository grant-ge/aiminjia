# Auto Title Generation — Design Spec

Date: 2026-04-24

## Overview

After the first AI reply in a conversation completes, automatically generate a concise title (6-12 Chinese characters) via a lightweight non-streaming LLM call, replacing the default `"New Conversation"` placeholder. The feature is fully transparent to the user: the sidebar title updates silently; any failure is swallowed and logged.

---

## Trigger Conditions

Generate the title if and only if **all** of the following are true when the first-turn `TurnCompleted` event fires:

1. The conversation's stored title is exactly `"New Conversation"` (i.e. the user has not renamed it).
2. At least one user message and at least one assistant message exist in storage for this conversation.
3. The turn outcome is `Success` or `MaxIterationsReached` (not `Cancelled` / `ExecutionError`).

These checks are evaluated **after** `StreamDone` and `TurnCompleted` have been emitted, so the main reply is already fully delivered before the title call starts.

---

## Architecture

### Placement

A new async function `generate_and_set_title` is added to **`src-tauri/src/runtime/conversation_service.rs`**. It is called from the turn-completion path inside **`TauriLegacyTurnExecutor`** (the `send_message` impl in `transport/tauri_commands/chat.rs`), after all runtime events have been emitted.

Calling site pseudo-code:

```
// After run_chat_turn returns Ok(()) and all events are flushed:
if turn_is_first_success(outcome) {
    tokio::spawn(generate_and_set_title(
        services.db.clone(),
        services.gateway.clone(),
        services.app.clone(),
        conversation_id,
    ));
}
```

The call is `tokio::spawn`ed so it does not block or delay the response to the frontend.

### Turn-is-first check

Rather than tracking turn_counter end-to-end through the driver, the caller performs a lightweight check: load the conversation's stored title and count messages in storage. Both are cheap file reads already available in the call context.

---

## LLM Call

**Function:** `LlmGateway::send_message` (non-streaming, already exists)

**Messages:**

```json
[
  {
    "role": "user",
    "content": "<first user message text, truncated to 500 chars>"
  },
  {
    "role": "assistant",
    "content": "<first assistant reply text, truncated to 500 chars>"
  }
]
```

**System prompt:**

```
你是一个对话标题生成器。根据下面的对话内容，用 6 到 12 个中文字生成一个简洁的标题，直接输出标题文字，不加引号、不加标点、不加解释。
```

**Parameters:**

- `max_tokens`: 32 (titles are short)
- `tool_defs_override`: `Some(vec![])` (no tools)
- `masking_level`: `MaskingLevel::None`
- `context_message`: `None`
- The call uses whatever model the current routing selects for `TaskType::Simple` (same as other non-streaming calls).

---

## Title Sanitization

After receiving the LLM response, apply before saving:

1. Strip leading/trailing whitespace.
2. Remove any surrounding quotation marks (`"`, `"`, `"`, `'`, `'`).
3. If the result contains a newline, keep only the first line.
4. Truncate to 30 characters maximum.
5. If the sanitized result is empty, abort without updating.

---

## Storage & Event

Reuse existing infrastructure without modification:

- **Store:** `ConversationStore::rename_conversation(id, title)` — already idempotent.
- **Event:** `app.emit("conversation:title-updated", { conversationId, title })` — frontend `App.tsx:145` already listens and updates the store.

No new Tauri command, no new IPC surface, no schema change.

---

## Error Handling

All errors are non-fatal:

| Error | Behavior |
|---|---|
| LLM call fails (network / auth / timeout) | `log::warn`, return early, title stays `"New Conversation"` |
| Sanitized title is empty | `log::warn`, return early |
| `rename_conversation` fails | `log::warn`, return early |
| `app.emit` fails | ignored (already `let _ =` convention in codebase) |

No user-visible error toast or notification.

---

## Files to Create / Modify

| File | Change |
|---|---|
| `src-tauri/src/runtime/conversation_service.rs` | Add `pub async fn generate_and_set_title(...)` |
| `src-tauri/src/transport/tauri_commands/chat.rs` | Call `generate_and_set_title` via `tokio::spawn` after first successful turn |

No other files need to change.

---

## Out of Scope

- Regenerating titles after subsequent turns.
- Titles in languages other than Chinese (the system prompt can be evolved later).
- Frontend "loading" indicator during title generation.
- User preference to disable auto-title.
- Exposing the title model as a separate setting.
