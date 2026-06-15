# Permission Approval Surface Design

Date: 2026-06-03

## Context

Lotus currently receives permission requests through `permission:ask` and renders a global dialog from `App.tsx`. This works for a single desktop-only confirmation, but it does not fit the product shape we want:

- Permission approval suspends the current run and returns control to the user.
- The app should make that blocking state visible at the exact place where the user would otherwise continue typing.
- IM channels can still receive text while the run is waiting for approval, and that text is still user context for the current session.
- Approval controls are explicit shortcuts, but natural-language replies are valid interaction input. The runtime must only execute a permission decision after the current agent/model has interpreted the reply and the permission control plane has validated the resulting structured decision.

Reference projects point in the same direction:

- qob replaces the chat input with a pending question or tool approval panel.
- OpenClaw routes approvals through explicit buttons or `/approve <id> <decision>` commands. Lotus additionally needs IM/app natural-language input because the product has a conversational IM surface.

## Goals

1. Replace the in-app chat input with a reusable pending-action panel when the current conversation is waiting for permission approval.
2. Use explicit approval actions in IM surfaces as fast paths: buttons when supported, clear commands or card actions as fallback.
3. Let natural-language app/IM replies remain part of the conversation context while the run is waiting for user interaction.
4. Avoid automatic permission timeout. Approval remains pending until the user or runtime lifecycle resolves it.
5. Keep one shared approval state so app and IM surfaces update each other immediately.
6. Treat `UserInteractionRequired` / AskUserQuestion as the same class of user-interaction blocking state: no automatic timeout, app input replacement, natural-language answer input, and lifecycle cleanup only.

## Non-Goals

- Do not execute or persist model-inferred permission changes without runtime validation of the structured resolution.
- Do not treat a pending permission approval as a special queueing mode. Queueing is only about an actively executing run; a run suspended on user interaction is not active/busy.
- Do not auto-cancel the approval just because the user sends a new message.
- Do not implement every IM channel's native button support in the first step. Channels can fall back to deterministic text commands.

## Product Rules

### App Surface

When the active conversation has a pending permission request:

- Replace the composer area with `PendingActionSurface`.
- Show the tool name, request message, optional suggestions, and remember-scope controls.
- Provide explicit actions: allow, deny, cancel current task.
- Provide natural-language input for scoped or adjusted approval, such as "以后 `/a/b` 这个文件夹都可以读" when the pending request is for `/a/b/c`.
- Do not show a countdown.
- Do not show the normal composer while the pending action is active; the replacement surface owns both explicit controls and natural-language interaction input.

When the active conversation has a pending AskUserQuestion interaction:

- Replace the composer area with `PendingActionSurface`.
- Render the question form, options, or free-text answer controls from the interaction payload.
- Provide explicit actions: submit answer and cancel current task.
- Do not show a countdown.
- Do not show the normal composer while the interaction is active; the replacement surface owns the answer input.

The pending action is scoped by conversation. A pending request in another conversation must not hijack the current conversation's composer.

Conversation switching must preserve this behavior:

- Leaving a conversation with a pending approval does not clear or resolve the pending approval.
- Returning to that conversation restores `PendingActionSurface` immediately.
- The restored surface is derived from the shared pending state, not from component-local dialog state.
- If the backend no longer reports the pending approval after a refresh or restart, the composer returns to normal.

### IM Surface

When an IM-originated run requests permission:

- Send or update an AI card with explicit approval actions.
- Prefer native buttons for channels that can support them.
- Fallback to explicit commands such as `/approve <request-id> allow` and `/approve <request-id> deny`.
- A normal text message is not synchronously treated as approve/deny by the IM adapter.
- A normal text message falls through to the same channel message path. If another run is actively executing, `PendingQueueManager` queues it through the ordinary busy-run path; if the previous run is suspended on user interaction, the new text can start a normal turn with the pending approval/question included as context.
- There is no approval-specific queue ACK. Any queue user feedback belongs to the general busy-session queue behavior, not to permission approval.

### IM Channel Policy

All IM channels must share the same approval state machine. A channel adapter can choose a richer or poorer presentation, but it must not invent different approval semantics.

Channel capability tiers:

| Channel | First implementation | Native direction |
| --- | --- | --- |
| DingTalk | AI card with explicit actions, plus command fallback | Use AI card action callbacks to resolve approval and update the card after app-side resolution. |
| Feishu | Interactive card text or command fallback until action callbacks are wired | Use CardKit / `card.action.trigger` once the Feishu inbound action path is connected. |
| WeCom | Markdown/text fallback with explicit approval commands | Consider template-card events later if the active bot mode supports stable callbacks. |
| WeChat | Text fallback with explicit approval commands | Keep markdown/text only unless a reliable callback surface is added. |
| Telegram | Text fallback in current Lotus code | If Telegram bot inline keyboards are added, mirror OpenClaw's inline button model. |
| WhatsApp | Text fallback with explicit approval commands and reaction/status updates | Use quick-reply/button callbacks only if the current WhatsApp bridge exposes them reliably. |

The required fallback for every channel is:

```text
/approve <request-id> allow
/approve <request-id> deny
/approve <request-id> cancel
```

AskUserQuestion fallback commands use the same deterministic pattern:

```text
/answer <interaction-id> <answer text>
/answer <interaction-id> {"answers":["structured answer"]}
/answer <interaction-id> cancel
```

Free-form messages are interaction/context input, not deterministic adapter-level approval. Short free-form replies such as "可以", "ok", "不用了", or scoped requests such as "以后 `/a/b` 都可以读" must be handled by the current agent/model and then resolved through the validated runtime control plane.

Fallback commands must be parsed deterministically. When native controls are unavailable, the pending approval message should include the explicit commands and the request id, while also making clear that the user can reply in natural language.

### Approval Resolution

The approval can end through these paths:

- User allows the request in the app.
- User denies the request in the app.
- User cancels the current task in the app.
- User clicks an IM approval button.
- User sends an explicit approval command.
- User sends natural-language interaction input, the current agent/model interprets it, and the runtime validates a structured permission or question resolution.
- The runtime/run is cancelled or no longer exists.

There is no automatic "deny after N minutes" rule.

## Architecture

### Shared State

Introduce a single logical pending-approval source keyed by `toolCallId` or backend request id:

```ts
type PendingPermissionAction = {
  toolCallId: string
  conversationId: string
  runId: string
  toolName: string
  message: string
  suggestions: string[]
  mode: string
  rememberOptions: PermissionDestination[]
  defaultDestination?: PermissionDestination
  createdAt: number
  status: 'pending'
}
```

The frontend can continue storing this in `streamingStore.pendingAsks`, but selection must be conversation-scoped rather than "first pending ask globally".

The selected pending action should be derived on render from the active `pendingSessionId` or `conversationId`. This makes the input interception stable across route changes, conversation switches, component unmount/remount, and app refresh recovery.

### App Integration

`ChatBottomArea` should derive the active pending action for its `pendingSessionId`. It renders:

```tsx
{activePendingAction ? (
  <PendingActionSurface action={activePendingAction} />
) : (
  <RichComposer ... />
)}
```

`PermissionAskDialog` can be retired or kept only as a temporary fallback during migration.

Permission resolution needs a structured result that can represent both explicit controls and model-interpreted natural-language replies:

```ts
type PermissionResolutionDraft = {
  decision: 'allow' | 'deny' | 'cancel'
  remember?: boolean
  destination?: PermissionDestination
  grantScopeOverride?: string
  updatedInput?: unknown
  userMessage?: string
}
```

The app or IM adapter may collect `userMessage`, but it must not apply `grantScopeOverride`, `destination`, or `updatedInput` directly. Those fields are only accepted after agent/model interpretation and runtime validation.

### IM Integration

`IMAskCoordinator` should become the shared pending-action coordinator for all IM channels, not a DingTalk-only behavior. It should split inbound messages into:

- explicit approval action: card/button/command resolves the pending approval;
- explicit question answer action: command/control resolves AskUserQuestion;
- invalid pending-action command: stop normal dispatch and return a correction message;
- ordinary text: return `NotPending` so the channel's normal dispatch path can handle it as context or queue it as a busy-session input.

This keeps adapter behavior deterministic without throwing away the conversational meaning of a natural-language reply.

Each channel worker should call the same coordinator before normal message dispatch. The coordinator returns:

- `NotPending`: dispatch normally;
- `ApprovalResolved`: stop normal dispatch and update the approval surface;
- `AnswerResolved`: stop normal dispatch and update the question surface;
- `InvalidApprovalAction`: stop normal dispatch and tell the user the approval command or button is no longer valid.

### Queue Interaction

Pending queue behavior is not permission-specific:

- The queue is used when a session/run is actively executing.
- A run suspended on permission approval or AskUserQuestion must release its active-run/busy marker while waiting, then reacquire it before continuing after an explicit resolution.
- The queue item should preserve its original message content and channel metadata.
- When the current run resolves, queue draining continues through the existing pending queue path.
- A second, third, or later message that arrives while a run is actively executing can receive general queue feedback, but that feedback must not imply that permission approval has special queue semantics.

## Error Handling

- If approving/denying fails, keep the pending surface visible and show an error toast or card update.
- If general queued-message feedback fails, still keep the queued message and log the delivery failure.
- If an approval action arrives after the backend request was resolved, update the IM card to an unavailable/resolved state instead of starting a new turn.
- If the app restarts, pending approvals should be restored only when the backend still has the pending request.

## Testing

Focused tests should cover:

- `ChatBottomArea` renders `PendingActionSurface` instead of `RichComposer` for the active conversation.
- A pending ask from another conversation does not replace the current composer.
- Switching away from a pending-approval conversation and back restores `PendingActionSurface`.
- Clearing or resolving the pending approval while away makes the composer normal when returning.
- App allow/deny/cancel calls the existing Tauri commands and clears the UI only after successful resolution or confirmed backend cleanup.
- IM normal text during `waiting_approval` falls through to the normal channel dispatch path; it is not converted into an approval-specific queued outcome.
- IM explicit approval action resolves the pending request and does not enqueue a normal message.
- Natural-language permission input can express a scoped grant override and must only be applied after structured runtime validation.
- AskUserQuestion custom/free-text answers remain available to the current agent/model.
- Every IM channel worker consults the shared coordinator before normal dispatch.
- Channels without native buttons include deterministic fallback approval commands.
- App-side resolution updates or closes IM approval surfaces where the channel supports updates, and sends a resolved notice where it does not.
- No deadline timer denies permission automatically.

## Recommended Implementation Order

1. Add `PendingActionSurface` and conversation-scoped pending selection in the app.
2. Move permission approval rendering from global dialog to the chat bottom area.
3. Promote the IM approval coordinator from DingTalk-specific wiring to a shared pre-dispatch gate for all channel workers.
4. Remove IM automatic deadline denial and replace it with lifecycle cleanup only.
5. Change IM pending-approval handling so normal text falls through to normal dispatch instead of being judged by adapter-side approval logic or queued through an approval-specific branch.
6. Add deterministic fallback approval commands for every channel.
7. Add native button/action resolution for DingTalk first, then Feishu, then any channel whose transport exposes stable callbacks.
