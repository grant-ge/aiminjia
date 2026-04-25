# Skill Command Composer Token Implementation Plan

> Required workflow: use `superpowers:subagent-driven-development` for implementation/review coordination and `superpowers:verification-before-completion` before claiming completion or committing.

## Goal

Align Lotus skill launch UX with `/Users/a20250311/github/claude-code-best` prompt-command semantics for this phase:

- Clicking a skill creates/navigates to a chat conversation.
- Composer displays a polished selected-skill token.
- The token stores the underlying command semantics as `/skill-id` for the next phase.
- Do not auto-send `triggerText`.
- Do not change backend skill execution, message payload composition, or sending-time expansion in this phase.

## Confirmed Scope

This phase is frontend loading/display only. The Composer token carries `command: /skill-id`, but `ChatBottomArea` does not inject that command into the message payload yet. Sending-time expansion of `SKILL.md`, workflow triggering, or backend `SkillSessionStore` changes are intentionally deferred.

## Final Design

- `src/stores/chatStore.ts`
  - Add `ComposerSkillCommand`.
  - Store selected commands as `selectedSkillCommands: Record<string, ComposerSkillCommand>` keyed by `conversationId` so selected skills do not leak across conversations.
  - Add `setSelectedSkillCommand(conversationId, command)` and `clearSelectedSkillCommand(conversationId?)`.

- `src/hooks/useChat.ts`
  - `createConversationFromSkill(skillId)` creates the conversation, routes to chat, and sets the selected command for that new conversation.
  - Command value is `/${skillId}` and label uses `skill.displayName || skillId`.
  - It no longer auto-sends `triggerText`.
  - `sendUserMessage(...)` returns `Promise<boolean>` so UI can distinguish successful send from busy/failed/timeout paths.

- `src/components/chat-scene/ChatBottomArea.tsx`
  - Reads the token for the active conversation only.
  - Passes token and clear handler to `ChatComposerCompact`.
  - Sends the existing user text/file payload unchanged in this phase.
  - Clears input/files/token only when `sendUserMessage` reports success.
  - Keeps input/files/token when send fails or times out.

- `src/components/chat-scene/ChatComposerCompact.tsx`
  - Renders a warm visual token with skill label and `/skill-id` badge.
  - Shows the skill toolbar button as loaded with accessible name/pressed state.
  - Allows manually removing the token.
  - Uses theme accent variables rather than hard-coded hex colors.

## Tests

- `src/hooks/useChat.skill.test.ts`
  - Verifies skill launch sets the selected command and does not call `sendMessage`.

- `src/components/chat-scene/__tests__/ChatComposerCompact.test.tsx`
  - Verifies token rendering, `/skill-id` display, loaded button state, and clear action.

- `src/components/chat-scene/__tests__/ChatBottomArea.test.tsx`
  - Verifies active-conversation token rendering.
  - Verifies selected skill does not leak across conversations.
  - Verifies failed send preserves input and token.
  - Verifies successful send clears input and token.

## Verification Commands

```bash
npm test -- src/hooks/useChat.skill.test.ts src/components/chat-scene/__tests__/ChatComposerCompact.test.tsx src/components/chat-scene/__tests__/ChatBottomArea.test.tsx
npm run build
```
