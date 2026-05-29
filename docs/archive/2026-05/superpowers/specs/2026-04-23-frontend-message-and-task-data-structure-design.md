# 前端消息与任务数据结构设计

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task.

**Goal:** 用 role-based 消息模型贯通工具调用的实时展示与历史回显；扩展 task 数据结构支持辅助栏持久展示；所有数据格式对齐后端 OpenAI camelCase 存储格式。

**Architecture:** 消息层新增 `role: 'tool'`，工具调用历史从 `get_messages` 接口恢复；实时阶段通过扩展事件 payload 驱动；task 与 session 绑定，走独立存储通道，不混入消息流；辅助栏（RightPanel）常驻展示，包含三个 accordion section：待办（task）、产物（generatedFiles）、技能与 MCP（占位导航入口）。

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

在 `ChatPage` 右侧新增 `RightPanel`，和 `BrowserPanel` 平级。**常驻显示**，固定宽度 260px：

```tsx
// ChatPage 结构
<div className="relative flex flex-1 overflow-hidden">
  <div className="flex flex-1 flex-col overflow-hidden">
    <ChatArea />
    <ChatBottomArea />
  </div>
  <RightPanel conversationId={conversationId} />   // 新增，常驻
  <BrowserPanel />
</div>
```

### 6.2 RightPanel 整体结构

文件：`src/components/chat/RightPanel.tsx`

- 固定宽度 260px，`border-l border-border bg-background`
- 顶部标题：`任务监控`
- 三个 accordion section 从上到下：**待办 → 产物 → 技能与 MCP**
- 每个 section 有折叠/展开状态，默认展开

```tsx
export function RightPanel({ conversationId }: { conversationId: string }) {
  return (
    <div className="flex w-[260px] shrink-0 flex-col border-l border-border bg-background overflow-y-auto">
      {/* 标题 */}
      <div className="px-4 py-4">
        <h2 className="text-[15px] font-semibold text-foreground">任务监控</h2>
      </div>
      {/* Sections */}
      <TaskSection conversationId={conversationId} />
      <ArtifactSection conversationId={conversationId} />
      <SkillMcpSection />
    </div>
  )
}
```

### 6.3 待办 Section（TaskSection）

文件：`src/components/chat/RightPanel.tsx`（同文件）

- 默认展开（`∨` chevron）
- 有运行中 task 时自动展开（不可折叠）
- 空状态：`暂无待办`（muted 文字）
- 有 task：每条显示 TaskItem

```tsx
function TaskSection({ conversationId }: { conversationId: string }) {
  const tasks = useChatStore((s) => s.taskStates[conversationId] ?? [])
  const hasRunning = tasks.some(t => t.status === 'running')
  const [open, setOpen] = useState(true)

  // 有运行中任务时强制展开
  useEffect(() => {
    if (hasRunning) setOpen(true)
  }, [hasRunning])

  return (
    <div className="border-b border-border">
      <button
        onClick={() => !hasRunning && setOpen(v => !v)}
        className="flex w-full items-center justify-between px-4 py-3"
      >
        <span className="text-[13px] font-semibold text-foreground">待办</span>
        <ChevronDown className={cn("h-4 w-4 text-muted-foreground transition-transform", open ? "" : "-rotate-90")} />
      </button>
      {open && (
        <div className="px-4 pb-3">
          {tasks.length === 0 ? (
            <p className="text-[12px] text-muted-foreground">暂无待办</p>
          ) : (
            <div className="flex flex-col gap-1">
              {tasks.map(task => <TaskItem key={task.taskId} task={task} />)}
            </div>
          )}
        </div>
      )}
    </div>
  )
}
```

### 6.4 TaskItem

```tsx
function TaskItem({ task }: { task: ConversationTaskState }) {
  return (
    <div className="flex items-start gap-2 py-1.5">
      <TaskStatusIcon status={task.status} />
      <div className="flex flex-col gap-0.5 min-w-0">
        <span className={cn(
          "text-[12px] font-medium leading-tight",
          task.status === 'completed'
            ? "text-muted-foreground line-through"
            : "text-foreground"
        )}>
          {task.subject}
        </span>
        {task.status === 'running' && task.activeForm && (
          <span className="text-[11px] text-primary">{task.activeForm}</span>
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
      return <Loader2 className="mt-0.5 h-3 w-3 shrink-0 animate-spin text-primary" />
    case 'completed':
      return <CheckCircle2 className="mt-0.5 h-3 w-3 shrink-0" style={{ color: '#16A34A' }} />
    case 'failed':
      return <XCircle className="mt-0.5 h-3 w-3 shrink-0 text-destructive" />
    case 'cancelled':
      return <MinusCircle className="mt-0.5 h-3 w-3 shrink-0 text-muted-foreground" />
    default: // pending
      return <Circle className="mt-0.5 h-3 w-3 shrink-0 text-muted-foreground/40" />
  }
}
```

### 6.6 产物 Section（ArtifactSection）

文件：`src/components/chat/RightPanel.tsx`（同文件）

数据来源：从当前对话的 `messages` 中提取所有 `content.generatedFiles`，去重后展示。

- 默认展开
- 空状态：`暂无产物`（muted 文字）
- 有产物：每个文件一行，显示文件名 + 类型图标 + 打开按钮

```tsx
function ArtifactSection({ conversationId }: { conversationId: string }) {
  const messages = useChatStore((s) => s.messages)
  const [open, setOpen] = useState(true)

  // 从所有消息的 generatedFiles 汇总，按文件 id 去重，只取 isLatest
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
        onClick={() => setOpen(v => !v)}
        className="flex w-full items-center justify-between px-4 py-3"
      >
        <span className="text-[13px] font-semibold text-foreground">产物</span>
        <ChevronDown className={cn("h-4 w-4 text-muted-foreground transition-transform", open ? "" : "-rotate-90")} />
      </button>
      {open && (
        <div className="px-4 pb-3">
          {files.length === 0 ? (
            <p className="text-[12px] text-muted-foreground">暂无产物</p>
          ) : (
            <div className="flex flex-col gap-1">
              {files.map(f => <ArtifactItem key={f.id} file={f} />)}
            </div>
          )}
        </div>
      )}
    </div>
  )
}
```

### 6.7 ArtifactItem

```tsx
function ArtifactItem({ file }: { file: GeneratedFile }) {
  return (
    <div className="flex items-center gap-2 py-1">
      <FileIcon fileType={file.fileType} />
      <span className="min-w-0 flex-1 truncate text-[12px] text-foreground">
        {file.fileName}
      </span>
    </div>
  )
}

// FileIcon：根据 fileType 返回对应图标（FileSpreadsheet/FileText/Image 等）
function FileIcon({ fileType }: { fileType: string }) {
  switch (fileType) {
    case 'excel': case 'csv':
      return <FileSpreadsheet className="h-3.5 w-3.5 shrink-0 text-green-600" />
    case 'html': case 'pdf':
      return <FileText className="h-3.5 w-3.5 shrink-0 text-blue-500" />
    case 'png':
      return <Image className="h-3.5 w-3.5 shrink-0 text-purple-500" />
    default:
      return <File className="h-3.5 w-3.5 shrink-0 text-muted-foreground" />
  }
}
```

### 6.8 技能与 MCP Section（SkillMcpSection）

占位导航入口，`>` 箭头，点击暂不跳转（`onClick` 为空或 `console.log`）：

```tsx
function SkillMcpSection() {
  return (
    <div className="border-b border-border">
      <button className="flex w-full items-center justify-between px-4 py-3">
        <span className="text-[13px] font-semibold text-foreground">技能与 MCP</span>
        <ChevronRight className="h-4 w-4 text-muted-foreground" />
      </button>
    </div>
  )
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
- useTurnRenderModel 分组逻辑扩展（支持 tool role）
- RightPanel 常驻辅助栏：待办（TaskSection）、产物（ArtifactSection）、技能与 MCP 占位
- useStreaming 事件处理更新（tool:executing 加 input、tool:completed 改为 upsert Message）
- chatStore upsertMessage

**不包含（后续专项）：**
- ToolGroupCard 渲染 tool 消息步骤详情（入参/出参展开）——需后端 tool:executing 先扩展入参
- 后端 `get_tasks` 接口实现——需 Rust 侧暴露接口，前端 switchConversation 时调用恢复 task 列表
- 后端 `task:status-changed` payload 补全字段（subject/activeForm 等）——需 Rust 侧修改事件发射
- 后端 `tool:completed` 改为推完整 Message——需 Rust 侧修改 tauri_event_adapter.rs
- 后端 `tool:executing` 加 input 字段——需 Rust 侧在 ToolCallExecuting 事件加 args
