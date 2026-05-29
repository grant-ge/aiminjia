# Skill Command End-to-End Execution Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development to implement/review task-by-task. Use verification-before-completion before claiming completion or committing.

**Goal:** Make a selected Composer skill token activate that skill end-to-end and preserve a Claude Code Best-style visible slash command breadcrumb for the user turn.

**Architecture:** Frontend sends a structured `selectedSkillId` and skill command metadata alongside the existing message payload. Backend stores `selected_skill_id` on `ChatTurnRequest`, explicitly switches skill before building `TurnConfig`, persists a visible slash-command breadcrumb (`/<skill-id> args`) for the user message, and uses SKILL.md body as hidden prompt/system content for execution. This mirrors Claude Code Best: visible command metadata for the transcript, model-facing skill content out of the ordinary user text.

**Tech Stack:** React 19, Zustand, Vitest, Tauri IPC, Rust chat runtime, `SkillSessionStore`, `DeclarativeSkill`.

---

## Scope

- Do pass `selectedSkillId` from Composer token to `send_message`.
- Do not concatenate `/skill-id` into the free-form user text field before sending.
- Do persist/render a visible Claude Code Best-style command breadcrumb for selected skills: `/<skill-id> <user text>`.
- Do not change the token UI.
- Do not remove existing keyword-based `resolve_turn_context` behavior.
- Do load SKILL.md body as prompt content for SKILL.md-only skills.

## Task 1: Frontend Selected Skill IPC Payload

**Files:**
- Modify: `src/lib/tauri.ts`
- Modify: `src/hooks/useChat.ts`
- Modify: `src/components/chat-scene/ChatBottomArea.tsx`
- Test: `src/components/chat-scene/__tests__/ChatBottomArea.test.tsx`
- Test: `src/hooks/useChat.skill.test.ts`

**Acceptance:** When Composer has a selected skill token, sending a message calls IPC with `selectedSkillId` and command metadata. The normal `text` remains the original user text, while the visible transcript can show the command breadcrumb `/<skill-id> <user text>`.

## Task 2: Backend Explicit Skill Selection

**Files:**
- Modify: `src-tauri/src/runtime/chat/chat_turn_driver.rs`
- Modify: `src-tauri/src/transport/tauri_commands/chat.rs`
- Test: existing unit tests near `load_turn_config_overrides` or `SkillSessionStore` tests.

**Acceptance:** `ChatTurnRequest.selected_skill_id = Some("skill-smith")` causes the turn config override to use `switch_skill`, persist the active skill, and use that skill's prompt/tools/budget for the same turn.

## Task 3: SKILL.md Body Prompt Loading

**Files:**
- Modify: `src-tauri/src/plugin/manifest.rs` or `src-tauri/src/plugin/declarative_skill.rs`
- Test: `src-tauri/src/plugin/declarative_skill.rs` or `src-tauri/src/plugin/manifest.rs`

**Acceptance:** A skill directory with only `SKILL.md` and markdown body produces a `DeclarativeSkill.system_prompt()` containing the body content after frontmatter.

## Task 4: Claude Code Best-Style Visible Skill Breadcrumb

**Files:**
- Modify: `src/types/message.ts`
- Modify: `src/hooks/useChat.ts`
- Modify: `src/components/chat/UserBubble.tsx`
- Modify: `src-tauri/src/runtime/chat/chat_turn_driver.rs`
- Modify: `src-tauri/src/transport/tauri_commands/chat.rs`
- Test: `src/hooks/useChat.skill.test.ts`
- Test: `src/components/chat/UserBubble.test.tsx`

**Acceptance:** Selected skill turns preserve a visible command breadcrumb equivalent to Claude Code Best's `/skill-name args`, while model-facing execution still uses structured `selected_skill_id` and SKILL.md prompt content instead of mutating the free-form user text.

## Task 5: Default Workspace Fallback

**Files:**
- Modify: `src-tauri/src/runtime/session_runtime.rs`
- Modify: `src-tauri/src/runtime/query_engine.rs`

**Acceptance:** When a conversation has no explicit authorized workspace, runtime tools receive `~/.renlijia/defaultFolder` as the authorized workspace fallback instead of the internal `~/.renlijia` sandbox root.

## Verification

```bash
npm test -- src/hooks/useChat.skill.test.ts src/components/chat-scene/__tests__/ChatBottomArea.test.tsx
# Avoid long Cargo runs during interactive repair unless explicitly requested.
npm run build
```

If the package name differs, use targeted `cargo test` filters without broad all-crate filtering.
