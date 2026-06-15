# Sidebar Row Interaction Status Design

## Goal

Unify the status shown on regular conversation rows and IM channel conversation rows in the sidebar.

Users should be able to scan the sidebar and distinguish:

- A conversation is currently running.
- A conversation is waiting for permission review.
- A conversation is waiting for the user's reply to an AskUserQuestion interaction.

## Scope

This design covers:

- Regular sidebar conversation rows rendered by `src/components/sidebar/ConversationRow.tsx`.
- Concrete IM channel conversation rows rendered by `ChannelConversationRow` inside `src/components/sidebar/AppSidebar.tsx`, such as DingTalk private chat, WeChat chat, Telegram chat, and other platform session rows.

This design does not cover:

- Platform heading rows such as DingTalk, WeChat, Telegram, Feishu, WeCom, or WhatsApp.
- Human interaction runtime behavior.
- PermissionAsk or AskUserQuestion parsing, resume, pending queue, or output routing.
- Platform-level aggregate status.

## Existing Behavior

`ConversationRow` already has a right-side status/action slot:

- On hover, the slot shows row actions such as pin and archive.
- When not hovering:
  - `waitingApproval` shows a chip with the current copy `审批`.
  - Otherwise `loading` shows a `Loader2` spinner.
- `waitingApproval` takes precedence over `loading`.

The status source currently lives in `AppSidebar.tsx`.

`AppSidebar` calls `selectPendingActionsForSession()` from `src/components/chat-scene/pendingActionSelectors.ts`, but it flattens the result to a boolean through `hasPendingUserAction()`. This loses the distinction between permission requests and user-question interactions.

`pendingActionSelectors.ts` already exposes enough semantic information:

- `permission`
- `user-question`
- `stale-permission`
- `stale-interaction`

The IM channel row component is currently an inline `ChannelConversationRow` inside `AppSidebar.tsx`. It shows the conversation label and unread count, but it does not reuse the regular conversation row status behavior.

## Design

### Status Model

Introduce a shared sidebar row status type:

```ts
export type SidebarRowStatus =
  | 'permission-review'
  | 'waiting-reply'
  | 'loading'
  | null
```

Status semantics:

- `permission-review`: a PermissionAsk is pending, including stale permission state.
- `waiting-reply`: an AskUserQuestion interaction is pending, including stale interaction state.
- `loading`: the conversation is busy or streaming.
- `null`: no visible status.

Status priority:

```ts
permission-review > waiting-reply > loading > null
```

This priority preserves the current behavior where pending user action is more important than a running indicator.

### Copy

Chinese:

- `permission-review`: `审核`
- `waiting-reply`: `等待回复`
- `loading`: spinner only, no new copy.

English:

- `permission-review`: `Review`
- `waiting-reply`: `Waiting reply`
- `loading`: spinner only, no new copy.

### Shared Indicator

Create a shared component:

```ts
SidebarRowStatusIndicator
```

Responsibilities:

- Accept `status: SidebarRowStatus`.
- Render the `审核` chip for `permission-review`.
- Render the `等待回复` chip for `waiting-reply`.
- Render the existing `Loader2` spinner for `loading`.
- Render nothing for `null`.

The chip styling should follow the existing chip style in `ConversationRow`. This is a targeted status upgrade, not a sidebar redesign.

The component should own the visible copy and tooltip copy so `ConversationRow` and `ChannelConversationRow` do not duplicate status rendering logic.

### ConversationRow

Change `ConversationRow` from boolean props:

```ts
loading?: boolean
waitingApproval?: boolean
```

to a semantic status prop:

```ts
status?: SidebarRowStatus
```

Interaction behavior stays the same:

- Hovering the row shows row actions.
- Not hovering shows the status indicator if `status` is non-null.
- The title should keep its current alignment and truncation behavior.
- The right-side slot should stay narrow and stable to avoid row jitter.

### AppSidebar Status Derivation

Replace `hasPendingUserAction()` with a status derivation helper:

```ts
function sidebarStatusForConversation(conversationId: string): SidebarRowStatus
```

Behavior:

1. Call `selectPendingActionForSession()` with the conversation id, pending permission asks, pending interactions, and current turn stage.
2. If the selected action is `permission` or `stale-permission`, return `permission-review`.
3. If the selected action is `user-question` or `stale-interaction`, return `waiting-reply`.
4. If there is no pending action and `isConversationBusy(conversationId)` is true, return `loading`.
5. Otherwise return `null`.

All regular conversation rows should receive:

```tsx
status={sidebarStatusForConversation(conversation.id)}
```

### ChannelConversationRow

Add a status prop:

```ts
status?: SidebarRowStatus
```

Right-side rendering priority:

1. If `status` is non-null, render `SidebarRowStatusIndicator`.
2. Else if `conversation.unreadCount > 0`, render the unread badge.
3. Else render nothing.

When unread count and status both exist, status wins and the unread badge is hidden. The right-side slot is too narrow to show both without making the row noisy, and pending human action is more urgent than unread count.

`renderChannelRows()` should pass:

```tsx
status={sidebarStatusForConversation(conversation.sessionId)}
```

The platform heading rows should not receive aggregate status.

## Tests

Add or update tests to cover:

1. `ConversationRow` shows the loader for `status="loading"`.
2. `ConversationRow` shows `审核` for `status="permission-review"`.
3. `ConversationRow` shows `等待回复` for `status="waiting-reply"`.
4. `ConversationRow` shows pending status instead of loader when a pending status is present.
5. `ConversationRow` hides status and shows row actions on hover.
6. `AppSidebar` shows `审核` for a regular conversation with a pending permission ask.
7. `AppSidebar` shows `等待回复` for a regular conversation with a pending AskUserQuestion interaction.
8. `AppSidebar` shows `审核` for an IM channel conversation with a pending permission ask.
9. `AppSidebar` shows `等待回复` for an IM channel conversation with a pending AskUserQuestion interaction.
10. `ChannelConversationRow` shows status instead of unread count when both exist.

## Acceptance Criteria

- Regular conversation rows and concrete IM channel conversation rows use the same status semantics.
- PermissionAsk rows show `审核`.
- AskUserQuestion rows show `等待回复`.
- Loading rows continue to show the existing spinner.
- Pending human interaction status takes precedence over loading.
- IM channel conversation status takes precedence over unread count.
- Platform heading rows do not show aggregate status.
- No human interaction runtime behavior changes are included in this work.

## Implementation Notes

- Keep this change in the sidebar frontend layer.
- Prefer extracting `SidebarRowStatusIndicator` rather than duplicating chip rendering in two row components.
- Keep the existing hover behavior in `ConversationRow`.
- Avoid widening sidebar rows or changing the overall sidebar layout.
