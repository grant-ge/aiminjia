# Conversation More Create Actions Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a chat-header more menu that reuses conversation actions, sends conversation-summary creation messages for skills and scheduled tasks, and renders successful creation results as interactive product cards.

**Architecture:** The header menu stays a normal frontend action surface: it calls existing `useChat` actions and sends generated user messages through `sendUserMessage`. Assistant markdown carries result references through fenced `aijia-card` JSON blocks, and message rendering swaps recognized blocks for live cards while keeping `messages.jsonl` immutable. Cards resolve current skill or agenda state from existing stores/IPCs; scheduled-task edits update agenda data only.

**Tech Stack:** React, TypeScript, Zustand, react-markdown, Radix dropdown via `AppDropdown`, lucide-react, Vitest, Testing Library, Tauri IPC wrappers in `src/lib/tauri.ts`.

---

## Source Spec

- `docs/superpowers/specs/2026-06-12-conversation-more-create-actions-design.md`

## File Structure

- Create `src/features/chat/conversationCreatePrompts.ts`
  - Owns generated user-message text for `总结对话并创建技能` and `总结对话并创建定时任务`.
  - Pure functions, no React, easy unit tests.
- Create `src/features/chat/conversationCreatePrompts.test.ts`
  - Verifies prompt text includes required instructions and `aijia-card` result requirements.
- Create `src/components/sidebar/ConversationRenameDialog.tsx`
  - Extracts the existing rename dialog from `AppSidebar` so header and sidebar use the same UI.
- Modify `src/components/sidebar/AppSidebar.tsx`
  - Replace inline rename dialog markup with `ConversationRenameDialog`.
- Modify `src/components/sidebar/__tests__/AppSidebar.test.tsx`
  - Keep existing rename behavior green after extraction.
- Modify `src/components/shell/ChatTopBar.tsx`
  - Add `moreMenuItems?: AppDropdownItem[]`.
  - Render `AppDropdown` when menu items are provided; keep `onMore` fallback for existing callers.
- Modify `src/components/shell/ChatTopBar.test.tsx`
  - Verify the more dropdown opens and triggers an item.
- Modify `src/features/chat/ChatPage.tsx`
  - Wire header menu actions: rename, pin/unpin, export, copy ID, create skill, create scheduled task, archive.
  - Own the rename dialog state for the active conversation.
- Modify `src/features/chat/ChatPage.test.tsx`
  - Verify generated messages are sent for both creation actions.
  - Verify pin/archive/copy/export are present enough to prevent regression.
- Create `src/components/chat-scene/result-cards/aijiaCardPayload.ts`
  - Defines `AijiaCardPayload`, `SkillCreatedCardPayload`, `ScheduleCreatedCardPayload`.
  - Parses and validates `aijia-card` JSON.
- Create `src/components/chat-scene/result-cards/aijiaCardPayload.test.ts`
  - Unit tests invalid JSON, unknown type, valid skill card, valid schedule card.
- Create `src/components/chat-scene/result-cards/AijiaCardCodeBlock.tsx`
  - React-markdown code-block adapter. Recognized `language-aijia-card` blocks render product cards; all other blocks delegate to `MarkdownCodeBlock`.
- Modify `src/components/chat-scene/markdown/MarkdownCodeBlock.tsx`
  - Export the existing node-to-text helper so `AijiaCardCodeBlock` can read fenced block content without duplication.
- Modify `src/components/chat-scene/markdown/markdownComponents.tsx`
  - Use `AijiaCardCodeBlock` for `code`.
- Create `src/components/chat-scene/result-cards/SkillCreatedCard.tsx`
  - Display-only skill result card with `查看技能` and `复制 ID`.
- Create `src/components/chat-scene/result-cards/ScheduleCreatedCard.tsx`
  - Scheduled-task card with `编辑` and `查看定时任务`.
  - Loads current agenda item by `scheduleId` using `getAgendaItem`.
  - Opens existing `AgendaItemEditor` for editing.
- Create `src/components/chat-scene/result-cards/AijiaResultCard.test.tsx`
  - Renderer-level tests for fallback and both card variants.
- Modify `src/i18n/zh-CN.json`
  - Add menu and card labels.
- Modify `src/i18n/en-US.json`
  - Add matching English labels.

## Task 1: Creation Prompt Contract

**Files:**
- Create: `src/features/chat/conversationCreatePrompts.ts`
- Create: `src/features/chat/conversationCreatePrompts.test.ts`

- [ ] **Step 1: Write failing tests**

Add `src/features/chat/conversationCreatePrompts.test.ts`:

```ts
import { describe, expect, it } from 'vitest'

import {
  buildCreateScheduleFromConversationPrompt,
  buildCreateSkillFromConversationPrompt,
} from './conversationCreatePrompts'

describe('conversation create prompts', () => {
  it('builds a skill creation prompt that requires a skill_created aijia-card', () => {
    const prompt = buildCreateSkillFromConversationPrompt()

    expect(prompt).toContain('请总结当前对话内容')
    expect(prompt).toContain('创建为一个技能')
    expect(prompt).toContain('技能名称、适用场景、输入、执行步骤、输出格式和注意事项')
    expect(prompt).toContain('```aijia-card')
    expect(prompt).toContain('"type": "skill_created"')
    expect(prompt).toContain('"skillId"')
  })

  it('builds a scheduled-task creation prompt that requires a schedule_created aijia-card', () => {
    const prompt = buildCreateScheduleFromConversationPrompt()

    expect(prompt).toContain('请总结当前对话内容')
    expect(prompt).toContain('创建一个定时任务')
    expect(prompt).toContain('标题、任务提示词、建议频率和开始时间')
    expect(prompt).toContain('```aijia-card')
    expect(prompt).toContain('"type": "schedule_created"')
    expect(prompt).toContain('"scheduleId"')
  })
})
```

- [ ] **Step 2: Run the failing test**

Run:

```bash
pnpm exec vitest run src/features/chat/conversationCreatePrompts.test.ts
```

Expected: FAIL because `src/features/chat/conversationCreatePrompts.ts` does not exist.

- [ ] **Step 3: Implement the prompt builders**

Create `src/features/chat/conversationCreatePrompts.ts`:

```ts
export function buildCreateSkillFromConversationPrompt(): string {
  return `请总结当前对话内容，并把其中可复用的工作流程创建为一个技能。

要求：
1. 从当前对话提炼技能名称、适用场景、输入、执行步骤、输出格式和注意事项。
2. 按当前技能系统规范创建技能并刷新技能列表。
3. 创建成功后，用 aijia-card 返回 skill_created 结果，至少包含 skillId。

返回格式示例：
\`\`\`aijia-card
{
  "type": "skill_created",
  "skillId": "created-skill-id",
  "title": "技能名称",
  "description": "技能说明"
}
\`\`\``;
}

export function buildCreateScheduleFromConversationPrompt(): string {
  return `请总结当前对话内容，并创建一个定时任务。

要求：
1. 把当前对话提炼成定时任务标题、任务提示词、建议频率和开始时间。
2. 使用定时任务能力创建任务；如果对话没有明确时间，请选择保守默认值并在结果说明。
3. 创建成功后，用 aijia-card 返回 schedule_created 结果，至少包含 scheduleId。

返回格式示例：
\`\`\`aijia-card
{
  "type": "schedule_created",
  "scheduleId": "created-schedule-id",
  "title": "定时任务标题",
  "prompt": "任务提示词",
  "frequencyLabel": "每天 09:00",
  "nextFireAt": "2026-06-13T09:00:00+08:00"
}
\`\`\``;
}
```

- [ ] **Step 4: Run the prompt tests**

Run:

```bash
pnpm exec vitest run src/features/chat/conversationCreatePrompts.test.ts
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/features/chat/conversationCreatePrompts.ts src/features/chat/conversationCreatePrompts.test.ts
git commit -m "feat: add conversation creation prompts"
```

## Task 2: Shared Conversation Rename Dialog

**Files:**
- Create: `src/components/sidebar/ConversationRenameDialog.tsx`
- Modify: `src/components/sidebar/AppSidebar.tsx`
- Test: `src/components/sidebar/__tests__/AppSidebar.test.tsx`

- [ ] **Step 1: Confirm existing sidebar rename test still covers the flow**

Run:

```bash
pnpm exec vitest run src/components/sidebar/__tests__/AppSidebar.test.tsx -t "rename"
```

Expected: PASS before extraction, or the command reports no matching test. If there is no matching rename test, add the focused test from Step 2.

- [ ] **Step 2: Add or preserve a focused sidebar rename assertion**

In `src/components/sidebar/__tests__/AppSidebar.test.tsx`, ensure there is a test with this behavior:

```ts
it('renames a conversation from the shared rename dialog', async () => {
  // Keep the file's existing setup helpers and mocks.
  render(<AppSidebar />)

  fireEvent.contextMenu(screen.getByText('测试会话'))
  fireEvent.click(screen.getByRole('menuitem', { name: '重命名聊天' }))
  fireEvent.change(screen.getByRole('textbox'), { target: { value: '新的标题' } })
  fireEvent.click(screen.getByRole('button', { name: '确认' }))

  await waitFor(() => {
    expect(mockRenameConversation).toHaveBeenCalledWith('conv-1', '新的标题')
  })
})
```

Use the actual existing mock variable names in this file. The assertions must prove the extracted dialog still calls `renameConversation(id, title)`.

- [ ] **Step 3: Create the shared dialog**

Create `src/components/sidebar/ConversationRenameDialog.tsx`:

```tsx
import { useEffect, useState } from 'react'

import {
  Dialog,
  DialogContent,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'

interface ConversationRenameDialogProps {
  open: boolean
  initialTitle: string
  onOpenChange: (open: boolean) => void
  onConfirm: (title: string) => void | Promise<void>
}

export function ConversationRenameDialog({
  open,
  initialTitle,
  onOpenChange,
  onConfirm,
}: ConversationRenameDialogProps) {
  const [value, setValue] = useState(initialTitle)

  useEffect(() => {
    if (open) setValue(initialTitle)
  }, [initialTitle, open])

  const trimmed = value.trim()
  const handleConfirm = () => {
    if (!trimmed) return
    void onConfirm(trimmed)
  }

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="w-[400px]">
        <DialogHeader>
          <DialogTitle>重命名聊天</DialogTitle>
        </DialogHeader>
        <Input
          value={value}
          onChange={(event) => setValue(event.target.value)}
          onKeyDown={(event) => {
            if (event.key === 'Enter') handleConfirm()
          }}
          autoFocus
        />
        <DialogFooter>
          <Button variant="outline" onClick={() => onOpenChange(false)}>
            取消
          </Button>
          <Button onClick={handleConfirm} disabled={!trimmed}>
            确认
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  )
}
```

- [ ] **Step 4: Replace the inline dialog in `AppSidebar`**

In `src/components/sidebar/AppSidebar.tsx`:

1. Import `ConversationRenameDialog`.
2. Keep `renamingId` state.
3. Remove `renameValue` state and inline dialog JSX.
4. Derive `renamingConversation` from `conversations`.

Use this shape:

```tsx
const renamingConversation = renamingId
  ? conversations.find((conversation) => conversation.id === renamingId) ?? null
  : null

const handleRenameOpen = (id: string) => {
  setRenamingId(id)
}

const handleRenameConfirm = async (title: string) => {
  if (!renamingId) return
  await renameConversation(renamingId, title)
  setRenamingId(null)
}
```

Replace the bottom inline dialog with:

```tsx
<ConversationRenameDialog
  open={Boolean(renamingId)}
  initialTitle={renamingConversation?.title ?? ''}
  onOpenChange={(open) => {
    if (!open) setRenamingId(null)
  }}
  onConfirm={handleRenameConfirm}
/>
```

- [ ] **Step 5: Run sidebar tests**

Run:

```bash
pnpm exec vitest run src/components/sidebar/__tests__/AppSidebar.test.tsx src/components/sidebar/__tests__/ConversationRow.test.tsx
```

Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add src/components/sidebar/ConversationRenameDialog.tsx src/components/sidebar/AppSidebar.tsx src/components/sidebar/__tests__/AppSidebar.test.tsx
git commit -m "refactor: share conversation rename dialog"
```

## Task 3: Chat Header More Dropdown

**Files:**
- Modify: `src/components/shell/ChatTopBar.tsx`
- Modify: `src/components/shell/ChatTopBar.test.tsx`

- [ ] **Step 1: Write failing dropdown test**

Add to `src/components/shell/ChatTopBar.test.tsx`:

```tsx
it('renders more menu items through AppDropdown', () => {
  const onSelect = vi.fn()

  render(
    <ChatTopBar
      title="新对话"
      moreMenuItems={[
        {
          id: 'copy-id',
          label: '复制对话 ID',
          onSelect: () => onSelect(),
        },
      ]}
    />,
  )

  fireEvent.click(screen.getByRole('button', { name: '更多' }))
  fireEvent.click(screen.getByRole('menuitem', { name: '复制对话 ID' }))

  expect(onSelect).toHaveBeenCalledTimes(1)
})
```

- [ ] **Step 2: Run the failing test**

Run:

```bash
pnpm exec vitest run src/components/shell/ChatTopBar.test.tsx
```

Expected: FAIL because `moreMenuItems` is not a prop yet.

- [ ] **Step 3: Add dropdown support**

In `src/components/shell/ChatTopBar.tsx`:

1. Import `AppDropdown` and `AppDropdownItem`.
2. Add `moreMenuItems?: AppDropdownItem[]` to `ChatTopBarProps`.
3. Destructure `moreMenuItems`.
4. Replace the `onMore` button branch with this logic:

```tsx
{moreMenuItems && moreMenuItems.length > 0 ? (
  <AppDropdown
    ariaLabel="更多"
    align="end"
    sideOffset={6}
    items={moreMenuItems}
    trigger={
      <Button
        unstyled
        type="button"
        title="更多"
        className="flex h-8 w-8 items-center justify-center rounded-md text-muted-foreground transition-colors hover:bg-muted hover:text-foreground"
      >
        <Ellipsis className="h-4 w-4" />
      </Button>
    }
  />
) : onMore ? (
  <Button
    unstyled
    type="button"
    aria-label="更多"
    onClick={onMore}
    className="flex h-8 w-8 items-center justify-center rounded-md text-muted-foreground transition-colors hover:bg-muted hover:text-foreground"
  >
    <Ellipsis className="h-4 w-4" />
  </Button>
) : null}
```

- [ ] **Step 4: Run top-bar tests**

Run:

```bash
pnpm exec vitest run src/components/shell/ChatTopBar.test.tsx
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/components/shell/ChatTopBar.tsx src/components/shell/ChatTopBar.test.tsx
git commit -m "feat: add chat top bar more menu"
```

## Task 4: Wire Header Conversation Actions

**Files:**
- Modify: `src/features/chat/ChatPage.tsx`
- Modify: `src/features/chat/ChatPage.test.tsx`
- Modify: `src/i18n/zh-CN.json`
- Modify: `src/i18n/en-US.json`

- [ ] **Step 1: Expand the `useChat` mock in `ChatPage.test.tsx`**

Change the mock to expose every header action:

```ts
const chatMocks = vi.hoisted(() => ({
  switchConversation: vi.fn(),
  renameConversation: vi.fn(),
  archiveConversation: vi.fn(),
  setConversationPinned: vi.fn(),
  sendUserMessage: vi.fn(),
}))

vi.mock('@/hooks/useChat', () => ({
  useChat: () => chatMocks,
}))
```

Update existing `switchConversationMock` references to `chatMocks.switchConversation`.

- [ ] **Step 2: Stop mocking away the top bar dropdown for the new tests**

Either remove the `vi.mock('@/components/shell/ChatTopBar', ...)` block from `ChatPage.test.tsx`, or update the mock so it renders and exercises `moreMenuItems`:

```tsx
vi.mock('@/components/shell/ChatTopBar', () => ({
  ChatTopBar: ({
    title,
    sourceLabel,
    employee,
    onShare,
    shareLabel,
    moreMenuItems,
  }: {
    title: string
    sourceLabel?: string
    employee?: { name: string; role: string; defaultSkillLabel?: string | null }
    onShare?: () => void
    shareLabel?: string
    moreMenuItems?: Array<{ id: string; label: string; onSelect?: () => void }>
  }) => (
    <header data-testid="chat-header">
      {employee ? <span data-testid="chat-employee-label">{employee.role} · {employee.name}</span> : title}
      {employee?.defaultSkillLabel ? <span data-testid="chat-default-skill">{employee.defaultSkillLabel}</span> : null}
      {sourceLabel ? <span data-testid="chat-source-label">{sourceLabel}</span> : null}
      {onShare ? <button onClick={onShare}>{shareLabel ?? '分享'}</button> : null}
      {moreMenuItems?.map((item) => (
        <button key={item.id} onClick={() => item.onSelect?.()}>
          {item.label}
        </button>
      ))}
    </header>
  ),
}))
```

The lighter mock is acceptable in `ChatPage.test.tsx`; `ChatTopBar.test.tsx` owns the actual dropdown behavior.

- [ ] **Step 3: Add failing tests for generated creation messages**

Add to `src/features/chat/ChatPage.test.tsx`:

```tsx
it('sends a generated message for creating a skill from the current conversation', async () => {
  useChatStore.setState({
    activeConversationId: 'conv-create',
    conversations: [{ id: 'conv-create', title: '需求讨论', createdAt: '', updatedAt: '', isArchived: false }],
    messages: [],
  })

  render(<ChatPage conversationId="conv-create" />)

  fireEvent.click(screen.getByRole('button', { name: '总结对话并创建技能' }))

  await waitFor(() => {
    expect(chatMocks.sendUserMessage).toHaveBeenCalledTimes(1)
  })
  expect(chatMocks.sendUserMessage.mock.calls[0][0]).toContain('创建为一个技能')
  expect(chatMocks.sendUserMessage.mock.calls[0][0]).toContain('"type": "skill_created"')
})

it('sends a generated message for creating a scheduled task from the current conversation', async () => {
  useChatStore.setState({
    activeConversationId: 'conv-schedule',
    conversations: [{ id: 'conv-schedule', title: '日报讨论', createdAt: '', updatedAt: '', isArchived: false }],
    messages: [],
  })

  render(<ChatPage conversationId="conv-schedule" />)

  fireEvent.click(screen.getByRole('button', { name: '总结对话并创建定时任务' }))

  await waitFor(() => {
    expect(chatMocks.sendUserMessage).toHaveBeenCalledTimes(1)
  })
  expect(chatMocks.sendUserMessage.mock.calls[0][0]).toContain('创建一个定时任务')
  expect(chatMocks.sendUserMessage.mock.calls[0][0]).toContain('"type": "schedule_created"')
})
```

- [ ] **Step 4: Add failing tests for inherited actions**

Add:

```tsx
it('wires pin archive copy and export actions in the header more menu', async () => {
  const writeText = vi.fn().mockResolvedValue(undefined)
  Object.assign(navigator, { clipboard: { writeText } })

  useChatStore.setState({
    activeConversationId: 'conv-actions',
    conversations: [{ id: 'conv-actions', title: '操作会话', createdAt: '', updatedAt: '', isArchived: false, isPinned: false }],
    messages: [],
  })

  render(<ChatPage conversationId="conv-actions" />)

  fireEvent.click(screen.getByRole('button', { name: '置顶对话' }))
  expect(chatMocks.setConversationPinned).toHaveBeenCalledWith('conv-actions', true)

  fireEvent.click(screen.getByRole('button', { name: '复制对话 ID' }))
  await waitFor(() => expect(writeText).toHaveBeenCalledWith('conv-actions'))

  fireEvent.click(screen.getByRole('button', { name: '归档聊天' }))
  expect(chatMocks.archiveConversation).toHaveBeenCalledWith('conv-actions')

  fireEvent.click(screen.getByRole('button', { name: '导出对话' }))
  expect(tauriMocks.exportConversation).toHaveBeenCalled()
})
```

- [ ] **Step 5: Run the failing chat-page tests**

Run:

```bash
pnpm exec vitest run src/features/chat/ChatPage.test.tsx
```

Expected: FAIL because `ChatPage` does not pass `moreMenuItems` or call prompt builders yet.

- [ ] **Step 6: Add i18n keys**

In `src/i18n/zh-CN.json`, add under a suitable top-level key such as `chatHeader`:

```json
"chatHeader": {
  "more": "更多",
  "exportConversation": "导出对话",
  "createSkillFromConversation": "总结对话并创建技能",
  "createScheduleFromConversation": "总结对话并创建定时任务"
}
```

In `src/i18n/en-US.json`, add:

```json
"chatHeader": {
  "more": "More",
  "exportConversation": "Export conversation",
  "createSkillFromConversation": "Summarize and create skill",
  "createScheduleFromConversation": "Summarize and create scheduled task"
}
```

If `chatHeader` already exists, merge these keys without removing existing entries.

- [ ] **Step 7: Wire actions in `ChatPage`**

In `src/features/chat/ChatPage.tsx`:

1. Import icons:

```ts
import { Archive, CalendarClock, Copy, Download, Pencil, Pin, PinOff, Sparkles } from 'lucide-react'
```

2. Import the prompt builders and rename dialog:

```ts
import { ConversationRenameDialog } from '@/components/sidebar/ConversationRenameDialog'
import {
  buildCreateScheduleFromConversationPrompt,
  buildCreateSkillFromConversationPrompt,
} from '@/features/chat/conversationCreatePrompts'
```

3. Expand `useChat` destructuring:

```ts
const {
  switchConversation,
  renameConversation,
  archiveConversation,
  setConversationPinned,
  sendUserMessage,
} = useChat()
```

4. Add rename state:

```ts
const [renameOpen, setRenameOpen] = useState(false)
```

5. Add handlers:

```ts
const handleRenameConfirm = async (nextTitle: string) => {
  if (!conv) return
  await renameConversation(conv.id, nextTitle)
  setRenameOpen(false)
}

const handleCopyConversationId = () => {
  if (!conv) return
  void navigator.clipboard.writeText(conv.id)
}

const handleCreateSkillFromConversation = () => {
  void sendUserMessage(buildCreateSkillFromConversationPrompt())
}

const handleCreateScheduleFromConversation = () => {
  void sendUserMessage(buildCreateScheduleFromConversationPrompt())
}
```

6. Build `moreMenuItems` only when `conv` exists:

```tsx
const moreMenuItems = conv
  ? [
      {
        id: 'rename',
        label: t('sidebar.renameChat'),
        icon: <Pencil />,
        onSelect: () => setRenameOpen(true),
      },
      {
        id: 'pin',
        label: conv.isPinned ? t('sidebar.unpinChat') : t('sidebar.pinChat'),
        icon: conv.isPinned ? <PinOff /> : <Pin />,
        onSelect: () => void setConversationPinned(conv.id, !conv.isPinned),
      },
      {
        id: 'export',
        label: t('chatHeader.exportConversation', '导出对话'),
        icon: <Download />,
        onSelect: conversationExport.openExportDialog,
      },
      {
        id: 'copy-id',
        label: t('sidebar.copyConversationId'),
        icon: <Copy />,
        onSelect: handleCopyConversationId,
      },
      {
        id: 'create-skill',
        label: t('chatHeader.createSkillFromConversation', '总结对话并创建技能'),
        icon: <Sparkles />,
        onSelect: handleCreateSkillFromConversation,
      },
      {
        id: 'create-schedule',
        label: t('chatHeader.createScheduleFromConversation', '总结对话并创建定时任务'),
        icon: <CalendarClock />,
        onSelect: handleCreateScheduleFromConversation,
      },
      {
        id: 'archive',
        label: t('sidebar.archiveChat'),
        icon: <Archive />,
        className: 'text-destructive focus:text-destructive',
        onSelect: () => void archiveConversation(conv.id),
      },
    ]
  : []
```

7. Pass `moreMenuItems` to `ChatTopBar`:

```tsx
<ChatTopBar
  ...
  onShare={conversationExport.openExportDialog}
  shareLabel={t('chatHeader.exportConversation', '导出对话')}
  moreMenuItems={moreMenuItems}
/>
```

8. Render the shared rename dialog near `ConversationExportDialog`:

```tsx
<ConversationRenameDialog
  open={renameOpen}
  initialTitle={conv?.title ?? ''}
  onOpenChange={setRenameOpen}
  onConfirm={handleRenameConfirm}
/>
```

- [ ] **Step 8: Run chat-page tests**

Run:

```bash
pnpm exec vitest run src/features/chat/ChatPage.test.tsx
```

Expected: PASS.

- [ ] **Step 9: Run related header/sidebar tests**

Run:

```bash
pnpm exec vitest run src/components/shell/ChatTopBar.test.tsx src/components/sidebar/__tests__/AppSidebar.test.tsx src/features/chat/ChatPage.test.tsx
```

Expected: PASS.

- [ ] **Step 10: Commit**

```bash
git add src/features/chat/ChatPage.tsx src/features/chat/ChatPage.test.tsx src/i18n/zh-CN.json src/i18n/en-US.json
git commit -m "feat: add chat header conversation actions"
```

## Task 5: Parse `aijia-card` Payloads

**Files:**
- Create: `src/components/chat-scene/result-cards/aijiaCardPayload.ts`
- Create: `src/components/chat-scene/result-cards/aijiaCardPayload.test.ts`

- [ ] **Step 1: Write parser tests**

Create `src/components/chat-scene/result-cards/aijiaCardPayload.test.ts`:

```ts
import { describe, expect, it } from 'vitest'

import { parseAijiaCardPayload } from './aijiaCardPayload'

describe('parseAijiaCardPayload', () => {
  it('returns null for invalid JSON', () => {
    expect(parseAijiaCardPayload('{ nope')).toBeNull()
  })

  it('returns null for unknown card types', () => {
    expect(parseAijiaCardPayload('{"type":"other","id":"x"}')).toBeNull()
  })

  it('parses a skill_created payload with snapshot fields', () => {
    expect(parseAijiaCardPayload(JSON.stringify({
      type: 'skill_created',
      skillId: 'sales-followup',
      title: '销售跟进',
      description: '整理客户下一步动作',
    }))).toEqual({
      type: 'skill_created',
      skillId: 'sales-followup',
      title: '销售跟进',
      description: '整理客户下一步动作',
    })
  })

  it('parses a schedule_created payload with snapshot fields', () => {
    expect(parseAijiaCardPayload(JSON.stringify({
      type: 'schedule_created',
      scheduleId: 'agenda-1',
      title: '日报提醒',
      prompt: '每天总结日报',
      frequencyLabel: '每天 09:00',
      nextFireAt: '2026-06-13T09:00:00+08:00',
    }))).toEqual({
      type: 'schedule_created',
      scheduleId: 'agenda-1',
      title: '日报提醒',
      prompt: '每天总结日报',
      frequencyLabel: '每天 09:00',
      nextFireAt: '2026-06-13T09:00:00+08:00',
    })
  })
})
```

- [ ] **Step 2: Run the failing parser test**

Run:

```bash
pnpm exec vitest run src/components/chat-scene/result-cards/aijiaCardPayload.test.ts
```

Expected: FAIL because the parser file does not exist.

- [ ] **Step 3: Implement parser and types**

Create `src/components/chat-scene/result-cards/aijiaCardPayload.ts`:

```ts
export interface SkillCreatedCardPayload {
  type: 'skill_created'
  skillId: string
  title?: string
  description?: string
}

export interface ScheduleCreatedCardPayload {
  type: 'schedule_created'
  scheduleId: string
  title?: string
  prompt?: string
  frequencyLabel?: string
  nextFireAt?: string
}

export type AijiaCardPayload = SkillCreatedCardPayload | ScheduleCreatedCardPayload

function optionalString(value: unknown): string | undefined {
  return typeof value === 'string' && value.trim() ? value : undefined
}

export function parseAijiaCardPayload(raw: string): AijiaCardPayload | null {
  let parsed: unknown
  try {
    parsed = JSON.parse(raw)
  } catch {
    return null
  }

  if (!parsed || typeof parsed !== 'object') return null
  const record = parsed as Record<string, unknown>

  if (record.type === 'skill_created') {
    const skillId = optionalString(record.skillId)
    if (!skillId) return null
    return {
      type: 'skill_created',
      skillId,
      title: optionalString(record.title),
      description: optionalString(record.description),
    }
  }

  if (record.type === 'schedule_created') {
    const scheduleId = optionalString(record.scheduleId)
    if (!scheduleId) return null
    return {
      type: 'schedule_created',
      scheduleId,
      title: optionalString(record.title),
      prompt: optionalString(record.prompt),
      frequencyLabel: optionalString(record.frequencyLabel),
      nextFireAt: optionalString(record.nextFireAt),
    }
  }

  return null
}
```

- [ ] **Step 4: Run parser tests**

Run:

```bash
pnpm exec vitest run src/components/chat-scene/result-cards/aijiaCardPayload.test.ts
```

Expected: PASS.

- [ ] **Step 5: Commit**

```bash
git add src/components/chat-scene/result-cards/aijiaCardPayload.ts src/components/chat-scene/result-cards/aijiaCardPayload.test.ts
git commit -m "feat: parse aijia result card payloads"
```

## Task 6: Render Skill And Schedule Result Cards

**Files:**
- Create: `src/components/chat-scene/result-cards/SkillCreatedCard.tsx`
- Create: `src/components/chat-scene/result-cards/ScheduleCreatedCard.tsx`
- Create: `src/components/chat-scene/result-cards/AijiaCardCodeBlock.tsx`
- Create: `src/components/chat-scene/result-cards/AijiaResultCard.test.tsx`
- Modify: `src/components/chat-scene/markdown/MarkdownCodeBlock.tsx`
- Modify: `src/components/chat-scene/markdown/markdownComponents.tsx`
- Modify: `src/i18n/zh-CN.json`
- Modify: `src/i18n/en-US.json`

- [ ] **Step 1: Write renderer tests**

Create `src/components/chat-scene/result-cards/AijiaResultCard.test.tsx`:

```tsx
import '@testing-library/jest-dom'
import { fireEvent, render, screen, waitFor } from '@testing-library/react'
import { beforeEach, describe, expect, it, vi } from 'vitest'

import { AssistantMarkdown } from '@/components/chat-scene/AssistantMarkdown'
import { useSkillStore } from '@/stores/skillStore'
import { useUiStore } from '@/stores/uiStore'

const tauriMocks = vi.hoisted(() => ({
  getAgendaItem: vi.fn(),
  updateAgendaItem: vi.fn(),
  createAgendaItem: vi.fn(),
  getDefaultFolder: vi.fn(),
  pickLocalDirectory: vi.fn(),
}))

vi.mock('@/hooks/useAuthorizedWorkspace', () => ({
  useAuthorizedWorkspace: () => ({ workspace: null }),
}))

vi.mock('@/lib/tauri', async () => {
  const actual = await vi.importActual<typeof import('@/lib/tauri')>('@/lib/tauri')
  return {
    ...actual,
    getAgendaItem: tauriMocks.getAgendaItem,
    updateAgendaItem: tauriMocks.updateAgendaItem,
    createAgendaItem: tauriMocks.createAgendaItem,
    getDefaultFolder: tauriMocks.getDefaultFolder,
    pickLocalDirectory: tauriMocks.pickLocalDirectory,
  }
})

describe('aijia result cards in assistant markdown', () => {
  beforeEach(() => {
    useSkillStore.setState({
      skills: [],
      isLoading: false,
    })
    useUiStore.setState({ route: { kind: 'chat', conversationId: 'conv-1' } })
    tauriMocks.getAgendaItem.mockReset()
    tauriMocks.updateAgendaItem.mockReset()
    tauriMocks.createAgendaItem.mockReset()
    tauriMocks.getDefaultFolder.mockResolvedValue(null)
    tauriMocks.pickLocalDirectory.mockResolvedValue(null)
  })

  it('renders invalid aijia-card JSON as a normal code block', () => {
    render(<AssistantMarkdown text={'```aijia-card\\n{ nope\\n```'} />)

    expect(screen.getByText('aijia-card')).toBeInTheDocument()
    expect(screen.getByText('{ nope')).toBeInTheDocument()
  })

  it('renders a skill-created card and opens skill detail', () => {
    useSkillStore.setState({
      skills: [{
        id: 'sales-followup',
        displayName: '销售跟进',
        displayNameEn: 'Sales Follow-up',
        description: '整理客户下一步动作',
        source: 'custom',
        hasWorkflow: true,
        shortDescription: '整理客户下一步动作',
        shortDescriptionEn: 'Plan next steps',
        triggerText: '/sales-followup',
        category: 'general',
        icon: '',
        updatedAt: null,
      }],
      isLoading: false,
    })

    render(<AssistantMarkdown text={'```aijia-card\\n{"type":"skill_created","skillId":"sales-followup"}\\n```'} />)

    expect(screen.getByText('技能已创建')).toBeInTheDocument()
    expect(screen.getByText('销售跟进')).toBeInTheDocument()

    fireEvent.click(screen.getByRole('button', { name: '查看技能' }))
    expect(useUiStore.getState().route).toEqual({ kind: 'skill-detail', skillId: 'sales-followup' })
  })

  it('renders a schedule-created card and opens the editor with live agenda data', async () => {
    tauriMocks.getAgendaItem.mockResolvedValue({
      id: 'agenda-1',
      title: '日报提醒',
      prompt: '每天总结日报',
      organizerEmployeeId: 'default',
      participants: [],
      startAt: '2026-06-13T01:00:00.000Z',
      timezone: 'Asia/Shanghai',
      rule: { freq: 'daily', interval: 1, endCondition: { kind: 'never' } },
      skipDates: [],
      nextFireAt: '2026-06-13T01:00:00.000Z',
      occurrenceCount: 0,
      status: 'active',
      overrideOf: null,
      workspacePath: null,
      createdAt: '2026-06-12T01:00:00.000Z',
      updatedAt: '2026-06-12T01:00:00.000Z',
    })

    render(<AssistantMarkdown text={'```aijia-card\\n{"type":"schedule_created","scheduleId":"agenda-1","title":"旧标题"}\\n```'} />)

    expect(await screen.findByText('定时任务已创建')).toBeInTheDocument()
    expect(screen.getByText('日报提醒')).toBeInTheDocument()

    fireEvent.click(screen.getByRole('button', { name: '编辑' }))
    expect(await screen.findByTestId('agenda-editor-title')).toHaveValue('日报提醒')
  })
})
```

- [ ] **Step 2: Run the failing renderer tests**

Run:

```bash
pnpm exec vitest run src/components/chat-scene/result-cards/AijiaResultCard.test.tsx
```

Expected: FAIL because result-card components do not exist.

- [ ] **Step 3: Export markdown node text helper**

In `src/components/chat-scene/markdown/MarkdownCodeBlock.tsx`, change:

```ts
function textFromNode(node: React.ReactNode): string {
```

to:

```ts
export function textFromNode(node: React.ReactNode): string {
```

- [ ] **Step 4: Create `AijiaCardCodeBlock`**

Create `src/components/chat-scene/result-cards/AijiaCardCodeBlock.tsx`:

```tsx
import { MarkdownCodeBlock, textFromNode } from '@/components/chat-scene/markdown/MarkdownCodeBlock'
import { parseAijiaCardPayload } from './aijiaCardPayload'
import { SkillCreatedCard } from './SkillCreatedCard'
import { ScheduleCreatedCard } from './ScheduleCreatedCard'

interface CodeProps {
  inline?: boolean
  className?: string
  children?: React.ReactNode
}

export function AijiaCardCodeBlock(props: CodeProps) {
  const rawCodeText = textFromNode(props.children).replace(/\n$/, '')
  const isAijiaCard = !props.inline && props.className === 'language-aijia-card'

  if (!isAijiaCard) return <MarkdownCodeBlock {...props} />

  const payload = parseAijiaCardPayload(rawCodeText)
  if (!payload) return <MarkdownCodeBlock {...props} />

  if (payload.type === 'skill_created') {
    return <SkillCreatedCard payload={payload} />
  }
  return <ScheduleCreatedCard payload={payload} />
}
```

- [ ] **Step 5: Create `SkillCreatedCard`**

Create `src/components/chat-scene/result-cards/SkillCreatedCard.tsx`:

```tsx
import { CheckCircle2, Copy, ExternalLink, Sparkles } from 'lucide-react'
import { useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'

import { Button } from '@/components/ui/button'
import { useSkillStore } from '@/stores/skillStore'
import { useUiStore } from '@/stores/uiStore'
import type { SkillCreatedCardPayload } from './aijiaCardPayload'

interface SkillCreatedCardProps {
  payload: SkillCreatedCardPayload
}

export function SkillCreatedCard({ payload }: SkillCreatedCardProps) {
  const { t, i18n } = useTranslation()
  const skill = useSkillStore((state) => state.getById(payload.skillId))
  const reload = useSkillStore((state) => state.reload)
  const setRoute = useUiStore((state) => state.setRoute)
  const [copied, setCopied] = useState(false)

  useEffect(() => {
    if (!skill) void reload().catch(() => undefined)
  }, [reload, skill])

  const title = skill
    ? i18n.language.toLowerCase().startsWith('en')
      ? skill.displayNameEn || skill.displayName || skill.id
      : skill.displayName || skill.id
    : payload.title || payload.skillId
  const description = skill?.shortDescription || skill?.description || payload.description || t('resultCards.skill.fallbackDescription')

  const handleCopy = () => {
    void navigator.clipboard.writeText(payload.skillId).then(() => {
      setCopied(true)
      setTimeout(() => setCopied(false), 1600)
    })
  }

  return (
    <div className="my-3 overflow-hidden rounded-lg border border-border bg-card shadow-sm" data-aijia-result-card="skill_created">
      <div className="flex items-start gap-3 border-b border-border/70 bg-muted/35 px-4 py-3">
        <div className="flex h-9 w-9 shrink-0 items-center justify-center rounded-md bg-primary/10 text-primary">
          <Sparkles className="h-4 w-4" aria-hidden />
        </div>
        <div className="min-w-0 flex-1">
          <div className="flex items-center gap-1.5 text-sm font-semibold text-foreground">
            <CheckCircle2 className="h-4 w-4 text-emerald-600" aria-hidden />
            {t('resultCards.skill.created')}
          </div>
          <div className="mt-1 truncate text-base font-semibold text-foreground">{title}</div>
          <div className="mt-1 line-clamp-2 text-sm text-muted-foreground">{description}</div>
        </div>
      </div>
      <div className="flex flex-wrap items-center justify-between gap-2 px-4 py-3">
        <code className="rounded bg-muted px-1.5 py-0.5 text-xs text-muted-foreground">{payload.skillId}</code>
        <div className="flex items-center gap-2">
          <Button size="sm" variant="outline" onClick={handleCopy}>
            <Copy className="h-3.5 w-3.5" />
            {copied ? t('common.copied', 'Copied') : t('resultCards.copyId')}
          </Button>
          <Button size="sm" onClick={() => setRoute({ kind: 'skill-detail', skillId: payload.skillId })}>
            <ExternalLink className="h-3.5 w-3.5" />
            {t('resultCards.skill.view')}
          </Button>
        </div>
      </div>
    </div>
  )
}
```

- [ ] **Step 6: Create `ScheduleCreatedCard`**

Create `src/components/chat-scene/result-cards/ScheduleCreatedCard.tsx`:

```tsx
import { CalendarClock, CheckCircle2, ExternalLink, Pencil } from 'lucide-react'
import { useCallback, useEffect, useState } from 'react'
import { useTranslation } from 'react-i18next'

import { AgendaItemEditor } from '@/features/agenda/AgendaItemEditor'
import { Button } from '@/components/ui/button'
import { type AgendaItem, getAgendaItem } from '@/lib/tauri'
import { useUiStore } from '@/stores/uiStore'
import type { ScheduleCreatedCardPayload } from './aijiaCardPayload'

interface ScheduleCreatedCardProps {
  payload: ScheduleCreatedCardPayload
}

export function ScheduleCreatedCard({ payload }: ScheduleCreatedCardProps) {
  const { t } = useTranslation()
  const setRoute = useUiStore((state) => state.setRoute)
  const [item, setItem] = useState<AgendaItem | null>(null)
  const [loadFailed, setLoadFailed] = useState(false)
  const [editorOpen, setEditorOpen] = useState(false)

  const loadItem = useCallback(async () => {
    try {
      const next = await getAgendaItem(payload.scheduleId)
      setItem(next)
      setLoadFailed(false)
    } catch {
      setLoadFailed(true)
    }
  }, [payload.scheduleId])

  useEffect(() => {
    void loadItem()
  }, [loadItem])

  const title = item?.title || payload.title || payload.scheduleId
  const prompt = item?.prompt || payload.prompt || t('resultCards.schedule.fallbackPrompt')
  const nextFire = item?.nextFireAt || payload.nextFireAt || null
  const scheduleLabel = payload.frequencyLabel || (item?.rule ? t(`schedules.frequency.noun.${item.rule.freq}`) : t('schedules.editor.freqOptions.oneShot'))

  return (
    <div className="my-3 overflow-hidden rounded-lg border border-border bg-card shadow-sm" data-aijia-result-card="schedule_created">
      <div className="flex items-start gap-3 border-b border-border/70 bg-muted/35 px-4 py-3">
        <div className="flex h-9 w-9 shrink-0 items-center justify-center rounded-md bg-primary/10 text-primary">
          <CalendarClock className="h-4 w-4" aria-hidden />
        </div>
        <div className="min-w-0 flex-1">
          <div className="flex items-center gap-1.5 text-sm font-semibold text-foreground">
            <CheckCircle2 className="h-4 w-4 text-emerald-600" aria-hidden />
            {t('resultCards.schedule.created')}
          </div>
          <div className="mt-1 truncate text-base font-semibold text-foreground">{title}</div>
          <div className="mt-1 line-clamp-2 text-sm text-muted-foreground">{prompt}</div>
        </div>
      </div>
      <div className="grid gap-2 px-4 py-3 text-sm text-muted-foreground sm:grid-cols-2">
        <div>
          <span className="text-foreground">{t('resultCards.schedule.frequency')}</span>
          <span className="ml-2">{scheduleLabel}</span>
        </div>
        <div>
          <span className="text-foreground">{t('resultCards.schedule.nextFire')}</span>
          <span className="ml-2">{nextFire ? new Date(nextFire).toLocaleString() : '-'}</span>
        </div>
      </div>
      {loadFailed ? (
        <div className="px-4 pb-2 text-xs text-muted-foreground">{t('resultCards.schedule.unavailable')}</div>
      ) : null}
      <div className="flex flex-wrap items-center justify-between gap-2 px-4 pb-3">
        <code className="rounded bg-muted px-1.5 py-0.5 text-xs text-muted-foreground">{payload.scheduleId}</code>
        <div className="flex items-center gap-2">
          <Button size="sm" variant="outline" onClick={() => setEditorOpen(true)} disabled={!item}>
            <Pencil className="h-3.5 w-3.5" />
            {t('resultCards.schedule.edit')}
          </Button>
          <Button size="sm" onClick={() => setRoute({ kind: 'schedules' })}>
            <ExternalLink className="h-3.5 w-3.5" />
            {t('resultCards.schedule.view')}
          </Button>
        </div>
      </div>
      <AgendaItemEditor
        open={editorOpen}
        initial={item}
        onClose={() => setEditorOpen(false)}
        onSaved={() => {
          void loadItem()
        }}
      />
    </div>
  )
}
```

- [ ] **Step 7: Wire the markdown code override**

In `src/components/chat-scene/markdown/markdownComponents.tsx`:

```ts
import { AijiaCardCodeBlock } from '@/components/chat-scene/result-cards/AijiaCardCodeBlock'
```

Replace:

```ts
code: MarkdownCodeBlock,
```

with:

```ts
code: AijiaCardCodeBlock,
```

Remove the now-unused `MarkdownCodeBlock` import from this file.

- [ ] **Step 8: Add card i18n keys**

In `src/i18n/zh-CN.json`, merge:

```json
"resultCards": {
  "copyId": "复制 ID",
  "skill": {
    "created": "技能已创建",
    "view": "查看技能",
    "fallbackDescription": "技能已创建，可前往技能详情查看。"
  },
  "schedule": {
    "created": "定时任务已创建",
    "edit": "编辑",
    "view": "查看定时任务",
    "frequency": "频率",
    "nextFire": "下次运行",
    "fallbackPrompt": "定时任务已创建。",
    "unavailable": "当前无法读取这个定时任务，可能已被删除或暂时不可用。"
  }
}
```

In `src/i18n/en-US.json`, merge:

```json
"resultCards": {
  "copyId": "Copy ID",
  "skill": {
    "created": "Skill created",
    "view": "View skill",
    "fallbackDescription": "The skill was created and can be opened from skill details."
  },
  "schedule": {
    "created": "Scheduled task created",
    "edit": "Edit",
    "view": "View scheduled tasks",
    "frequency": "Frequency",
    "nextFire": "Next run",
    "fallbackPrompt": "The scheduled task was created.",
    "unavailable": "This scheduled task cannot be loaded. It may have been deleted or is temporarily unavailable."
  }
}
```

- [ ] **Step 9: Run renderer tests**

Run:

```bash
pnpm exec vitest run src/components/chat-scene/result-cards/aijiaCardPayload.test.ts src/components/chat-scene/result-cards/AijiaResultCard.test.tsx src/components/chat-scene/__tests__/AssistantMarkdown.test.tsx src/components/chat-scene/markdown/__tests__/AssistantMarkdown.test.tsx
```

Expected: PASS.

- [ ] **Step 10: Commit**

```bash
git add src/components/chat-scene/result-cards src/components/chat-scene/markdown/MarkdownCodeBlock.tsx src/components/chat-scene/markdown/markdownComponents.tsx src/i18n/zh-CN.json src/i18n/en-US.json
git commit -m "feat: render creation result cards in chat"
```

## Task 7: Preserve Chat History Immutability In Schedule Editing Tests

**Files:**
- Modify: `src/components/chat-scene/result-cards/AijiaResultCard.test.tsx`

- [ ] **Step 1: Add a focused immutability test**

Append this test to `AijiaResultCard.test.tsx`:

```tsx
it('edits a schedule through agenda APIs without mutating assistant markdown', async () => {
  const originalText = '```aijia-card\\n{"type":"schedule_created","scheduleId":"agenda-1","title":"旧标题"}\\n```'
  tauriMocks.getAgendaItem.mockResolvedValue({
    id: 'agenda-1',
    title: '日报提醒',
    prompt: '每天总结日报',
    organizerEmployeeId: 'default',
    participants: [],
    startAt: '2026-06-13T01:00:00.000Z',
    timezone: 'Asia/Shanghai',
    rule: null,
    skipDates: [],
    nextFireAt: null,
    occurrenceCount: 0,
    status: 'active',
    overrideOf: null,
    workspacePath: null,
    createdAt: '2026-06-12T01:00:00.000Z',
    updatedAt: '2026-06-12T01:00:00.000Z',
  })
  tauriMocks.updateAgendaItem.mockResolvedValue({
    id: 'agenda-1',
    title: '日报提醒新版',
    prompt: '每天总结日报',
    organizerEmployeeId: 'default',
    participants: [],
    startAt: '2026-06-13T01:00:00.000Z',
    timezone: 'Asia/Shanghai',
    rule: null,
    skipDates: [],
    nextFireAt: null,
    occurrenceCount: 0,
    status: 'active',
    overrideOf: null,
    workspacePath: null,
    createdAt: '2026-06-12T01:00:00.000Z',
    updatedAt: '2026-06-12T02:00:00.000Z',
  })

  const { rerender } = render(<AssistantMarkdown text={originalText} />)
  fireEvent.click(await screen.findByRole('button', { name: '编辑' }))
  fireEvent.change(await screen.findByTestId('agenda-editor-title'), {
    target: { value: '日报提醒新版' },
  })
  fireEvent.click(screen.getByTestId('agenda-editor-title').closest('[data-aijia-agenda-editor]')!.querySelector('[data-aijia-agenda-action="save"]')!)

  await waitFor(() => {
    expect(tauriMocks.updateAgendaItem).toHaveBeenCalledWith('agenda-1', expect.objectContaining({
      title: '日报提醒新版',
    }))
  })

  rerender(<AssistantMarkdown text={originalText} />)
  expect(originalText).toContain('"title":"旧标题"')
})
```

This test does not inspect local `messages.jsonl` directly. It proves the card edit path uses `update_agenda_item` and leaves the markdown input immutable.

- [ ] **Step 2: Run the immutability test**

Run:

```bash
pnpm exec vitest run src/components/chat-scene/result-cards/AijiaResultCard.test.tsx -t "without mutating assistant markdown"
```

Expected: PASS.

- [ ] **Step 3: Commit**

```bash
git add src/components/chat-scene/result-cards/AijiaResultCard.test.tsx
git commit -m "test: cover schedule card edit immutability"
```

## Task 8: Final Verification

**Files:**
- No new source files expected in this task.

- [ ] **Step 1: Run focused frontend tests**

Run:

```bash
pnpm exec vitest run \
  src/features/chat/conversationCreatePrompts.test.ts \
  src/components/shell/ChatTopBar.test.tsx \
  src/components/sidebar/__tests__/AppSidebar.test.tsx \
  src/components/sidebar/__tests__/ConversationRow.test.tsx \
  src/features/chat/ChatPage.test.tsx \
  src/components/chat-scene/result-cards/aijiaCardPayload.test.ts \
  src/components/chat-scene/result-cards/AijiaResultCard.test.tsx \
  src/components/chat-scene/__tests__/AssistantMarkdown.test.tsx \
  src/components/chat-scene/markdown/__tests__/AssistantMarkdown.test.tsx
```

Expected: PASS.

- [ ] **Step 2: Run broader build check**

Run:

```bash
pnpm build
```

Expected: PASS. If the build fails because of a pre-existing unrelated issue, capture the exact error and run `pnpm exec tsc --noEmit` or the repo's existing narrower check that isolates changed files.

- [ ] **Step 3: Check retired flow references did not return in non-document code**

Run:

```bash
rg -n -P "小程(?!序)|builtin:xiaocheng|xiaocheng|程砚舟|Skill-Smith|skill-smith|skill_smith|/create-skill" src scripts public src-tauri
```

Expected: no output.

- [ ] **Step 4: Inspect working tree**

Run:

```bash
git status --short
```

Expected: only files touched by this plan are modified, or the executor clearly separates pre-existing user changes from this implementation.

- [ ] **Step 5: Final commit if previous tasks were not committed individually**

If the executor did not commit per task, commit the complete feature:

```bash
git add src/features/chat src/components/shell src/components/sidebar src/components/chat-scene src/i18n/zh-CN.json src/i18n/en-US.json
git commit -m "feat: create skills and schedules from chat header"
```

## Manual QA

- Open an existing conversation.
- Click the header more icon.
- Verify menu order: 重命名, 置顶/取消置顶, 导出对话, 复制会话 ID, 总结对话并创建技能, 总结对话并创建定时任务, 归档.
- Click `总结对话并创建技能`.
- Verify a normal user message is sent in the current conversation.
- Paste or produce a valid `skill_created` `aijia-card` block and verify it becomes a card.
- Click `查看技能` and verify it routes to skill detail.
- Click `总结对话并创建定时任务`.
- Verify a normal user message is sent in the current conversation.
- Paste or produce a valid `schedule_created` `aijia-card` block and verify it becomes a card.
- Click `编辑`, change the title, save, and verify the card reflects agenda state while the original assistant markdown input remains unchanged.

## Acceptance Criteria Mapping

- Header more dropdown: Task 3 and Task 4.
- Inherited conversation actions: Task 2 and Task 4.
- Creation actions send one generated user message: Task 1 and Task 4.
- Skill-created card rendering: Task 5 and Task 6.
- Scheduled-task card rendering and editing: Task 5, Task 6, and Task 7.
- No `messages.jsonl` rewrite on schedule edit: Task 7.
- Retired flow names do not return in code: Task 8 Step 3.

## Self-Review Notes

- Spec coverage: all goals from the design spec map to at least one task above.
- Placeholder scan: the plan avoids unresolved markers and unspecified "add tests" steps; every test step includes concrete assertions or commands.
- Type consistency: payload names are consistent across parser, renderer, and prompt examples: `skill_created` with `skillId`, `schedule_created` with `scheduleId`.
