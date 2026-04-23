# 前端消息与任务数据结构设计

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task.

**Goal:** 用 role-based 消息模型贯通工具调用的实时展示与历史回显；扩展 task 数据结构支持辅助栏持久展示；所有数据格式对齐后端 OpenAI camelCase 存储格式。

**Architecture:** 消息层新增 `role: 'tool'`，工具调用历史从 `get_messages` 接口恢复；实时阶段通过扩展事件 payload 驱动；task 与 session 绑定，走独立存储通道，不混入消息流；辅助栏（RightPanel）作为独立面板在 ChatPage 右侧展示 task 列表。

**Tech Stack:** TypeScript, React, Zustand, Tauri IPC, `src/types/message.ts`, `src/stores/streamingStore.ts`, `src/lib/tauri.ts`

---

## 背景与约束

- 后端已完成聊天记录存储修复（`toolCalls` 存入 assistant 消息，`tool` 消息独立写磁盘）
- 老数据（无 `toolCalls` 的 assistant 消息）兼容：无 `toolCalls` → 正常渲染 AiBubble，不展示 ToolGroup
- 实时阶段（turn 进行中）和历史回显（切换对话）共用同一套类型，不做区分
- task 不是消息，走独立存储，不新增 `MessageRole`

---

## 一、消息类型变更（`src/types/message.ts`）

### 1.1 MessageRole 新增 `'tool'`

```ts
export type MessageRole = 'user' | 'assistant' | 'system' | 'tool'
```

### 1.2 新增 AssistantToolCall

```ts
export interface AssistantToolCall {
  id: string          // tool_call_id，与 tool 消息的 toolCallId 对应
  name: string        // 工具名，如 "browse_navigate"
  arguments: unknown  // 完整入参 JSON 对象
}
```

### 1.3 新增 ToolResultContent

```ts
export interface ToolResultContent {
  toolCallId: string       // 与 AssistantToolCall.id 对应
  name: string             // 工具名
  content: string          // 完整工具输出文本
  isError: boolean         // 是否执行失败
  durationMs?: number      // 执行耗时（ms），实时事件携带
}
```

### 1.4 Message 扩展

```ts
export interface Message {
  id: string
  conversationId: string
  role: MessageRole
  createdAt: string
  content: MessageContent
  sender?: MessageSender
  // 新增：assistant 消息专用，来自磁盘 toolCalls 字段
  toolCalls?: AssistantToolCall[]
  // 新增：tool 消息专用
  toolResult?: ToolResultContent
}
```

**兼容规则：**
- `role: 'assistant'` 无 `toolCalls` → 普通 AiBubble，不渲染 ToolGroup
- `role: 'assistant'` 有 `toolCalls` → 触发 turn 分组渲染 ToolGroup
- `role: 'tool'` → 渲染为 ToolGroup 内的步骤行（由 useTurnRenderModel 分组）

---

## 二、事件 Payload 扩展（`src/lib/tauri.ts`）

### 2.1 ToolExecutingPayload 加 input

```ts
export interface ToolExecutingPayload {
  conversationId: string
  toolName: string
  toolId: string        // tool_call_id
  purpose?: string
  input?: unknown       // 新增：完整入参 JSON 对象
}
```

### 2.2 tool:completed payload 改为完整 Message

```ts
// 旧
export interface ToolCompletedPayload {
  conversationId: string
  toolName: string
  toolId: string
  success: boolean
  summary?: string
}

// 新：直接是完整 Message（role: 'tool'）
// tool:completed payload 类型改为 Message
// 即 listen<Message>('tool:completed', ...)
```

### 2.3 TaskStatusChangedPayload 补全字段

```ts
export interface TaskStatusChangedPayload {
  conversationId: string
  taskId: string
  status: string
  runId: string
  // 新增
  subject: string
  description?: string
  activeForm?: string   // spinner 显示文字（如"探索项目上下文"）
  owner?: string        // agent id
  blockedBy?: string[]
  createdAt?: string
}
```

---

## 三、Store 变更（`src/stores/streamingStore.ts`）

### 3.1 ToolExecution 加 input 字段

```ts
export interface ToolExecution {
  toolName: string
  toolId: string
  status: 'executing' | 'completed' | 'error'
  summary?: string
  startedAt?: number
  durationMs?: number
  input?: unknown       // 新增：来自 tool:executing 事件
}
```

### 3.2 ConversationTaskState 补全字段

```ts
export interface ConversationTaskState {
  taskId: string
  status: string
  runId: string
  // 新增
  subject: string
  description?: string
  activeForm?: string
  owner?: string
  blockedBy?: string[]
  createdAt?: string
}
```

---

## 四、渲染层分组（`src/hooks/useTurnRenderModel.ts`）

### 4.1 Turn 模型扩展

把 `get_messages` 返回的消息列表分组为 Turn 数组：

```ts
interface TurnItem {
  userMessage?: Message                 // role: user
  assistantToolCalls?: Message          // role: assistant，有 toolCalls
  toolResults?: Message[]               // role: tool，按 toolCallId 与 assistantToolCalls 对应
  assistantReply?: Message              // role: assistant，无 toolCalls（最终回复）
}
```

分组规则：
1. 遇到 `role: user` → 新建一个 Turn
2. 遇到 `role: assistant` 且有 `toolCalls` → 归入当前 Turn 的 `assistantToolCalls`
3. 遇到 `role: tool` → 归入当前 Turn 的 `toolResults`
4. 遇到 `role: assistant` 且无 `toolCalls` → 归入当前 Turn 的 `assistantReply`

渲染输出：
- `assistantToolCalls` + `toolResults` → `ToolGroupCard`
- `assistantReply` → `AiBubble`
- 无 `toolCalls` 的 assistant → 直接 `AiBubble`

---

## 五、后端接口补充

### 5.1 get_tasks 接口

```ts
// src/lib/tauri.ts 新增
export function getTasks(conversationId: string): Promise<ConversationTaskState[]> {
  return invoke<ConversationTaskState[]>('get_tasks', { conversationId })
}
```

切换对话时调用此接口恢复 task 列表（类似 `getMessages` 的调用时机）。

---

## 六、辅助栏（RightPanel）

### 6.1 布局

在 `ChatPage` 右侧新增 `RightPanel`，和 `BrowserPanel` 平级：

```tsx
// ChatPage 结构
<div className="relative flex flex-1 overflow-hidden">
  <div className="flex flex-1 flex-col overflow-hidden">
    <ChatArea />
    <ChatBottomArea />
  </div>
  <RightPanel conversationId={conversationId} />   // 新增
  <BrowserPanel />
</div>
```

### 6.2 RightPanel 组件

文件：`src/components/chat/RightPanel.tsx`

- 固定宽度 280px，右侧 border-l
- 无 task 时不渲染（`return null`）
- 有 task 时展示 TaskList

```tsx
export function RightPanel({ conversationId }: { conversationId: string }) {
  const tasks = useChatStore((s) => s.taskStates[conversationId] ?? [])
  if (tasks.length === 0) return null
  return (
    <div className="flex w-[280px] shrink-0 flex-col border-l border-border bg-background">
      <TaskList tasks={tasks} />
    </div>
  )
}
```

### 6.3 TaskList 组件

文件：`src/components/chat/TaskList.tsx`（替换现有 `TaskStatusList.tsx`）

参考 Claude Code cowork 风格：
- 顶部 header：`任务列表` + 运行中数量徽章
- 每条 task 一行：状态图标 + subject + activeForm（运行中时显示）
- 状态颜色：pending=muted，running=primary+spinner，completed=green，failed=red，cancelled=muted

```tsx
interface TaskListProps {
  tasks: ConversationTaskState[]
}

export function TaskList({ tasks }: TaskListProps) {
  const running = tasks.filter(t => t.status === 'running')
  return (
    <div className="flex flex-col gap-0">
      {/* Header */}
      <div className="flex items-center justify-between px-4 py-3 border-b border-border">
        <span className="text-xs font-semibold text-foreground">任务</span>
        {running.length > 0 && (
          <span className="rounded-full bg-primary/10 px-2 py-0.5 text-[11px] font-medium text-primary">
            {running.length} 进行中
          </span>
        )}
      </div>
      {/* Task items */}
      <div className="flex flex-col overflow-y-auto">
        {tasks.map((task) => (
          <TaskItem key={task.taskId} task={task} />
        ))}
      </div>
    </div>
  )
}
```

### 6.4 TaskItem 组件

```tsx
function TaskItem({ task }: { task: ConversationTaskState }) {
  return (
    <div className="flex items-start gap-3 px-4 py-3 border-b border-border/50 last:border-0">
      <TaskStatusIcon status={task.status} />
      <div className="flex flex-col gap-0.5 min-w-0">
        <span className={cn(
          "text-[13px] font-medium truncate",
          task.status === 'completed' ? "text-muted-foreground line-through" : "text-foreground"
        )}>
          {task.subject}
        </span>
        {task.status === 'running' && task.activeForm && (
          <span className="text-[11px] text-primary">{task.activeForm}</span>
        )}
        {task.description && task.status !== 'completed' && (
          <span className="text-[11px] text-muted-foreground line-clamp-2">{task.description}</span>
        )}
      </div>
    </div>
  )
}
```

### 6.5 TaskStatusIcon

```tsx
function TaskStatusIcon({ status }: { status: string }) {
  switch (status) {
    case 'running':
      return <Loader2 className="h-3.5 w-3.5 shrink-0 animate-spin text-primary mt-0.5" />
    case 'completed':
      return <CheckCircle2 className="h-3.5 w-3.5 shrink-0 mt-0.5" style={{ color: '#16A34A' }} />
    case 'failed':
      return <XCircle className="h-3.5 w-3.5 shrink-0 text-destructive mt-0.5" />
    case 'pending':
      return <Circle className="h-3.5 w-3.5 shrink-0 text-muted-foreground/50 mt-0.5" />
    default:
      return <Circle className="h-3.5 w-3.5 shrink-0 text-muted-foreground/30 mt-0.5" />
  }
}
```

---

## 七、事件订阅更新（`src/hooks/useStreaming.ts`）

### 7.1 tool:executing 处理加 input

```ts
onToolExecuting(({ conversationId, toolId, toolName, input }) => {
  useChatStore.getState().addConversationToolExecution(conversationId, {
    toolId,
    toolName,
    status: 'executing',
    startedAt: Date.now(),
    input,  // 新增
  })
})
```

### 7.2 tool:completed 处理改为 upsert Message

```ts
// 旧：更新 ToolExecution store
// 新：将完整 Message upsert 进消息列表
onToolCompleted((message: Message) => {
  useChatStore.getState().upsertMessage(message)
  // 同时更新 ToolExecution store 状态
  useChatStore.getState().updateConversationToolExecution(
    message.conversationId,
    message.toolResult!.toolCallId,
    {
      status: message.toolResult!.isError ? 'error' : 'completed',
      durationMs: message.toolResult!.durationMs,
    }
  )
})
```

### 7.3 task:status-changed 处理补全字段

```ts
onTaskStatusChanged((payload: TaskStatusChangedPayload) => {
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
})
```

---

## 八、chatStore 新增 upsertMessage

`sessionStore.ts` 新增：

```ts
upsertMessage: (message: Message) =>
  set((state) => {
    const idx = state.messages.findIndex((m) => m.id === message.id)
    if (idx >= 0) {
      const updated = [...state.messages]
      updated[idx] = message
      return { messages: updated }
    }
    return { messages: [...state.messages, message] }
  }),
```

---

## 范围边界

**本 spec 包含：**
- 类型定义变更（message.ts、tauri.ts、streamingStore.ts）
- useTurnRenderModel 分组逻辑扩展
- RightPanel + TaskList + TaskItem 组件（task 展示）
- useStreaming 事件处理更新
- chatStore upsertMessage

**不包含（后续专项）：**
- ToolGroupCard 渲染 tool 消息步骤详情（入参/出参展开）
- 辅助栏展示"对话产物"（generatedFiles 等）
- 后端 `get_tasks` 接口实现
- 后端 `task:status-changed` payload 字段扩展
- `tool:completed` 后端改为推完整 Message
