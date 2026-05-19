# rules.md — streaming-e2e 流式输出端到端测试意图

来源：[LUT-7](mention://issue/77843043-e831-4c28-90be-7feced8a9fcc)

验证方式：cargo test（使用 MockLlmExecutor + AppStorage TempDir，不调真实 provider）

---

## 意图 1：正常 turn 完成后 EventBus 包含 StreamStarted → StreamDelta → StreamDone 且顺序正确

**场景**
用户发消息，LLM 流式回复，前端通过 EventBus 逐字追加显示。三个阶段事件必须按顺序出现，顺序错误会导致前端状态机卡死。

**前提**
- MockLlmExecutor：第 1 轮返回 `ContentComplete { content: "你好，我是助手", .. }`

**操作**
1. 调用 `run_chat_turn` 并等待完成
2. 从 EventBus 收集事件，提取 kind 标签序列

**断言**
- 事件序列中包含 `StreamStarted`，至少 1 个 `StreamDelta`，`StreamDone`，且按此顺序
- 所有 `StreamDelta` 事件的 `content` 字段按顺序拼接后等于 `"你好，我是助手"`
- `StreamDone` 在所有 `StreamDelta` 之后出现

---

## 意图 2：正常 turn 完成后 MessagePersisted 包含完整 assistant 消息，出现在 StreamDone 之后

**场景**
流式结束后，消息必须被持久化并通知前端，`MessagePersisted` 让前端将流式气泡转为正式消息记录。

**前提**
- 同意图 1

**操作**
1. 调用 `run_chat_turn` 并等待完成
2. 从 EventBus 找到 `MessagePersisted` 事件（`role == "assistant"`）

**断言**
- `MessagePersisted` 事件存在，`role` 字段等于 `"assistant"`
- `content.text` 字段等于 `"你好，我是助手"`
- `message_id` 字段非空
- `MessagePersisted` 出现在 `StreamDone` 之后

---

## 意图 3：正常 turn 完成后 assistant 消息写入存储，get_messages 可读取

**场景**
消息不仅存在于 EventBus，还必须持久化到磁盘。重启后历史消息可读，依赖此保证。

**前提**
- 使用 `TempDir` + `AppStorage::new` 创建隔离存储，conversation_id 为 `"conv-stream-test"`
- MockLlmExecutor：返回 `ContentComplete { content: "你好", .. }`

**操作**
1. 调用 `run_chat_turn` 并等待完成
2. 调用 `storage.get_messages("conv-stream-test")`

**断言**
- 返回消息列表长度为 2
- 第 1 条 `role == "user"`，`content["text"] == "用户发送的消息内容"`（与 request.content 一致）
- 第 2 条 `role == "assistant"`，`content["text"] == "你好"`

---

## 意图 4：流式输出中途取消，已输出内容被持久化，StreamDone 不出现

**场景**
用户在 AI 流式输出一半时停止，已到达的内容不能丢失，同时 `StreamDone` 不应出现（流未正常完成）。

**前提**
- MockLlmExecutor：先发出部分内容 delta，收到 cancel 后返回 `Ok(LlmStepResult::Cancelled)`
- 使用 TempDir + AppStorage
- turn 开始后 50ms 触发取消

**操作**
1. 启动 `run_chat_turn`，50ms 后触发 `cancel_token.cancel_with_reason(CancellationReason::UserCancel)`
2. 等待返回后调用 `storage.get_messages(conversation_id)`

**断言**
- EventBus 中 `TurnCompleted` 的 `outcome` 序列化值等于 `"Cancelled"`
- EventBus 中 `StreamDone` **不出现**
- EventBus 中 `AgentIdle` 出现
- `storage.get_messages` 返回列表中 assistant 消息的 `content["text"]` 不为空字符串（已输出内容被保存）

---

## 意图 5：同一 turn 内所有 streaming 事件携带相同的 run_id

**场景**
前端通过 `runId` 将流式 delta 归属到正确的 turn，run_id 不一致会导致内容追加到错误位置。

**前提**
- request 中 `run_id = RunId::new("run-stream-001")`
- MockLlmExecutor：返回 `ContentComplete { content: "ABC", .. }`

**操作**
1. 调用 `run_chat_turn` 并等待完成
2. 从 EventBus 收集所有 `StreamStarted`、`StreamDelta`、`StreamDone`、`TurnCompleted` 事件的 `run_id` 字段

**断言**
- 所有上述事件的 `run_id` 字段均等于 `"run-stream-001"`
