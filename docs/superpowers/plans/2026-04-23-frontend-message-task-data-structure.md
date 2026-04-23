# 前端消息与任务数据结构 — 实现计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 新增 `role: 'tool'` 消息类型、扩展事件 payload 类型、重构 `useTurnRenderModel` 支持历史 tool 消息分组，并在 ChatPage 右侧新增常驻辅助栏（RightPanel）展示待办/产物/技能占位三个 accordion section。

**Architecture:** 类型层（message.ts、tauri.ts、streamingStore.ts）先行；然后 sessionStore 加 `upsertMessage`；useStreaming 事件处理接入新 payload；useTurnRenderModel 扩展 tool role 分组逻辑；最后 RightPanel 组件落地并接入 ChatPage。所有改动向后兼容——无 `toolCalls` 的老数据继续正常渲染。

**Tech Stack:** TypeScript, React, Zustand, Tauri IPC, lucide-react, Tailwind CSS

---

## 文件变更索引

| 文件 | 操作 | 职责 |
|------|------|------|
| `src/types/message.ts` | Modify | 加 `tool` role、`AssistantToolCall`、`ToolResultContent`、Message 扩展字段 |
| `src/lib/tauri.ts` | Modify | 扩展 `ToolExecutingPayload`、`TaskStatusChangedPayload`；`tool:completed` 改为 `Message` 类型；加 `getTasks` 函数 |
| `src/stores/streamingStore.ts` | Modify | `ToolExecution` 加 `input`；`ConversationTaskState` 补全字段 |
| `src/stores/sessionStore.ts` | Modify | 加 `upsertMessage` action |
| `src/hooks/useStreaming.ts` | Modify | 更新三个事件的处理逻辑 |
| `src/hooks/useTurnRenderModel.ts` | Modify | `buildTurnsFromMessages` 支持 `role: 'tool'` 分组 |
| `src/hooks/useChat.ts` | Modify | `switchConversation` 时调用 `getTasks` 恢复 task 列表 |
| `src/components/chat/RightPanel.tsx` | Create | RightPanel 含三个 section |
| `src/features/chat/ChatPage.tsx` | Modify | 挂载 RightPanel |

---

## Task 1：消息类型定义扩展

**Files:**
- Modify: `src/types/message.ts`

- [ ] **Step 1.1：在 `message.ts` 里加三个新类型和扩展 Message**

找到 `export type MessageRole = ...`，改为：

```ts
export type MessageRole = 'user' | 'assistant' | 'system' | 'tool'
```

在文件末尾（`SubAgentTranscriptEntry` 之后）加：

```ts
// --- Tool Call ---

/** assistant 消息里的工具调用入参（来自磁盘 toolCalls 字段） */
export interface AssistantToolCall {
  id: string          // tool_call_id，与 tool 消息的 toolCallId 对应
  name: string        // 工具名，如 "browse_navigate"
  arguments: unknown  // 完整入参 JSON 对象
}

/** role: 'tool' 消息的工具结果内容 */
export interface ToolResultContent {
  toolCallId: string   // 与 AssistantToolCall.id 对应
  name: string         // 工具名
  content: string      // 完整工具输出文本
  isError: boolean     // 是否执行失败
  durationMs?: number  // 执行耗时（ms）
}
```

找到 `export interface Message {`，在 `sender?: MessageSender` 之后加两行：

```ts
  /** assistant 消息专用：工具调用入参列表，来自磁盘 toolCalls 字段 */
  toolCalls?: AssistantToolCall[]
  /** tool 消息专用：工具执行结果 */
  toolResult?: ToolResultContent
```

- [ ] **Step 1.2：运行类型检查确认无报错**

```bash
cd /Users/oayzz/project/lotus/lotus-workbench/lotus-app && pnpm tsc --noEmit 2>&1 | grep "error TS" | head -20
```

预期：0 error TS（可能有 warning，忽略）。

- [ ] **Step 1.3：Commit**

```bash
git add src/types/message.ts
git commit -m "feat(types): add tool MessageRole, AssistantToolCall, ToolResultContent"
```

---

## Task 2：tauri.ts 事件类型扩展 + getTasks

**Files:**
- Modify: `src/lib/tauri.ts`

- [ ] **Step 2.1：扩展 `ToolExecutingPayload` 加 input**

找到：
```ts
export interface ToolExecutingPayload {
  conversationId: string
  toolName: string
  toolId: string
  purpose?: string
}
```

改为：
```ts
export interface ToolExecutingPayload {
  conversationId: string
  toolName: string
  toolId: string
  purpose?: string
  input?: unknown  // 完整入参 JSON 对象
}
```

- [ ] **Step 2.2：tool:completed payload 改为 Message**

找到：
```ts
export interface ToolCompletedPayload {
  conversationId: string
  toolName: string
  toolId: string
  success: boolean
  summary?: string
}
```

替换为（保留旧接口作为类型注释，不删除，避免引用报错）：

```ts
/** @deprecated tool:completed 现在直接推完整 Message，保留此类型仅供旧引用过渡 */
export interface ToolCompletedPayload {
  conversationId: string
  toolName: string
  toolId: string
  success: boolean
  summary?: string
}
```

找到 `onToolCompleted` 函数，将其类型参数从 `ToolCompletedPayload` 改为 `Message`：

```ts
export function onToolCompleted(
  handler: (payload: Message) => void,
): Promise<() => void> {
  return listen<Message>(TAURI_EVENTS.TOOL_COMPLETED, (event) => {
    handler(event.payload)
  })
}
```

在文件顶部 `import type { Message ...}` 处确认 `Message` 已导入（已有）。

- [ ] **Step 2.3：扩展 `TaskStatusChangedPayload` 补全字段**

找到：
```ts
export interface TaskStatusChangedPayload {
  conversationId: string
  taskId: string
  status: string
  runId: string
}
```

改为：
```ts
export interface TaskStatusChangedPayload {
  conversationId: string
  taskId: string
  status: string
  runId: string
  subject: string
  description?: string
  activeForm?: string
  owner?: string
  blockedBy?: string[]
  createdAt?: string
}
```

- [ ] **Step 2.4：新增 getTasks 函数**

在 `getSubagentTranscript` 函数附近加：

```ts
export function getTasks(
  conversationId: string,
): Promise<import('@/stores/streamingStore').ConversationTaskState[]> {
  return invoke('get_tasks', { conversationId })
}
```

- [ ] **Step 2.5：类型检查**

```bash
pnpm tsc --noEmit 2>&1 | grep "error TS" | head -20
```

预期：0 error TS。

- [ ] **Step 2.6：Commit**

```bash
git add src/lib/tauri.ts
git commit -m "feat(tauri): extend tool/task event payloads and add getTasks"
```

---

## Task 3：Store 字段扩展 + upsertMessage

**Files:**
- Modify: `src/stores/streamingStore.ts`
- Modify: `src/stores/sessionStore.ts`

- [ ] **Step 3.1：streamingStore — ToolExecution 加 input**

找到：
```ts
export interface ToolExecution {
  toolName: string
  toolId: string
  status: 'executing' | 'completed' | 'error'
  summary?: string
  startedAt?: number
  durationMs?: number
}
```

在 `durationMs` 之后加：
```ts
  input?: unknown  // 来自 tool:executing 事件
```

- [ ] **Step 3.2：streamingStore — ConversationTaskState 补全字段**

找到：
```ts
export interface ConversationTaskState {
  taskId: string
  status: string
  runId: string
}
```

改为：
```ts
export interface ConversationTaskState {
  taskId: string
  status: string
  runId: string
  subject: string
  description?: string
  activeForm?: string
  owner?: string
  blockedBy?: string[]
  createdAt?: string
}
```

- [ ] **Step 3.3：sessionStore — 加 upsertMessage action**

在 `SessionState` interface 中，`updateMessage` 之后加：
```ts
  upsertMessage: (message: Message) => void
```

在 `createSessionSlice` 的 `updateMessage` 实现之后加：

```ts
upsertMessage: (message) =>
  apply((state) => {
    const idx = state.messages.findIndex((m) => m.id === message.id)
    if (idx >= 0) {
      const updated = [...state.messages]
      updated[idx] = message
      return { messages: updated } as Partial<T>
    }
    return { messages: [...state.messages, message] } as Partial<T>
  }),
```

- [ ] **Step 3.4：类型检查**

```bash
pnpm tsc --noEmit 2>&1 | grep "error TS" | head -20
```

预期：0 error TS。

- [ ] **Step 3.5：Commit**

```bash
git add src/stores/streamingStore.ts src/stores/sessionStore.ts
git commit -m "feat(stores): extend ToolExecution/TaskState fields and add upsertMessage"
```

---

## Task 4：useStreaming 事件处理更新

**Files:**
- Modify: `src/hooks/useStreaming.ts`

- [ ] **Step 4.1：更新 tool:executing 处理加 input**

找到（约第 256 行）：
```ts
onToolExecuting(({ conversationId, toolName, toolId, purpose }: ToolExecutingPayload) => {
  console.log('[tool:executing]', conversationId, toolName, toolId, purpose)
  touchActivity(conversationId)
  useChatStore.getState().addConversationToolExecution(conversationId, {
    toolName,
    toolId,
    status: 'executing',
    summary: purpose,
  })
}),
```

改为：
```ts
onToolExecuting(({ conversationId, toolName, toolId, purpose, input }: ToolExecutingPayload) => {
  console.log('[tool:executing]', conversationId, toolName, toolId)
  touchActivity(conversationId)
  useChatStore.getState().addConversationToolExecution(conversationId, {
    toolName,
    toolId,
    status: 'executing',
    summary: purpose,
    input,
  })
}),
```

- [ ] **Step 4.2：更新 tool:completed 处理改为 upsert Message**

找到（约第 270 行）：
```ts
onToolCompleted(({ conversationId, toolId, success, summary }: ToolCompletedPayload) => {
  console.log('[tool:completed]', conversationId, toolId, success, summary)
  touchActivity(conversationId)
  useChatStore.getState().updateConversationToolExecution(conversationId, toolId, {
    status: success ? 'completed' : 'error',
    summary,
  })
}),
```

改为：
```ts
onToolCompleted((message: Message) => {
  console.log('[tool:completed]', message.conversationId, message.toolResult?.name)
  touchActivity(message.conversationId)
  useChatStore.getState().upsertMessage(message)
  if (message.toolResult) {
    useChatStore.getState().updateConversationToolExecution(
      message.conversationId,
      message.toolResult.toolCallId,
      {
        status: message.toolResult.isError ? 'error' : 'completed',
        durationMs: message.toolResult.durationMs,
      },
    )
  }
}),
```

在文件顶部确认 `Message` 已从 `@/types/message` 导入（若没有则加入）。同时将 `ToolCompletedPayload` 从 import 里移除（已改为 Message 不再需要）。

- [ ] **Step 4.3：更新 task:status-changed 处理补全字段**

找到（约第 346 行）：
```ts
onTaskStatusChanged(({ conversationId, taskId, status, runId }: TaskStatusChangedPayload) => {
  console.log('[task:status-changed]', conversationId, taskId, status, runId)
  useChatStore.getState().upsertConversationTaskState(conversationId, {
    taskId,
    status,
    runId,
  })
}),
```

改为：
```ts
onTaskStatusChanged((payload: TaskStatusChangedPayload) => {
  console.log('[task:status-changed]', payload.conversationId, payload.taskId, payload.status)
  useChatStore.getState().upsertConversationTaskState(payload.conversationId, {
    taskId: payload.taskId,
    status: payload.status,
    runId: payload.runId,
    subject: payload.subject,
    description: payload.description,
    activeForm: payload.activeForm,
    owner: payload.owner,
    blockedBy: payload.blockedBy,
    createdAt: payload.createdAt,
  })
}),
```

- [ ] **Step 4.4：类型检查**

```bash
pnpm tsc --noEmit 2>&1 | grep "error TS" | head -20
```

预期：0 error TS。

- [ ] **Step 4.5：Commit**

```bash
git add src/hooks/useStreaming.ts
git commit -m "feat(streaming): update tool/task event handlers for new payload shapes"
```

---

## Task 5：useTurnRenderModel 扩展支持 tool role 分组

**Files:**
- Modify: `src/hooks/useTurnRenderModel.ts`

### 背景

现有 `buildTurnsFromMessages` 只处理 `user` 和 `assistant` role，忽略 `tool` role。新逻辑：
- `role: 'assistant'` 且有 `toolCalls` → 创建/更新 toolGroup
- `role: 'tool'` → 追加到当前 turn 的 toolGroup steps
- 实时的 `toolExecutions`（store 里）和历史的 `tool` messages 合并到同一个 toolGroup

- [ ] **Step 5.1：扩展 `buildTurnsFromMessages`**

将整个函数替换为：

```ts
export function buildTurnsFromMessages(
  messages: Message[],
  toolExecutions: ToolExecution[],
): RenderTurn[] {
  const turns: RenderTurn[] = []
  let current: RenderTurn | null = null

  for (const m of messages) {
    if (m.role === 'user') {
      current = {
        userMessage: { id: m.id, text: m.content.text ?? '' },
        aiSegments: [],
        toolGroup: undefined,
        generatedFiles: [],
        suggestions: [],
      }
      turns.push(current)
      continue
    }

    if (!current) {
      current = {
        userMessage: undefined,
        aiSegments: [],
        toolGroup: undefined,
        generatedFiles: [],
        suggestions: [],
      }
      turns.push(current)
    }

    if (m.role === 'assistant') {
      // assistant 有 toolCalls → 这是一次工具调用轮次，初始化 toolGroup
      if (m.toolCalls && m.toolCalls.length > 0) {
        if (!current.toolGroup) {
          current.toolGroup = { status: 'running', steps: [], durationMs: 0 }
        }
        // toolCalls 里的步骤作为 pending 占位，等 tool 消息填充
      }
      // 有文字内容才加 aiSegment
      if (m.content.text) {
        current.aiSegments.push({ id: m.id, text: m.content.text, message: m })
      }
      if (m.content.generatedFiles?.length) {
        for (const f of m.content.generatedFiles) {
          current.generatedFiles.push(normalizeGeneratedFile(f))
        }
      }
      continue
    }

    if (m.role === 'tool' && m.toolResult) {
      // 确保 toolGroup 存在
      if (!current.toolGroup) {
        current.toolGroup = { status: 'done', steps: [], durationMs: 0 }
      }
      const result = m.toolResult
      current.toolGroup.steps.push({
        index: current.toolGroup.steps.length + 1,
        name: result.name,
        status: result.isError ? 'error' : 'done',
        durationMs: result.durationMs,
      })
      current.toolGroup.durationMs =
        current.toolGroup.steps.reduce((acc, s) => acc + (s.durationMs ?? 0), 0)
    }
  }

  // 最后一个 turn：用实时 toolExecutions 覆盖/补充 toolGroup（turn 正在进行时）
  if (toolExecutions.length > 0 && turns.length > 0) {
    const target = turns[turns.length - 1]
    const steps: RenderToolStep[] = toolExecutions.map((t, i) => ({
      index: i + 1,
      name: t.toolName,
      status: toolExecStatusToStep(t.status),
      durationMs: t.durationMs,
    }))
    const running = steps.some((s) => s.status === 'running')
    target.toolGroup = {
      status: running ? 'running' : 'done',
      steps,
      durationMs: steps.reduce((acc, s) => acc + (s.durationMs ?? 0), 0),
    }
  }

  // 整理所有 toolGroup 最终状态
  for (const turn of turns) {
    if (turn.toolGroup && turn.toolGroup.steps.length > 0) {
      const hasRunning = turn.toolGroup.steps.some((s) => s.status === 'running')
      turn.toolGroup.status = hasRunning ? 'running' : 'done'
    }
  }

  return turns
}
```

在文件顶部的 import 中确认 `Message` 已从 `@/types/message` 导入（已有）。

- [ ] **Step 5.2：更新 `useTurnRenderModel` hook，传入 messages**

`useTurnRenderModel` 已经传 `messages`，无需改动，直接验证。

- [ ] **Step 5.3：更新现有单元测试（如有）**

```bash
grep -r "buildTurnsFromMessages" /Users/oayzz/project/lotus/lotus-workbench/lotus-app/src --include="*.test.*" -l
```

若有测试文件，检查是否需要补充 `role: 'tool'` 的测试用例。

- [ ] **Step 5.4：类型检查**

```bash
pnpm tsc --noEmit 2>&1 | grep "error TS" | head -20
```

预期：0 error TS。

- [ ] **Step 5.5：Commit**

```bash
git add src/hooks/useTurnRenderModel.ts
git commit -m "feat(turn-model): support tool role grouping in buildTurnsFromMessages"
```

---

## Task 6：useChat — switchConversation 恢复 task 列表

**Files:**
- Modify: `src/hooks/useChat.ts`

- [ ] **Step 6.1：在 switchConversation 里调用 getTasks**

找到（约第 156 行）`switchConversation` 函数，在 `getMessages(id)` 调用旁边，并行发起 `getTasks`：

找到：
```ts
const msgs = await getMessages(id)
console.log('[useChat] getMessages OK, count:', msgs.length)
useChatStore.getState().setMessages(msgs)
```

改为：
```ts
const [msgs, tasks] = await Promise.all([
  getMessages(id),
  getTasks(id).catch(() => []),
])
console.log('[useChat] getMessages OK, count:', msgs.length)
console.log('[useChat] getTasks OK, count:', tasks.length)
useChatStore.getState().setMessages(msgs)
// 恢复 task 列表到 store
const store = useChatStore.getState()
for (const task of tasks) {
  store.upsertConversationTaskState(id, task)
}
```

在文件顶部 import 里加 `getTasks`：
```ts
import { getMessages, getTasks, ... } from '@/lib/tauri'
```

- [ ] **Step 6.2：类型检查**

```bash
pnpm tsc --noEmit 2>&1 | grep "error TS" | head -20
```

预期：0 error TS。

- [ ] **Step 6.3：Commit**

```bash
git add src/hooks/useChat.ts
git commit -m "feat(chat): restore task list on conversation switch via getTasks"
```

---

## Task 7：RightPanel 组件

**Files:**
- Create: `src/components/chat/RightPanel.tsx`

设计稿参考：
- 整体 accordion 风格：section header 左文字右 chevron，`border-b border-border`
- 标题区：`px-4 py-4`，`text-[15px] font-semibold`
- Section header：`px-4 py-3`，`text-[13px] font-semibold`
- 空状态：`text-[12px] text-muted-foreground`，`px-4 pb-3`
- TaskItem 状态图标：`h-3 w-3`，running=spinner+primary，completed=green(#16A34A)，failed=destructive，cancelled/pending=muted
- ArtifactItem：复用 GeneratedFileCard 的文件类型图标颜色逻辑

- [ ] **Step 7.1：创建 RightPanel.tsx**

```tsx
/**
 * @designSource design.pen 用户截图 — 任务监控 accordion 面板
 * 三个 section：待办 / 产物 / 技能与 MCP
 */
import { useMemo, useState, useEffect } from 'react'
import {
  ChevronDown,
  ChevronRight,
  CheckCircle2,
  XCircle,
  MinusCircle,
  Circle,
  Loader2,
  File,
  FileSpreadsheet,
  FileText,
  Image,
} from 'lucide-react'

import { cn } from '@/lib/utils'
import { useChatStore } from '@/stores/chatStore'
import type { ConversationTaskState } from '@/stores/streamingStore'
import type { GeneratedFile } from '@/types/message'

// ─── RightPanel root ──────────────────────────────────────────────────────────

interface RightPanelProps {
  conversationId: string
}

export function RightPanel({ conversationId }: RightPanelProps) {
  return (
    <div className="flex w-[260px] shrink-0 flex-col overflow-y-auto border-l border-border bg-background">
      <div className="px-4 py-4">
        <h2 className="text-[15px] font-semibold text-foreground">任务监控</h2>
      </div>
      <TaskSection conversationId={conversationId} />
      <ArtifactSection conversationId={conversationId} />
      <SkillMcpSection />
    </div>
  )
}

// ─── TaskSection ──────────────────────────────────────────────────────────────

function TaskSection({ conversationId }: { conversationId: string }) {
  const tasks = useChatStore((s) => s.taskStates[conversationId] ?? [])
  const hasRunning = tasks.some((t) => t.status === 'running')
  const [open, setOpen] = useState(true)

  useEffect(() => {
    if (hasRunning) setOpen(true)
  }, [hasRunning])

  return (
    <div className="border-b border-border">
      <button
        type="button"
        onClick={() => !hasRunning && setOpen((v) => !v)}
        className="flex w-full items-center justify-between px-4 py-3 text-left"
      >
        <span className="text-[13px] font-semibold text-foreground">待办</span>
        <ChevronDown
          className={cn(
            'h-4 w-4 text-muted-foreground transition-transform duration-150',
            !open && '-rotate-90',
          )}
        />
      </button>
      {open && (
        <div className="px-4 pb-3">
          {tasks.length === 0 ? (
            <p className="text-[12px] text-muted-foreground">暂无待办</p>
          ) : (
            <div className="flex flex-col gap-0.5">
              {tasks.map((task) => (
                <TaskItem key={task.taskId} task={task} />
              ))}
            </div>
          )}
        </div>
      )}
    </div>
  )
}

function TaskItem({ task }: { task: ConversationTaskState }) {
  return (
    <div className="flex items-start gap-2 py-1.5">
      <TaskStatusIcon status={task.status} />
      <div className="flex min-w-0 flex-col gap-0.5">
        <span
          className={cn(
            'text-[12px] font-medium leading-tight',
            task.status === 'completed'
              ? 'text-muted-foreground line-through'
              : 'text-foreground',
          )}
        >
          {task.subject}
        </span>
        {task.status === 'running' && task.activeForm && (
          <span className="text-[11px] text-primary">{task.activeForm}</span>
        )}
      </div>
    </div>
  )
}

function TaskStatusIcon({ status }: { status: string }) {
  switch (status) {
    case 'running':
      return <Loader2 className="mt-0.5 h-3 w-3 shrink-0 animate-spin text-primary" />
    case 'completed':
      return (
        <CheckCircle2 className="mt-0.5 h-3 w-3 shrink-0" style={{ color: '#16A34A' }} />
      )
    case 'failed':
      return <XCircle className="mt-0.5 h-3 w-3 shrink-0 text-destructive" />
    case 'cancelled':
      return <MinusCircle className="mt-0.5 h-3 w-3 shrink-0 text-muted-foreground" />
    default:
      return <Circle className="mt-0.5 h-3 w-3 shrink-0 text-muted-foreground/40" />
  }
}

// ─── ArtifactSection ──────────────────────────────────────────────────────────

function ArtifactSection({ conversationId: _conversationId }: { conversationId: string }) {
  const messages = useChatStore((s) => s.messages)
  const [open, setOpen] = useState(true)

  const files = useMemo(() => {
    const seen = new Set<string>()
    const result: GeneratedFile[] = []
    for (const msg of messages) {
      for (const f of msg.content.generatedFiles ?? []) {
        if (!seen.has(f.id) && f.isLatest) {
          seen.add(f.id)
          result.push(f)
        }
      }
    }
    return result
  }, [messages])

  return (
    <div className="border-b border-border">
      <button
        type="button"
        onClick={() => setOpen((v) => !v)}
        className="flex w-full items-center justify-between px-4 py-3 text-left"
      >
        <span className="text-[13px] font-semibold text-foreground">产物</span>
        <ChevronDown
          className={cn(
            'h-4 w-4 text-muted-foreground transition-transform duration-150',
            !open && '-rotate-90',
          )}
        />
      </button>
      {open && (
        <div className="px-4 pb-3">
          {files.length === 0 ? (
            <p className="text-[12px] text-muted-foreground">暂无产物</p>
          ) : (
            <div className="flex flex-col gap-1">
              {files.map((f) => (
                <ArtifactItem key={f.id} file={f} />
              ))}
            </div>
          )}
        </div>
      )}
    </div>
  )
}

function ArtifactItem({ file }: { file: GeneratedFile }) {
  return (
    <div className="flex items-center gap-2 py-1">
      <ArtifactFileIcon fileType={file.fileType} />
      <span className="min-w-0 flex-1 truncate text-[12px] text-foreground">
        {file.fileName}
      </span>
    </div>
  )
}

function ArtifactFileIcon({ fileType }: { fileType: string }) {
  switch (fileType) {
    case 'excel':
    case 'csv':
      return <FileSpreadsheet className="h-3.5 w-3.5 shrink-0 text-green-600" />
    case 'html':
    case 'pdf':
      return <FileText className="h-3.5 w-3.5 shrink-0 text-blue-500" />
    case 'png':
      return <Image className="h-3.5 w-3.5 shrink-0 text-purple-500" />
    default:
      return <File className="h-3.5 w-3.5 shrink-0 text-muted-foreground" />
  }
}

// ─── SkillMcpSection ──────────────────────────────────────────────────────────

function SkillMcpSection() {
  return (
    <div className="border-b border-border">
      <button
        type="button"
        className="flex w-full items-center justify-between px-4 py-3 text-left"
        onClick={() => {
          // TODO: 跳转到技能与 MCP 配置页（占位）
        }}
      >
        <span className="text-[13px] font-semibold text-foreground">技能与 MCP</span>
        <ChevronRight className="h-4 w-4 text-muted-foreground" />
      </button>
    </div>
  )
}
```

- [ ] **Step 7.2：类型检查**

```bash
pnpm tsc --noEmit 2>&1 | grep "error TS" | head -20
```

预期：0 error TS。

- [ ] **Step 7.3：Commit**

```bash
git add src/components/chat/RightPanel.tsx
git commit -m "feat(ui): add RightPanel with task/artifact/skill accordion sections"
```

---

## Task 8：ChatPage 挂载 RightPanel

**Files:**
- Modify: `src/features/chat/ChatPage.tsx`

- [ ] **Step 8.1：在 ChatPage 里导入并挂载 RightPanel**

找到：
```tsx
import { BrowserPanel } from '@/components/browser/BrowserPanel'
```

在其后加：
```tsx
import { RightPanel } from '@/components/chat/RightPanel'
```

找到：
```tsx
<div className="relative flex flex-1 overflow-hidden">
  <div className="flex flex-1 flex-col overflow-hidden">
    <ChatArea />
    <ChatBottomArea />
  </div>
  <BrowserPanel />
</div>
```

改为：
```tsx
<div className="relative flex flex-1 overflow-hidden">
  <div className="flex flex-1 flex-col overflow-hidden">
    <ChatArea />
    <ChatBottomArea />
  </div>
  <RightPanel conversationId={conversationId} />
  <BrowserPanel />
</div>
```

- [ ] **Step 8.2：类型检查 + 前端构建验证**

```bash
pnpm tsc --noEmit 2>&1 | grep "error TS" | head -20
```

预期：0 error TS。

- [ ] **Step 8.3：启动开发服务器，目视验证 RightPanel**

```bash
pnpm dev
```

在浏览器中打开对话页，确认：
- RightPanel 常驻在消息区右侧，260px 宽，有左侧 border
- 顶部显示"任务监控"标题
- 三个 accordion section：待办（默认展开，显示"暂无待办"）/ 产物（显示"暂无产物"）/ 技能与 MCP（`>` 箭头）
- chevron 点击可折叠/展开（技能与 MCP 无折叠，只有 `>` 箭头）

- [ ] **Step 8.4：Commit**

```bash
git add src/features/chat/ChatPage.tsx
git commit -m "feat(chat): mount RightPanel in ChatPage alongside BrowserPanel"
```

---

## 自检

- [x] Task 1 覆盖 spec §一（MessageRole、AssistantToolCall、ToolResultContent、Message 扩展）
- [x] Task 2 覆盖 spec §二（ToolExecutingPayload、tool:completed→Message、TaskStatusChangedPayload、getTasks）
- [x] Task 3 覆盖 spec §三（ToolExecution.input、ConversationTaskState 补全、upsertMessage）
- [x] Task 4 覆盖 spec §七（useStreaming 三个事件处理更新）
- [x] Task 5 覆盖 spec §四（useTurnRenderModel 支持 tool role 分组）
- [x] Task 6 覆盖 spec §五（getTasks 在 switchConversation 时调用）
- [x] Task 7 + 8 覆盖 spec §六（RightPanel 三个 section + ChatPage 挂载）
- [x] 老数据兼容：`buildTurnsFromMessages` 遇到无 `toolCalls` 的 assistant → 走原有 aiSegments 路径，不渲染 toolGroup
- [x] `ToolCompletedPayload` 加 `@deprecated` 注释但不删除，避免可能存在的旧引用编译报错
- [x] `Image` 是 lucide-react 的 `Image` 图标，不是 next/image，无冲突
