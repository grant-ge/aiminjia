# 前端工具状态可视化（Plan-S）

> **For agentic workers:** REQUIRED SUB-SKILL: `superpowers:test-driven-development` — 每个 Task 的测试必须先写（red），再写实现（green），再提交。

**Goal:** 让工具错误和子任务进度对用户可见  
**Tech Stack:** React, TypeScript, Zustand  
**Worktree branch:** pzc  
**Estimated effort:** S1 ~1h, S2 ~1.5h, S3 ~0.5h

---

## 现有结构摘要

### 数据流（已就绪，零 UI 消费）

- `useStreaming.ts` 中 `tool:completed` handler 已将 `success: false` 路径写入 store：
  ```ts
  updateConversationToolExecution(conversationId, toolId, {
    status: success ? 'completed' : 'error',
    summary,
  })
  ```
- `useStreaming.ts` 中 `task:status-changed` handler 已写入：
  ```ts
  upsertConversationTaskState(conversationId, { taskId, status, runId })
  ```
- `streamingStore.ts` 类型：
  - `ToolExecution.status: 'executing' | 'completed' | 'error'`
  - `ToolExecution.summary?: string`（error 时包含错误信息）
  - `ConversationTaskState: { taskId, status, runId }`（`status` 是 `string`，值域约定为 `pending/running/completed/failed`）
  - `taskStates: Record<string, ConversationTaskState[]>`（key 是 conversationId）

### 渲染缺口

`StreamingBubble.tsx`（`src/components/chat/StreamingBubble.tsx`）：

- `activeTool` 只取 `status === 'executing'` 的第一个工具（`find`）
- `status === 'error'` 工具被完全忽略
- `taskStates` 从未被读取

---

## Task S1 — Tool Error 可见性

**文件：** `src/components/chat/StreamingBubble.tsx`

### 目标行为

- 在流式气泡下方，保留现有 `activeTool`（executing）的转圈 spinner 展示逻辑不变
- 新增：展示所有 `status === 'error'` 的工具，每条显示：`❌ <toolLabel>: <summary截断到80字符>`
- error 列表与 executing 状态并列（error 在 executing 行下方）
- 样式：使用 `var(--color-semantic-red)` 或 `text-red-500`，字号 `text-xs`

### 实现步骤

**Step 1：先写测试（TDD red phase）**

在 `src/components/chat/StreamingBubble.test.tsx`（新文件）写：

```tsx
import { render } from '@testing-library/react'
import { describe, it, expect, vi, beforeEach } from 'vitest'

// Mock dependencies
vi.mock('@tauri-apps/api/event', () => ({ listen: vi.fn(() => Promise.resolve(() => {})) }))
vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn() }))
vi.mock('react-i18next', () => ({
  useTranslation: () => ({ t: (key: string, fallback?: string) => fallback ?? key }),
}))
vi.mock('@/hooks/useProductName', () => ({ useProductName: () => 'AIjia' }))

import { StreamingBubble } from './StreamingBubble'
import { useChatStore } from '@/stores/chatStore'

describe('StreamingBubble — tool error visibility', () => {
  beforeEach(() => {
    useChatStore.setState({
      activeConversationId: 'conv-1',
      streamStates: {},
      taskStates: {},
      toolExecutions: [],
      isStreaming: false,
      streamingContent: '',
      busyConversations: new Set(),
      conversations: [],
      messages: [],
    })
  })

  it('renders error tool with ❌ icon and truncated summary', () => {
    useChatStore.setState({
      activeConversationId: 'conv-1',
      streamStates: {
        'conv-1': {
          isStreaming: true,
          streamingContent: '',
          toolExecutions: [
            {
              toolName: 'execute_python',
              toolId: 'tool-err-1',
              status: 'error',
              summary: 'ModuleNotFoundError: No module named pandas. Please install it first.',
            },
          ],
        },
      },
      toolExecutions: [
        {
          toolName: 'execute_python',
          toolId: 'tool-err-1',
          status: 'error',
          summary: 'ModuleNotFoundError: No module named pandas. Please install it first.',
        },
      ],
    })

    const { getByText, getByLabelText } = render(<StreamingBubble content="" />)
    // ❌ icon must be present (aria-label="tool error")
    expect(getByLabelText('tool error')).toBeTruthy()
    // summary must appear, truncated to 80 chars
    expect(getByText(/ModuleNotFoundError/)).toBeTruthy()
  })

  it('does not render error section when no error tools', () => {
    useChatStore.setState({
      activeConversationId: 'conv-1',
      streamStates: {
        'conv-1': {
          isStreaming: true,
          streamingContent: '',
          toolExecutions: [
            { toolName: 'load_data', toolId: 'tool-1', status: 'executing' },
          ],
        },
      },
      toolExecutions: [
        { toolName: 'load_data', toolId: 'tool-1', status: 'executing' },
      ],
    })

    const { queryByLabelText } = render(<StreamingBubble content="" />)
    expect(queryByLabelText('tool error')).toBeNull()
  })

  it('truncates summary longer than 80 characters', () => {
    const longSummary = 'A'.repeat(120)
    useChatStore.setState({
      activeConversationId: 'conv-1',
      streamStates: {
        'conv-1': {
          isStreaming: true,
          streamingContent: '',
          toolExecutions: [
            { toolName: 'execute_python', toolId: 'tool-2', status: 'error', summary: longSummary },
          ],
        },
      },
      toolExecutions: [
        { toolName: 'execute_python', toolId: 'tool-2', status: 'error', summary: longSummary },
      ],
    })

    const { getByLabelText } = render(<StreamingBubble content="" />)
    const errorEl = getByLabelText('tool error')
    // The rendered text should not exceed 80 + label chars
    expect(errorEl.textContent?.length).toBeLessThanOrEqual(120) // rough bound
    expect(errorEl.textContent).toContain('…')
  })
})
```

**Step 2：修改 `StreamingBubble.tsx`（green phase）**

在现有 `activeTool` spinner 逻辑后，增加 error 工具列表渲染：

```tsx
// 现有代码中 toolExecutions 来自 legacy scalar；需改为读 per-conv stream state
const { toolExecutions, activeConversationId, streamStates, taskStates } = useChatStore((s) => ({
  toolExecutions: s.activeConversationId
    ? (s.streamStates[s.activeConversationId]?.toolExecutions ?? s.toolExecutions)
    : s.toolExecutions,
  activeConversationId: s.activeConversationId,
  streamStates: s.streamStates,
  taskStates: s.taskStates,
}))

const errorTools = toolExecutions.filter((t) => t.status === 'error')
```

错误工具列表 JSX（插入在 `activeTool` spinner 块之后，`TypingIndicator` 块之前）：

```tsx
{errorTools.length > 0 && (
  <div className="mt-2 flex flex-col gap-1">
    {errorTools.map((tool) => {
      const label = t('streaming.tools.' + tool.toolName, tool.toolName)
      const rawSummary = tool.summary ?? ''
      const summary = rawSummary.length > 80 ? rawSummary.slice(0, 80) + '…' : rawSummary
      return (
        <div
          key={tool.toolId}
          className="flex items-start gap-1.5 text-xs"
          style={{ color: 'var(--color-semantic-red, #ef4444)' }}
        >
          <span aria-label="tool error" className="mt-px shrink-0">❌</span>
          <span>
            <span className="font-medium">{label}</span>
            {summary ? <span className="opacity-80">: {summary}</span> : null}
          </span>
        </div>
      )
    })}
  </div>
)}
```

**Step 3：git commit**

```bash
git add src/components/chat/StreamingBubble.tsx src/components/chat/StreamingBubble.test.tsx
git commit -m "feat(streaming-bubble): surface tool error status with summary (S1)"
```

---

## Task S2 — Task Status 子任务列表组件

**文件：** `src/components/chat/StreamingBubble.tsx`（inline component）或新建 `src/components/chat/TaskStatusList.tsx`

### 目标行为

- 读取 `taskStates[activeConversationId]`（`ConversationTaskState[]`）
- 每个 task 显示一行：`<statusIcon> <taskId 后8位>（或将来的 title 字段）`
- `status` 图标映射：
  - `pending` → `⏳`
  - `running` → 转圈 SVG spinner（同 activeTool 的 spinner）
  - `completed` → `✓`（绿色）
  - `failed` → `✗`（红色）
  - 其他未知值 → `•`
- 折叠设计：默认折叠（`<details>` 元素或 `useState`）；当有 `running` 状态的 task 时自动展开
- 标题行文本：`{runningCount > 0 ? t('streaming.tasks.running', { count: runningCount }) : t('streaming.tasks.done', { count: tasks.length })}`

### 实现步骤

**Step 1：先写测试（TDD red phase）**

新建 `src/components/chat/TaskStatusList.test.tsx`：

```tsx
import { render, screen } from '@testing-library/react'
import { describe, it, expect } from 'vitest'
import { TaskStatusList } from './TaskStatusList'
import type { ConversationTaskState } from '@/stores/streamingStore'

describe('TaskStatusList', () => {
  it('renders nothing when tasks array is empty', () => {
    const { container } = render(<TaskStatusList tasks={[]} />)
    expect(container.firstChild).toBeNull()
  })

  it('renders running task with spinner icon', () => {
    const tasks: ConversationTaskState[] = [
      { taskId: 'task-abcd1234', status: 'running', runId: 'run-1' },
    ]
    render(<TaskStatusList tasks={tasks} />)
    // Should show spinning indicator
    expect(screen.getByRole('img', { name: /running/i })).toBeTruthy()
  })

  it('renders completed task with check icon', () => {
    const tasks: ConversationTaskState[] = [
      { taskId: 'task-abcd1234', status: 'completed', runId: 'run-1' },
    ]
    render(<TaskStatusList tasks={tasks} />)
    expect(screen.getByRole('img', { name: /completed/i })).toBeTruthy()
  })

  it('renders failed task with red ✗ icon', () => {
    const tasks: ConversationTaskState[] = [
      { taskId: 'task-abcd1234', status: 'failed', runId: 'run-1' },
    ]
    render(<TaskStatusList tasks={tasks} />)
    expect(screen.getByRole('img', { name: /failed/i })).toBeTruthy()
  })

  it('auto-expands when any task is running', () => {
    const tasks: ConversationTaskState[] = [
      { taskId: 'task-1', status: 'completed', runId: 'run-1' },
      { taskId: 'task-2', status: 'running', runId: 'run-2' },
    ]
    render(<TaskStatusList tasks={tasks} />)
    // Task items should be visible (not hidden)
    expect(screen.getAllByRole('listitem').length).toBe(2)
  })

  it('is collapsed by default when no running tasks', () => {
    const tasks: ConversationTaskState[] = [
      { taskId: 'task-1', status: 'completed', runId: 'run-1' },
      { taskId: 'task-2', status: 'completed', runId: 'run-2' },
    ]
    const { container } = render(<TaskStatusList tasks={tasks} />)
    // details element should not have 'open' attribute
    const details = container.querySelector('details')
    expect(details?.hasAttribute('open')).toBe(false)
  })

  it('displays last 8 chars of taskId', () => {
    const tasks: ConversationTaskState[] = [
      { taskId: 'task-abcd1234', status: 'pending', runId: 'run-1' },
    ]
    render(<TaskStatusList tasks={tasks} />)
    expect(screen.getByText(/abcd1234/)).toBeTruthy()
  })
})
```

**Step 2：新建 `src/components/chat/TaskStatusList.tsx`**

```tsx
import type { ConversationTaskState } from '@/stores/streamingStore'
import { useTranslation } from 'react-i18next'

interface TaskStatusListProps {
  tasks: ConversationTaskState[]
}

function StatusIcon({ status }: { status: string }) {
  switch (status) {
    case 'running':
      return (
        <svg
          role="img"
          aria-label="running"
          className="h-3 w-3 animate-spin shrink-0"
          viewBox="0 0 24 24"
          fill="none"
          stroke="currentColor"
          strokeWidth="2.5"
        >
          <circle cx="12" cy="12" r="10" strokeDasharray="50" strokeDashoffset="20" strokeLinecap="round" />
        </svg>
      )
    case 'completed':
      return (
        <span
          role="img"
          aria-label="completed"
          className="shrink-0 text-xs font-bold"
          style={{ color: 'var(--color-semantic-green, #22c55e)' }}
        >
          ✓
        </span>
      )
    case 'failed':
      return (
        <span
          role="img"
          aria-label="failed"
          className="shrink-0 text-xs font-bold"
          style={{ color: 'var(--color-semantic-red, #ef4444)' }}
        >
          ✗
        </span>
      )
    case 'pending':
      return (
        <span role="img" aria-label="pending" className="shrink-0 text-xs opacity-50">⏳</span>
      )
    default:
      return <span role="img" aria-label="unknown" className="shrink-0 text-xs opacity-40">•</span>
  }
}

export function TaskStatusList({ tasks }: TaskStatusListProps) {
  const { t } = useTranslation()

  if (tasks.length === 0) return null

  const runningCount = tasks.filter((task) => task.status === 'running').length
  const isOpen = runningCount > 0

  const summaryText = runningCount > 0
    ? t('streaming.tasks.running', { count: runningCount, defaultValue: `${runningCount} task(s) running` })
    : t('streaming.tasks.done', { count: tasks.length, defaultValue: `${tasks.length} task(s) done` })

  return (
    <details
      open={isOpen}
      className="mt-2 text-xs"
      style={{ color: 'var(--color-text-muted)' }}
    >
      <summary className="cursor-pointer select-none list-none hover:opacity-80">
        {summaryText}
      </summary>
      <ul className="mt-1 flex flex-col gap-0.5 pl-1">
        {tasks.map((task) => (
          <li key={task.taskId} className="flex items-center gap-1.5">
            <StatusIcon status={task.status} />
            <span className="font-mono opacity-70">
              {task.taskId.slice(-8)}
            </span>
          </li>
        ))}
      </ul>
    </details>
  )
}
```

**Step 3：在 `StreamingBubble.tsx` 中集成 `TaskStatusList`**

在 error tools 列表之后（`</div>` 闭合后）插入：

```tsx
import { TaskStatusList } from './TaskStatusList'

// 在 StreamingBubble 组件函数内，读取 taskStates：
const tasks = useChatStore((s) => {
  const activeId = s.activeConversationId
  return activeId ? (s.taskStates[activeId] ?? []) : []
})

// JSX 中，在 errorTools 区块后：
<TaskStatusList tasks={tasks} />
```

**Step 4：git commit**

```bash
git add src/components/chat/TaskStatusList.tsx src/components/chat/TaskStatusList.test.tsx src/components/chat/StreamingBubble.tsx
git commit -m "feat(streaming-bubble): add TaskStatusList sub-task progress UI (S2)"
```

---

## Task S3 — 联调测试补充

**目标：** 补充 `StreamingBubble.test.tsx` 中的集成快照，确保 S1 + S2 同时存在时渲染正确。

**文件：** `src/components/chat/StreamingBubble.test.tsx`（追加 describe block）

```tsx
describe('StreamingBubble — S1+S2 combined', () => {
  it('renders both error tools and task status when both are present', () => {
    useChatStore.setState({
      activeConversationId: 'conv-1',
      streamStates: {
        'conv-1': {
          isStreaming: true,
          streamingContent: 'Analyzing...',
          toolExecutions: [
            { toolName: 'execute_python', toolId: 'tool-err', status: 'error', summary: 'TypeError: unsupported operand' },
            { toolName: 'load_data', toolId: 'tool-ok', status: 'executing' },
          ],
        },
      },
      toolExecutions: [
        { toolName: 'execute_python', toolId: 'tool-err', status: 'error', summary: 'TypeError: unsupported operand' },
        { toolName: 'load_data', toolId: 'tool-ok', status: 'executing' },
      ],
      taskStates: {
        'conv-1': [
          { taskId: 'task-running-1', status: 'running', runId: 'run-1' },
          { taskId: 'task-done-0000', status: 'completed', runId: 'run-0' },
        ],
      },
    })

    const { getByLabelText, getByRole } = render(<StreamingBubble content="Analyzing..." />)
    // Error tool visible
    expect(getByLabelText('tool error')).toBeTruthy()
    // Running task spinner visible
    expect(getByRole('img', { name: /running/i })).toBeTruthy()
  })
})
```

**运行验证命令：**

```bash
pnpm exec vitest run src/components/chat/StreamingBubble.test.tsx src/components/chat/TaskStatusList.test.tsx
```

**Step 4：git commit**

```bash
git add src/components/chat/StreamingBubble.test.tsx
git commit -m "test(streaming-bubble): add S1+S2 combined integration test (S3)"
```

---

## 实施检查清单

- [ ] S1 测试先通过 red，再写实现让其 green
- [ ] S2 测试先通过 red，再写实现让其 green
- [ ] `pnpm lint` 无报错
- [ ] `pnpm exec vitest run src/components/chat/StreamingBubble.test.tsx src/components/chat/TaskStatusList.test.tsx` 全绿
- [ ] `pnpm exec vitest run src/hooks/useStreaming.integration.test.tsx` 无回归

## 已知限制 / 后续可做

- `ConversationTaskState.taskId` 是后端内部 UUID，目前显示后 8 位；后续若后端增加 `title` 字段，直接替换渲染逻辑
- `status` 字段为 `string` 类型，`StatusIcon` 的 switch 已覆盖 `pending/running/completed/failed` 四种已知值，其他后向兼容为 `•`
- error 工具列表仅在 **流式进行中**（`StreamingBubble` 存在时）可见；如需历史消息中也展示错误，需在 `AiBubble.tsx` 中另行处理（不在本计划范围内）
