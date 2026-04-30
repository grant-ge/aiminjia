# SSE 消息全链路审计与修复设计（review 修订版）

> 审计日期：2026-04-24  
> 修订依据：Opus 架构 review、前端状态 review、后端 runtime review、5 个 haiku 分段审计、第二轮 targeted verification  
> 覆盖范围：前端发送 → Tauri IPC → Rust Runtime → LLM SSE → RuntimeEvent → Tauri emit → 前端订阅 → Store → Render Model → UI

---

## 一、完整链路时序（E2E）

```
前端 useChat.sendUserMessage()
  ├─ 生成乐观 user message（当前 id = crypto.randomUUID()）
  ├─ store.addMessage(userMessage)
  ├─ store.setConversationStreaming(cId, true) + addBusyConversation()
  └─ invoke('send_message', {conversationId, content, fileIds, agentName})
       ↓
commands/chat.rs → TauriChatCommandAdapter::send_message()
  └─ SessionRuntime::run_chat_request()
       ├─ 生成权威 RunId（UUID v4）
       └─ RuntimeChatTurnDriver::run_chat_turn_s4()
            ├─ persist_user_message() → DB（id = "msg-<uuid>"，当前不回传前端）⚠
            ├─ emit StreamStarted（内部事件）
            ├─ [迭代循环]
            │    ├─ run_llm_step() → LLM OpenAI-compatible SSE
            │    │    ├─ ContentDelta → RuntimeEvent::StreamDelta → streaming:delta
            │    │    ├─ ToolCallStart（内部聚合，不发事件）
            │    │    ├─ chunk timeout / stream error retry 时重新调用 stream_message() ⚠
            │    │    └─ Done → LlmStepResult::ContentComplete / ToolCalls
            │    │
            │    └─ [若 ToolCalls]
            │         ├─ ToolRoundDriver::execute_round()（并发 spawn）
            │         │    ├─ emit ToolCallExecuting → tool:executing
            │         │    ├─ dispatcher.dispatch()
            │         │    └─ emit ToolCallCompleted（当前 msg_id="tool-<uuid1>"）→ tool:completed ⚠
            │         ├─ persist_iteration_assistant_message() → DB（当前无事件）
            │         └─ persist_tool_messages()（当前 msg_id="tool-<uuid2>"，≠uuid1）⚠
            │
            └─ [完成后]
                 ├─ persist_assistant_message() → DB（等待 MessageWriteQueue ack，正常路径 DB 已可读）
                 ├─ emit MessagePersisted → message:updated
                 ├─ emit StreamDone → streaming:done
                 ├─ emit TurnCompleted → turn:completed
                 ├─ [若 stop hook] emit StopHookPreventedContinuation → stop:prevented-continuation（前端当前无订阅）⚠
                 └─ emit AgentIdle(Primary) → agent:idle

前端事件处理（useStreaming.ts）
  ├─ streaming:delta → buffer/rAF flush → streamStates[convId].streamingContent → StreamingBubble
  ├─ tool:executing → addConversationToolExecution(convId, toolCallId)
  ├─ tool:completed → 当前无 activeConversationId 守卫 upsertMessage() ⚠ + updateToolExecution()
  ├─ message:updated → 仅 active 会话 upsert/add；当前无 role=user 乐观替换分支 ⚠
  ├─ streaming:done → clearConversationStreamState() + removeBusyConversation()
  ├─ message:updated(role=assistant) → clearConversationStreamState()，当前不 removeBusyConversation ⚠
  ├─ turn:completed → toast + setLastTurnSummary()
  └─ agent:idle(primary) → clearConversationStreamState() + removeBusyConversation()

渲染路径
  MessageList → useTurnRenderModel(messages, toolExecutions) → RenderTurn[]
    ├─ role=user → UserMessageBubble
    ├─ role=assistant + toolCalls → 当前只初始化空 toolGroup，未生成 inputJson
    ├─ role=tool → 当前生成 step，但 output/inputJson 未映射
    └─ role=assistant text → AiBubble
  streaming 期间额外渲染 StreamingBubble（直连 streamingContent）

历史加载
  switchConversation() → getMessages(id) → setMessages(msgs)
  conversation_service::transform_message_json_for_frontend()
  → 同一 useTurnRenderModel 渲染（历史与实时路径汇合）
```

---

## 二、已确认结论与误判剔除

### 已确认真实问题

| 编号 | 问题 | 结论 |
|---|---|---|
| C-1 | user optimistic id 与 DB id 不一致 | 真实，需引入 `clientMessageId` 精确替换 |
| C-2 | tool result 实时 id 与 DB id 不一致 | 真实，需同一个 msg_id 贯穿事件和持久化 |
| C-3 | `tool:completed` 无会话守卫 | 真实，但只能阻止 `upsertMessage`，不能跳过 per-conversation toolExecution 更新 |
| C-4 | `switchConversation` 无竞态保护 | 真实，需 version counter |
| C-5 | 乐观消息失败不回滚 | 真实，需 `removeMessage` + pending 映射 |
| C-6 | `UserBubble`/`AiBubble` 直接调 IPC | 真实，应通过 props 调用统一 sendUserMessage 流程，不能在每个 bubble 里直接 `useChat()` |
| C-7 | `useTurnRenderModel` 入参/输出缺失 | 真实，需按 `toolCallId` merge，不可按 name |
| C-8 | `message:updated(role=assistant)` 清 streaming 但不 remove busy | 真实，`streaming:done` 丢失时需等 200s watchdog |
| C-9 | LLM retry 后 delta 重复 | 真实，retry 会重新调用 `stream_message()`，已 emit delta 无法撤回 |
| C-10 | `MessagePersisted` payload 当前无法承载顶级 `toolCalls` | 真实，若为 assistant[toolCalls] emit，需要 adapter/schema 支持 |

### 已剔除误判

| 误判 | 最终结论 |
|---|---|
| rAF buffer 执行中会被新 delta 打断导致丢失 | 误判。JS 单线程，`deltaBufferRef.current = {}` 后新 delta 写入新对象，不丢失 |
| `ToolExecution.toolId` 与 `ToolResultContent.toolCallId` 不一致 | 误判。两者都来自 Rust `tool_call_id`，只是 JSON 字段名不同 |
| `MessagePersisted` emit 时 DB 可能尚未写完 | 正常路径已等 MessageWriteQueue ack，DB 可读；异常路径会返回错误，不 emit |

---

## 三、问题清单（按优先级）

### P0：消息身份一致性

#### P0-1：user message optimistic id 与 DB id 不一致

- 前端当前用 `crypto.randomUUID()` 作为 optimistic message id。
- 后端 `persist_user_message()` 生成 `msg-<uuid>`，返回值在 driver 中被 `_user_msg_id` 丢弃。
- 如果后续让后端 emit `MessagePersisted(role=user)`，前端当前 `message:updated` 只按 `message.id` upsert，会 append 而不是替换。
- **不能用 `conversationId + content.text` 匹配**：重复文本、空格/换行 normalize、文件消息都会错配。

#### P0-2：tool result 实时 id 与 DB id 不一致

- 实时事件：`ToolCallCompleted.msg_id = tool-<uuid1>`。
- DB 写入：`persist_tool_messages()` 生成 `tool-<uuid2>`。
- 两者独立，重载后实时消息和历史消息无法幂等合并。
- **不要用 `toolCallId` 本身当消息 id**。claude-code-best 中消息 uuid 与 LLM `tool_use_id` 是两层：消息 uuid 用 `randomUUID()`，`tool_use_id` 只负责配对。AIjia 应生成一次消息 id，再同时用于事件和持久化。

### P1：前端状态与渲染一致性

#### P1-1：`tool:completed` 无会话守卫导致串消息

- 当前直接 `upsertMessage(message)`，后台会话工具消息会进入当前 active `messages[]`。
- 但 `updateConversationToolExecution` 是按 conversationId 分桶的，不能被守卫 return 掉。

#### P1-2：`switchConversation` 无竞态保护

- 快速切换时，旧请求晚返回会覆盖新 active 会话的 `messages/tasks`。

#### P1-3：乐观消息失败后不回滚

- IPC 失败和 streaming error 后，乐观 user message 残留。
- store 当前没有 `removeMessage` 方法，需先补充。

#### P1-4：`UserBubble`/`AiBubble` 绕过统一发送流程

- 两处直接调底层 `sendMessage()` IPC，绕过 busy 检查、乐观消息、streaming 状态。
- 不应在每个 bubble 内直接调用 `useChat()`，否则每个气泡订阅 `messages` 等状态，长对话会放大重渲染。

#### P1-5：工具消息渲染缺失 input/output，且需按 toolCallId merge

- `RenderToolStep` 当前没有 `toolCallId`。
- 历史 assistant.toolCalls、实时 toolExecutions、role=tool result 三个来源应按 `toolCallId` 合并。
- 不能按 `name` 匹配，同一轮可能多次调用同名工具。
- 现有 `hasHistoricalSteps` override 机制应废弃，否则 assistant[toolCalls] 事件到达后会遮蔽实时 running 状态。

#### P1-6：`message:updated(role=assistant)` 少调 `removeBusyConversation`

- 若 `streaming:done` 丢失、但 `message:updated(role=assistant)` 到达，当前会清 streaming state，但 busyConversations 仍保留，直到 200s watchdog。

#### P1-7：LLM retry 后 delta 重复

- chunk timeout / stream error retry 会重新调用 `stream_message()`。
- 已 emit 到前端的 delta 无法撤销，前端继续 append，可能显示重复内容。
- 当前前端已有 `streaming:step-reset` 订阅但后端从不发，可复用为 retry reset 事件。

#### P1-8：工具执行耗时缺失

- 多处 `duration_ms: None`，包括 `query_engine.rs` 的正常/错误路径、permission deny 路径、legacy `run_tool_with_bus`。

### P2：类型契约和遗留事件

- `streaming:done` 前端类型声明 `messageId`，后端不发，前端也不使用。
- `streaming:error` 前端声明 `errorType/partialContent/...`，后端只发 `error/rawError`。
- `stop:prevented-continuation` 后端会发，前端无订阅。
- `task:status-changed` 前端声明 `description/blockedBy/createdAt`，后端不发。
- `OrphanedPermissionDetected` RuntimeEvent 无 Tauri 映射。
- 多个前端事件常量是空订阅：`file:parsed`、`file:generated`、`notification`、`auth:expired`、`browser:closed`。

---

## 四、修复设计

### 方案 A：统一 user message ID（clientMessageId 精确替换）

**原则**：后端 DB id 是最终消息 id；前端 optimistic id 只是临时 clientMessageId。

#### A1：前端发送时生成 `clientMessageId`

- `useChat.sendUserMessage()` 生成 `clientMessageId = crypto.randomUUID()`。
- 乐观消息使用该 id 展示，并标记为 pending（可在 `content` 或额外 metadata 中维护，不落盘）。
- `sendMessage()` IPC 增加可选参数 `clientMessageId`。

#### A2：后端 command/request 透传 `clientMessageId`

- `src/lib/tauri.ts::sendMessage()` 参数增加 `clientMessageId?: string`。
- `src-tauri/src/commands/chat.rs::send_message()` 增加 `client_message_id: Option<String>`。
- `TauriChatCommandAdapter::send_message()` 写入 `ChatTurnRequest.client_message_id`。
- `ChatTurnRequest` 增加 `client_message_id: Option<String>`。

#### A3：后端 user message 持久化后 emit `MessagePersisted(role=user)`

- `persist_user_message()` 返回 `user_msg_id`。
- driver 不再丢弃 `_user_msg_id`。
- emit 时序：应在 `StreamStarted` 之后、进入 LLM 迭代前 emit，避免前端在 streaming 状态尚未建立时处理 user echo。
- payload 中包含：
  - `id/messageId = user_msg_id`
  - `role = "user"`
  - `content.text`
  - `clientMessageId`

#### A4：前端 `message:updated` 增加 role=user 分支

- 当前通用逻辑按 `message.id` 判断，会 append。
- 新逻辑：当 `message.role === 'user' && message.clientMessageId`：
  - 查找本地 pending optimistic message（id/clientMessageId 相同）
  - 找到则替换 id 为后端 id，并 merge 后端 content；保留前端 `sender/files` 等 UI 元数据
  - 找不到才 addMessage（兼容旧路径）
- `content.text` 仅可作为降级兜底，不作为主路径。

### 方案 B：统一 tool result message ID（生成一次，事件与 DB 共用）

**原则**：消息 id 不是 `toolCallId`，而是一次性生成的消息 uuid；`toolCallId` 只用于 assistant.toolCalls 与 tool result 配对。

#### B1：在工具结果对象里携带 `msg_id`

- 修改 `ToolRoundResults.tool_result_messages` 的 JSON 结构，增加 `msgId` 字段。
- `collect_results` 在构造每条 tool result JSON 时保留来自 `RuntimeToolCallOutcome` 的 `msg_id`。

#### B2：QueryEngine 生成一次 `msg_id`

- `run_tool_call_with_bus_internal()` 中生成 `msg_id = format!("tool-{}", uuid::Uuid::new_v4())`。
- 该 id 同时：
  - 写入 `ToolCallCompleted { msg_id }` 事件
  - 写入 `RuntimeToolCallOutcome` / collector 结果
  - 最终传到 `persist_tool_messages()`

#### B3：`persist_tool_messages()` 从 JSON 读取 `msgId`

- 不再独立 `uuid::Uuid::new_v4()`。
- 如果旧数据/旧路径没有 `msgId`，可保留 fallback 生成 id，但新主路径必须使用事件同源 id。

### 方案 C：前端状态守卫与竞态修复

#### C1：`tool:completed` 守卫只包住 `upsertMessage`

```ts
onToolCompleted((message) => {
  touchActivity(message.conversationId)

  const store = useChatStore.getState()
  if (message.conversationId === store.activeConversationId) {
    store.upsertMessage(message)
  }

  if (message.toolResult) {
    store.updateConversationToolExecution(message.conversationId, message.toolResult.toolCallId, {
      status: message.toolResult.isError ? 'error' : 'completed',
      durationMs: message.toolResult.durationMs,
      output: message.toolResult.content,
    })
  }
})
```

#### C2：`switchConversation` 加 version counter

- `switchVersionRef` 必须在 `useChat()` hook 顶层声明。
- version 不匹配时，`messages` 和 `tasks` 都不能写入。

```ts
const switchVersionRef = useRef(0)

const switchConversation = useCallback(async (id: string) => {
  const loadVersion = ++switchVersionRef.current
  store.setActiveConversation(id)
  store.setMessages([])

  const [msgs, tasks] = await Promise.all([getMessages(id), getTasks(id)])
  if (switchVersionRef.current !== loadVersion) return

  useChatStore.getState().setMessages(msgs)
  useChatStore.getState().setTasks(tasks)
}, [])
```

#### C3：新增 `removeMessage`，用于乐观消息失败回滚

- 在 `SessionState` / `createSessionSlice` 增加 `removeMessage(id: string)`。
- IPC 失败 catch 中删除 optimistic message。
- `streaming:error` 也应基于 `clientMessageId` 或 pending map 删除未确认 optimistic message。

#### C4：`UserBubble`/`AiBubble` 通过 props 走统一发送流程

- `MessageList` 或上层容器只调用一次 `useChat()` 取得 `sendUserMessage`。
- 通过 props 传给 `UserMessageBubble` / `AiBubble`。
- 避免每个 bubble 内部调用 `useChat()` 订阅大块状态。

#### C5：`message:updated(role=assistant)` 清状态时同步 remove busy

- 在 assistant persisted 兜底清 streaming 的地方补 `removeBusyConversation(conversationId)`。
- 这样即使 `streaming:done` 丢失，也不会等 200s watchdog。

### 方案 D：工具渲染 merge-by-toolCallId

#### D1：扩展 RenderToolStep

```ts
export interface RenderToolStep {
  index: number
  toolCallId: string
  name: string
  status: 'running' | 'done' | 'error'
  durationMs?: number
  inputJson?: string
  output?: ReactNode
}
```

#### D2：三路来源按 toolCallId 合并

- assistant.toolCalls 贡献：`toolCallId/name/inputJson`
- streaming `ToolExecution` 贡献：`toolCallId/status/durationMs/input/output`
- role=tool result 贡献：`toolCallId/status/output/durationMs`

合并策略：
1. 按消息顺序扫描当前 turn。
2. 遇到 assistant.toolCalls：为每个 `tc.id` upsert step，设置 inputJson。
3. 遇到 role=tool：按 `toolResult.toolCallId` upsert step，设置 output/status/durationMs。
4. 最后将 active conversation 的 `toolExecutions` merge 进去，只补实时状态，不清空历史字段。
5. 不再使用 `hasHistoricalSteps` 二选一 override。

#### D3：ToolExecution 增加 output 字段

```ts
export interface ToolExecution {
  toolName: string
  toolId: string // 实际值为 toolCallId，保持兼容命名
  status: 'executing' | 'completed' | 'error'
  input?: unknown
  output?: string
  durationMs?: number
}
```

#### D4：output 截断策略

- 普通输出保留前 20 行。
- error 输出保留后 20 行（traceback 关键错误通常在尾部）。

### 方案 E：LLM retry reset 事件

#### E1：后端 retry 前发 reset 事件

- chunk timeout / stream error 进入 retry 分支前，emit 一个已有或新增事件：
  - 可复用 `streaming:step-reset`（前端已有订阅但后端当前不发）
  - 或新增 `streaming:reset`
- payload 至少包含 `conversationId/runId`。

#### E2：前端收到 reset 清空当前 streamingContent

- 清空该 conversation 的 `streamingContent` 与 delta buffer。
- 不清空 toolExecutions，除非 retry 发生在 tool_calls 阶段前且明确需要。

### 方案 F：后端补全与类型清理

#### F1：assistant[toolCalls] 是否 emit MessagePersisted 暂缓

review 后结论：直接 emit 会与实时 toolExecutions 产生双源冲突，且当前 `MessagePersisted` payload 无法承载顶级 `toolCalls`。

本轮先不把它作为 P1 必修项。若后续要做，必须同时：
- 修改 `RuntimeEventKind::MessagePersisted` 支持顶级 `tool_calls`
- 修改 `tauri_event_adapter` 输出顶级 `toolCalls`
- 确认前端 merge-by-toolCallId 已落地
- 更新 golden trace 测试

#### F2：工具执行耗时

- 在 dispatch 前记录 `Instant::now()`，await 后计算耗时。
- 覆盖所有 `duration_ms: None` 出现点：
  - `query_engine.rs` 正常成功路径
  - `query_engine.rs` 错误路径
  - permission deny 路径
  - legacy `run_tool_with_bus` 路径（如仍可达）

#### F3：类型契约清理

- 删除 `StreamingDonePayload.messageId`。
- `StreamingErrorPayload` 使用后端真实字段；若需要 `errorKind`，后端新增结构化字段，不在前端字符串猜测。
- 订阅 `stop:prevented-continuation`，toast 并清理 loading。
- `task:status-changed` 类型删除后端不发字段，或后端补发。
- 清理空订阅事件常量，或明确标为 legacy unused。

---

## 五、实施顺序

```
Phase 1：消息身份一致性（P0）
  A：clientMessageId 贯通 user optimistic → DB id 替换
  B：tool msg_id 单点生成，事件与持久化共用
  测试：user optimistic 替换、tool completed 重载不重复

Phase 2：状态管理兜底（P1）
  C1：tool:completed 只守卫 upsertMessage
  C2：switchConversation version counter
  C3：removeMessage + IPC/streaming error 回滚 optimistic
  C4：UserBubble/AiBubble props 走 sendUserMessage
  C5：message:updated assistant 清 busy
  测试：跨会话 tool 完成不串消息、快速切换不串历史、失败回滚

Phase 3：工具渲染一致性（P1）
  D：merge-by-toolCallId，补 inputJson/output/durationMs
  测试：同名工具多次调用、多轮 toolCalls、实时→历史一致

Phase 4：SSE retry reset 与类型清理（P1/P2）
  E：retry reset 事件，避免重复 delta
  F：duration_ms、stop:prevented-continuation、payload 类型清理
  测试：retry 后 streamingContent 不重复、stop hook UI 有反馈
```

---

## 六、测试计划

### 后端测试

- `review_message_updated_payload_compatibility_test.rs`
  - 新增 `MessagePersisted(role=user)` payload 测试
  - 验证 `clientMessageId` 回传
- `review_backend_event_payload_test.rs`
  - 验证 `tool:completed` 的 `msg_id` 与 collector/persist 使用同源 id
  - 验证 `durationMs` 有值，`None` 时前端可处理
- tool round E2E 测试
  - DB 顺序：assistant[toolCalls] → tool → assistant[text]
  - 重载后 tool message 不重复
- streaming retry 测试
  - retry 前发 reset 事件

### 前端测试

- `useStreaming`：
  - role=user message:updated 用 clientMessageId 替换 optimistic message
  - `tool:completed` 非 active 只更新 toolExecution，不 upsert messages
  - `message:updated(role=assistant)` 清 streaming 且 remove busy
  - streaming error 回滚 optimistic message
- `useChat`：
  - switchConversation 快速切换只保留最后一次结果
- `useTurnRenderModel`：
  - 同名工具多次调用按 toolCallId 匹配
  - 多轮 assistant.toolCalls 不跨轮错配
  - 实时 toolExecutions + 历史 messages 合并不重复
  - output 普通截前 20 行、error 截后 20 行
- Bubble 组件：
  - UserBubble/AiBubble 通过 props 触发 sendUserMessage，不直接 invoke IPC

---

## 七、不在本次做的事

- `runId` 作为所有前端事件的严格 guard（需要先明确 busy 状态单一事实源）
- `is_agent_busy` busy 来源对齐（legacy gateway vs SessionRuntime）
- 并发工具执行 emit 顺序强制稳定化（merge-by-toolCallId 后降级为视觉顺序问题）
- assistant[toolCalls] 实时 MessagePersisted 事件（需 schema/adapter/golden trace 同步升级）
- malformed JSON salvage 重构（当前作为 SSE provider 鲁棒性专项，另列）

---

## 八、关键文件索引

| 文件 | 关键改动 |
|---|---|
| `src/lib/tauri.ts` | `sendMessage(clientMessageId?)`、payload 类型清理、stop 事件订阅 |
| `src/hooks/useChat.ts` | clientMessageId 生成、switch version、失败回滚 |
| `src/hooks/useStreaming.ts` | role=user 替换、tool completed 守卫、assistant 清 busy、retry reset |
| `src/stores/sessionStore.ts` | `removeMessage` |
| `src/stores/streamingStore.ts` | `ToolExecution.output`、必要状态清理 |
| `src/hooks/useTurnRenderModel.ts` | merge-by-toolCallId、input/output 映射 |
| `src/components/chat/MessageList.tsx` | 向 bubble 传 sendUserMessage props |
| `src/components/chat/UserBubble.tsx` | 不直接 invoke IPC |
| `src/components/chat/AiBubble.tsx` | 不直接 invoke IPC |
| `src-tauri/src/commands/chat.rs` | `client_message_id` 参数 |
| `src-tauri/src/runtime/chat/chat_turn_driver.rs` | user MessagePersisted emit、retry reset |
| `src-tauri/src/runtime/query_engine.rs` | tool msg_id 单点生成、duration_ms |
| `src-tauri/src/runtime/chat/tool_result_collector.rs` | tool result JSON 携带 `msgId` |
| `src-tauri/src/transport/tauri_commands/chat.rs` | persist_tool_messages 读取 msgId、user persist 回传 |
| `src-tauri/src/transport/tauri_event_adapter.rs` | payload 类型修正、stop/orphan 映射（如做） |
