# Permission Approval Surface Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build a reusable pending-action surface for permission approval and AskUserQuestion, remove automatic interaction timeouts, and make all IM channels handle pending approval with deterministic actions plus queue-and-ACK behavior.

**Architecture:** Frontend pending actions are derived from shared stores by conversation/session and rendered in `ChatBottomArea` instead of global dialogs. Backend IM handling uses one shared coordinator that owns pending permission/user-question state, deterministic `/approve` and `/answer` commands, no deadline timer, and a pre-dispatch gate that each channel worker calls before normal message dispatch. Pending ordinary IM messages are queued through the existing `PendingQueueManager` and ACKed immediately.

**Tech Stack:** React, TypeScript, Zustand, Vitest, Tauri IPC, Rust, Tokio, existing `PendingQueueManager`, existing IM connector manager.

---

## File Structure

Create:

- `src/components/chat-scene/PendingActionSurface.tsx` — bottom-of-chat replacement surface for permission asks and user questions.
- `src/components/chat-scene/pendingActionSelectors.ts` — pure selectors that choose the current pending action for a conversation/session.
- `src/components/chat-scene/__tests__/PendingActionSurface.test.tsx` — unit tests for allow, deny, cancel, and user-question submit behavior.
- `src/components/chat-scene/__tests__/pendingActionSelectors.test.ts` — selector tests for conversation scoping and priority.
- `src-tauri/tests/review_im_approval_policy_test.rs` — static policy review test that checks every IM channel consults the shared coordinator or is intentionally excluded with a documented reason.

Modify:

- `docs/superpowers/specs/2026-06-03-permission-approval-surface-design.md` — add explicit AskUserQuestion no-timeout and multi-channel scoped coordinator wording before implementation starts.
- `src/components/chat-scene/ChatBottomArea.tsx` — render `PendingActionSurface` instead of `RichComposer` for active pending actions.
- `src/components/chat-scene/__tests__/ChatBottomArea.test.tsx` — add route-switch restoration and cross-conversation isolation tests.
- `src/App.tsx` — remove global `PermissionAskDialog` and `AskUserQuestionDialog` once bottom-surface wiring is in place.
- `src/stores/interactionStore.ts` — expose selector-friendly helpers if direct array filtering becomes repetitive.
- `src/stores/streamingStore.ts` — keep existing `pendingAsks` structure, add `createdAt` only if needed for deterministic ordering.
- `src-tauri/src/connector/im/shared/ask_coordinator.rs` — remove deadline auto-resolution, add deterministic command parsing and queue/ACK outcomes.
- `src-tauri/src/connector/im/manager.rs` — run the shared coordinator before normal dispatch in each channel worker.
- `src-tauri/src/connector/im/shared/reply_manager.rs` — add or reuse ACK delivery for pending approval queueing.
- Channel connector files only if a channel needs a small ACK send helper already unavailable through manager: `dingtalk`, `feishu`, `wecom`, `wechat`, `telegram`, `whatsapp`.

Do not touch unrelated dirty files except where this plan explicitly calls for them. Before each task, run `git status --short` and inspect overlapping changes.

---

### Task 1: Spec Amendment for AskUserQuestion and Scoped Coordinator

**Files:**
- Modify: `docs/superpowers/specs/2026-06-03-permission-approval-surface-design.md`

- [ ] **Step 1: Add explicit AskUserQuestion requirements to the spec**

Add this text under `Goals`:

```markdown
6. Treat `UserInteractionRequired` / AskUserQuestion as the same class of user-interaction blocking state: no automatic timeout, app input replacement, deterministic IM answer path, and lifecycle cleanup only.
```

Add this text under `App Surface` after the permission bullet list:

```markdown
When the active conversation has a pending AskUserQuestion interaction:

- Replace the composer area with `PendingActionSurface`.
- Render the question form, options, or free-text answer controls from the interaction payload.
- Provide explicit actions: submit answer and cancel current task.
- Do not show a countdown.
- Do not allow normal text input in that conversation until the interaction is resolved.
```

Add this text under `IM Channel Policy` after the `/approve` fallback block:

````markdown
AskUserQuestion fallback commands use the same deterministic pattern:

```text
/answer <interaction-id> <answer text>
/answer <interaction-id> {"answers":["structured answer"]}
/answer <interaction-id> cancel
```

Free-form messages are queued behind the pending interaction unless they arrive from a native answer control payload.
````

- [ ] **Step 2: Run red-flag scan**

Run:

```bash
awk 'BEGIN { todo = "TO" "DO"; tbd = "TB" "D"; wait = "待" "定"; undecided = "未" "定" } index($0, todo) || index($0, tbd) || index($0, wait) || index($0, undecided) { print FILENAME ":" FNR ":" $0 }' docs/superpowers/specs/2026-06-03-permission-approval-surface-design.md
```

Expected: no output.

- [ ] **Step 3: Commit spec amendment**

```bash
git add docs/superpowers/specs/2026-06-03-permission-approval-surface-design.md
git commit -m "docs: include ask-user interaction approval policy"
```

---

### Task 2: Frontend Pending Action Selector

**Files:**
- Create: `src/components/chat-scene/pendingActionSelectors.ts`
- Create: `src/components/chat-scene/__tests__/pendingActionSelectors.test.ts`

- [ ] **Step 1: Write selector tests**

Create `src/components/chat-scene/__tests__/pendingActionSelectors.test.ts`:

```ts
import { describe, expect, it } from 'vitest'
import type { InteractionRequiredPayload } from '@/lib/tauri'
import type { PendingAsk } from '@/stores/streamingStore'
import {
  selectPendingActionForSession,
  type PendingAction,
} from '../pendingActionSelectors'

function ask(overrides: Partial<PendingAsk> = {}): PendingAsk {
  return {
    conversationId: 'conv-1',
    runId: 'run-1',
    toolCallId: 'tool-1',
    toolName: 'Read',
    message: 'Read local file?',
    suggestions: ['allow', 'deny'],
    mode: 'default',
    rememberOptions: ['session', 'workspace'],
    defaultDestination: 'session',
    ...overrides,
  }
}

function interaction(
  overrides: Partial<InteractionRequiredPayload> = {},
): InteractionRequiredPayload {
  return {
    conversationId: 'conv-1',
    runId: 'run-1',
    interactionId: 'ask-1',
    toolCallId: 'tool-1',
    toolName: 'AskUser',
    kind: 'ask_user',
    payload: {
      questions: [
        {
          id: 'q1',
          question: 'Pick one',
          options: [
            { label: 'A', description: 'A path' },
            { label: 'B', description: 'B path' },
          ],
        },
      ],
    },
    ...overrides,
  }
}

describe('selectPendingActionForSession', () => {
  it('returns null when no session is active', () => {
    const result = selectPendingActionForSession({
      sessionId: null,
      pendingAsks: new Map([['tool-1', ask()]]),
      pendingInteractions: [interaction()],
    })

    expect(result).toBeNull()
  })

  it('selects the permission ask for the active conversation', () => {
    const result = selectPendingActionForSession({
      sessionId: 'conv-1',
      pendingAsks: new Map([
        ['tool-other', ask({ conversationId: 'conv-2', toolCallId: 'tool-other' })],
        ['tool-1', ask()],
      ]),
      pendingInteractions: [],
    })

    expect(result).toEqual<PendingAction>({
      kind: 'permission',
      ask: ask(),
    })
  })

  it('does not select a pending ask from another conversation', () => {
    const result = selectPendingActionForSession({
      sessionId: 'conv-1',
      pendingAsks: new Map([
        ['tool-other', ask({ conversationId: 'conv-2', toolCallId: 'tool-other' })],
      ]),
      pendingInteractions: [],
    })

    expect(result).toBeNull()
  })

  it('selects AskUserQuestion when there is no active permission ask', () => {
    const result = selectPendingActionForSession({
      sessionId: 'conv-1',
      pendingAsks: new Map(),
      pendingInteractions: [interaction()],
    })

    expect(result).toEqual<PendingAction>({
      kind: 'user-question',
      interaction: interaction(),
    })
  })

  it('prioritizes permission over AskUserQuestion for the same conversation', () => {
    const result = selectPendingActionForSession({
      sessionId: 'conv-1',
      pendingAsks: new Map([['tool-1', ask()]]),
      pendingInteractions: [interaction()],
    })

    expect(result?.kind).toBe('permission')
  })
})
```

- [ ] **Step 2: Run the selector test and verify it fails**

Run:

```bash
pnpm vitest run src/components/chat-scene/__tests__/pendingActionSelectors.test.ts
```

Expected: FAIL because `pendingActionSelectors.ts` does not exist.

- [ ] **Step 3: Implement selectors**

Create `src/components/chat-scene/pendingActionSelectors.ts`:

```ts
import type { InteractionRequiredPayload } from '@/lib/tauri'
import type { PendingAsk } from '@/stores/streamingStore'

export type PendingPermissionAction = {
  kind: 'permission'
  ask: PendingAsk
}

export type PendingUserQuestionAction = {
  kind: 'user-question'
  interaction: InteractionRequiredPayload
}

export type PendingAction = PendingPermissionAction | PendingUserQuestionAction

export function selectPendingActionForSession({
  sessionId,
  pendingAsks,
  pendingInteractions,
}: {
  sessionId: string | null
  pendingAsks: Map<string, PendingAsk>
  pendingInteractions: InteractionRequiredPayload[]
}): PendingAction | null {
  if (!sessionId) return null

  for (const ask of pendingAsks.values()) {
    if (ask.conversationId === sessionId) {
      return { kind: 'permission', ask }
    }
  }

  const interaction = pendingInteractions.find(
    (item) => item.conversationId === sessionId,
  )
  return interaction ? { kind: 'user-question', interaction } : null
}
```

- [ ] **Step 4: Run selector test and commit**

Run:

```bash
pnpm vitest run src/components/chat-scene/__tests__/pendingActionSelectors.test.ts
```

Expected: PASS.

Commit:

```bash
git add src/components/chat-scene/pendingActionSelectors.ts src/components/chat-scene/__tests__/pendingActionSelectors.test.ts
git commit -m "test: add pending action selector"
```

---

### Task 3: PendingActionSurface Component

**Files:**
- Create: `src/components/chat-scene/PendingActionSurface.tsx`
- Create: `src/components/chat-scene/__tests__/PendingActionSurface.test.tsx`

- [ ] **Step 1: Write component tests**

Create `src/components/chat-scene/__tests__/PendingActionSurface.test.tsx`:

```tsx
import '@testing-library/jest-dom'
import { describe, expect, it, vi } from 'vitest'
import { render, screen, waitFor } from '@testing-library/react'
import userEvent from '@testing-library/user-event'
import type { PendingAsk } from '@/stores/streamingStore'
import type { InteractionRequiredPayload } from '@/lib/tauri'
import { PendingActionSurface } from '../PendingActionSurface'

function permissionAsk(): PendingAsk {
  return {
    conversationId: 'conv-1',
    runId: 'run-1',
    toolCallId: 'tool-1',
    toolName: 'Read',
    message: 'Read /tmp/a.txt?',
    suggestions: ['Only allow if the path is expected.'],
    mode: 'default',
    rememberOptions: ['session', 'workspace', 'user'],
    defaultDestination: 'session',
  }
}

function userQuestion(): InteractionRequiredPayload {
  return {
    conversationId: 'conv-1',
    runId: 'run-1',
    interactionId: 'ask-1',
    toolCallId: 'tool-1',
    toolName: 'AskUser',
    kind: 'ask_user',
    payload: {
      questions: [
        {
          id: 'q1',
          question: 'Which branch?',
          options: [
            { label: 'main', description: 'Use main branch' },
            { label: 'dev', description: 'Use dev branch' },
          ],
        },
      ],
    },
  }
}

describe('PendingActionSurface', () => {
  it('allows a permission request with selected remember destination', async () => {
    const user = userEvent.setup()
    const onAllow = vi.fn().mockResolvedValue(undefined)

    render(
      <PendingActionSurface
        action={{ kind: 'permission', ask: permissionAsk() }}
        onAllowPermission={onAllow}
        onDenyPermission={vi.fn()}
        onCancelPermission={vi.fn()}
        onSubmitInteraction={vi.fn()}
        onCancelInteraction={vi.fn()}
      />,
    )

    await user.click(screen.getByRole('radio', { name: '记住到工作区' }))
    await user.click(screen.getByRole('button', { name: '允许' }))

    await waitFor(() =>
      expect(onAllow).toHaveBeenCalledWith('tool-1', {
        remember: true,
        destination: 'workspace',
      }),
    )
  })

  it('denies a permission request', async () => {
    const user = userEvent.setup()
    const onDeny = vi.fn().mockResolvedValue(undefined)

    render(
      <PendingActionSurface
        action={{ kind: 'permission', ask: permissionAsk() }}
        onAllowPermission={vi.fn()}
        onDenyPermission={onDeny}
        onCancelPermission={vi.fn()}
        onSubmitInteraction={vi.fn()}
        onCancelInteraction={vi.fn()}
      />,
    )

    await user.click(screen.getByRole('button', { name: '拒绝' }))

    await waitFor(() =>
      expect(onDeny).toHaveBeenCalledWith('tool-1', {
        remember: false,
        destination: 'session',
      }),
    )
  })

  it('submits a user question answer', async () => {
    const user = userEvent.setup()
    const onSubmit = vi.fn().mockResolvedValue(undefined)

    render(
      <PendingActionSurface
        action={{ kind: 'user-question', interaction: userQuestion() }}
        onAllowPermission={vi.fn()}
        onDenyPermission={vi.fn()}
        onCancelPermission={vi.fn()}
        onSubmitInteraction={onSubmit}
        onCancelInteraction={vi.fn()}
      />,
    )

    await user.click(screen.getByRole('radio', { name: /main/ }))
    await user.click(screen.getByRole('button', { name: '继续' }))

    await waitFor(() =>
      expect(onSubmit).toHaveBeenCalledWith('ask-1', {
        answers: { q1: 'main' },
      }),
    )
  })
})
```

- [ ] **Step 2: Run component test and verify it fails**

Run:

```bash
pnpm vitest run src/components/chat-scene/__tests__/PendingActionSurface.test.tsx
```

Expected: FAIL because `PendingActionSurface.tsx` does not exist.

- [ ] **Step 3: Implement component**

Create `src/components/chat-scene/PendingActionSurface.tsx`:

```tsx
import { useMemo, useState } from 'react'
import type { PendingAction } from './pendingActionSelectors'

type PermissionDestination = 'session' | 'workspace' | 'user'

type PermissionDecision = {
  remember: boolean
  destination: PermissionDestination
}

type Props = {
  action: PendingAction
  onAllowPermission: (toolCallId: string, decision: PermissionDecision) => Promise<void> | void
  onDenyPermission: (toolCallId: string, decision: PermissionDecision) => Promise<void> | void
  onCancelPermission: (toolCallId: string) => Promise<void> | void
  onSubmitInteraction: (interactionId: string, value: unknown) => Promise<void> | void
  onCancelInteraction: (interactionId: string) => Promise<void> | void
}

function permissionDestinations(action: Extract<PendingAction, { kind: 'permission' }>) {
  const options = action.ask.rememberOptions?.length
    ? action.ask.rememberOptions
    : ['session']
  return options as PermissionDestination[]
}

function destinationLabel(destination: PermissionDestination) {
  if (destination === 'workspace') return '记住到工作区'
  if (destination === 'user') return '记住到用户级'
  return '仅本次'
}

function PermissionPanel({
  action,
  onAllowPermission,
  onDenyPermission,
  onCancelPermission,
}: Pick<Props, 'onAllowPermission' | 'onDenyPermission' | 'onCancelPermission'> & {
  action: Extract<PendingAction, { kind: 'permission' }>
}) {
  const destinations = useMemo(() => permissionDestinations(action), [action])
  const [destination, setDestination] = useState<PermissionDestination>(
    action.ask.defaultDestination ?? destinations[0] ?? 'session',
  )
  const decision = { remember: destination !== 'session', destination }

  return (
    <section className="rounded-lg border border-border bg-card p-4 shadow-[var(--shadow-md)]">
      <div className="mb-3 flex items-center justify-between gap-3">
        <div>
          <p className="text-sm font-medium">需要权限确认</p>
          <p className="text-xs text-muted-foreground">{action.ask.toolName}</p>
        </div>
        <button
          type="button"
          className="rounded-md border border-border px-3 py-1.5 text-sm"
          onClick={() => void onCancelPermission(action.ask.toolCallId)}
        >
          取消当前任务
        </button>
      </div>
      <p className="mb-3 whitespace-pre-wrap text-sm">{action.ask.message}</p>
      {action.ask.suggestions?.length ? (
        <ul className="mb-3 list-disc pl-5 text-xs text-muted-foreground">
          {action.ask.suggestions.map((suggestion) => (
            <li key={suggestion}>{suggestion}</li>
          ))}
        </ul>
      ) : null}
      <fieldset className="mb-4 flex flex-wrap gap-3">
        {destinations.map((item) => (
          <label key={item} className="flex items-center gap-2 text-sm">
            <input
              type="radio"
              name={`permission-${action.ask.toolCallId}`}
              checked={destination === item}
              onChange={() => setDestination(item)}
            />
            {destinationLabel(item)}
          </label>
        ))}
      </fieldset>
      <div className="flex justify-end gap-2">
        <button
          type="button"
          className="rounded-md border border-border px-3 py-1.5 text-sm"
          onClick={() => void onDenyPermission(action.ask.toolCallId, decision)}
        >
          拒绝
        </button>
        <button
          type="button"
          className="rounded-md bg-primary px-3 py-1.5 text-sm text-primary-foreground"
          onClick={() => void onAllowPermission(action.ask.toolCallId, decision)}
        >
          允许
        </button>
      </div>
    </section>
  )
}

function UserQuestionPanel({
  action,
  onSubmitInteraction,
  onCancelInteraction,
}: Pick<Props, 'onSubmitInteraction' | 'onCancelInteraction'> & {
  action: Extract<PendingAction, { kind: 'user-question' }>
}) {
  const questions = Array.isArray(action.interaction.payload?.questions)
    ? action.interaction.payload.questions
    : []
  const [answers, setAnswers] = useState<Record<string, string>>({})

  return (
    <section className="rounded-lg border border-border bg-card p-4 shadow-[var(--shadow-md)]">
      <div className="mb-3 flex items-center justify-between gap-3">
        <div>
          <p className="text-sm font-medium">需要补充信息</p>
          <p className="text-xs text-muted-foreground">{action.interaction.toolName}</p>
        </div>
        <button
          type="button"
          className="rounded-md border border-border px-3 py-1.5 text-sm"
          onClick={() => void onCancelInteraction(action.interaction.interactionId)}
        >
          取消当前任务
        </button>
      </div>
      <div className="space-y-4">
        {questions.map((question: { id: string; question: string; options?: Array<{ label: string; description?: string }> }) => (
          <fieldset key={question.id} className="space-y-2">
            <legend className="text-sm">{question.question}</legend>
            {question.options?.length ? (
              question.options.map((option) => (
                <label key={option.label} className="flex items-start gap-2 text-sm">
                  <input
                    type="radio"
                    name={`interaction-${action.interaction.interactionId}-${question.id}`}
                    checked={answers[question.id] === option.label}
                    onChange={() =>
                      setAnswers((current) => ({ ...current, [question.id]: option.label }))
                    }
                  />
                  <span>
                    <span>{option.label}</span>
                    {option.description ? (
                      <span className="block text-xs text-muted-foreground">{option.description}</span>
                    ) : null}
                  </span>
                </label>
              ))
            ) : (
              <textarea
                className="min-h-20 w-full rounded-md border border-border bg-background p-2 text-sm"
                value={answers[question.id] ?? ''}
                onChange={(event) =>
                  setAnswers((current) => ({ ...current, [question.id]: event.target.value }))
                }
              />
            )}
          </fieldset>
        ))}
      </div>
      <div className="mt-4 flex justify-end">
        <button
          type="button"
          className="rounded-md bg-primary px-3 py-1.5 text-sm text-primary-foreground"
          onClick={() =>
            void onSubmitInteraction(action.interaction.interactionId, { answers })
          }
        >
          继续
        </button>
      </div>
    </section>
  )
}

export function PendingActionSurface(props: Props) {
  return props.action.kind === 'permission' ? (
    <PermissionPanel
      action={props.action}
      onAllowPermission={props.onAllowPermission}
      onDenyPermission={props.onDenyPermission}
      onCancelPermission={props.onCancelPermission}
    />
  ) : (
    <UserQuestionPanel
      action={props.action}
      onSubmitInteraction={props.onSubmitInteraction}
      onCancelInteraction={props.onCancelInteraction}
    />
  )
}
```

- [ ] **Step 4: Run component test and commit**

Run:

```bash
pnpm vitest run src/components/chat-scene/__tests__/PendingActionSurface.test.tsx
```

Expected: PASS.

Commit:

```bash
git add src/components/chat-scene/PendingActionSurface.tsx src/components/chat-scene/__tests__/PendingActionSurface.test.tsx
git commit -m "feat: add pending action surface"
```

---

### Task 4: Wire PendingActionSurface into ChatBottomArea

**Files:**
- Modify: `src/components/chat-scene/ChatBottomArea.tsx`
- Modify: `src/components/chat-scene/__tests__/ChatBottomArea.test.tsx`
- Modify: `src/App.tsx`

- [ ] **Step 1: Add ChatBottomArea tests for interception and restoration**

Add these imports at the top of `src/components/chat-scene/__tests__/ChatBottomArea.test.tsx` if they are not already present:

```tsx
import { useChatStore } from '@/stores/chatStore'
import { useStreamingStore } from '@/stores/streamingStore'
import { useInteractionStore } from '@/stores/interactionStore'
```

Append these tests to `src/components/chat-scene/__tests__/ChatBottomArea.test.tsx`:

```tsx
it('replaces the composer with permission surface for the active conversation', async () => {
  useStreamingStore.getState().addPendingAsk({
    conversationId: 'conv-1',
    runId: 'run-1',
    toolCallId: 'tool-1',
    toolName: 'Read',
    message: 'Read file?',
    suggestions: [],
    mode: 'default',
    rememberOptions: ['session'],
    defaultDestination: 'session',
  })

  render(<ChatBottomArea />)

  expect(await screen.findByText('需要权限确认')).toBeInTheDocument()
  expect(document.querySelector('.ProseMirror')).toBeNull()
})

it('does not replace the composer for another conversation pending ask', async () => {
  useStreamingStore.getState().addPendingAsk({
    conversationId: 'conv-2',
    runId: 'run-2',
    toolCallId: 'tool-2',
    toolName: 'Read',
    message: 'Other conversation',
    suggestions: [],
    mode: 'default',
    rememberOptions: ['session'],
    defaultDestination: 'session',
  })

  render(<ChatBottomArea />)

  await waitFor(() => expect(document.querySelector('.ProseMirror')).toBeTruthy())
  expect(screen.queryByText('需要权限确认')).not.toBeInTheDocument()
})

it('restores the permission surface after switching back to the pending conversation', async () => {
  useStreamingStore.getState().addPendingAsk({
    conversationId: 'conv-1',
    runId: 'run-1',
    toolCallId: 'tool-1',
    toolName: 'Read',
    message: 'Read file?',
    suggestions: [],
    mode: 'default',
    rememberOptions: ['session'],
    defaultDestination: 'session',
  })

  const { rerender } = render(<ChatBottomArea />)
  expect(await screen.findByText('需要权限确认')).toBeInTheDocument()

  useChatStore.setState({ activeConversationId: 'conv-2' })
  rerender(<ChatBottomArea />)
  await waitFor(() => expect(document.querySelector('.ProseMirror')).toBeTruthy())

  useChatStore.setState({ activeConversationId: 'conv-1' })
  rerender(<ChatBottomArea />)
  expect(await screen.findByText('需要权限确认')).toBeInTheDocument()
})

it('replaces the composer with AskUserQuestion surface for the active conversation', async () => {
  useInteractionStore.getState().addInteraction({
    conversationId: 'conv-1',
    runId: 'run-1',
    interactionId: 'ask-1',
    toolCallId: 'tool-1',
    toolName: 'AskUser',
    kind: 'ask_user',
    payload: {
      questions: [{ id: 'q1', question: 'Need input?' }],
    },
  })

  render(<ChatBottomArea />)

  expect(await screen.findByText('需要补充信息')).toBeInTheDocument()
  expect(document.querySelector('.ProseMirror')).toBeNull()
})
```

In the same `beforeEach`, add:

```ts
useStreamingStore.setState({ pendingAsks: new Map() })
useInteractionStore.setState({ pendingInteractions: [] })
```

- [ ] **Step 2: Run ChatBottomArea tests and verify failure**

Run:

```bash
pnpm vitest run src/components/chat-scene/__tests__/ChatBottomArea.test.tsx
```

Expected: FAIL because `ChatBottomArea` still always renders `RichComposer`.

- [ ] **Step 3: Wire component into ChatBottomArea**

Modify imports in `src/components/chat-scene/ChatBottomArea.tsx`:

```ts
import { PendingActionSurface } from './PendingActionSurface'
import { selectPendingActionForSession } from './pendingActionSelectors'
import {
  approvePermissionRequest,
  cancelPermissionRequest,
  denyPermissionRequest,
  submitUserInteraction,
  cancelUserInteraction,
  pendingSnapshotForSession,
} from '@/lib/tauri'
import { useStreamingStore } from '@/stores/streamingStore'
import { useInteractionStore } from '@/stores/interactionStore'
```

Inside `ChatBottomArea`, add store reads and action selection:

```ts
const pendingAsks = useStreamingStore((s) => s.pendingAsks)
const removePendingAsk = useStreamingStore((s) => s.removePendingAsk)
const pendingInteractions = useInteractionStore((s) => s.pendingInteractions)
const removeInteraction = useInteractionStore((s) => s.removeInteraction)
const pendingAction = selectPendingActionForSession({
  sessionId: pendingSessionId,
  pendingAsks,
  pendingInteractions,
})
```

Add handlers:

```ts
const handleAllowPermission = useCallback(async (
  toolCallId: string,
  decision: { remember: boolean; destination: 'session' | 'workspace' | 'user' },
) => {
  await approvePermissionRequest(toolCallId, null, decision.remember, decision.destination)
  removePendingAsk(toolCallId)
}, [removePendingAsk])

const handleDenyPermission = useCallback(async (
  toolCallId: string,
  decision: { remember: boolean; destination: 'session' | 'workspace' | 'user' },
) => {
  await denyPermissionRequest(toolCallId, undefined, decision.remember, decision.destination)
  removePendingAsk(toolCallId)
}, [removePendingAsk])

const handleCancelPermission = useCallback(async (toolCallId: string) => {
  await cancelPermissionRequest(toolCallId)
  removePendingAsk(toolCallId)
}, [removePendingAsk])

const handleSubmitInteraction = useCallback(async (interactionId: string, value: unknown) => {
  await submitUserInteraction(interactionId, value)
  removeInteraction(interactionId)
}, [removeInteraction])

const handleCancelInteraction = useCallback(async (interactionId: string) => {
  await cancelUserInteraction(interactionId)
  removeInteraction(interactionId)
}, [removeInteraction])
```

Replace the `RichComposer` block at the bottom with:

```tsx
{pendingAction ? (
  <PendingActionSurface
    action={pendingAction}
    onAllowPermission={handleAllowPermission}
    onDenyPermission={handleDenyPermission}
    onCancelPermission={handleCancelPermission}
    onSubmitInteraction={handleSubmitInteraction}
    onCancelInteraction={handleCancelInteraction}
  />
) : (
  <RichComposer
    ref={composerRef}
    placeholder={placeholderOverride ?? t('inputBar.placeholder')}
    onSubmit={handleSubmit}
    disabled={disabled}
    isStreaming={isStreaming}
    onStop={stopCurrentStream}
    clearOnSubmit
    autoFocus
    initialMarkdown={initialMarkdown}
    showProjectButton={false}
    onOpenSkill={() => setShowSkillPopover((prev) => !prev)}
    skillTokens={skillTokens}
    onOpenAttachment={isPickingAttachments ? undefined : () => void handlePickAttachments()}
    tips={<BottomTips />}
    containerClassName="shadow-[var(--shadow-md)]"
    limitEditorHeight
  />
)}
```

Remove global dialog imports, state reads, and render blocks from `src/App.tsx`:

```ts
import { PermissionAskDialog } from '@/components/common/PermissionAskDialog'
import type { PermissionAskDecision } from '@/components/common/PermissionAskDialog'
import { AskUserQuestionDialog } from '@/components/interactions/AskUserQuestionDialog'
import {
  approvePermissionRequest,
  cancelPermissionRequest,
  denyPermissionRequest,
} from '@/lib/tauri'
import { useStreamingStore } from '@/stores/streamingStore'
import { useInteractionStore } from '@/stores/interactionStore'
```

Keep `UpdaterPanel`, `ConfirmDialogHost`, and `ToastContainer`.

- [ ] **Step 4: Run frontend tests and commit**

Run:

```bash
pnpm vitest run \
  src/components/chat-scene/__tests__/pendingActionSelectors.test.ts \
  src/components/chat-scene/__tests__/PendingActionSurface.test.tsx \
  src/components/chat-scene/__tests__/ChatBottomArea.test.tsx
```

Expected: PASS.

Commit:

```bash
git add src/components/chat-scene/PendingActionSurface.tsx src/components/chat-scene/pendingActionSelectors.ts src/components/chat-scene/__tests__/PendingActionSurface.test.tsx src/components/chat-scene/__tests__/pendingActionSelectors.test.ts src/components/chat-scene/__tests__/ChatBottomArea.test.tsx src/components/chat-scene/ChatBottomArea.tsx src/App.tsx
git commit -m "feat: restore pending actions in chat composer"
```

---

### Task 5: Remove IM Ask Deadline Auto-Resolution

**Files:**
- Modify: `src-tauri/src/connector/im/shared/ask_coordinator.rs`

- [ ] **Step 1: Replace deadline test**

In `src-tauri/src/connector/im/shared/ask_coordinator.rs`, replace `deadline_denies_permission_and_clears_slot` with:

```rust
#[tokio::test(start_paused = true)]
async fn deadline_does_not_auto_resolve_permission_or_user_question() {
    let coordinator = make_coordinator(Arc::new(ScriptedJudge {
        result: StdMutex::new(JudgeResult::Ambiguous {
            reason: "unused".into(),
        }),
    }));

    coordinator.pending.lock().await.insert(
        "sess-im".into(),
        PendingAsk {
            session_id: SessionId::new("sess-im"),
            run_id: RunId::new("run-1"),
            kind: PendingAskKind::Permission {
                tool_call_id: ToolCallId::new("tool-1"),
                tool_name: "bash".into(),
                message: "run ls".into(),
                suggestions: vec![],
            },
            primary_model: "deepseek-v3".into(),
        },
    );

    tokio::time::advance(Duration::from_secs(60 * 60)).await;
    tokio::task::yield_now().await;

    assert!(
        coordinator.pending.lock().await.contains_key("sess-im"),
        "pending permission should not be auto-denied by a timer",
    );
}
```

- [ ] **Step 2: Run test and verify failure**

Run:

```bash
cd src-tauri && cargo test deadline_does_not_auto_resolve_permission_or_user_question
```

Expected: FAIL because `PendingAsk` still requires `deadline_at` and `cancel`.

- [ ] **Step 3: Remove timeout implementation**

In `src-tauri/src/connector/im/shared/ask_coordinator.rs`:

Remove these imports:

```rust
use std::time::{Duration, Instant};
use tokio_util::sync::CancellationToken;
```

Replace with:

```rust
use std::time::Duration;
```

Remove:

```rust
const ASK_DEADLINE: Duration = Duration::from_secs(10 * 60);
```

From `PendingAsk`, remove:

```rust
deadline_at: Instant,
cancel: CancellationToken,
```

In `try_handle_reply`, remove:

```rust
pending.cancel.cancel();
```

In `register_pending`, replace the pending construction and timer spawn with:

```rust
let pending = PendingAsk {
    session_id: event.session_id.clone(),
    run_id: event.run_id.clone(),
    kind,
    primary_model,
};
self.pending
    .lock()
    .await
    .insert(event.session_id.as_str().to_string(), pending);
```

Delete `clone_handle`, `IMAskCoordinatorHandle`, `resolve_deadline`, and `resolve_pending_as_timeout`.

Update all test `PendingAsk` literals to remove `deadline_at` and `cancel`.

- [ ] **Step 4: Run Rust tests and commit**

Run:

```bash
cd src-tauri && cargo test --lib connector::im::shared::ask_coordinator
```

Expected: PASS.

Commit:

```bash
git add src-tauri/src/connector/im/shared/ask_coordinator.rs
git commit -m "fix: remove im interaction auto timeout"
```

---

### Task 6: Deterministic IM Approval and Answer Commands

**Files:**
- Modify: `src-tauri/src/connector/im/shared/ask_coordinator.rs`

- [ ] **Step 1: Add command parser tests**

Add tests inside the existing `tests` module in `ask_coordinator.rs`:

```rust
#[test]
fn parse_approval_command_requires_explicit_format() {
    assert_eq!(
        parse_pending_action_command("/approve tool-1 allow"),
        Some(PendingActionCommand::Approve {
            id: "tool-1".to_string(),
            decision: ApprovalCommandDecision::Allow,
        })
    );
    assert_eq!(
        parse_pending_action_command("/approve tool-1 deny"),
        Some(PendingActionCommand::Approve {
            id: "tool-1".to_string(),
            decision: ApprovalCommandDecision::Deny,
        })
    );
    assert_eq!(
        parse_pending_action_command("/approve tool-1 cancel"),
        Some(PendingActionCommand::Approve {
            id: "tool-1".to_string(),
            decision: ApprovalCommandDecision::Cancel,
        })
    );
    assert_eq!(parse_pending_action_command("可以"), None);
    assert_eq!(parse_pending_action_command("帮我查天气"), None);
}

#[test]
fn parse_answer_command_preserves_answer_text() {
    assert_eq!(
        parse_pending_action_command("/answer ask-1 main branch"),
        Some(PendingActionCommand::Answer {
            id: "ask-1".to_string(),
            value: serde_json::json!({ "answer": "main branch" }),
        })
    );
    assert_eq!(
        parse_pending_action_command("/answer ask-1 cancel"),
        Some(PendingActionCommand::AnswerCancel {
            id: "ask-1".to_string(),
        })
    );
}
```

- [ ] **Step 2: Run parser tests and verify failure**

Run:

```bash
cd src-tauri && cargo test parse_approval_command_requires_explicit_format parse_answer_command_preserves_answer_text
```

Expected: FAIL because parser types do not exist.

- [ ] **Step 3: Implement command parser and outcomes**

Add near `HandleOutcome`:

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HandleOutcome {
    NotPending,
    ApprovalResolved,
    AnswerResolved,
    QueuedBehindApproval { content: String, ack: String },
    InvalidApprovalAction { message: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ApprovalCommandDecision {
    Allow,
    Deny,
    Cancel,
}

#[derive(Debug, Clone, PartialEq)]
pub enum PendingActionCommand {
    Approve {
        id: String,
        decision: ApprovalCommandDecision,
    },
    Answer {
        id: String,
        value: serde_json::Value,
    },
    AnswerCancel {
        id: String,
    },
}
```

Add parser:

```rust
pub fn parse_pending_action_command(content: &str) -> Option<PendingActionCommand> {
    let trimmed = content.trim();
    let mut parts = trimmed.splitn(4, char::is_whitespace).filter(|s| !s.is_empty());
    let command = parts.next()?;
    let id = parts.next()?.trim().to_string();
    let action = parts.next()?.trim().to_ascii_lowercase();

    match command.to_ascii_lowercase().as_str() {
        "/approve" | "approve" => {
            let decision = match action.as_str() {
                "allow" | "允许" => ApprovalCommandDecision::Allow,
                "deny" | "拒绝" => ApprovalCommandDecision::Deny,
                "cancel" | "取消" => ApprovalCommandDecision::Cancel,
                _ => return None,
            };
            Some(PendingActionCommand::Approve { id, decision })
        }
        "/answer" | "answer" => {
            if action == "cancel" || action == "取消" {
                return Some(PendingActionCommand::AnswerCancel { id });
            }
            let rest = parts.next().unwrap_or("").trim();
            let answer = if rest.is_empty() {
                action
            } else {
                format!("{action} {rest}")
            };
            let value = serde_json::from_str::<serde_json::Value>(&answer)
                .unwrap_or_else(|_| serde_json::json!({ "answer": answer }));
            Some(PendingActionCommand::Answer { id, value })
        }
        _ => None,
    }
}
```

Update `try_handle_reply` so it no longer calls `AskReplyJudge` for permission approval. The control flow should be:

```rust
let command = parse_pending_action_command(&content);
match (&pending.kind, command) {
    (
        PendingAskKind::Permission { tool_call_id, .. },
        Some(PendingActionCommand::Approve { id, decision }),
    ) if id == tool_call_id.as_str() => {
        self.resolve_permission_command(&pending, decision)?;
        return Ok(HandleOutcome::ApprovalResolved);
    }
    (
        PendingAskKind::UserQuestion { interaction_id, .. },
        Some(PendingActionCommand::Answer { id, value }),
    ) if id == interaction_id.as_str() => {
        self.resolve_user_question_answer(&pending, value)?;
        return Ok(HandleOutcome::AnswerResolved);
    }
    (
        PendingAskKind::UserQuestion { interaction_id, .. },
        Some(PendingActionCommand::AnswerCancel { id }),
    ) if id == interaction_id.as_str() => {
        self.resolve_abandoned(&pending, "user cancelled interaction".to_string())?;
        return Ok(HandleOutcome::AnswerResolved);
    }
    (_, Some(_)) => {
        self.pending
            .lock()
            .await
            .insert(session_id.as_str().to_string(), pending);
        return Ok(HandleOutcome::InvalidApprovalAction {
            message: "审批指令无效或已不匹配，请使用当前卡片上的按钮或指令。".to_string(),
        });
    }
    (_, None) => {
        self.pending
            .lock()
            .await
            .insert(session_id.as_str().to_string(), pending);
        return Ok(HandleOutcome::QueuedBehindApproval {
            content,
            ack: "当前任务正在等待权限确认。你的新消息已排队；请先处理上方审批，或取消当前任务。".to_string(),
        });
    }
}
```

Add helper:

```rust
fn resolve_permission_command(
    &self,
    pending: &PendingAsk,
    decision: ApprovalCommandDecision,
) -> Result<()> {
    if let PendingAskKind::Permission { tool_call_id, .. } = &pending.kind {
        if !self.permission_cp.is_pending(tool_call_id) {
            return Ok(());
        }
        match decision {
            ApprovalCommandDecision::Allow => self.permission_cp.resolve_pending_request(
                tool_call_id,
                PendingPermissionResolution::Allow {
                    updated_input: None,
                    remember: false,
                    destination: None,
                },
            )?,
            ApprovalCommandDecision::Deny => self.permission_cp.resolve_pending_request(
                tool_call_id,
                PendingPermissionResolution::Deny {
                    message: "Denied from IM approval command.".to_string(),
                    remember: false,
                    destination: None,
                },
            )?,
            ApprovalCommandDecision::Cancel => self.permission_cp.resolve_pending_request(
                tool_call_id,
                PendingPermissionResolution::Deny {
                    message: "Cancelled from IM approval command.".to_string(),
                    remember: false,
                    destination: None,
                },
            )?,
        }
    }
    Ok(())
}
```

- [ ] **Step 4: Update old judge tests**

Replace `answered_permission_is_consumed` with:

```rust
#[tokio::test]
async fn explicit_approval_command_is_resolved() {
    let coordinator = make_coordinator(Arc::new(ScriptedJudge {
        result: StdMutex::new(JudgeResult::Ambiguous {
            reason: "unused".into(),
        }),
    }));
    coordinator.pending.lock().await.insert(
        "sess-im".into(),
        PendingAsk {
            session_id: SessionId::new("sess-im"),
            run_id: RunId::new("run-1"),
            kind: PendingAskKind::Permission {
                tool_call_id: ToolCallId::new("tool-1"),
                tool_name: "bash".into(),
                message: "run ls".into(),
                suggestions: vec![],
            },
            primary_model: "deepseek-v3".into(),
        },
    );

    let outcome = coordinator
        .try_handle_reply(&SessionId::new("sess-im"), "/approve tool-1 allow".into())
        .await
        .unwrap();

    assert_eq!(outcome, HandleOutcome::ApprovalResolved);
}
```

Replace `abandoned_reply_is_rerouted` with:

```rust
#[tokio::test]
async fn ordinary_message_queues_behind_pending_approval() {
    let coordinator = make_coordinator(Arc::new(ScriptedJudge {
        result: StdMutex::new(JudgeResult::Abandoned {
            reason: "unused".into(),
        }),
    }));
    coordinator.pending.lock().await.insert(
        "sess-im".into(),
        PendingAsk {
            session_id: SessionId::new("sess-im"),
            run_id: RunId::new("run-1"),
            kind: PendingAskKind::Permission {
                tool_call_id: ToolCallId::new("tool-1"),
                tool_name: "bash".into(),
                message: "run ls".into(),
                suggestions: vec![],
            },
            primary_model: "deepseek-v3".into(),
        },
    );

    let outcome = coordinator
        .try_handle_reply(&SessionId::new("sess-im"), "帮我查天气".into())
        .await
        .unwrap();

    assert_eq!(
        outcome,
        HandleOutcome::QueuedBehindApproval {
            content: "帮我查天气".into(),
            ack: "当前任务正在等待权限确认。你的新消息已排队；请先处理上方审批，或取消当前任务。".into(),
        }
    );
    assert!(coordinator.pending.lock().await.contains_key("sess-im"));
}
```

- [ ] **Step 5: Run tests and commit**

Run:

```bash
cd src-tauri && cargo test --lib connector::im::shared::ask_coordinator
```

Expected: PASS.

Commit:

```bash
git add src-tauri/src/connector/im/shared/ask_coordinator.rs
git commit -m "feat: require explicit im approval commands"
```

---

### Task 7: Queue-and-ACK Hook for DingTalk

**Files:**
- Modify: `src-tauri/src/connector/im/manager.rs`
- Modify: `src-tauri/src/connector/im/shared/reply_manager.rs`

- [ ] **Step 1: Add or update DingTalk manager test**

Add a focused review test in `src-tauri/tests/review_im_approval_policy_test.rs`:

```rust
use std::fs;
use std::path::PathBuf;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

#[test]
fn dingtalk_queued_behind_approval_is_enqueued_and_acked() {
    let manager = fs::read_to_string(repo_root().join("src/connector/im/manager.rs"))
        .expect("manager.rs should be readable");

    assert!(
        manager.contains("HandleOutcome::QueuedBehindApproval"),
        "DingTalk pre-dispatch handling must branch on QueuedBehindApproval"
    );
    assert!(
        manager.contains("enqueue_or_send"),
        "QueuedBehindApproval must enqueue the ordinary message through PendingQueueManager"
    );
    assert!(
        manager.contains("deliver_pending_approval_ack")
            || manager.contains("send_pending_approval_ack"),
        "QueuedBehindApproval must send an immediate ACK to the IM user"
    );
}
```

- [ ] **Step 2: Run review test and verify failure**

Run:

```bash
cd src-tauri && cargo test --test review_im_approval_policy_test
```

Expected: FAIL until the DingTalk branch handles `QueuedBehindApproval`.

- [ ] **Step 3: Add ACK helper**

In `src-tauri/src/connector/im/shared/reply_manager.rs`, add a method near the existing ask output sink implementation:

```rust
impl DingtalkReplyManager {
    pub async fn deliver_pending_approval_ack(
        &self,
        session_id: &SessionId,
        message: &str,
    ) -> anyhow::Result<()> {
        self.deliver_ask_card(session_id, message.to_string()).await
    }
}
```

If `deliver_ask_card` is private to trait scope in the current file, call the existing lower-level markdown/card send helper used by `deliver_ask_card` and keep the method public.

- [ ] **Step 4: Handle queue-and-ACK in DingTalk branch**

In `src-tauri/src/connector/im/manager.rs`, replace the old `HandleOutcome::Reroute { content }` branch near the DingTalk worker with:

```rust
Ok(super::shared::ask_coordinator::HandleOutcome::QueuedBehindApproval { content, ack }) => {
    if let Some(reply_manager) = reply_manager_ref.as_ref() {
        let _ = reply_manager
            .deliver_pending_approval_ack(&session_for_ask, &ack)
            .await
            .inspect_err(|err| {
                log::warn!(
                    "[channel/dingtalk] pending approval ACK failed session={}: {:#}",
                    session_for_ask.as_str(),
                    err
                );
            });
    }

    let pending_item = build_pending_item_from_dingtalk_message(
        &session_id,
        &content,
        &message_context,
    );
    match self
        .pending_manager
        .enqueue_or_send(SessionId::new(session_id.clone()), pending_item)
        .await
    {
        Ok(crate::runtime::pending::EnqueueOutcome::Queued { snapshot }) => {
            log::info!(
                "[channel/dingtalk] message queued behind approval session={} queue_size={}",
                session_id,
                snapshot.len()
            );
        }
        Ok(crate::runtime::pending::EnqueueOutcome::SentDirectly { .. }) => {
            log::warn!(
                "[channel/dingtalk] pending approval queue unexpectedly sent directly session={}",
                session_id
            );
        }
        Ok(crate::runtime::pending::EnqueueOutcome::Rejected { reason }) => {
            log::warn!(
                "[channel/dingtalk] queue behind approval rejected session={} reason={:?}",
                session_id,
                reason
            );
        }
        Err(err) => {
            log::warn!(
                "[channel/dingtalk] queue behind approval failed session={}: {:#}",
                session_id,
                err
            );
        }
    }
    continue;
}
Ok(super::shared::ask_coordinator::HandleOutcome::InvalidApprovalAction { message }) => {
    if let Some(reply_manager) = reply_manager_ref.as_ref() {
        let _ = reply_manager
            .deliver_pending_approval_ack(&session_for_ask, &message)
            .await;
    }
    continue;
}
Ok(super::shared::ask_coordinator::HandleOutcome::ApprovalResolved)
| Ok(super::shared::ask_coordinator::HandleOutcome::AnswerResolved) => continue,
```

Use the existing DingTalk message-to-`PendingItem` construction already present near the old reroute branch. Do not create a second, divergent pending item shape.

- [ ] **Step 5: Run DingTalk policy test and commit**

Run:

```bash
cd src-tauri && cargo test --test review_im_approval_policy_test
```

Expected: PASS.

Commit:

```bash
git add src-tauri/src/connector/im/manager.rs src-tauri/src/connector/im/shared/reply_manager.rs src-tauri/tests/review_im_approval_policy_test.rs
git commit -m "feat: queue dingtalk messages behind approval"
```

---

### Task 8: Shared Coordinator Gate for All IM Channels

**Files:**
- Modify: `src-tauri/src/connector/im/manager.rs`
- Modify: `src-tauri/tests/review_im_approval_policy_test.rs`

- [ ] **Step 1: Extend review test for all channel workers**

Append to `src-tauri/tests/review_im_approval_policy_test.rs`:

```rust
#[test]
fn every_im_worker_has_pending_approval_pre_dispatch_gate() {
    let manager = fs::read_to_string(repo_root().join("src/connector/im/manager.rs"))
        .expect("manager.rs should be readable");

    for marker in [
        "[channel/dingtalk]",
        "[channel/feishu]",
        "[channel/wecom]",
        "[channel/wechat]",
        "[channel/telegram]",
        "[channel/whatsapp]",
    ] {
        let marker_index = manager
            .find(marker)
            .unwrap_or_else(|| panic!("{marker} worker marker should exist"));
        let tail = &manager[marker_index..manager.len().min(marker_index + 20_000)];
        assert!(
            tail.contains("try_handle_reply")
                || tail.contains("try_handle_pending_action")
                || tail.contains("HandleOutcome::QueuedBehindApproval"),
            "{marker} must consult the shared pending approval coordinator before normal dispatch"
        );
    }
}
```

- [ ] **Step 2: Run review test and verify failure for non-DingTalk channels**

Run:

```bash
cd src-tauri && cargo test --test review_im_approval_policy_test
```

Expected: FAIL until Feishu, WeCom, WeChat, Telegram, and WhatsApp workers consult the coordinator.

- [ ] **Step 3: Add a reusable helper inside manager.rs**

In `src-tauri/src/connector/im/manager.rs`, add a private helper near other manager helper functions:

```rust
async fn handle_pending_action_pre_dispatch(
    ask_coordinator: Option<&Arc<super::shared::ask_coordinator::IMAskCoordinator>>,
    session_id: &SessionId,
    content: &str,
) -> anyhow::Result<super::shared::ask_coordinator::HandleOutcome> {
    if let Some(coordinator) = ask_coordinator {
        coordinator
            .try_handle_reply(session_id, content.to_string())
            .await
    } else {
        Ok(super::shared::ask_coordinator::HandleOutcome::NotPending)
    }
}
```

If `manager.rs` already has a more suitable helper section, place it there.

- [ ] **Step 4: Use helper in each worker before normal dispatch**

For each worker loop, place the pre-dispatch gate immediately after session id and text content are resolved and before the channel calls the normal run/dispatch path.

Handle outcomes with this exact policy:

- `NotPending`: fall through to the current normal dispatch code.
- `ApprovalResolved` and `AnswerResolved`: `continue` the worker loop because the current IM message was consumed by the pending action.
- `QueuedBehindApproval { content, ack }`: send `ack` to the same IM target that sent the ordinary message, enqueue `content` through the existing `PendingQueueManager` path for that worker, then `continue`.
- `InvalidApprovalAction { message }`: send `message` to the same IM target, then `continue`.
- `Err(err)`: log with the channel marker and fall through to the current normal dispatch code so a coordinator failure does not drop the user's message.

Channel-specific wiring:

- DingTalk: keep the Task 7 `DingtalkReplyManager::deliver_pending_approval_ack` branch and reuse the same `PendingItem` construction used by the old DingTalk reroute path.
- Feishu: add the gate in the worker block marked `[channel/feishu]`; send ACK/error text through the Feishu text reply helper already used by that block, then enqueue with the same tenant/app/chat/thread metadata used for normal queued messages.
- WeCom: add the gate in the worker block marked `[channel/wecom]`; send ACK/error text with the WeCom text reply branch used for queue rejection, then enqueue with the current corp/account/conversation metadata.
- WeChat: add the gate in the worker block marked `[channel/wechat]`; send ACK/error text with the WeChat final-reply sender, then enqueue with the same user/session metadata used by normal inbound message handling.
- Telegram: add the gate in the worker block marked `[channel/telegram]`; send ACK/error text with the Telegram bot send-message helper used for queue rejection, then enqueue with the current chat id and message id metadata.
- WhatsApp: add the gate in the worker block marked `[channel/whatsapp]`; send ACK/error text with the WhatsApp text fallback used by `whatsapp/connector.rs`, then enqueue with the current account/chat/message metadata.

All six branches must preserve channel id, account id, chat id, thread id, user id, and source message id fields already available in their worker context.

- [ ] **Step 5: Run review test and focused cargo check**

Run:

```bash
cd src-tauri && cargo test --test review_im_approval_policy_test
cd src-tauri && cargo check
```

Expected: PASS.

Commit:

```bash
git add src-tauri/src/connector/im/manager.rs src-tauri/tests/review_im_approval_policy_test.rs
git commit -m "feat: gate im messages on pending approvals"
```

---

### Task 9: IM Pending Card/Message Content Includes Fallback Commands

**Files:**
- Modify: `src-tauri/src/connector/im/shared/ask_coordinator.rs`

- [ ] **Step 1: Add format tests**

Add tests:

```rust
#[test]
fn permission_markdown_includes_explicit_approval_commands() {
    let text = format_pending_ask_markdown(&PendingAskKind::Permission {
        tool_call_id: ToolCallId::new("tool-1"),
        tool_name: "bash".into(),
        message: "run ls".into(),
        suggestions: vec![],
    });

    assert!(text.contains("/approve tool-1 allow"));
    assert!(text.contains("/approve tool-1 deny"));
    assert!(text.contains("/approve tool-1 cancel"));
    assert!(!text.contains("超时"));
    assert!(!text.to_ascii_lowercase().contains("expires"));
}

#[test]
fn user_question_markdown_includes_explicit_answer_commands() {
    let text = format_pending_ask_markdown(&PendingAskKind::UserQuestion {
        interaction_id: InteractionId::new("ask-1"),
        tool_call_id: ToolCallId::new("tool-1"),
        questions: serde_json::json!({
            "questions": [{ "id": "q1", "question": "Which branch?" }]
        }),
    });

    assert!(text.contains("/answer ask-1"));
    assert!(text.contains("/answer ask-1 cancel"));
    assert!(!text.contains("超时"));
    assert!(!text.to_ascii_lowercase().contains("expires"));
}
```

- [ ] **Step 2: Run tests and verify failure**

Run:

```bash
cd src-tauri && cargo test permission_markdown_includes_explicit_approval_commands user_question_markdown_includes_explicit_answer_commands
```

Expected: FAIL until markdown contains deterministic commands.

- [ ] **Step 3: Update markdown formatting**

In `format_pending_ask_markdown`, for permission asks, append:

```rust
lines.push("可用操作：".to_string());
lines.push(format!("- 允许：`/approve {} allow`", tool_call_id.as_str()));
lines.push(format!("- 拒绝：`/approve {} deny`", tool_call_id.as_str()));
lines.push(format!("- 取消当前任务：`/approve {} cancel`", tool_call_id.as_str()));
lines.push("普通文字会先排队，等当前审批处理完再执行。".to_string());
```

For user question asks, append:

```rust
lines.push("可用操作：".to_string());
lines.push(format!("- 回答：`/answer {} <你的回答>`", interaction_id.as_str()));
lines.push(format!("- 取消当前任务：`/answer {} cancel`", interaction_id.as_str()));
lines.push("普通文字会先排队，等当前问题处理完再执行。".to_string());
```

- [ ] **Step 4: Run tests and commit**

Run:

```bash
cd src-tauri && cargo test --lib connector::im::shared::ask_coordinator
```

Expected: PASS.

Commit:

```bash
git add src-tauri/src/connector/im/shared/ask_coordinator.rs
git commit -m "feat: show deterministic im approval commands"
```

---

### Task 10: Final Verification

**Files:**
- Verify only.

- [ ] **Step 1: Run frontend focused tests**

```bash
pnpm vitest run \
  src/components/chat-scene/__tests__/pendingActionSelectors.test.ts \
  src/components/chat-scene/__tests__/PendingActionSurface.test.tsx \
  src/components/chat-scene/__tests__/ChatBottomArea.test.tsx \
  src/hooks/useStreaming.integration.test.tsx \
  src/stores/streamingStore.test.ts \
  src/stores/authStore.user-scope-reset.test.ts
```

Expected: PASS.

- [ ] **Step 2: Run Rust focused tests**

```bash
cd src-tauri && cargo test --lib connector::im::shared::ask_coordinator
cd src-tauri && cargo test --test review_im_approval_policy_test
cd src-tauri && cargo test --lib runtime::pending
```

Expected: PASS.

- [ ] **Step 3: Run build/check**

```bash
pnpm build
cd src-tauri && cargo check
```

Expected: PASS.

- [ ] **Step 4: Manual smoke test with `pnpm run tauri:dev`**

Run:

```bash
pnpm run tauri:dev
```

Manual checks:

1. Trigger a local permission ask in app.
2. Verify the input area is replaced by `PendingActionSurface`.
3. Switch to another conversation and verify normal composer appears.
4. Switch back and verify `PendingActionSurface` returns.
5. Resolve the ask from app and verify composer returns.
6. Trigger an AskUserQuestion and verify no countdown or auto-cancel occurs.
7. From DingTalk, send ordinary text while approval is pending and verify the bot ACKs and the message appears in pending queue.
8. Resolve the approval and verify queued text drains after the blocked run finishes.

- [ ] **Step 5: Review final working tree**

```bash
git status --short
```

Expected: only files from the tasks above are modified or already committed; no temporary debug artifacts are present.
