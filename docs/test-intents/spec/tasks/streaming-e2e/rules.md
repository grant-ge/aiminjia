# rules.md — streaming-e2e 流式输出端到端测试意图

来源：[LUT-7](mention://issue/77843043-e831-4c28-90be-7feced8a9fcc)

涉及核心模块：`runtime/chat/chat_turn_driver.rs`（流式推送）、`runtime/events.rs`（`StreamStarted` / `StreamDelta` / `StreamDone` / `MessagePersisted`）、`storage/file_store/messages.rs`（消息落盘）

`RuntimeEventKind` 关键变体（序列化用 `camelCase` tag）：
- `StreamStarted`
- `StreamDelta { content: String }`
- `StreamDone`
- `MessagePersisted { message_id: String, role: String, content: serde_json::Value }`

---

## 意图 1：正常 turn 完成后 EventBus 包含 StreamStarted → StreamDelta → StreamDone 且顺序正确

**场景**
用户发消息后，LLM 流式回复经过三个阶段：流开始、内容增量、流结束。顺序错误会导致前端状态机卡死。

**前提**
- MockLlmExecutor 预设：第 1 轮返回 `ContentComplete { content: "Hello World", .. }`
- 不注册任何工具

**操作**
1. 调用 `driver.run_chat_turn(&mut state, &request)` 并等待完成
2. 从 EventBus 收集事件，提取 kind 标签序列

**断言**
- 事件序列中包含 `StreamStarted`、至少 1 个 `StreamDelta`、`StreamDone`（按此顺序出现）
- `StreamStarted` 在所有 `StreamDelta` 之前出现
- `StreamDone` 在所有 `StreamDelta` 之后出现
- `StreamDelta` 事件的 `content` 字段拼接后等于 `"Hello World"`

---

## 意图 2：正常 turn 完成后 MessagePersisted 事件包含完整 assistant 消息

**场景**
流式结束后，assistant 消息应被持久化，`MessagePersisted` 事件携带完整内容。

**前提**
- 同意图 1

**操作**
1. 调用 `run_chat_turn` 并等待完成
2. 从 EventBus 找到 `MessagePersisted` 事件（role 为 "assistant"）

**断言**
- `MessagePersisted` 事件存在，且 `role == "assistant"`
- `content` 字段（`serde_json::Value`）中 `text` 字段值等于 `"Hello World"`
- `message_id` 字段非空字符串
- `MessagePersisted` 出现在 `StreamDone` 之后

---

## 意图 3：正常 turn 完成后 assistant 消息写入存储文件

**场景**
流式结束后，消息不仅发布 EventBus 事件，还应持久化到 `messages.jsonl`，重启后可读。

**前提**
- 使用 `TempDir` + `AppStorage::new` 创建隔离存储
- MockLlmExecutor 返回 `ContentComplete { content: "你好", .. }`

**操作**
1. 调用 `run_chat_turn` 并等待完成
2. 调用 `storage.get_messages(conversation_id)`

**断言**
- 返回消息列表长度为 2（user + assistant）
- assistant 消息 `content["text"] == "你好"`
- user 消息 `role == "user"`
- assistant 消息 `role == "assistant"`

---

## 意图 4：取消后已输出的流式内容被持久化，未输出的内容不出现

**场景**
用户在流式输出中途停止，已到达的 delta 内容应被保存，不丢失已输出部分。

**前提**
- MockLlmExecutor：开始输出 `"Hello "` 后（发出一个 StreamDelta），收到 cancel 信号，返回 `Cancelled`
- 取消在 turn 开始后 50ms 触发（`cancel_with_reason(CancellationReason::UserCancel)`）

**操作**
1. 同时启动 `run_chat_turn` 和 50ms 后触发取消的异步任务
2. 等待 `run_chat_turn` 返回
3. 调用 `storage.get_messages(conversation_id)`

**断言**
- `TurnCompleted` 的 `outcome` 序列化值等于 `"Cancelled"`
- 存储中 assistant 消息的 `content["text"]` 包含已输出的内容（不为空字符串）
- 存储中 assistant 消息内容**不包含**取消后 LLM 未输出的内容

---

## 意图 5：取消后 StreamDone 不出现，TurnCompleted 在 AgentIdle 之前出现

**场景**
流式取消时不应发出 `StreamDone`（因为流未正常完成），但 `TurnCompleted` 和 `AgentIdle` 必须发出以通知前端。

**前提**
- 同意图 4（取消场景）

**操作**
1. 同时启动 `run_chat_turn` 和取消任务，等待完成
2. 从 EventBus 收集所有事件的 kind 标签序列

**断言**
- `StreamDone` 不出现在事件序列中
- `TurnCompleted` 出现
- `AgentIdle` 出现
- `TurnCompleted` 在 `AgentIdle` 之前出现

---

## 意图 6：同一 turn 内所有 StreamDelta 和 StreamDone 事件携带相同的 run_id

**场景**
前端通过 `runId` 将流式 delta 归属到正确的 turn，run_id 不一致会导致内容追加到错误位置。

**前提**
- MockLlmExecutor 返回多个 delta：`ContentComplete { content: "ABC", .. }`（驱动器会拆成多个 delta）
- request 中 `run_id = RunId::new("run-test-001")`

**操作**
1. 调用 `run_chat_turn` 并等待完成
2. 从 EventBus 提取所有 `StreamDelta` 和 `StreamDone` 事件的 `run_id` 字段

**断言**
- 所有 `StreamDelta` 事件的 `run_id` 均等于 `"run-test-001"`
- `StreamDone` 事件的 `run_id` 等于 `"run-test-001"`
- `TurnCompleted` 事件的 `run_id` 等于 `"run-test-001"`
