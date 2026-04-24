# SSE 消息全链路审计与修复设计

> 审计日期：2026-04-24  
> 覆盖范围：前端发送 → Tauri IPC → Rust Runtime → LLM SSE → RuntimeEvent → Tauri emit → 前端订阅 → Store → Render Model → UI

---

## 一、完整链路时序（E2E）

```
前端 useChat.sendUserMessage()
  ├─ 生成乐观 user message（id = crypto.randomUUID()）
  ├─ store.addMessage(userMessage)
  ├─ store.setConversationStreaming(cId, true) + addBusyConversation()
  └─ invoke('send_message', {conversationId, content, fileIds, agentName})
       ↓
commands/chat.rs → TauriChatCommandAdapter::send_message()
  └─ SessionRuntime::run_chat_request()
       ├─ 生成权威 RunId（UUID v4）
       └─ RuntimeChatTurnDriver::run_chat_turn_s4()
            ├─ persist_user_message() → DB（id = "msg-<uuid>"，不回传前端）⚠
            ├─ [迭代循环]
            │    ├─ run_llm_step() → LLM OpenAI-compatible SSE
            │    │    ├─ ContentDelta → RuntimeEvent::StreamDelta → streaming:delta
            │    │    ├─ ToolCallStart（内部聚合，不发事件）
            │    │    └─ Done → LlmStepResult::ContentComplete / ToolCalls
            │    │
            │    └─ [若 ToolCalls]
            │         ├─ ToolRoundDriver::execute_round()（并发 spawn）
            │         │    ├─ emit ToolCallExecuting → tool:executing
            │         │    ├─ dispatcher.dispatch()
            │         │    └─ emit ToolCallCompleted（msg_id="tool-<uuid1>"）→ tool:completed ⚠
            │         ├─ persist_iteration_assistant_message() → DB（无事件）⚠
            │         └─ persist_tool_messages()（msg_id="tool-<uuid2>"，≠uuid1）⚠
            │
            └─ [完成后]
                 ├─ persist_assistant_message() → DB
                 ├─ emit MessagePersisted → message:updated
                 ├─ emit StreamDone → streaming:done
                 ├─ emit TurnCompleted → turn:completed
                 ├─ [若 stop hook] emit StopHookPreventedContinuation → stop:prevented-continuation ⚠ 前端无订阅
                 └─ emit AgentIdle(Primary) → agent:idle

前端事件处理（useStreaming.ts）
  ├─ streaming:delta → buffer/rAF flush → streamStates[convId].streamingContent → StreamingBubble
  ├─ tool:executing → addConversationToolExecution()
  ├─ tool:completed → upsertMessage() ⚠无 activeConversationId 守卫 + updateToolExecution()
  ├─ message:updated → 仅 active 会话 upsertMessage；role=assistant 清 streaming 状态
  ├─ streaming:done → clearConversationStreamState() + removeBusyConversation()
  ├─ turn:completed → toast + setLastTurnSummary()
  └─ agent:idle(primary) → clearConversationStreamState() + removeBusyConversation()

渲染路径
  MessageList → useTurnRenderModel(messages, toolExecutions) → RenderTurn[]
    ├─ role=user → UserMessageBubble
    ├─ role=assistant + toolCalls → ToolGroupCard（steps 来自 toolExecutions，inputJson/output 为空）⚠
    ├─ role=tool → ToolGroupCard step（output/inputJson 未映射）⚠
    └─ role=assistant text → AiBubble
  streaming 期间额外渲染 StreamingBubble（直连 streamingContent）

历史加载
  switchConversation() → getMessages(id) → setMessages(msgs)
  conversation_service::transform_message_json_for_frontend()
  → 同一 useTurnRenderModel 渲染（历史与实时路径汇合）
```

---

## 二、问题清单（按优先级）

### P0：数据一致性 — 消息 ID 不对齐

#### P0-1：user message 实时路径与历史路径 ID 永久不一致

- **现象**：用户发消息后切换会话再切回，同一条 user 消息显示两次。
- **根因**：
  - 前端：`useChat.ts:248` → `messageId = crypto.randomUUID()`
  - 后端：`persist_user_message()` → `msg_id = "msg-<new-uuid>"`（另一个 UUID，不回传）
  - `getMessages` 返回 DB id，前端 `addMessage` 追加，两条并存。
- **文件**：`src/hooks/useChat.ts:248`、`src-tauri/src/transport/tauri_commands/chat.rs:822`

#### P0-2：tool result message 实时 ID ≠ DB ID，重载必出重复

- **现象**：工具执行后切换会话再回来，同一条工具结果显示两次。
- **根因**：
  - 实时事件：`query_engine.rs:469,533` → `msg_id = "tool-<uuid1>"`，发 `ToolCallCompleted`
  - DB 写入：`chat.rs:893` → `msg_id = "tool-<uuid2>"`（另一个新 UUID）
  - 两次独立 UUID 生成，无法对齐。
- **文件**：`src-tauri/src/runtime/query_engine.rs:469,533`、`src-tauri/src/transport/tauri_commands/chat.rs:893`

### P1：前端状态管理 Bug

#### P1-1：`tool:completed` 无 activeConversationId 守卫

- **现象**：后台会话的工具完成消息串入当前活跃会话的 `messages[]`。
- **根因**：`useStreaming.ts:274` 直接调 `upsertMessage(message)`，未检查 `message.conversationId === store.activeConversationId`。
- **文件**：`src/hooks/useStreaming.ts:274`

#### P1-2：`switchConversation` 无竞态保护

- **现象**：快速切换 A→B→C，B 的 `getMessages` 晚返回，C 界面显示 B 的历史消息。
- **根因**：`useChat.ts:161-172` 无 AbortController/版本号，旧请求完成后直接 `setMessages`。
- **文件**：`src/hooks/useChat.ts:161-172`

#### P1-3：乐观 user message 失败后不回滚

- **现象**：IPC 失败或 `streaming:error` 后，孤立用户消息永久残留在界面，刷新才消失。
- **根因**：`useChat.ts:284-298` catch 块只清 streaming 状态，未 remove 乐观消息。
- **文件**：`src/hooks/useChat.ts:284-298`

#### P1-4：`UserBubble`/`AiBubble` 直接调 IPC，绕过所有守卫

- **现象**：编辑重发或点击建议时，可跳过 busy 检查，可能双重触发 agent loop。
- **根因**：
  - `UserBubble.tsx:34` 直接调 `sendMessage()` IPC
  - `AiBubble.tsx:72` 直接调 `sendMessage()` IPC
  - 两处均绕过 `useChat.sendUserMessage()` 的 busyConversations 检查、乐观消息、streaming 状态。
- **文件**：`src/components/chat/UserBubble.tsx:34`、`src/components/chat/AiBubble.tsx:72`

#### P1-5：渲染层 `inputJson`/`output` 始终为空

- **现象**：`ToolGroupCard` 展开后入参和输出区域为空白。
- **根因**：`useTurnRenderModel.ts` 构建 `RenderToolStep` 时从未填充这两个字段：
  - 历史 `role=tool`（127-132 行）：`toolResult.content` 未映射到 `output`
  - 历史 `role=assistant`（103-108 行）：`toolCalls[].arguments` 未映射到 `inputJson`
  - 实时 `toolExecutions`（144-149 行）：`ToolExecution.input` 未映射到 `inputJson`
  - `ToolExecution` 无 `output` 字段，实时路径工具输出无处存放。
- **文件**：`src/hooks/useTurnRenderModel.ts:103-149`、`src/stores/streamingStore.ts`

#### P1-6：`persist_iteration_assistant_message` 写 DB 无实时事件

- **现象**：实时期间看不到 assistant[toolCalls] 消息；切换会话后历史加载突然多出这些消息，UI 跳变。
- **根因**：`chat_turn_driver.rs:1094-1103` 写 DB 后无任何 `MessagePersisted` 事件发出。
- **文件**：`src-tauri/src/runtime/chat/chat_turn_driver.rs:1094-1103`

#### P1-7：`ToolCallCompleted` 事件 `duration_ms` 永为 None

- **现象**：`ToolGroupCard` 每步耗时始终显示为空。
- **根因**：`query_engine.rs:471,534` 两处 `duration_ms: None` 硬编码。
- **文件**：`src-tauri/src/runtime/query_engine.rs:471,534`

#### P1-8：切换回正在 streaming 的会话后消息过时

- **现象**：从 A 切到 B 再切回 A，A 在切换期间的消息需等 `agent:idle` 后手动刷新。
- **根因**：非活跃会话的 `message:updated` 被丢弃（设计决策），但切回时 `switchConversation` 只调一次 `getMessages`，如果 A 的 turn 尚未结束，拿到的是旧快照。
- **文件**：`src/hooks/useStreaming.ts:221-235`、`src/hooks/useChat.ts:158-181`

### P2：类型契约错误 / 遗留死代码

#### P2-1：`streaming:done` payload 声明了 `messageId`，后端从不发

- **文件**：`src/lib/tauri.ts:54-57`、`src-tauri/src/transport/tauri_event_adapter.rs:29-35`

#### P2-2：`streaming:error` 前端类型声明了后端不发的字段

- `errorType`、`partialContent`、`timeoutSeconds`、`iteration`、`maxIterations` 均为前端臆造字段，`errorType === 'chunk_timeout'` 分支永不命中。
- **文件**：`src/lib/tauri.ts`（`StreamingErrorPayload`）

#### P2-3：`stop:prevented-continuation` 前端无订阅

- stop hook 阻断继续时前端无感知，UI 等 200s watchdog 才清 loading。
- **文件**：`src-tauri/src/transport/tauri_event_adapter.rs:171-178`

#### P2-4：`task:status-changed` payload 前端声明了 3 个后端不发的字段

- `description`、`blockedBy`、`createdAt` 后端不发，前端 store 写入永为 `undefined`。
- **文件**：`src/lib/tauri.ts:121-132`、`src-tauri/src/transport/tauri_event_adapter.rs:153-170`

#### P2-5：前端空订阅的遗留事件（后端从不发）

`file:parsed`、`file:generated`、`notification`、`streaming:step-reset`、`auth:expired`、`browser:closed` — 均为遗留声明，对应处理代码是死代码。

#### P2-6：并发工具执行 `ToolCallCompleted` 顺序不确定

- `tool_round_driver` 并发 spawn，emit 顺序非确定性，前端工具步骤顺序可能与历史不一致。
- **文件**：`src-tauri/src/runtime/chat/tool_round_driver.rs:133-162`

#### P2-7：`persist_assistant_message` 空内容时跳过 DB 但仍 emit `MessagePersisted`

- 空 content turn 会向前端推一条空 assistant 消息，重载后消失，store 与持久层暂时不同步。
- **文件**：`src-tauri/src/transport/tauri_commands/chat.rs:970-979`

---

## 三、修复方案

### 方案 A（P0-1 + P0-2）：统一消息 ID

**核心原则：ID 由后端生成，通过事件回传前端，前端不自造 ID。**

**user message ID：**
1. 后端 `persist_user_message()` 写 DB 后，通过 bus emit `MessagePersisted{role:"user", message_id, content}` 事件。
2. 前端 `message:updated` handler 收到 `role=user` 消息时：
   - 找到乐观消息（匹配 `conversationId + content.text`）
   - 用后端 id 替换乐观 id，而不是 append。

**tool result message ID：**
1. `run_tool_call_with_bus_internal()` 生成一个稳定 id（如 `format!("tool-{}", tool_call_id)` 或提前分配），写入 `ToolCallCompleted` 事件。
2. `persist_tool_messages()` 用同一个 id 写 DB（通过参数传入，不再独立 `uuid::Uuid::new_v4()`）。

### 方案 B（P1-1 ~ P1-4）：前端状态守卫修复

**B1：`tool:completed` 加会话守卫**
```ts
// useStreaming.ts:274 前加：
if (message.conversationId !== useChatStore.getState().activeConversationId) {
  touchActivity(message.conversationId)
  return
}
```

**B2：`switchConversation` 加竞态保护**
```ts
// useChat.ts:161 前增加 version counter：
const loadVersion = ++switchVersionRef.current
const [msgs, tasks] = await Promise.all([getMessages(id), getTasks(id)])
if (switchVersionRef.current !== loadVersion) return  // 已被更新的切换覆盖
useChatStore.getState().setMessages(msgs)
```

**B3：乐观消息失败回滚**
```ts
// useChat.ts:284-298 catch 块中，额外 remove 乐观消息：
useChatStore.getState().removeMessage(messageId)
```

**B4：`UserBubble`/`AiBubble` 替换直接 IPC 调用**
将 `UserBubble.tsx:34`、`AiBubble.tsx:72` 的 `sendMessage()` 改为调用 `useChat().sendUserMessage()`，走完整守卫链路。

### 方案 C（P1-5）：渲染层补全映射

**C1：`useTurnRenderModel` 历史路径映射**
```ts
// role=assistant 时，为每个 toolCall 初始化 step：
tool_calls.forEach((tc, idx) => {
  current.toolGroup.steps.push({
    index: idx + 1,
    name: tc.name,
    status: 'done',
    inputJson: tc.arguments != null
      ? JSON.stringify(tc.arguments, null, 2)
      : undefined,
  })
})

// role=tool 时，找到对应 step 补充 output：
const step = current.toolGroup.steps.find(s => s.name === result.name) ?? {
  index: current.toolGroup.steps.length + 1,
  name: result.name,
  status: result.isError ? 'error' : 'done',
}
step.output = result.content
  ? truncateOutput(result.content, 20)
  : undefined
step.durationMs = result.durationMs
```

**C2：`streamingStore` 加 `output` 字段**
```ts
export interface ToolExecution {
  // ...现有字段...
  output?: string  // tool:completed 后写入
}
```

**C3：`tool:completed` 写 output 到 ToolExecution**
```ts
useChatStore.getState().updateConversationToolExecution(
  message.conversationId,
  message.toolResult.toolCallId,
  {
    status: ...,
    durationMs: ...,
    output: message.toolResult.content,  // 新增
  },
)
```

**C4：实时路径映射 inputJson + output**
```ts
// useTurnRenderModel.ts 实时覆盖段：
const steps = toolExecutions.map((t, i) => ({
  index: i + 1,
  name: t.toolName,
  status: toolExecStatusToStep(t.status),
  durationMs: t.durationMs,
  inputJson: t.input != null ? JSON.stringify(t.input, null, 2) : undefined,
  output: t.output ? truncateOutput(t.output, 20) : undefined,
}))
```

**C5：截断工具函数**
```ts
function truncateOutput(text: string, maxLines = 20): string {
  const lines = text.split('\n')
  if (lines.length <= maxLines) return text
  return lines.slice(0, maxLines).join('\n')
    + `\n…（共 ${lines.length} 行，已截断）`
}
```

### 方案 D（P1-6 + P1-7）：后端补全

**D1：`persist_iteration_assistant_message` 写 DB 后 emit `MessagePersisted`**
写入完成后通过 bus emit `RuntimeEventKind::MessagePersisted{role:"assistant", message_id, content:{toolCalls:[...]}}`，前端 `message:updated` 收到后 upsert。

**D2：工具执行耗时传递**
`run_tool_call_with_bus_internal()` 在 dispatch 前记 `Instant::now()`，完成后计算 `duration_ms` 填入 `ToolCallCompleted` 事件。

### 方案 E（P1-8）：切回 streaming 会话后重载

在 `switchConversation` 时，若 `busyConversations.has(id)` 为 true，记录 `needsReloadOnIdle[id] = true`；`agent:idle` handler 中，若该标志存在且 `convId === activeConversationId`，触发 `getMessages(convId)` 重新加载。

### 方案 F（P2）：类型清理

| 修复项 | 改动 |
|---|---|
| `streaming:done` 删除假 `messageId` 字段 | `src/lib/tauri.ts:56` |
| `streaming:error` 删除后端不发的字段，改为从 `rawError` 推断 `errorType` | `src/lib/tauri.ts` + `useStreaming.ts` |
| 订阅 `stop:prevented-continuation`，显示 toast | `src/lib/tauri.ts` + `useStreaming.ts` |
| `task:status-changed` 删除不存在的 3 个字段 | `src/lib/tauri.ts:121-132` |
| 清理 6 个空订阅遗留事件常量和 handler | `src/lib/tauri.ts`、`src/hooks/useStreaming.ts` |

---

## 四、不在本次做的事

- `runId` 作为事件守卫（需要后端 busyConversations 机制先完善）
- `is_agent_busy` busy 来源对齐（`legacy gateway` vs `SessionRuntime`）
- 并发工具执行顺序保证（`tool_round_driver` 改同步 emit）
- `persist_assistant_message` 空内容不 emit 修复（需精确评估影响面）

---

## 五、实施顺序

```
Phase 1（消除重复消息根因）
  A：统一消息 ID（P0-1 + P0-2）
    后端：persist_user_message emit MessagePersisted(role=user)
    后端：tool id 提前分配，ToolCallCompleted 与 persist 共用同一 id
    前端：message:updated 收到 role=user 时替换乐观消息

Phase 2（修复状态管理 Bug）
  B：前端状态守卫（P1-1 ~ P1-4）
    tool:completed 加 activeConversationId 守卫
    switchConversation 加竞态保护
    乐观消息失败回滚
    UserBubble/AiBubble 改走 sendUserMessage()

Phase 3（工具消息展示）
  C：渲染层补全映射（P1-5）
    useTurnRenderModel 历史/实时路径补 inputJson + output
    streamingStore 加 output 字段
    tool:completed handler 写 output

Phase 4（后端补全 + 类型清理）
  D：persist_iteration_assistant_message emit MessagePersisted
  D：工具执行耗时传递
  E：切回 streaming 会话重载
  F：类型契约清理
```

---

## 六、关键文件索引

| 文件 | 关键改动 |
|---|---|
| `src/hooks/useChat.ts` | P0-1 乐观消息替换、P1-2 竞态保护、P1-3 失败回滚 |
| `src/hooks/useStreaming.ts` | P1-1 守卫、P1-8 重载标志、P2 类型清理 |
| `src/hooks/useTurnRenderModel.ts` | P1-5 inputJson/output 映射 |
| `src/stores/streamingStore.ts` | P1-5 ToolExecution.output 字段 |
| `src/components/chat/UserBubble.tsx` | P1-4 改 sendUserMessage |
| `src/components/chat/AiBubble.tsx` | P1-4 改 sendUserMessage |
| `src/lib/tauri.ts` | P2 类型清理 |
| `src-tauri/src/transport/tauri_commands/chat.rs` | P0-1 user msg emit、P0-2 tool id 统一 |
| `src-tauri/src/runtime/query_engine.rs` | P0-2 tool id 提前分配、P1-7 duration_ms |
| `src-tauri/src/runtime/chat/chat_turn_driver.rs` | P1-6 emit MessagePersisted for toolCalls |
