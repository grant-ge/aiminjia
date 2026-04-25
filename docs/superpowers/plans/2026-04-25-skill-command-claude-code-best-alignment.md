# Skill Command Claude Code Best Alignment Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Align Lotus explicit skill selection with Claude Code Best semantics so a selected skill is a deterministic command invocation, not a model-guessed `switch_skill`.

**Architecture:** Treat `selected_skill_id` as a command-level fact at the backend runtime boundary. A selected skill directly resolves the skill turn context, persists visible command metadata, injects the selected skill prompt/tools into the turn, and removes `switch_skill` from that explicit-skill turn so the model cannot re-select a different skill. Automatic skill detection and model-driven `switch_skill` remain available only when the user did not explicitly select a skill.

**Tech Stack:** React + Zustand + Vitest on frontend; Tauri IPC + Rust async runtime + existing `SkillSessionStore`/`SkillRegistry`/`TurnConfigOverrides` on backend; file-store JSONL conversations and memory-backed active skill state.

---

## Claude Code Best Reference

Claude Code Best does not treat a user-invoked skill as an intent that the model must rediscover. The relevant reference points are:

- `/Users/a20250311/github/claude-code-best/src/utils/processUserInput/processSlashCommand.tsx`
  - `processPromptSlashCommand(...)` resolves a concrete `command` before the model turn.
  - `getMessagesForPromptSlashCommand(command, args, ...)` builds both visible command metadata and hidden model-facing prompt content.
- `/Users/a20250311/github/claude-code-best/src/skills/loadSkillsDir.ts`
  - `createSkillCommand(...).getPromptForCommand(args, toolUseContext)` returns the selected skill's markdown prompt directly.
- `/Users/a20250311/github/claude-code-best/src/tools/SkillTool/prompt.ts`
  - If a `<command-name>` tag exists in the current turn, the skill is already loaded and the model should follow the loaded instructions instead of invoking another skill.
- `/Users/a20250311/github/claude-code-best/src/components/messages/UserCommandMessage.tsx`
  - The UI renders command metadata separately from normal text.

Lotus parity target for this plan: when `selected_skill_id = salary-query`, the runtime must load `salary-query` directly and must not start from `daily-assistant`, must not expose `switch_skill` in that explicit-skill turn, and must not rely on model-generated `switch_skill`.

## Current Bug Evidence

Reported conversation: `4721a3a2-29e1-414d-a259-43736418489f`.

Observed storage:

```json
{"role":"user","content":{"commandText":"/salary-query 用这个技能吧","skillCommand":{"command":"/salary-query","id":"salary-query","label":"salary-query"},"text":"用这个技能吧"}}
```

Observed tool call:

```json
{"name":"switch_skill","arguments":{"skill_id":"comp-analysis-v2"}}
```

Observed active skill memory:

```json
{"skillId":"comp-analysis-v2","currentStep":"step0","stepStatus":{"step0":"active"}}
```

Root cause: frontend and persistence carried `salary-query`, but backend turn config still resolved from `daily-assistant`/activation path and exposed `switch_skill`, allowing the model to choose `comp-analysis-v2`.

## File Responsibility Map

- `src-tauri/src/transport/tauri_commands/chat.rs`
  - Owns Tauri chat command adapter, `TauriLegacyTurnExecutor`, and `load_turn_config_overrides`.
  - Add a small testable helper for resolving `SkillTurnContext` from `ChatTurnRequest`.
  - Explicit `selected_skill_id` must call `SkillSessionStore::switch_skill` and then remove `switch_skill` from allowed tools for that turn.
  - Fallback path keeps existing `resolve_turn_context` behavior and may keep `switch_skill` available.
  - Emit structured `[skill-command]` diagnostics with `trace_id`, `conversation_id`, and selected skill fields.

- `src-tauri/src/runtime/chat/chat_turn_driver.rs`
  - Owns `ChatTurnRequest`, user message persistence event, and `build_user_content_json`.
  - Keep selected skill metadata available to persistence and frontend event echo.
  - Add a focused unit test for `build_user_content_json` if missing.

- `src-tauri/src/runtime/chat/skill_session.rs`
  - Owns explicit `switch_skill` and implicit `resolve_turn_context` behavior.
  - Keep `SkillSessionStore::switch_skill` unchanged unless tests show the helper cannot cleanly remove `switch_skill` from the explicit turn.

- `src-tauri/src/commands/chat.rs`
  - Tauri command boundary must continue accepting `selected_skill_id` / `selected_skill_label` and forwarding them to the adapter.

- `src/hooks/useChat.ts`, `src/lib/tauri.ts`, `src/components/chat-scene/ChatBottomArea.tsx`
  - Frontend must continue passing `selectedSkillId` structurally, without rewriting user text.
  - Existing tests should stay green.

- `src/hooks/useTurnRenderModel.ts`, `src/components/chat-scene/UserMessageBubble.tsx`
  - UI remains presentation-only. Do not use UI token rendering as execution source of truth.

## Task 1: Extract a Testable Selected-Skill Resolution Helper

**Files:**
- Modify: `src-tauri/src/transport/tauri_commands/chat.rs`

- [ ] **Step 1: Add helper function near `build_skill_session_store`**

Add this helper before `TauriLegacyTurnExecutor` so tests can call it without constructing a full `TauriChatCommandAdapter`:

```rust
async fn resolve_skill_turn_context_for_request(
    skill_sessions: &SkillSessionStore,
    skill_registry: &SkillRegistry,
    all_tool_names: &[String],
    request: &ChatTurnRequest,
) -> Result<(crate::runtime::chat::skill_session::SkillTurnContext, bool), TurnError> {
    let has_files = !request.file_ids.is_empty();
    let selected_skill_id = request
        .selected_skill_id
        .as_deref()
        .map(str::trim)
        .filter(|id| !id.is_empty());

    if let Some(selected_skill_id) = selected_skill_id {
        log::info!(
            "[skill-command][turn-config-selected-skill] trace_id={:?} conversation_id={} selected_skill_id={}",
            request
                .client_message_id
                .as_deref()
                .or(request.selected_skill_id.as_deref()),
            request.conversation_id,
            selected_skill_id
        );
        let mut ctx = skill_sessions
            .switch_skill(
                skill_registry,
                all_tool_names,
                request.conversation_id.as_str(),
                selected_skill_id,
                has_files,
            )
            .await
            .map_err(|err| {
                TurnError::PersistenceError(format!(
                    "Failed to switch selected skill '{}': {err}",
                    selected_skill_id
                ))
            })?;

        // Claude Code Best treats explicit command loading as already resolved.
        // Do not expose switch_skill in the same turn, or the model can re-pick a different skill.
        if let Some(allowed_tools) = ctx.allowed_tools.as_mut() {
            allowed_tools.remove("switch_skill");
        }

        return Ok((ctx, true));
    }

    let ctx = skill_sessions
        .resolve_turn_context(
            skill_registry,
            all_tool_names,
            request.conversation_id.as_str(),
            request.content.as_str(),
            has_files,
        )
        .await
        .map_err(|err| {
            TurnError::PersistenceError(format!("Failed to resolve skill session: {err}"))
        })?;
    Ok((ctx, false))
}
```

This helper returns `bool` to identify explicit selection in tests and diagnostics.

- [ ] **Step 2: Compile-check the helper syntax with a narrow static check**

Run:

```bash
rg -n "resolve_skill_turn_context_for_request|turn-config-selected-skill|allowed_tools.remove\(\"switch_skill\"\)" src-tauri/src/transport/tauri_commands/chat.rs
```

Expected: all three patterns appear.

## Task 2: Add Backend Regression Tests for Explicit Skill Selection

**Files:**
- Modify: `src-tauri/src/transport/tauri_commands/chat.rs`

- [ ] **Step 1: Extend `registry_with_test_skills()` with `salary-query`**

In the existing `registry_with_test_skills()` test helper, after registering `comp-analysis`, register this skill:

```rust
registry
    .register(
        Arc::new(TestSkill {
            id: "salary-query",
            trigger: None,
            prompt_prefix: "salary",
            default_tools: vec!["bash".to_string(), "switch_skill".to_string()],
            workflow: None,
        }),
        "test",
    )
    .await;
```

Use `default_tools` with `switch_skill` intentionally so the explicit-selection helper test can prove it removes `switch_skill` from the selected turn.

- [ ] **Step 2: Add failing test for selected skill winning over activation detection**

Add this test to `src-tauri/src/transport/tauri_commands/chat.rs` inside the existing `#[cfg(test)] mod tests`:

```rust
#[tokio::test]
async fn selected_skill_id_overrides_activation_detection_for_turn_context() {
    let registry = registry_with_test_skills().await;
    let skill_sessions = SkillSessionStore::new();
    let all_tools = vec![
        "bash".to_string(),
        "search_files".to_string(),
        "read_workspace_file".to_string(),
        "switch_skill".to_string(),
    ];
    let mut request = ChatTurnRequest::new("c-selected-skill", "请分析这个问题", Vec::new());
    request.client_message_id = Some("client-selected".to_string());
    request.selected_skill_id = Some("salary-query".to_string());
    request.selected_skill_label = Some("salary-query".to_string());

    let (ctx, explicit) = resolve_skill_turn_context_for_request(
        &skill_sessions,
        &registry,
        &all_tools,
        &request,
    )
    .await
    .expect("selected skill should resolve");

    assert!(explicit, "selected_skill_id should mark this as explicit selection");
    assert_eq!(ctx.skill_id, "salary-query");
    assert!(
        ctx.system_prompt.starts_with("salary:"),
        "expected selected salary-query prompt, got {}",
        ctx.system_prompt
    );
    assert!(
        !ctx.allowed_tools
            .as_ref()
            .map(|tools| tools.contains("switch_skill"))
            .unwrap_or(false),
        "explicit skill turn must not expose switch_skill"
    );

    let restored = skill_sessions
        .resolve_turn_context(
            &registry,
            &all_tools,
            "c-selected-skill",
            "后续问题",
            false,
        )
        .await
        .expect("selected state should persist");
    assert_eq!(restored.skill_id, "salary-query");
}
```

Before Task 1 implementation this test fails to compile because the helper is missing. After Task 1 but before Task 3, it should fail if `switch_skill` is still exposed or if selected skill is ignored.

- [ ] **Step 3: Add fallback test proving automatic detection still works without selected skill**

Add this test next to the previous test:

```rust
#[tokio::test]
async fn missing_selected_skill_id_keeps_activation_detection() {
    let registry = registry_with_test_skills().await;
    let skill_sessions = SkillSessionStore::new();
    let all_tools = vec![
        "bash".to_string(),
        "search_files".to_string(),
        "read_workspace_file".to_string(),
        "switch_skill".to_string(),
    ];
    let request = ChatTurnRequest::new("c-auto-skill", "请分析这个问题", Vec::new());

    let (ctx, explicit) = resolve_skill_turn_context_for_request(
        &skill_sessions,
        &registry,
        &all_tools,
        &request,
    )
    .await
    .expect("automatic skill detection should resolve");

    assert!(!explicit, "no selected_skill_id should use fallback resolution");
    assert_eq!(ctx.skill_id, "comp-analysis");
    assert!(
        ctx.allowed_tools
            .as_ref()
            .map(|tools| tools.contains("switch_skill"))
            .unwrap_or(false),
        "fallback skill turns may keep switch_skill available"
    );
}
```

- [ ] **Step 4: Run focused Rust tests and verify RED/GREEN based on progress**

Run from repository root:

```bash
cd src-tauri && cargo test --no-default-features selected_skill_id_overrides_activation_detection_for_turn_context -- --nocapture
cd src-tauri && cargo test --no-default-features missing_selected_skill_id_keeps_activation_detection -- --nocapture
```

Expected after Task 1/2 implementation: both pass. If Cargo blocks on the build directory file lock for more than 10 seconds, stop the command and record that Rust execution is blocked by Cargo lock. Do not leave a long-running Cargo process.

## Task 3: Wire the Helper Into `load_turn_config_overrides`

**Files:**
- Modify: `src-tauri/src/transport/tauri_commands/chat.rs`

- [ ] **Step 1: Replace inline `resolve_turn_context` block**

In `TauriLegacyTurnExecutor::load_turn_config_overrides`, replace this current block:

```rust
let skill_ctx = self
    .services
    .skill_sessions
    .resolve_turn_context(
        self.services.skill_registry.as_ref(),
        &all_tools,
        request.conversation_id.as_str(),
        request.content.as_str(),
        !request.file_ids.is_empty(),
    )
    .await
    .map_err(|err| TurnError::PersistenceError(format!("Failed to resolve skill session: {err}")))?;
```

with:

```rust
let (skill_ctx, explicit_skill_selected) = resolve_skill_turn_context_for_request(
    self.services.skill_sessions.as_ref(),
    self.services.skill_registry.as_ref(),
    &all_tools,
    request,
)
.await?;

log::info!(
    "[skill-command][turn-config-resolved] trace_id={:?} conversation_id={} skill_id={} explicit_skill_selected={} switch_skill_allowed={}",
    request
        .client_message_id
        .as_deref()
        .or(request.selected_skill_id.as_deref()),
    request.conversation_id,
    skill_ctx.skill_id,
    explicit_skill_selected,
    skill_ctx
        .allowed_tools
        .as_ref()
        .map(|tools| tools.contains("switch_skill"))
        .unwrap_or(false)
);
```

Keep all existing downstream logic that builds `visible_tool_defs`, `TurnConfigOverrides`, `max_iterations`, and `token_budget` from `skill_ctx`.

- [ ] **Step 2: Run focused static validation**

Run:

```bash
rg -n "turn-config-resolved|resolve_skill_turn_context_for_request\(" src-tauri/src/transport/tauri_commands/chat.rs
```

Expected: `load_turn_config_overrides` calls the helper, and the helper itself exists.

## Task 4: Ensure Persisted User Message and Event Echo Keep Skill Metadata

**Files:**
- Modify: `src-tauri/src/runtime/chat/chat_turn_driver.rs`
- Verify: `src-tauri/src/transport/tauri_commands/chat.rs`

- [ ] **Step 1: Add/verify test for `build_user_content_json`**

In `src-tauri/src/runtime/chat/chat_turn_driver.rs`, add this unit test if it does not already exist:

```rust
#[test]
fn build_user_content_json_includes_selected_skill_metadata() {
    let content = build_user_content_json(
        "用这个技能吧",
        &[],
        Some("salary-query"),
        Some("salary-query"),
    );

    assert_eq!(content["text"], "用这个技能吧");
    assert_eq!(content["commandText"], "/salary-query 用这个技能吧");
    assert_eq!(content["skillCommand"]["id"], "salary-query");
    assert_eq!(content["skillCommand"]["command"], "/salary-query");
    assert_eq!(content["skillCommand"]["label"], "salary-query");
}
```

- [ ] **Step 2: Run focused test from repository root**

Run:

```bash
cd src-tauri && cargo test --no-default-features build_user_content_json_includes_selected_skill_metadata -- --nocapture
```

Expected: PASS. If Cargo file lock blocks for more than 10 seconds, stop it and manually inspect `build_user_content_json`.

- [ ] **Step 3: Confirm adapter persistence uses the helper**

Inspect `src-tauri/src/transport/tauri_commands/chat.rs` and confirm `RuntimeLlmExecutor for TauriLegacyTurnExecutor::persist_user_message(...)` calls:

```rust
crate::runtime::chat::chat_turn_driver::build_user_content_json(
    content,
    file_ids,
    selected_skill_id,
    selected_skill_label,
)
```

Do not reimplement a separate JSON shape in the adapter.

## Task 5: Keep UI as Presentation-Only and Verify Frontend Regression

**Files:**
- Modify only if tests fail: `src/hooks/useTurnRenderModel.ts`
- Modify only if tests fail: `src/components/chat-scene/UserMessageBubble.tsx`
- Test: `src/hooks/__tests__/useTurnRenderModel.test.ts`
- Test: `src/components/chat-scene/__tests__/UserMessageBubble.test.tsx`
- Test: `src/components/chat-scene/__tests__/ChatBottomArea.test.tsx`
- Test: `src/hooks/useChat.skill.test.ts`

- [ ] **Step 1: Run frontend regression tests**

Run:

```bash
npm test -- src/components/chat-scene/__tests__/UserMessageBubble.test.tsx src/hooks/__tests__/useTurnRenderModel.test.ts src/components/chat-scene/__tests__/ChatBottomArea.test.tsx src/hooks/useChat.skill.test.ts
```

Expected: all four files pass. These tests prove:

- selected skill token is visible in the user bubble;
- slash text can be normalized for display;
- `ChatBottomArea` passes selected skill id to `sendUserMessage`;
- `useChat` sends selected skill id structurally while preserving plain user text.

- [ ] **Step 2: Do not move execution decisions into UI**

Check that no UI file imports backend skill registry/session logic. This command should return no production-code matches:

```bash
rg -n "SkillSession|skill_registry|switch_skill\(" src/components src/hooks src/features
```

Expected: no output except existing plain string references in tests or comments.

## Task 6: Reproduce the Reported Conversation Failure With New Logs

**Files:**
- No code changes expected.
- Use local app logs and storage.

- [ ] **Step 1: Restart dev app if needed**

If Tauri dev is not running, run:

```bash
npm run tauri:dev
```

If Vite port is already in use, stop only the stale dev process for this repo and rerun. Do not run long Cargo test filters in this step.

- [ ] **Step 2: Create a new conversation from `salary-query` and send text that previously confused the model**

In the app:

1. Open skill center.
2. Select `salary-query`.
3. Send: `用这个技能吧`.

Expected backend logs include:

```text
[skill-command][send-message] ... selected_skill_id=Some("salary-query") ...
[skill-command][turn-config-selected-skill] ... selected_skill_id=salary-query
[skill-command][turn-config-resolved] ... skill_id=salary-query ... explicit_skill_selected=true ... switch_skill_allowed=false
[skill-command][message-persisted-event] ... role=user ... has_skill_command=true ...
```

Expected active skill memory:

```bash
rg -n "<new-conversation-id>.*active_skill_state|salary-query|comp-analysis-v2" /Users/a20250311/.renlijia/shared/memory/memory.jsonl -S
```

Expected for the new conversation:

```json
{"skillId":"salary-query"}
```

Expected conversation storage:

```bash
rg -n "salary-query|comp-analysis-v2|switch_skill" /Users/a20250311/.renlijia/conversations/<new-conversation-id> -S
```

Expected:

- `salary-query` appears in user message metadata.
- There is no assistant tool call `switch_skill` in the first turn.
- There is no `comp-analysis-v2` in the new conversation transcript.

## Task 7: Final Verification and Commit

**Files:**
- All files touched by previous tasks.

- [ ] **Step 1: Run whitespace check**

Run:

```bash
git diff --check
```

Expected: no output and exit code 0.

- [ ] **Step 2: Run frontend regression tests**

Run:

```bash
npm test -- src/components/chat-scene/__tests__/UserMessageBubble.test.tsx src/hooks/__tests__/useTurnRenderModel.test.ts src/components/chat-scene/__tests__/ChatBottomArea.test.tsx src/hooks/useChat.skill.test.ts
```

Expected: all tests pass.

- [ ] **Step 3: Run focused Rust tests if Cargo is not locked**

Run each focused test separately and stop if it blocks on Cargo file lock for more than 10 seconds:

```bash
cd src-tauri && cargo test --no-default-features selected_skill_id_overrides_activation_detection_for_turn_context -- --nocapture
cd src-tauri && cargo test --no-default-features missing_selected_skill_id_keeps_activation_detection -- --nocapture
cd src-tauri && cargo test --no-default-features build_user_content_json_includes_selected_skill_metadata -- --nocapture
```

Expected when runnable: all pass.

- [ ] **Step 4: Commit**

Run:

```bash
git add -A
git commit -m "fix(skill): honor explicitly selected skill"
```

Expected: one commit containing the deterministic selected-skill execution fix and tests.

---

## Self-Review

- Spec coverage: Covers Claude Code Best deterministic command selection, explicit selected-skill prompt loading, visible metadata preservation, UI remaining presentation-only, and the reported `salary-query` -> `comp-analysis-v2` failure.
- No-model-repick coverage: Explicit selected-skill turns remove `switch_skill` from allowed tools, matching Claude Code Best's "already loaded" guard at Lotus's tool layer.
- Placeholder scan: No TBD/TODO/fill-later placeholders. Each task has concrete files, commands, code blocks, and expected outcomes.
- Type consistency: Uses existing Lotus names `selected_skill_id`, `selectedSkillId`, `ChatTurnRequest`, `TurnConfigOverrides`, `build_user_content_json`, `SkillSessionStore::switch_skill`, and `SkillTurnContext` consistently.
