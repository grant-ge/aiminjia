# SSE 消息全链路修复 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 修复 SSE 消息全链路中的消息重复、消息错乱、工具渲染缺失、状态卡死等问题，覆盖 P0/P1 优先级的 10 个已确认 bug。

**Architecture:**
- Phase 1：消息 ID 一致性（后端 → 前端 `clientMessageId` 精确替换 optimistic user message；tool result 事件与 DB 使用同一 msg_id）
- Phase 2：前端状态守卫（tool:completed 会话过滤、switchConversation 竞态、失败回滚、busy 卡死）
- Phase 3：工具渲染补全（`useTurnRenderModel` 按 toolCallId merge，补 inputJson/output/durationMs）
- Phase 4：LLM retry reset + 类型清理

**Tech Stack:** Rust 1.77+、TypeScript、Tauri 2.x、Zustand、React、Vitest

---

## 文件变更清单

| 文件 | 改动说明 |
|---|---|
| `src/lib/tauri.ts` | `sendMessage` 增加 `clientMessageId?`；删除 `StreamingDonePayload.messageId`；清理虚假类型字段；新增 `STREAMING_RETRY_RESET` 常量 |
| `src/hooks/useChat.ts` | `sendUserMessage` 生成并传 `clientMessageId`；`switchConversation` 加 version counter；新增乐观消息失败回滚 |
| `src/hooks/useStreaming.ts` | `message:updated` 新增 role=user 乐观替换分支；`tool:completed` 加会话守卫；`message:updated(assistant)` 补 `removeBusyConversation`；新增 `streaming:retry-reset` handler |
| `src/stores/sessionStore.ts` | 新增 `removeMessage(id)` 方法 |
| `src/stores/streamingStore.ts` | `ToolExecution` 新增 `output?: string` 字段 |
| `src/hooks/useTurnRenderModel.ts` | `RenderToolStep` 新增 `toolCallId`；`buildTurnsFromMessages` 改为 merge-by-toolCallId；补 `inputJson`/`output`/`durationMs` |
| `src/components/chat/MessageList.tsx` | 从 `useChat()` 取 `sendUserMessage` 传给子组件 |
| `src/components/chat/UserBubble.tsx` | 不再直接 `invoke('send_message')`；接收 `onResend` prop |
| `src/components/chat/AiBubble.tsx` | 不再直接 `invoke('send_message')`；接收 `onUserResponse` prop |
| `src-tauri/src/commands/chat.rs` | `send_message` 增加 `client_message_id: Option<String>` |
| `src-tauri/src/runtime/chat/chat_turn_driver.rs` | `ChatTurnRequest` 增加 `client_message_id`；user msg 持久化后 emit `MessagePersisted(role=user, clientMessageId)`；retry 前 emit reset 事件 |
| `src-tauri/src/transport/tauri_commands/chat.rs` | `send_message` 透传 `client_message_id`；`persist_user_message` 接收并回传 `client_message_id` |
| `src-tauri/src/runtime/query_engine.rs` | `run_tool_call_with_bus_internal` 提前生成 `msg_id`；填写 `duration_ms` |
| `src-tauri/src/runtime/chat/tool_result_collector.rs` | `ToolRoundResults.tool_result_messages` JSON 增加 `msgId` 字段 |
| `src-tauri/src/runtime/events.rs` | `MessagePersisted` 增加 `client_message_id: Option<String>`；`StreamRetryReset` 新 variant |
| `src-tauri/src/transport/tauri_event_adapter.rs` | `MessagePersisted` payload 增加 `clientMessageId`；映射 `StreamRetryReset` → `streaming:retry-reset` |

---

## Phase 1：消息 ID 一致性

### Task A1：后端 `ChatTurnRequest` 增加 `client_message_id`

**Files:**
- Modify: `src-tauri/src/runtime/chat/chat_turn_driver.rs`
- Modify: `src-tauri/src/transport/tauri_commands/chat.rs`
- Modify: `src-tauri/src/commands/chat.rs`

- [ ] **Step 1：给 `ChatTurnRequest` 加字段**

修改 `src-tauri/src/runtime/chat/chat_turn_driver.rs` 的 `ChatTurnRequest`（第 42 行 struct）：

```rust
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChatTurnRequest {
    pub conversation_id: SessionId,
    pub content: String,
    pub file_ids: Vec<String>,
    pub agent_name: Option<String>,
    pub permission_mode: PermissionMode,
    pub run_id: RunId,
    pub hook_registry: Option<Arc<HookRegistry>>,
    pub client_message_id: Option<String>,  // 新增
}
```

在 `ChatTurnRequest::new(...)` impl 里加默认值：

```rust
pub fn new(
    conversation_id: impl Into<SessionId>,
    content: impl Into<String>,
    file_ids: Vec<String>,
) -> Self {
    Self {
        // ... 原有字段 ...
        client_message_id: None,  // 新增
    }
}
```

- [ ] **Step 2：`send_message` Tauri command 接收 `client_message_id`**

修改 `src-tauri/src/commands/chat.rs` 第 21-32 行：

```rust
#[tauri::command]
pub async fn send_message(
    adapter: State<'_, Arc<crate::transport::tauri_commands::chat::TauriChatCommandAdapter>>,
    conversation_id: String,
    content: String,
    file_ids: Vec<String>,
    permission_mode: Option<crate::runtime::tools::permission::PermissionMode>,
    agent_name: Option<String>,
    client_message_id: Option<String>,  // 新增
) -> Result<(), String> {
    adapter
        .send_message(conversation_id, content, file_ids, permission_mode, agent_name, client_message_id)
        .await
}
```

- [ ] **Step 3：`TauriChatCommandAdapter::send_message` 透传字段**

在 `src-tauri/src/transport/tauri_commands/chat.rs` 中找 `send_message` 方法，签名增加 `client_message_id: Option<String>`，并在构建 `ChatTurnRequest` 时赋值：

```rust
pub async fn send_message(
    &self,
    conversation_id: String,
    content: String,
    file_ids: Vec<String>,
    permission_mode: Option<PermissionMode>,
    agent_name: Option<String>,
    client_message_id: Option<String>,  // 新增
) -> Result<(), String> {
    let mut request = ChatTurnRequest::new(conversation_id, content, file_ids);
    request.agent_name = agent_name;
    request.permission_mode = permission_mode.unwrap_or_default();
    request.client_message_id = client_message_id;  // 新增
    self.runtime.run_chat_request(request).await
        .map_err(|e| e.to_string())
}
```

- [ ] **Step 4：编译确认**

```bash
cd src-tauri && cargo build 2>&1 | grep -E "^error" | head -20
```

期望：0 errors（可能有 warnings 忽略）

- [ ] **Step 5：Commit**

```bash
git add src-tauri/src/commands/chat.rs \
        src-tauri/src/runtime/chat/chat_turn_driver.rs \
        src-tauri/src/transport/tauri_commands/chat.rs
git commit -m "feat(chat): add client_message_id passthrough to ChatTurnRequest"
```

---

### Task A2：后端 `MessagePersisted` 携带 `client_message_id`，user message 落库后 emit

**Files:**
- Modify: `src-tauri/src/runtime/events.rs`
- Modify: `src-tauri/src/transport/tauri_commands/chat.rs`
- Modify: `src-tauri/src/runtime/chat/chat_turn_driver.rs`
- Modify: `src-tauri/src/transport/tauri_event_adapter.rs`
- Test: `src-tauri/tests/review_backend_event_payload_test.rs`

- [ ] **Step 1：写失败测试**

在 `src-tauri/tests/review_backend_event_payload_test.rs` 末尾新增：

```rust
#[tokio::test]
async fn review_user_message_persisted_includes_client_message_id() {
    let (bus, host) = make_bus_with_host();
    let session_id = SessionId::new("s-user-1");
    let run_id = RunId::new("r-user-1");

    bus.emit(RuntimeEvent::new(
        session_id.clone(),
        run_id.clone(),
        RuntimeEventKind::MessagePersisted {
            message_id: "msg-abc".to_string(),
            role: "user".to_string(),
            content: serde_json::json!({ "text": "hello" }),
            client_message_id: Some("client-uuid-123".to_string()),
        },
    ))
    .await
    .unwrap();

    let trace = host.trace();
    let event = trace
        .events
        .iter()
        .find(|e| e.name == "message:updated")
        .expect("message:updated must be emitted");

    assert_eq!(event.payload["role"].as_str(), Some("user"));
    assert_eq!(event.payload["id"].as_str(), Some("msg-abc"));
    assert_eq!(
        event.payload["clientMessageId"].as_str(),
        Some("client-uuid-123"),
        "message:updated for user must include clientMessageId"
    );
}
```

- [ ] **Step 2：运行确认失败**

```bash
cd src-tauri && cargo test review_user_message_persisted_includes_client_message_id --test review_backend_event_payload_test -- --nocapture 2>&1 | tail -10
```

期望：compile error（字段不存在）

- [ ] **Step 3：扩展 `RuntimeEventKind::MessagePersisted`**

修改 `src-tauri/src/runtime/events.rs` 中 `MessagePersisted` variant：

```rust
MessagePersisted {
    message_id: String,
    role: String,
    content: serde_json::Value,
    client_message_id: Option<String>,  // 新增
},
```

同文件中如果有 `message_persisted()` 构造函数，同步更新：

```rust
pub fn message_persisted(
    session_id: SessionId,
    run_id: RunId,
    message_id: String,
    role: &str,
    content: serde_json::Value,
) -> RuntimeEvent {
    RuntimeEvent::new(
        session_id,
        run_id,
        RuntimeEventKind::MessagePersisted {
            message_id,
            role: role.to_string(),
            content,
            client_message_id: None,  // 新增，默认 None
        },
    )
}
```

- [ ] **Step 4：更新 `tauri_event_adapter` 映射**

在 `src-tauri/src/transport/tauri_event_adapter.rs` 中 `MessagePersisted` 分支（约第 123 行）：

```rust
RuntimeEventKind::MessagePersisted { message_id, role, content, client_message_id } => {
    let mut payload = json!({
        "conversationId": conversation_id,
        "messageId": message_id,
        "id": message_id,
        "role": role,
        "content": transform_message_json_for_frontend(json!({"content": content}))["content"].clone(),
        "createdAt": chrono::Utc::now().to_rfc3339(),
        "runId": event.run_id.as_str(),
    });
    if let Some(cid) = client_message_id {
        payload["clientMessageId"] = json!(cid);
    }
    Some(LegacyEvent {
        name: "message:updated".to_string(),
        payload,
    })
}
```

- [ ] **Step 5：`persist_user_message` 接收并回传 `client_message_id`**

修改 `src-tauri/src/transport/tauri_commands/chat.rs` 中 `persist_user_message`：

```rust
async fn persist_user_message(
    &self,
    conversation_id: &str,
    content: &str,
    file_ids: &[String],
    client_message_id: Option<&str>,   // 新增参数
) -> Result<String, TurnError> {
    let msg_id = format!("msg-{}", uuid::Uuid::new_v4());
    let content_json = if file_ids.is_empty() {
        serde_json::json!({ "text": content }).to_string()
    } else {
        let files_meta: Vec<serde_json::Value> =
            file_ids.iter().map(|id| serde_json::json!({ "id": id })).collect();
        serde_json::json!({ "text": content, "files": files_meta }).to_string()
    };
    self.services.db.insert_message(&msg_id, conversation_id, "user", &content_json)?;
    // 回传给上层，带 client_message_id
    // 注意：此处返回 (msg_id, client_message_id) 让 driver 可以 emit 事件
    let _ = client_message_id; // 先编译通过，下一步在 driver 层使用
    Ok(msg_id)
}
```

因为 `client_message_id` 最终需要在 driver 层拿到 emit，这里返回签名暂不变（仍返回 `String`）。driver 在调用完后从 `request.client_message_id` 直接取即可。

- [ ] **Step 6：driver 中 user 落库后 emit `MessagePersisted`**

修改 `src-tauri/src/runtime/chat/chat_turn_driver.rs` 第 733-743 行附近：

```rust
// Step 2b: Persist user message
let user_msg_id = executor
    .persist_user_message(
        request.conversation_id.as_str(),
        &request.content,
        &request.file_ids,
        request.client_message_id.as_deref(),
    )
    .await
    .map_err(|e| anyhow::anyhow!("{}", e))?;

// Step 2c: Emit MessagePersisted(role=user) AFTER StreamStarted to avoid
// front-end receiving user echo before streaming state is initialized.
// (StreamStarted is emitted at Step 4 / L748; we delay emit until then.)
// Store user_msg_id and client_message_id for Step 4.
let pending_user_msg_id = user_msg_id;
let pending_client_msg_id = request.client_message_id.clone();
```

找到 Step 4 `StreamStarted` emit 之后（约 L748），追加 user MessagePersisted emit：

```rust
// After StreamStarted emit:
self.event_bus.emit(RuntimeEvent::new(
    turn.session_id().clone(),
    turn.run_id().clone(),
    RuntimeEventKind::MessagePersisted {
        message_id: pending_user_msg_id.clone(),
        role: "user".to_string(),
        content: serde_json::json!({ "text": request.content }),
        client_message_id: pending_client_msg_id.clone(),
    },
)).await?;
```

- [ ] **Step 7：运行测试**

```bash
cd src-tauri && cargo test review_user_message_persisted_includes_client_message_id --test review_backend_event_payload_test -- --nocapture
```

期望：PASS

- [ ] **Step 8：确认全量后端集成测试不回归**

```bash
cd src-tauri && cargo test --test review_backend_event_payload_test -- --nocapture 2>&1 | tail -15
```

期望：全部 pass

- [ ] **Step 9：Commit**

```bash
git add src-tauri/src/runtime/events.rs \
        src-tauri/src/transport/tauri_event_adapter.rs \
        src-tauri/src/transport/tauri_commands/chat.rs \
        src-tauri/src/runtime/chat/chat_turn_driver.rs \
        src-tauri/tests/review_backend_event_payload_test.rs
git commit -m "feat(events): emit MessagePersisted(role=user) with clientMessageId after StreamStarted"
```

---

### Task A3：前端 `sendMessage` 传 `clientMessageId`，`message:updated` 替换乐观消息

**Files:**
- Modify: `src/lib/tauri.ts`
- Modify: `src/hooks/useChat.ts`
- Modify: `src/hooks/useStreaming.ts`

- [ ] **Step 1：`tauri.ts` 的 `sendMessage` 增加 `clientMessageId`**

```ts
// src/lib/tauri.ts
export function sendMessage(
  conversationId: string,
  content: string,
  fileIds?: string[],
  agentName?: string | null,
  clientMessageId?: string,           // 新增
): Promise<void> {
  return invoke<void>('send_message', {
    conversationId,
    content,
    fileIds: fileIds ?? [],
    agentName: agentName ?? null,
    clientMessageId: clientMessageId ?? null,  // 新增
  })
}
```

同时删除 `StreamingDonePayload.messageId` 字段（虚假类型）：

```ts
export interface StreamingDonePayload {
  conversationId: string
  runId?: string
}
```

- [ ] **Step 2：`useChat.sendUserMessage` 生成并传递 `clientMessageId`**

在 `src/hooks/useChat.ts` 中找到 `sendUserMessage`，在生成 `userMessage` 处提取 id（通常已是 `messageId = generateId()`），作为 `clientMessageId` 传给 IPC：

```ts
const messageId = generateId()  // 这是当前 optimistic id 的生成处

// 乐观消息仍用 messageId
const userMessage: Message = { id: messageId, ... }
store.addMessage(userMessage)
store.setConversationStreaming(conversationId, true)
store.addBusyConversation(conversationId)

// 透传 clientMessageId 给后端
await sendMessage(conversationId, text, fileIds, agentName, messageId)
```

- [ ] **Step 3：`useStreaming.ts` 的 `message:updated` 增加 role=user 替换逻辑**

在 `src/hooks/useStreaming.ts` 的 `message:updated` handler（第 213 行附近）中，在现有 `activeConversationId` 守卫内，在通用 `exists` 判断**之前**插入 user 消息替换逻辑：

```ts
onMessageUpdated((message) => {
  const store = useChatStore.getState()

  if (message.conversationId === store.activeConversationId) {
    // role=user：用 clientMessageId 精确替换 optimistic message
    if (message.role === 'user' && (message as any).clientMessageId) {
      const clientId = (message as any).clientMessageId as string
      const optimisticIdx = store.messages.findIndex((m) => m.id === clientId)
      if (optimisticIdx >= 0) {
        // 替换 id，保留前端 sender/files 等 UI 字段
        const merged = { ...store.messages[optimisticIdx], ...message, id: message.id }
        store.updateMessage(clientId, merged)
        // updateMessage 按 id 找，但 id 变了，需要直接 splice
        // 改用 setMessages 替换整个数组
        const next = [...store.messages]
        next[optimisticIdx] = { ...next[optimisticIdx], id: message.id }
        useChatStore.setState({ messages: next })
        return
      }
      // clientId 没找到，降级走通用 upsert
    }

    // 通用路径（非 user 或 clientMessageId 缺失）
    const exists = store.messages.some((m) => m.id === message.id)
    if (exists) {
      store.updateMessage(message.id, message)
    } else {
      store.addMessage(message)
    }
  }

  // role=assistant：清 streaming state + removeBusyConversation
  if (message.role === 'assistant') {
    const streamState = store.streamStates[message.conversationId]
    if (streamState?.isStreaming) {
      flushConversationDeltas(message.conversationId)
      delete lastActivityRef.current[message.conversationId]
      store.clearConversationStreamState(message.conversationId)
      store.removeBusyConversation(message.conversationId)  // 补上：防止 streaming:done 丢失时 busy 卡死
    }
  }
})
```

注意：上面 `store.streamStates` 访问需要通过 `useStreamingStore.getState()`，根据实际代码调整。

- [ ] **Step 4：补充 `Message` 类型中的 `clientMessageId` 可选字段**

在 `src/types/message.ts` 的 `Message` 接口末尾追加（用 `any` 转型的根本原因是类型缺失）：

```ts
export interface Message {
  // ... 现有字段 ...
  clientMessageId?: string  // 后端 echo 回的 optimistic id，仅出现在 message:updated role=user 时
}
```

- [ ] **Step 5：运行前端测试**

```bash
pnpm test -- --run src/hooks/__tests__/useTurnRenderModel.test.ts 2>&1 | tail -10
```

期望：现有测试全部 pass（本 task 未改 useTurnRenderModel）

- [ ] **Step 6：Commit**

```bash
git add src/lib/tauri.ts \
        src/hooks/useChat.ts \
        src/hooks/useStreaming.ts \
        src/types/message.ts
git commit -m "feat(frontend): pass clientMessageId to IPC and replace optimistic user message on persisted echo"
```

---

### Task A4：统一 tool result msg_id（事件与 DB 共用同一 UUID）

**Files:**
- Modify: `src-tauri/src/runtime/query_engine.rs`
- Modify: `src-tauri/src/runtime/chat/tool_result_collector.rs`
- Modify: `src-tauri/src/transport/tauri_commands/chat.rs`
- Test: `src-tauri/tests/review_backend_event_payload_test.rs`

- [ ] **Step 1：写失败测试**

在 `review_backend_event_payload_test.rs` 末尾新增：

```rust
#[tokio::test]
async fn review_tool_completed_msg_id_matches_persisted_record() {
    // 验证 ToolCallCompleted 事件里的 msg_id 与 collect_results 输出的 msgId 字段一致
    use app_lib::runtime::chat::tool_result_collector::collect_results;
    use app_lib::runtime::chat::tool_round_types::RuntimeToolCallOutcome;

    let outcomes: Vec<(RuntimeToolCallOutcome, usize)> = vec![(
        RuntimeToolCallOutcome::Completed {
            tool_call_id: "tc-1".to_string(),
            tool_name: "run_python".to_string(),
            content: "result output".to_string(),
            is_error: false,
            file_meta: None,
            is_degraded: false,
            degradation_notice: None,
            max_result_size_chars: 8000,
            context_modifier_message: None,
            skill_runtime_patch: None,
        },
        8000,
    )];

    let results = collect_results(outcomes, false, &[]);

    // collect_results 必须在 tool_result_messages JSON 里携带 msgId 字段
    let msg = &results.tool_result_messages[0];
    let msg_id = msg["msgId"].as_str().expect("tool_result_messages[0] must have msgId field");
    assert!(msg_id.starts_with("tool-"), "msgId must start with 'tool-', got: {}", msg_id);
}
```

- [ ] **Step 2：运行确认失败**

```bash
cd src-tauri && cargo test review_tool_completed_msg_id_matches_persisted_record --test review_backend_event_payload_test -- --nocapture 2>&1 | tail -10
```

期望：FAIL（`msgId` 字段缺失）

- [ ] **Step 3：`collect_results` 生成并写入 `msgId`**

修改 `src-tauri/src/runtime/chat/tool_result_collector.rs` 中 `collect_results` 函数，在构建 `tool_result_messages` 时增加 `msgId`：

找到向 `tool_result_messages` push 的地方（大约：`results.tool_result_messages.push(json!({...}))`），改为：

```rust
let msg_id = format!("tool-{}", uuid::Uuid::new_v4());
results.tool_result_messages.push(serde_json::json!({
    "msgId": msg_id,           // 新增
    "role": "tool",
    "toolCallId": tr_id,
    "name": tr_name,
    "content": truncated_result,
}));
```

同时保存 `msg_id` 到一个新的 `tool_msg_ids: Vec<String>` 字段（用于传给 QueryEngine 做 emit）。

在 `ToolRoundResults` 新增字段：

```rust
pub struct ToolRoundResults {
    pub tool_result_messages: Vec<serde_json::Value>,
    pub tool_msg_ids: Vec<String>,    // 新增：与 tool_result_messages 等长，顺序一一对应
    // ... 其余字段不变 ...
}
```

- [ ] **Step 4：`run_tool_call_with_bus_internal` 使用来自 collector 的 `msg_id`**

目前 `query_engine.rs` 在 emit `ToolCallCompleted` 时独立生成 `uuid::Uuid::new_v4()`。现在改为：在 `ToolRoundResults` 中携带 `msg_id`，QueryEngine 从 results 里取 id 再 emit。

但 `run_tool_call_with_bus_internal` 是对单个 tool call 执行的，与 collector 流程相互独立。最简单的做法：在 `run_tool_call_with_bus_internal` 内部**提前生成**一个 `msg_id`，同时传入 emit 和 collector outcomes：

在 `run_tool_call_with_bus_internal` 中找到工具执行结果处理段，在 dispatch 前生成：

```rust
// 在 `dispatcher.dispatch(...)` 之前生成 msg_id
let msg_id = format!("tool-{}", uuid::Uuid::new_v4());

// dispatch 并计时
let dispatch_start = std::time::Instant::now();
let dispatch_result = dispatcher.dispatch(&call.tool_name, call.args.clone(), ctx).await;
let duration_ms = dispatch_start.elapsed().as_millis() as u64;
```

成功路径（原 L459-473）：

```rust
RuntimeEventKind::ToolCallCompleted {
    tool_call_id: ToolCallId::new(call.tool_call_id.clone()),
    tool_name: call.tool_name.clone(),
    is_error: false,
    content: tool_result.content.clone(),
    msg_id: msg_id.clone(),    // 使用提前生成的 msg_id
    duration_ms: Some(duration_ms),   // 填写实际耗时
},
```

错误路径（原 L523-537）：

```rust
RuntimeEventKind::ToolCallCompleted {
    tool_call_id: ToolCallId::new(call.tool_call_id.clone()),
    tool_name: call.tool_name.clone(),
    is_error: true,
    content: content.clone(),
    msg_id: msg_id.clone(),    // 使用提前生成的 msg_id
    duration_ms: Some(duration_ms),
},
```

同时将 `msg_id` 写入 `RuntimeToolCallOutcome::Completed` 供 collector 使用：

```rust
pub enum RuntimeToolCallOutcome {
    Completed {
        // ... 现有字段 ...
        msg_id: String,    // 新增：供 collector 写入 tool_result_messages[n].msgId
    },
    AskRequired { ... },
}
```

- [ ] **Step 5：`persist_tool_messages` 从 JSON 读取 `msgId`**

修改 `src-tauri/src/transport/tauri_commands/chat.rs` 中 `persist_tool_messages`：

```rust
async fn persist_tool_messages(
    &self,
    conversation_id: &str,
    tool_messages: &[serde_json::Value],
) -> Result<(), TurnError> {
    for msg in tool_messages {
        // 优先用 collector 写入的 msgId，fallback 生成新 UUID（兼容旧数据路径）
        let msg_id = msg["msgId"]
            .as_str()
            .map(|s| s.to_string())
            .unwrap_or_else(|| format!("tool-{}", uuid::Uuid::new_v4()));

        let tool_call_id = match msg.get("toolCallId").and_then(|v| v.as_str()) {
            Some(v) => v.to_string(),
            None => {
                log::warn!("[persist_tool_messages] skipping msg missing toolCallId");
                continue;
            }
        };
        let name = msg.get("name").and_then(|v| v.as_str()).unwrap_or_default().to_string();
        let content = msg.get("content").and_then(|v| v.as_str()).unwrap_or_default().to_string();

        let stored = crate::storage::file_store::types::StoredMessage {
            id: msg_id.clone(),
            conversation_id: conversation_id.to_string(),
            role: "tool".to_string(),
            content: serde_json::json!({ "text": content }),
            created_at: chrono::Utc::now().to_rfc3339(),
            tool_call_id: Some(tool_call_id),
            name: Some(name),
            tool_calls: None,
            run_id: None,
            schema_version: Some(2),
            sequence: None,
            seq: None,
            rev: None,
        };
        if let Err(e) = self.services.db.insert_chat_message_record(&stored) {
            log::warn!("[persist_tool_messages] Failed to save tool message id={}: {}", msg_id, e);
        }
    }
    Ok(())
}
```

- [ ] **Step 6：运行测试**

```bash
cd src-tauri && cargo test review_tool_completed_msg_id_matches_persisted_record --test review_backend_event_payload_test -- --nocapture
```

期望：PASS

- [ ] **Step 7：运行所有后端相关测试**

```bash
cd src-tauri && cargo test --test review_backend_event_payload_test --test message_storage_v2_test --test history_rebuild_test -- --nocapture 2>&1 | tail -20
```

期望：全部 pass

- [ ] **Step 8：Commit**

```bash
git add src-tauri/src/runtime/query_engine.rs \
        src-tauri/src/runtime/chat/tool_result_collector.rs \
        src-tauri/src/runtime/chat/tool_round_types.rs \
        src-tauri/src/transport/tauri_commands/chat.rs \
        src-tauri/tests/review_backend_event_payload_test.rs
git commit -m "fix(storage): unify tool result msg_id between ToolCallCompleted event and persist_tool_messages"
```

---

## Phase 2：前端状态守卫

### Task B1：`tool:completed` 加会话守卫，`updateConversationToolExecution` 不受影响

**Files:**
- Modify: `src/hooks/useStreaming.ts`

- [ ] **Step 1：修改 `tool:completed` handler（第 270-286 行）**

```ts
onToolCompleted((message: Message) => {
  console.log('[tool:completed]', message.conversationId, message.toolResult?.name)
  touchActivity(message.conversationId)

  const store = useChatStore.getState()
  // 只有 active 会话才写入 messages[]；非活跃会话的消息等切换回来时由 getMessages 加载
  if (message.conversationId === store.activeConversationId) {
    store.upsertMessage(message)
  }

  // toolExecution 状态是 per-conversation 的，无论是否 active 都要更新
  if (message.toolResult) {
    store.updateConversationToolExecution(
      message.conversationId,
      message.toolResult.toolCallId,
      {
        status: message.toolResult.isError ? 'error' : 'completed',
        durationMs: message.toolResult.durationMs,
        output: message.toolResult.content,   // 补充 output（Phase 3 的 ToolExecution 类型已加）
      },
    )
  }
})
```

- [ ] **Step 2：运行集成测试**

```bash
pnpm exec vitest run src/hooks/useStreaming.integration.test.tsx 2>&1 | tail -15
```

期望：pass（若无相关测试用例，输出"no tests"也可）

- [ ] **Step 3：Commit**

```bash
git add src/hooks/useStreaming.ts
git commit -m "fix(streaming): guard tool:completed upsertMessage to active conversation only"
```

---

### Task B2：`switchConversation` 竞态保护

**Files:**
- Modify: `src/hooks/useChat.ts`

- [ ] **Step 1：在 `useChat` hook 顶层声明 `switchVersionRef`，修改 `switchConversation`**

在 `src/hooks/useChat.ts` 中找到 `useChat` 函数体，在所有 `useCallback` 之前（hook 顶层）添加：

```ts
const switchVersionRef = useRef(0)
```

然后把 `switchConversation` 改为：

```ts
const switchConversation = useCallback(async (id: string) => {
  const loadVersion = ++switchVersionRef.current

  store.setActiveConversation(id)
  store.setMessages([])
  useUiStore.getState().setRoute({ kind: 'chat', conversationId: id })

  const [msgs, tasks] = await Promise.all([
    getMessages(id),
    getTasks(id).catch(() => []),
  ])

  // 若切换期间又触发了新的 switchConversation，本次结果作废
  if (switchVersionRef.current !== loadVersion) return

  useChatStore.getState().setMessages(msgs)
  for (const task of tasks) {
    store.upsertConversationTaskState(id, task)
  }
}, [])
```

- [ ] **Step 2：Commit**

```bash
git add src/hooks/useChat.ts
git commit -m "fix(chat): add version counter to switchConversation to prevent stale response overwrites"
```

---

### Task B3：乐观消息失败回滚 + `removeMessage` 方法

**Files:**
- Modify: `src/stores/sessionStore.ts`
- Modify: `src/hooks/useChat.ts`
- Modify: `src/hooks/useStreaming.ts`

- [ ] **Step 1：`SessionState` 新增 `removeMessage`**

在 `src/stores/sessionStore.ts` 的 `SessionState` 接口末尾追加：

```ts
removeMessage: (id: string) => void
```

在 `createSessionSlice` 中实现：

```ts
removeMessage: (id) =>
  set((state) => ({
    messages: state.messages.filter((m) => m.id !== id),
  })),
```

- [ ] **Step 2：IPC 失败时回滚**

在 `src/hooks/useChat.ts` 的 `sendUserMessage` 的 catch 块中（第 284-298 行附近），追加删除乐观消息：

```ts
} catch (error) {
  console.error('[sendUserMessage] IPC failed', error)
  useChatStore.getState().removeMessage(messageId)  // 回滚乐观消息
  useChatStore.getState().clearConversationStreamState(conversationId)
  useChatStore.getState().removeBusyConversation(conversationId)
}
```

- [ ] **Step 3：`streaming:error` 时也回滚**

在 `src/hooks/useStreaming.ts` 的 `streaming:error` handler 中，在清状态之后追加：

在 `streaming:error` handler 找到 `clearConversationStreamState` 调用处，在它之后加：

```ts
// 查找该会话尚未确认的乐观 user message（最后一条 role=user 且尚无 DB id 特征的消息）
// 简单策略：最后一条 role=user 消息如果 id 不以 'msg-' 开头（说明是前端生成的 optimistic id），则移除
const store = useChatStore.getState()
const lastUserMsg = [...store.messages].reverse().find(m => m.role === 'user')
if (lastUserMsg && !lastUserMsg.id.startsWith('msg-')) {
  store.removeMessage(lastUserMsg.id)
}
```

- [ ] **Step 4：Commit**

```bash
git add src/stores/sessionStore.ts src/hooks/useChat.ts src/hooks/useStreaming.ts
git commit -m "fix(chat): rollback optimistic user message on IPC failure or streaming error"
```

---

### Task B4：`UserBubble`/`AiBubble` 通过 props 发送消息

**Files:**
- Modify: `src/components/chat/MessageList.tsx`
- Modify: `src/components/chat/UserBubble.tsx`（或 `src/components/chat-scene/UserMessageBubble.tsx`，以实际路径为准）
- Modify: `src/components/chat/AiBubble.tsx`（或对应文件）

- [ ] **Step 1：先确认实际组件文件位置**

```bash
grep -r "invoke.*send_message\|sendMessage(" src/components --include="*.tsx" -l
```

记录输出的文件路径，以下步骤中替换为实际路径。

- [ ] **Step 2：修改 `UserBubble`（或对应组件）**

在组件 props 类型中增加 `onResend`：

```ts
interface UserBubbleProps {
  message: Message
  onResend?: (text: string, files?: PendingFileInfo[]) => void  // 新增
  // ...其他现有 props
}
```

找到直接调用 `sendMessage()` / `invoke('send_message')` 的地方，改为：

```ts
// 旧
await sendMessage(conversationId, text, fileIds)
// 新
onResend?.(text)
```

- [ ] **Step 3：修改 `AiBubble`**

同样增加 `onUserResponse` prop：

```ts
interface AiBubbleProps {
  message: Message
  onUserResponse?: (text: string) => void  // 新增
  // ...其他现有 props
}
```

把直接 IPC 调用改为 `onUserResponse?.(selectedText)`。

- [ ] **Step 4：在 `MessageList` 中传入 prop**

在 `MessageList.tsx` 中调用一次 `useChat()`，取出 `sendUserMessage`，传给各气泡：

```ts
const { sendUserMessage } = useChat()
// ...
<UserMessageBubble message={m} onResend={(text) => sendUserMessage(text)} />
<AiBubble message={m} onUserResponse={(text) => sendUserMessage(text)} />
```

- [ ] **Step 5：编译检查**

```bash
pnpm build 2>&1 | grep -E "^src.*error TS" | head -20
```

期望：0 TypeScript errors

- [ ] **Step 6：Commit**

```bash
git add src/components/chat/MessageList.tsx \
        src/components/chat/UserBubble.tsx \
        src/components/chat/AiBubble.tsx
git commit -m "fix(chat): route UserBubble/AiBubble resend through useChat.sendUserMessage via props"
```

---

## Phase 3：工具渲染补全（merge-by-toolCallId）

### Task C1：`ToolExecution` 增加 `output` 字段

**Files:**
- Modify: `src/stores/streamingStore.ts`

- [ ] **Step 1：扩展 `ToolExecution` 接口**

```ts
export interface ToolExecution {
  toolName: string
  toolId: string          // 实际值等同 toolCallId
  status: 'executing' | 'completed' | 'error'
  summary?: string
  startedAt?: number
  durationMs?: number
  input?: unknown
  output?: string         // 新增：tool:completed 后写入，来自 toolResult.content
}
```

- [ ] **Step 2：Commit**

```bash
git add src/stores/streamingStore.ts
git commit -m "feat(streaming): add output field to ToolExecution"
```

---

### Task C2：`RenderToolStep` 增加 `toolCallId`，重写 `buildTurnsFromMessages`

**Files:**
- Modify: `src/hooks/useTurnRenderModel.ts`
- Test: `src/hooks/__tests__/useTurnRenderModel.test.ts`

- [ ] **Step 1：写失败测试**

在 `src/hooks/__tests__/useTurnRenderModel.test.ts` 末尾追加：

```ts
import type { AssistantToolCall, ToolResultContent } from '@/types/message'

function assistantMsgWithToolCalls(id: string, toolCalls: AssistantToolCall[]): Message {
  return {
    id, conversationId: 'c1', role: 'assistant',
    createdAt: new Date().toISOString(),
    content: { text: '' },
    toolCalls,
  }
}

function toolResultMsg(id: string, toolResult: ToolResultContent): Message {
  return {
    id, conversationId: 'c1', role: 'tool',
    createdAt: new Date().toISOString(),
    content: { text: '' },
    toolResult,
  }
}

describe('buildTurnsFromMessages – toolCallId merge', () => {
  it('maps inputJson from assistant.toolCalls by toolCallId', () => {
    const msgs = [
      userMsg('u1', 'go'),
      assistantMsgWithToolCalls('a1', [
        { id: 'tc-1', name: 'run_python', arguments: { code: 'print(1)' } },
      ]),
      toolResultMsg('t1', { toolCallId: 'tc-1', name: 'run_python', content: '1\n', isError: false }),
    ]
    const turns = buildTurnsFromMessages(msgs, [])
    const step = turns[0].toolGroup?.steps[0]
    expect(step?.toolCallId).toBe('tc-1')
    expect(step?.inputJson).toContain('print(1)')
    expect(step?.output).toContain('1')
  })

  it('does not confuse same-name tools called twice', () => {
    const msgs = [
      userMsg('u1', 'go'),
      assistantMsgWithToolCalls('a1', [
        { id: 'tc-1', name: 'browse', arguments: { url: 'http://a.com' } },
        { id: 'tc-2', name: 'browse', arguments: { url: 'http://b.com' } },
      ]),
      toolResultMsg('t1', { toolCallId: 'tc-1', name: 'browse', content: 'page A', isError: false }),
      toolResultMsg('t2', { toolCallId: 'tc-2', name: 'browse', content: 'page B', isError: false }),
    ]
    const turns = buildTurnsFromMessages(msgs, [])
    const steps = turns[0].toolGroup?.steps ?? []
    expect(steps).toHaveLength(2)
    expect(steps.find(s => s.toolCallId === 'tc-1')?.output).toContain('page A')
    expect(steps.find(s => s.toolCallId === 'tc-2')?.output).toContain('page B')
  })

  it('error output preserved in step output', () => {
    const msgs = [
      userMsg('u1', 'go'),
      assistantMsgWithToolCalls('a1', [{ id: 'tc-1', name: 'run_python', arguments: {} }]),
      toolResultMsg('t1', { toolCallId: 'tc-1', name: 'run_python', content: 'Traceback...\nValueError: bad', isError: true }),
    ]
    const turns = buildTurnsFromMessages(msgs, [])
    const step = turns[0].toolGroup?.steps[0]
    expect(step?.status).toBe('error')
    expect(step?.output).toBeDefined()
  })
})
```

- [ ] **Step 2：运行确认失败**

```bash
pnpm exec vitest run src/hooks/__tests__/useTurnRenderModel.test.ts 2>&1 | tail -20
```

期望：新增的 3 个测试 FAIL

- [ ] **Step 3：扩展 `RenderToolStep`**

```ts
export interface RenderToolStep {
  index: number
  toolCallId: string      // 新增：唯一 key
  name: string
  status: 'running' | 'done' | 'error'
  durationMs?: number
  inputJson?: string
  output?: ReactNode
}
```

- [ ] **Step 4：重写 `buildTurnsFromMessages` 工具消息处理段**

替换 `role === 'assistant'` 和 `role === 'tool'` 以及实时覆盖段（约第 100-165 行）：

```ts
// role === 'assistant'
if (m.role === 'assistant') {
  if (m.toolCalls && m.toolCalls.length > 0) {
    if (!current.toolGroup) {
      current.toolGroup = { status: 'running', steps: [], durationMs: 0 }
    }
    // 为每个 toolCall 初始化 step（按 toolCallId，不按 name）
    for (let i = 0; i < m.toolCalls.length; i++) {
      const tc = m.toolCalls[i]
      const existing = current.toolGroup.steps.find(s => s.toolCallId === tc.id)
      if (!existing) {
        current.toolGroup.steps.push({
          index: current.toolGroup.steps.length + 1,
          toolCallId: tc.id,
          name: tc.name,
          status: 'running',
          inputJson: tc.arguments != null
            ? JSON.stringify(tc.arguments, null, 2)
            : undefined,
        })
      }
    }
  }
  if (m.content.text) {
    current.aiSegments.push({ id: m.id, message: m })
  }
  if (m.content.generatedFiles?.length) {
    // ... 保持现有 normalizeGeneratedFile 逻辑不变
  }
}

// role === 'tool'
if (m.role === 'tool' && m.toolResult) {
  if (!current.toolGroup) {
    current.toolGroup = { status: 'running', steps: [], durationMs: 0 }
  }
  const result = m.toolResult
  const existing = current.toolGroup.steps.find(s => s.toolCallId === result.toolCallId)
  if (existing) {
    // 已有 step（来自 assistant.toolCalls），补充 output/status/durationMs
    existing.status = result.isError ? 'error' : 'done'
    existing.output = result.content ? truncateOutput(result.content, result.isError) : undefined
    existing.durationMs = result.durationMs
  } else {
    // 历史路径没有 assistant.toolCalls（旧数据），新建 step
    current.toolGroup.steps.push({
      index: current.toolGroup.steps.length + 1,
      toolCallId: result.toolCallId,
      name: result.name,
      status: result.isError ? 'error' : 'done',
      output: result.content ? truncateOutput(result.content, result.isError) : undefined,
      durationMs: result.durationMs,
    })
  }
  current.toolGroup.durationMs += result.durationMs ?? 0
}
```

实时 `toolExecutions` 覆盖段改为 merge 而非 override（约第 140-157 行）：

```ts
// 实时 toolExecutions merge：用 toolId 补充或覆盖历史 steps 的 status/durationMs/output
if (toolExecutions.length > 0 && current.toolGroup) {
  for (const t of toolExecutions) {
    const step = current.toolGroup.steps.find(s => s.toolCallId === t.toolId)
    if (step) {
      // 实时状态优先（正在 running 时不用 done 覆盖）
      step.status = toolExecStatusToStep(t.status)
      if (t.durationMs != null) step.durationMs = t.durationMs
      if (t.input != null && !step.inputJson) {
        step.inputJson = JSON.stringify(t.input, null, 2)
      }
      if (t.output && !step.output) {
        step.output = truncateOutput(t.output, false)
      }
    }
    // 若没有历史 step 则新建（纯实时路径）
    if (!step) {
      current.toolGroup.steps.push({
        index: current.toolGroup.steps.length + 1,
        toolCallId: t.toolId,
        name: t.toolName,
        status: toolExecStatusToStep(t.status),
        durationMs: t.durationMs,
        inputJson: t.input != null ? JSON.stringify(t.input, null, 2) : undefined,
        output: t.output ? truncateOutput(t.output, false) : undefined,
      })
    }
  }
}
```

新增 `truncateOutput` helper（放在文件末尾或 helpers 段）：

```ts
function truncateOutput(text: string, isError: boolean, maxLines = 20): string {
  const lines = text.split('\n')
  if (lines.length <= maxLines) return text
  if (isError) {
    // error：保留尾部（traceback 关键信息在末尾）
    return `…（共 ${lines.length} 行，已截断）\n` + lines.slice(-maxLines).join('\n')
  }
  return lines.slice(0, maxLines).join('\n') + `\n…（共 ${lines.length} 行，已截断）`
}
```

- [ ] **Step 5：运行测试**

```bash
pnpm exec vitest run src/hooks/__tests__/useTurnRenderModel.test.ts 2>&1 | tail -20
```

期望：全部 pass（包含原有 4 个 + 新增 3 个）

- [ ] **Step 6：Commit**

```bash
git add src/hooks/useTurnRenderModel.ts src/hooks/__tests__/useTurnRenderModel.test.ts
git commit -m "feat(render): rewrite buildTurnsFromMessages to merge tool steps by toolCallId with inputJson/output"
```

---

## Phase 4：LLM retry reset + 类型清理

### Task D1：LLM retry 前发 `streaming:retry-reset` 事件

**Files:**
- Modify: `src-tauri/src/runtime/events.rs`
- Modify: `src-tauri/src/runtime/chat/chat_turn_driver.rs`（run_llm_step 的 retry 分支）
- Modify: `src-tauri/src/transport/tauri_event_adapter.rs`
- Modify: `src/lib/tauri.ts`
- Modify: `src/hooks/useStreaming.ts`

- [ ] **Step 1：后端 `RuntimeEventKind` 新增 `StreamRetryReset` variant**

```rust
// src-tauri/src/runtime/events.rs
pub enum RuntimeEventKind {
    // ... 现有 variants ...
    StreamRetryReset,  // 新增：LLM 流 retry 前发出，通知前端清空 streamingContent
}
```

- [ ] **Step 2：`tauri_event_adapter` 映射新 variant**

```rust
RuntimeEventKind::StreamRetryReset => Some(LegacyEvent {
    name: "streaming:retry-reset".to_string(),
    payload: json!({
        "conversationId": conversation_id,
        "runId": event.run_id.as_str(),
    }),
}),
```

- [ ] **Step 3：在 `run_llm_step` retry 分支前 emit**

在 `src-tauri/src/transport/tauri_commands/chat.rs` 中找 chunk_timeout retry（`iter_content.clear()` 处），在 `iter_content.clear()` 之前 emit reset 事件：

```rust
_ = chunk_timeout => {
    if stream_retry_count < MAX_STREAM_RETRIES {
        stream_retry_count += 1;
        // 通知前端清空已显示的流式内容，避免重试后重复
        let _ = bus.emit(RuntimeEvent::new(
            session_id.clone(),
            run_id.clone(),
            RuntimeEventKind::StreamRetryReset,
        )).await;
        iter_content.clear();
        tool_calls.clear();
        stream_needs_retry = true;
        break;
    }
}
```

stream error retry 分支同样处理：

```rust
if stream_retry_count < MAX_STREAM_RETRIES {
    stream_retry_count += 1;
    let _ = bus.emit(RuntimeEvent::new(
        session_id.clone(),
        run_id.clone(),
        RuntimeEventKind::StreamRetryReset,
    )).await;
    iter_content.clear();
    tool_calls.clear();
    stream_needs_retry = true;
    break;
}
```

- [ ] **Step 4：前端 `tauri.ts` 新增常量和类型**

```ts
export const TAURI_EVENTS = {
  // ... 现有 ...
  STREAMING_RETRY_RESET: 'streaming:retry-reset',  // 新增
} as const

export interface StreamingRetryResetPayload {
  conversationId: string
  runId?: string
}

export function onStreamingRetryReset(
  handler: (payload: StreamingRetryResetPayload) => void,
): Promise<() => void> {
  return listen<StreamingRetryResetPayload>(TAURI_EVENTS.STREAMING_RETRY_RESET, (event) => {
    handler(event.payload)
  })
}
```

- [ ] **Step 5：前端 `useStreaming.ts` 新增 handler**

在 `useStreaming.ts` 中的其他 `useTauriEvent` 块旁边添加：

```ts
useTauriEvent(() =>
  onStreamingRetryReset(({ conversationId }) => {
    console.log('[streaming:retry-reset]', conversationId)
    // 清空 delta buffer 和 streamingContent，前端从头接新流
    delete deltaBufferRef.current[conversationId]
    useChatStore.getState().resetConversationStreamContent(conversationId)
  }),
)
```

- [ ] **Step 6：编译验证**

```bash
cd src-tauri && cargo build 2>&1 | grep "^error" | head -10
pnpm build 2>&1 | grep "error TS" | head -10
```

期望：0 errors

- [ ] **Step 7：Commit**

```bash
git add src-tauri/src/runtime/events.rs \
        src-tauri/src/runtime/chat/chat_turn_driver.rs \
        src-tauri/src/transport/tauri_event_adapter.rs \
        src/lib/tauri.ts \
        src/hooks/useStreaming.ts
git commit -m "fix(streaming): emit StreamRetryReset before LLM retry to prevent duplicate content on frontend"
```

---

### Task D2：类型契约清理（P2 全部）

**Files:**
- Modify: `src/lib/tauri.ts`
- Modify: `src/hooks/useStreaming.ts`

- [ ] **Step 1：清理 `tauri.ts` 中的虚假字段和空订阅常量**

```ts
// 1. StreamingDonePayload 删除 messageId（已在 Task A3 完成，此处确认）
// 2. StreamingErrorPayload 删除后端不发的字段
export interface StreamingErrorPayload {
  conversationId: string
  error: string
  rawError?: string
  // 删除：errorType, partialContent, timeoutSeconds, iteration, maxIterations
}

// 3. TAURI_EVENTS 新增 stop:prevented-continuation（若后端已映射）
STOP_PREVENTED_CONTINUATION: 'stop:prevented-continuation',

// 4. 可选：标记或注释掉空订阅常量（不删除，避免编译错误，只加注释说明未使用）
/** @deprecated 后端不发送此事件 */
FILE_PARSED: 'file:parsed',
```

- [ ] **Step 2：`useStreaming.ts` 新增 `stop:prevented-continuation` 订阅**

```ts
useTauriEvent(() =>
  listen<{ conversationId: string; reason?: string }>(
    TAURI_EVENTS.STOP_PREVENTED_CONTINUATION,
    (event) => {
      const { conversationId } = event.payload
      console.warn('[stop:prevented-continuation]', conversationId)
      useChatStore.getState().clearConversationStreamState(conversationId)
      useChatStore.getState().removeBusyConversation(conversationId)
      // 可选：显示 toast 提示
    },
  ),
)
```

- [ ] **Step 3：`streaming:error` handler 不再依赖 `errorType`**

在 `useStreaming.ts` 中找 `streaming:error` handler，把依赖 `errorType` 的分支（如 auto-hide 时长）改为根据 `rawError` 字符串推断，或统一使用默认时长：

```ts
onStreamingError(({ conversationId, error, rawError }) => {
  // 不再依赖 errorType（后端不发）；根据 rawError 推断
  const isTimeout = rawError === 'chunk_timeout' || rawError === 'agent_timeout'
  const autoHideSecs = isTimeout ? 10 : 5
  // ... 其余保持不变
})
```

- [ ] **Step 4：编译验证**

```bash
pnpm build 2>&1 | grep "error TS" | head -10
```

期望：0 TypeScript errors

- [ ] **Step 5：Commit**

```bash
git add src/lib/tauri.ts src/hooks/useStreaming.ts
git commit -m "fix(types): clean up phantom payload fields and wire stop:prevented-continuation"
```

---

## 验证矩阵

运行以下命令确认整体无回归：

```bash
# 前端测试
pnpm exec vitest run src/hooks/__tests__/useTurnRenderModel.test.ts \
                     src/hooks/useStreaming.integration.test.tsx \
                     src/lib/tauri.events.test.ts 2>&1 | tail -15

# 后端核心测试
cd src-tauri && cargo test \
  --test review_backend_event_payload_test \
  --test message_storage_v2_test \
  --test history_rebuild_test \
  --test review_chat_history_persistence_test \
  -- --nocapture 2>&1 | tail -20

# Rust 编译检查
cd src-tauri && cargo build 2>&1 | grep "^error" | head -10
```

期望：全部 pass，0 errors。
