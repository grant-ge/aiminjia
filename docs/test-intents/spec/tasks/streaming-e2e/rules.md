# rules.md — streaming-e2e 流式输出端到端测试意图

来源：[LUT-7](mention://issue/77843043-e831-4c28-90be-7feced8a9fcc)

---

## 意图 1：正常 turn 完成后 EventBus 中 StreamStarted → StreamDelta → StreamDone 顺序正确

**场景**
用户发消息，LLM 流式回复，前端通过 EventBus 事件逐字追加显示。三个阶段事件必须按顺序出现，顺序错误会导致前端状态机卡死。

**前提**
- MockLlmExecutor 预设：返回 `ContentComplete { content: "你好，我是助手" }`

**操作**
- driver 执行一轮正常对话

**断言**
- EventBus 中包含 `StreamStarted`、至少 1 个 `StreamDelta`、`StreamDone`，且按此顺序出现
- 所有 `StreamDelta` 事件的 `content` 字段按出现顺序拼接后等于 `"你好，我是助手"`
- `StreamDone` 在所有 `StreamDelta` 之后出现

---

## 意图 2：正常 turn 完成后 MessagePersisted 包含完整 assistant 消息，出现在 StreamDone 之后

**场景**
流式结束后消息必须被持久化并通知前端，`MessagePersisted` 让前端将流式气泡转为正式消息记录。

**前提**
- MockLlmExecutor 预设：返回 `ContentComplete { content: "你好，我是助手" }`

**操作**
- driver 执行一轮正常对话

**断言**
- EventBus 中出现 `MessagePersisted` 事件，`role` 字段等于 `"assistant"`
- `MessagePersisted` 的 `content.text` 等于 `"你好，我是助手"`
- `message_id` 字段非空
- `MessagePersisted` 出现在 `StreamDone` 之后

---

## 意图 3：正常 turn 完成后 assistant 消息写入存储，get_messages 可读取

**场景**
消息不仅存在于 EventBus，还必须持久化到磁盘。重启后历史消息可读，依赖此保证。

**前提**
- 使用 `TempDir` + `AppStorage` 创��隔离存储，conversation_id 为 `"conv-stream-test"`
- MockLlmExecutor 预设：返回 `ContentComplete { content: "你好" }`
- 用户消息内容为 `"请问你好吗"`

**操作**
- driver 执行一轮正常对话

**断言**
- `storage.get_messages("conv-stream-test")` 返回长度为 2 的列表
- 第 1 条 `role` 等于 `"user"`，`content["text"]` 等于 `"请问你好吗"`
- 第 2 条 `role` 等于 `"assistant"`，`content["text"]` 等于 `"你好"`

---

## 意图 4：流式输出中途取消后 StreamDone 不出现，已输出内容被持久化

**场景**
用户在 AI 流式输出一半时停止，已到达的内容不能丢失，同时 `StreamDone` 不应出现（流未正常完成）。

**前提**
- MockLlmExecutor 预设：先发出部分 delta，收到 cancel 后返回 `Cancelled`
- 使用 TempDir + AppStorage
- turn 开始后 50ms 触发取消

**操作**
- driver 执行对话，50ms 后触发取消

**断言**
- EventBus 中 `TurnCompleted` 的 `outcome` 序列化值为 `"Cancelled"`
- EventBus 中 `StreamDone` 不出现
- EventBus 中 `AgentIdle` 出现
- `storage.get_messages` 返回列表中 assistant 消息的 `content["text"]` 不为空（已输出内容被保存）

---

## 意图 5：同一 turn 内所有 streaming 事件携带相同的 run_id

**场景**
前端通过 `runId` 将流式 delta 归属到正确的 turn，run_id 不一致会导致内容追加到错误位置。

**前提**
- MockLlmExecutor 预设：返回 `ContentComplete { content: "ABC" }`
- request 中 `run_id` 为 `"run-stream-001"`

**操作**
- driver 执行一轮正常对话

**断言**
- EventBus 中所有 `StreamStarted`、`StreamDelta`、`StreamDone`、`TurnCompleted` 事件的 `run_id` 字段均等于 `"run-stream-001"`
