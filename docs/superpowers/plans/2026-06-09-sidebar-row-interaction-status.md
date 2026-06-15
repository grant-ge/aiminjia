# Sidebar Row Interaction Status Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Show distinct sidebar row states for permission review, waiting for user reply, and loading across regular conversation rows and concrete IM channel conversation rows.

**Architecture:** Keep the existing sidebar row layout and right-side status/action slot. Extract a shared `SidebarRowStatusIndicator`, replace boolean `waitingApproval/loading` props with a semantic `SidebarRowStatus`, and derive that status once in `AppSidebar` from existing pending action selectors and busy state.

**Tech Stack:** React, TypeScript, Zustand stores, i18next locale JSON, Vitest, Testing Library.

---

## Scope And Ground Rules

- Worktree: `/Users/oayzz/.codex/worktrees/9a36/lotus-app`.
- This is a frontend sidebar display change only.
- Do not modify human interaction runtime, permissionAsk parsing, AskUserQuestion parsing, pending queue, or IM output routing.
- Do not change platform heading rows such as DingTalk, WeChat, Telegram, Feishu, WeCom, or WhatsApp.
- Keep existing `ConversationRow` hover behavior: hover shows row actions; non-hover shows status.
- The current worktree may contain unrelated dirty Rust changes from another thread. Do not stage or commit them.

## File Structure

Create:

- `src/components/sidebar/SidebarRowStatusIndicator.tsx` — shared status type and renderer for sidebar row status chips/spinner.
- `src/components/sidebar/__tests__/SidebarRowStatusIndicator.test.tsx` — focused tests for copy and loader behavior.

Modify:

- `src/components/sidebar/ConversationRow.tsx` — replace `loading/waitingApproval` boolean props with `status?: SidebarRowStatus`.
- `src/components/sidebar/__tests__/ConversationRow.test.tsx` — update existing status tests and hover behavior tests.
- `src/components/sidebar/AppSidebar.tsx` — derive typed sidebar row status and pass it to regular conversation rows and channel conversation rows.
- `src/components/sidebar/__tests__/AppSidebar.test.tsx` — cover regular conversation rows and IM channel conversation rows.
- `src/i18n/zh-CN.json` — add status copy.
- `src/i18n/en-US.json` — add status copy.

---

### Task 1: Add Shared Sidebar Row Status Indicator

**Files:**
- Create: `src/components/sidebar/SidebarRowStatusIndicator.tsx`
- Create: `src/components/sidebar/__tests__/SidebarRowStatusIndicator.test.tsx`
- Modify: `src/i18n/zh-CN.json`
- Modify: `src/i18n/en-US.json`

- [ ] **Step 1: Write failing indicator tests**

Create `src/components/sidebar/__tests__/SidebarRowStatusIndicator.test.tsx`:

```tsx
import '@testing-library/jest-dom'
import { render, screen } from '@testing-library/react'
import { describe, expect, it } from 'vitest'

import { SidebarRowStatusIndicator } from '../SidebarRowStatusIndicator'

describe('SidebarRowStatusIndicator', () => {
  it('renders permission review chip copy', () => {
    render(<SidebarRowStatusIndicator status="permission-review" />)

    expect(screen.getByText('审核')).toBeInTheDocument()
    expect(screen.queryByLabelText('对话运行中')).not.toBeInTheDocument()
  })

  it('renders waiting reply chip copy', () => {
    render(<SidebarRowStatusIndicator status="waiting-reply" />)

    expect(screen.getByText('等待回复')).toBeInTheDocument()
    expect(screen.queryByLabelText('对话运行中')).not.toBeInTheDocument()
  })

  it('renders loader for loading status', () => {
    const { container } = render(<SidebarRowStatusIndicator status="loading" />)

    expect(container.querySelector('[data-icon="loader"]')).toBeInTheDocument()
    expect(screen.getByLabelText('对话运行中')).toBeInTheDocument()
  })

  it('renders nothing for null status', () => {
    const { container } = render(<SidebarRowStatusIndicator status={null} />)

    expect(container.firstChild).toBeNull()
  })
})
```

- [ ] **Step 2: Run the failing indicator tests**

Run:

```bash
pnpm exec vitest run src/components/sidebar/__tests__/SidebarRowStatusIndicator.test.tsx
```

Expected: FAIL because `SidebarRowStatusIndicator.tsx` does not exist.

- [ ] **Step 3: Add locale copy**

Modify `src/i18n/zh-CN.json`, inside the existing `sidebar` object, add:

```json
"status": {
  "permissionReviewChip": "审核",
  "permissionReviewTooltip": "等待你审核权限请求",
  "waitingReplyChip": "等待回复",
  "waitingReplyTooltip": "等待你回复问题"
}
```

Keep the existing `waitingApprovalChip` and `waitingApprovalTooltip` keys for backward compatibility during this task.

Modify `src/i18n/en-US.json`, inside the existing `sidebar` object, add:

```json
"status": {
  "permissionReviewChip": "Review",
  "permissionReviewTooltip": "Waiting for your permission review",
  "waitingReplyChip": "Waiting reply",
  "waitingReplyTooltip": "Waiting for your reply"
}
```

- [ ] **Step 4: Implement the shared indicator**

Create `src/components/sidebar/SidebarRowStatusIndicator.tsx`:

```tsx
import { Loader2, ShieldQuestionMark } from 'lucide-react'
import { useTranslation } from 'react-i18next'

import {
  Tooltip,
  TooltipContent,
  TooltipProvider,
  TooltipTrigger,
} from '@/components/ui/tooltip'

export type SidebarRowStatus =
  | 'permission-review'
  | 'waiting-reply'
  | 'loading'
  | null

interface SidebarRowStatusIndicatorProps {
  status?: SidebarRowStatus
}

export function SidebarRowStatusIndicator({
  status = null,
}: SidebarRowStatusIndicatorProps) {
  const { t } = useTranslation()

  if (!status) return null

  if (status === 'loading') {
    return (
      <Loader2
        aria-label={t('sidebar.conversationLoading')}
        data-icon="loader"
        className="h-3.5 w-3.5 animate-spin text-muted-foreground"
      />
    )
  }

  const label =
    status === 'permission-review'
      ? t('sidebar.status.permissionReviewChip')
      : t('sidebar.status.waitingReplyChip')
  const tooltip =
    status === 'permission-review'
      ? t('sidebar.status.permissionReviewTooltip')
      : t('sidebar.status.waitingReplyTooltip')

  return (
    <TooltipProvider delayDuration={400}>
      <Tooltip>
        <TooltipTrigger asChild>
          <span className="inline-flex h-5 items-center gap-1 rounded-full bg-primary/10 px-1.5 text-[10px] font-medium leading-none text-primary">
            <ShieldQuestionMark className="h-3 w-3" />
            {label}
          </span>
        </TooltipTrigger>
        <TooltipContent side="top">{tooltip}</TooltipContent>
      </Tooltip>
    </TooltipProvider>
  )
}
```

- [ ] **Step 5: Run the indicator tests**

Run:

```bash
pnpm exec vitest run src/components/sidebar/__tests__/SidebarRowStatusIndicator.test.tsx
```

Expected: PASS.

- [ ] **Step 6: Commit Task 1**

Run:

```bash
git add src/components/sidebar/SidebarRowStatusIndicator.tsx \
  src/components/sidebar/__tests__/SidebarRowStatusIndicator.test.tsx \
  src/i18n/zh-CN.json \
  src/i18n/en-US.json
git commit -m "feat(sidebar): add shared row status indicator"
```

Expected: commit succeeds and does not include unrelated Rust files.

---

### Task 2: Migrate ConversationRow To Semantic Status

**Files:**
- Modify: `src/components/sidebar/ConversationRow.tsx`
- Modify: `src/components/sidebar/__tests__/ConversationRow.test.tsx`

- [ ] **Step 1: Update ConversationRow tests first**

Modify `src/components/sidebar/__tests__/ConversationRow.test.tsx`.

Replace the existing loading test with:

```tsx
it('shows a loader icon when status is loading', () => {
  const { container } = render(
    <ConversationRow id="c3" title="X" status="loading" onClick={() => {}} />,
  )
  expect(container.querySelector('[data-icon="loader"]')).toBeInTheDocument()
})
```

Replace the existing pending approval test with:

```tsx
it('shows permission review chip instead of the loader while permission review is pending', () => {
  const { container } = render(
    <ConversationRow id="c3-pending" title="X" status="permission-review" onClick={() => {}} />,
  )

  expect(screen.getByText('审核')).toBeInTheDocument()
  expect(container.querySelector('[data-icon="loader"]')).not.toBeInTheDocument()
})
```

Add this test:

```tsx
it('shows waiting reply chip while ask user question is pending', () => {
  const { container } = render(
    <ConversationRow id="c3-waiting-reply" title="X" status="waiting-reply" onClick={() => {}} />,
  )

  expect(screen.getByText('等待回复')).toBeInTheDocument()
  expect(container.querySelector('[data-icon="loader"]')).not.toBeInTheDocument()
})
```

Update the hover test setup from:

```tsx
<ConversationRow id="c3-hover" title="X" loading onClick={() => {}} onArchive={onArchive} />
```

to:

```tsx
<ConversationRow id="c3-hover" title="X" status="loading" onClick={() => {}} onArchive={onArchive} />
```

- [ ] **Step 2: Run failing ConversationRow tests**

Run:

```bash
pnpm exec vitest run src/components/sidebar/__tests__/ConversationRow.test.tsx
```

Expected: FAIL because `ConversationRow` does not accept `status` yet.

- [ ] **Step 3: Update ConversationRow imports**

Modify `src/components/sidebar/ConversationRow.tsx`.

Remove this import:

```tsx
import { Archive, Copy, Loader2, Pencil, Pin, PinOff, ShieldQuestionMark } from 'lucide-react'
```

Replace it with:

```tsx
import { Archive, Copy, Pencil, Pin, PinOff } from 'lucide-react'
```

Remove these imports:

```tsx
import {
  Tooltip,
  TooltipContent,
  TooltipProvider,
  TooltipTrigger,
} from '@/components/ui/tooltip'
```

Replace them with:

```tsx
import {
  Tooltip,
  TooltipContent,
  TooltipProvider,
  TooltipTrigger,
} from '@/components/ui/tooltip'
import { SidebarRowStatusIndicator, type SidebarRowStatus } from './SidebarRowStatusIndicator'
```

The tooltip imports are still needed for row actions.

- [ ] **Step 4: Update ConversationRow props**

Modify the `ConversationRowProps` interface in `src/components/sidebar/ConversationRow.tsx`.

Replace:

```tsx
  loading?: boolean
  waitingApproval?: boolean
```

with:

```tsx
  status?: SidebarRowStatus
```

Modify the function parameters.

Replace:

```tsx
  loading = false,
  waitingApproval = false,
```

with:

```tsx
  status = null,
```

- [ ] **Step 5: Update status-slot logic**

In `ConversationRow`, replace:

```tsx
  const showStatus = !showActions && (waitingApproval || loading)
```

with:

```tsx
  const showStatus = !showActions && status !== null
```

Replace the status rendering block:

```tsx
) : showStatus ? (
  waitingApproval ? (
    <TooltipProvider delayDuration={400}>
      <Tooltip>
        <TooltipTrigger asChild>
          <span className="inline-flex h-5 items-center gap-1 rounded-full bg-primary/10 px-1.5 text-[10px] font-medium leading-none text-primary">
            <ShieldQuestionMark className="h-3 w-3" />
            {t('sidebar.waitingApprovalChip')}
          </span>
        </TooltipTrigger>
        <TooltipContent side="top">{t('sidebar.waitingApprovalTooltip')}</TooltipContent>
      </Tooltip>
    </TooltipProvider>
  ) : (
    <Loader2
      aria-label={t('sidebar.conversationLoading')}
      data-icon="loader"
      className="h-3.5 w-3.5 animate-spin text-muted-foreground"
    />
  )
) : null}
```

with:

```tsx
) : showStatus ? (
  <SidebarRowStatusIndicator status={status} />
) : null}
```

- [ ] **Step 6: Run ConversationRow tests**

Run:

```bash
pnpm exec vitest run src/components/sidebar/__tests__/ConversationRow.test.tsx
```

Expected: PASS.

- [ ] **Step 7: Commit Task 2**

Run:

```bash
git add src/components/sidebar/ConversationRow.tsx \
  src/components/sidebar/__tests__/ConversationRow.test.tsx
git commit -m "refactor(sidebar): use semantic row status in conversation rows"
```

Expected: commit succeeds and does not include unrelated Rust files.

---

### Task 3: Derive Typed Status For Regular Conversations

**Files:**
- Modify: `src/components/sidebar/AppSidebar.tsx`
- Modify: `src/components/sidebar/__tests__/AppSidebar.test.tsx`

- [ ] **Step 1: Update regular-conversation AppSidebar tests first**

Modify `src/components/sidebar/__tests__/AppSidebar.test.tsx`.

Replace the existing test named `shows a compact loading indicator in the conversation action slot` with:

```tsx
it('shows a compact loading indicator in the conversation action slot', () => {
  chatState.conversations = [{ id: 'conv-loading', title: '运行中的对话', workspaceName: '默认项目' }]
  chatState.busyConversations = new Set(['conv-loading'])

  const { container } = render(<AppSidebar />)

  expect(screen.getByRole('button', { name: '运行中的对话' })).toBeInTheDocument()
  expect(container.querySelector('[data-icon="loader"]')).toBeInTheDocument()
})
```

Replace the existing test named `shows a pending approval chip for conversations waiting on user action` with:

```tsx
it('shows permission review chip for regular conversations waiting on permission ask', () => {
  chatState.conversations = [{ id: 'conv-pending', title: '等审批的对话', workspaceName: '默认项目' }]
  chatState.pendingAsks = new Map([
    [
      'call-1',
      {
        conversationId: 'conv-pending',
        runId: 'run-1',
        toolCallId: 'call-1',
        toolName: 'Read',
        message: '需要授权',
        suggestions: null,
        mode: 'default',
        rememberOptions: null,
        defaultDestination: null,
      },
    ],
  ])

  render(<AppSidebar />)

  expect(screen.getByText('审核')).toBeInTheDocument()
  expect(screen.queryByText('审批')).not.toBeInTheDocument()
})
```

Add this test:

```tsx
it('shows waiting reply chip for regular conversations waiting on ask user question', () => {
  chatState.conversations = [{ id: 'conv-question', title: '等回复的对话', workspaceName: '默认项目' }]
  useInteractionStore.getState().addInteraction({
    conversationId: 'conv-question',
    runId: 'run-1',
    interactionId: 'ask-1',
    toolCallId: 'tool-1',
    toolName: 'AskUserQuestion',
    kind: 'AskUserQuestion',
    payload: {
      questions: [{ id: 'topic', question: '想写什么' }],
    },
    primaryModel: 'qwen-plus',
  })

  render(<AppSidebar />)

  expect(screen.getByText('等待回复')).toBeInTheDocument()
})
```

Add this test:

```tsx
it('regular conversation pending status takes priority over loading', () => {
  chatState.conversations = [{ id: 'conv-priority', title: '等审批且运行中', workspaceName: '默认项目' }]
  chatState.busyConversations = new Set(['conv-priority'])
  chatState.pendingAsks = new Map([
    [
      'call-1',
      {
        conversationId: 'conv-priority',
        runId: 'run-1',
        toolCallId: 'call-1',
        toolName: 'Read',
        message: '需要授权',
        suggestions: null,
        mode: 'default',
        rememberOptions: null,
        defaultDestination: null,
      },
    ],
  ])

  const { container } = render(<AppSidebar />)

  expect(screen.getByText('审核')).toBeInTheDocument()
  expect(container.querySelector('[data-icon="loader"]')).not.toBeInTheDocument()
})
```

- [ ] **Step 2: Run failing AppSidebar tests**

Run:

```bash
pnpm exec vitest run src/components/sidebar/__tests__/AppSidebar.test.tsx
```

Expected: FAIL because `AppSidebar` still passes `loading` and `waitingApproval` props to `ConversationRow`.

- [ ] **Step 3: Update AppSidebar imports**

Modify `src/components/sidebar/AppSidebar.tsx`.

Replace:

```tsx
import { selectPendingActionsForSession } from '@/components/chat-scene/pendingActionSelectors'
```

with:

```tsx
import { selectPendingActionForSession } from '@/components/chat-scene/pendingActionSelectors'
```

Add:

```tsx
import { SidebarRowStatusIndicator, type SidebarRowStatus } from './SidebarRowStatusIndicator'
```

- [ ] **Step 4: Replace boolean pending helper with typed helper**

In `AppSidebar`, delete:

```tsx
  const hasPendingUserAction = (conversationId: string) =>
    selectPendingActionsForSession({
      sessionId: conversationId,
      pendingAsks,
      pendingInteractions,
      turnStage: streamStates[conversationId]?.turnStage ?? null,
    }).length > 0
```

Add:

```tsx
  const sidebarStatusForConversation = (conversationId: string): SidebarRowStatus => {
    const action = selectPendingActionForSession({
      sessionId: conversationId,
      pendingAsks,
      pendingInteractions,
      turnStage: streamStates[conversationId]?.turnStage ?? null,
    })

    if (action?.kind === 'permission' || action?.kind === 'stale-permission') {
      return 'permission-review'
    }

    if (action?.kind === 'user-question' || action?.kind === 'stale-interaction') {
      return 'waiting-reply'
    }

    if (isConversationBusy(conversationId)) {
      return 'loading'
    }

    return null
  }
```

- [ ] **Step 5: Update withSidebarState**

Replace:

```tsx
  const withSidebarState = <T extends { id: string }>(conversation: T) => ({
    ...conversation,
    loading: isConversationBusy(conversation.id),
    waitingApproval: hasPendingUserAction(conversation.id),
  })
```

with:

```tsx
  const withSidebarState = <T extends { id: string }>(conversation: T) => ({
    ...conversation,
    status: sidebarStatusForConversation(conversation.id),
  })
```

- [ ] **Step 6: Update ConversationRow call sites**

In `renderFlatTab`, replace:

```tsx
            loading={isConversationBusy(conversation.id)}
            waitingApproval={hasPendingUserAction(conversation.id)}
```

with:

```tsx
            status={sidebarStatusForConversation(conversation.id)}
```

In global pinned rendering, replace:

```tsx
                  loading={isConversationBusy(conversation.id)}
                  waitingApproval={hasPendingUserAction(conversation.id)}
```

with:

```tsx
                  status={sidebarStatusForConversation(conversation.id)}
```

Check `ConversationTree` props. If `ConversationTree` passes through `loading` and `waitingApproval` to `ConversationRow`, update its conversation item type and render path to use `status` instead. Use this search:

```bash
rg -n "waitingApproval|loading=\\{|\\.loading|\\.waitingApproval" src/components/sidebar
```

Expected after this task: no `waitingApproval` references remain in sidebar components.

- [ ] **Step 7: Run regular-conversation AppSidebar tests**

Run:

```bash
pnpm exec vitest run src/components/sidebar/__tests__/AppSidebar.test.tsx
```

Expected: PASS for the regular-conversation status tests. If `ConversationTree` still expects `loading/waitingApproval`, update it to accept `status` and rerun.

- [ ] **Step 8: Commit Task 3**

Run:

```bash
git add src/components/sidebar/AppSidebar.tsx \
  src/components/sidebar/ConversationTree.tsx \
  src/components/sidebar/ProjectAccordion.tsx \
  src/components/sidebar/__tests__/AppSidebar.test.tsx
git commit -m "feat(sidebar): derive typed statuses for conversation rows"
```

Expected: commit succeeds. If `ConversationTree.tsx` or `ProjectAccordion.tsx` were not modified, omit them from `git add`.

---

### Task 4: Show Status On IM Channel Conversation Rows

**Files:**
- Modify: `src/components/sidebar/AppSidebar.tsx`
- Modify: `src/components/sidebar/__tests__/AppSidebar.test.tsx`

- [ ] **Step 1: Add channel-row tests first**

Modify `src/components/sidebar/__tests__/AppSidebar.test.tsx`.

Add this test:

```tsx
it('shows permission review chip for channel conversations waiting on permission ask', async () => {
  localStorage.setItem('aijia-sidebar-tab', 'channel')
  chatState.pendingAsks = new Map([
    [
      'call-channel',
      {
        conversationId: 'dt-session-1',
        runId: 'run-1',
        toolCallId: 'call-channel',
        toolName: 'Read',
        message: '需要授权',
        suggestions: null,
        mode: 'default',
        rememberOptions: null,
        defaultDestination: null,
      },
    ],
  ])

  render(<AppSidebar />)

  expect(screen.getByRole('button', { name: /钉钉私聊/ })).toBeInTheDocument()
  expect(screen.getByText('审核')).toBeInTheDocument()
})
```

Add this test:

```tsx
it('shows waiting reply chip for channel conversations waiting on ask user question', () => {
  localStorage.setItem('aijia-sidebar-tab', 'channel')
  useInteractionStore.getState().addInteraction({
    conversationId: 'dt-session-1',
    runId: 'run-1',
    interactionId: 'ask-channel',
    toolCallId: 'tool-channel',
    toolName: 'AskUserQuestion',
    kind: 'AskUserQuestion',
    payload: {
      questions: [{ id: 'topic', question: '想写什么' }],
    },
    primaryModel: 'qwen-plus',
  })

  render(<AppSidebar />)

  expect(screen.getByRole('button', { name: /钉钉私聊/ })).toBeInTheDocument()
  expect(screen.getByText('等待回复')).toBeInTheDocument()
})
```

Add this test:

```tsx
it('shows loader for busy channel conversations', () => {
  localStorage.setItem('aijia-sidebar-tab', 'channel')
  chatState.busyConversations = new Set(['dt-session-1'])

  const { container } = render(<AppSidebar />)

  expect(screen.getByRole('button', { name: /钉钉私聊/ })).toBeInTheDocument()
  expect(container.querySelector('[data-icon="loader"]')).toBeInTheDocument()
})
```

Add this test:

```tsx
it('channel row status takes priority over unread count', () => {
  localStorage.setItem('aijia-sidebar-tab', 'channel')
  chatState.pendingAsks = new Map([
    [
      'call-channel',
      {
        conversationId: 'dt-session-1',
        runId: 'run-1',
        toolCallId: 'call-channel',
        toolName: 'Read',
        message: '需要授权',
        suggestions: null,
        mode: 'default',
        rememberOptions: null,
        defaultDestination: null,
      },
    ],
  ])

  render(<AppSidebar />)

  expect(screen.getByText('审核')).toBeInTheDocument()
  expect(screen.queryByText('7')).not.toBeInTheDocument()
})
```

Before this last test can assert unread priority, update the `useChannelStore` mock conversation in the test file from:

```tsx
unreadCount: 0,
```

to:

```tsx
unreadCount: 7,
```

Then update any existing tests that relied on unread count being zero by asserting row label with regex instead of exact button text when needed.

- [ ] **Step 2: Run failing channel-row tests**

Run:

```bash
pnpm exec vitest run src/components/sidebar/__tests__/AppSidebar.test.tsx
```

Expected: FAIL because `ChannelConversationRow` does not accept or render status.

- [ ] **Step 3: Update ChannelConversationRow props**

Modify `ChannelConversationRowProps` in `src/components/sidebar/AppSidebar.tsx`.

Replace:

```tsx
interface ChannelConversationRowProps {
  active: boolean
  conversation: ChannelConversation
  label: string
  copyLabel: string
  onSelect: () => void
}
```

with:

```tsx
interface ChannelConversationRowProps {
  active: boolean
  conversation: ChannelConversation
  label: string
  copyLabel: string
  status?: SidebarRowStatus
  onSelect: () => void
}
```

Update the function params:

```tsx
function ChannelConversationRow({
  active,
  conversation,
  label,
  copyLabel,
  status = null,
  onSelect,
}: ChannelConversationRowProps) {
```

- [ ] **Step 4: Render status before unread badge**

In `ChannelConversationRow`, replace:

```tsx
          <span className="truncate">{label}</span>
          {conversation.unreadCount > 0 && (
            <span className="ml-2 rounded-full bg-primary px-1.5 text-xs text-primary-foreground">
              {conversation.unreadCount}
            </span>
          )}
```

with:

```tsx
          <span className="truncate">{label}</span>
          <span className="ml-2 flex min-w-[44px] shrink-0 items-center justify-end">
            {status ? (
              <SidebarRowStatusIndicator status={status} />
            ) : conversation.unreadCount > 0 ? (
              <span className="rounded-full bg-primary px-1.5 text-xs text-primary-foreground">
                {conversation.unreadCount}
              </span>
            ) : null}
          </span>
```

- [ ] **Step 5: Pass status in renderChannelRows**

In `renderChannelRows`, add:

```tsx
        status={sidebarStatusForConversation(conversation.sessionId)}
```

The full call should include:

```tsx
      <ChannelConversationRow
        key={conversation.sessionId}
        active={channelActiveSessionId === conversation.sessionId}
        conversation={conversation}
        label={channelConversationLabel(conversation, t)}
        copyLabel={t('sidebar.copyConversationId')}
        status={sidebarStatusForConversation(conversation.sessionId)}
        onSelect={() => selectChannelSession(conversation.sessionId)}
      />
```

- [ ] **Step 6: Run AppSidebar tests**

Run:

```bash
pnpm exec vitest run src/components/sidebar/__tests__/AppSidebar.test.tsx
```

Expected: PASS.

- [ ] **Step 7: Commit Task 4**

Run:

```bash
git add src/components/sidebar/AppSidebar.tsx \
  src/components/sidebar/__tests__/AppSidebar.test.tsx
git commit -m "feat(sidebar): show statuses on channel conversation rows"
```

Expected: commit succeeds and does not include unrelated Rust files.

---

### Task 5: Final Sidebar Verification

**Files:**
- Read: `src/components/sidebar/ConversationRow.tsx`
- Read: `src/components/sidebar/AppSidebar.tsx`
- Read: `src/components/sidebar/SidebarRowStatusIndicator.tsx`
- Read: `src/components/sidebar/__tests__/ConversationRow.test.tsx`
- Read: `src/components/sidebar/__tests__/AppSidebar.test.tsx`

- [ ] **Step 1: Search for obsolete sidebar approval props**

Run:

```bash
rg -n "waitingApproval|waitingApprovalChip|waitingApprovalTooltip|hasPendingUserAction" src/components/sidebar src/i18n
```

Expected: no matches in sidebar components. Existing i18n keys may remain only if intentionally kept for backward compatibility; they should not be used by sidebar components after this work.

- [ ] **Step 2: Run focused sidebar tests**

Run:

```bash
pnpm exec vitest run \
  src/components/sidebar/__tests__/SidebarRowStatusIndicator.test.tsx \
  src/components/sidebar/__tests__/ConversationRow.test.tsx \
  src/components/sidebar/__tests__/AppSidebar.test.tsx
```

Expected: PASS.

- [ ] **Step 3: Run broader frontend tests**

Run:

```bash
pnpm test
```

Expected: PASS, or only unrelated pre-existing failures. If unrelated failures occur, capture the failing test names and do not fix unrelated areas in this task.

- [ ] **Step 4: Review staged scope**

Run:

```bash
git status --short
git diff --stat
```

Expected changed files for this feature are limited to:

- `src/components/sidebar/SidebarRowStatusIndicator.tsx`
- `src/components/sidebar/__tests__/SidebarRowStatusIndicator.test.tsx`
- `src/components/sidebar/ConversationRow.tsx`
- `src/components/sidebar/__tests__/ConversationRow.test.tsx`
- `src/components/sidebar/AppSidebar.tsx`
- `src/components/sidebar/__tests__/AppSidebar.test.tsx`
- `src/i18n/zh-CN.json`
- `src/i18n/en-US.json`
- `src/components/sidebar/ConversationTree.tsx` only if required by its props.
- `src/components/sidebar/ProjectAccordion.tsx` only if required by its props.

- [ ] **Step 5: Commit final verification note if needed**

If Task 5 required no code changes, do not create a commit. If it required small test-only or prop-threading fixes, commit only those files:

```bash
git add <changed-sidebar-files-only>
git commit -m "test(sidebar): cover row interaction statuses"
```

Expected: commit succeeds only if Task 5 changed files.

## Execution Handoff

After this plan is approved, execute with `superpowers:executing-plans` or `superpowers:subagent-driven-development`.

Suggested command to continue in the main implementation thread:

```text
Use superpowers:executing-plans to execute docs/superpowers/plans/2026-06-09-sidebar-row-interaction-status.md.
This is a sidebar frontend status-display task only.
Do not continue the earlier human interaction runtime plan.
Do not modify unrelated Rust files.
Follow TDD and commit each task checkpoint.
```
