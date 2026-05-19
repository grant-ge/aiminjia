# rules.md — streaming-e2e 流式输出端到端测试意图

来源：[LUT-7](mention://issue/77843043-e831-4c28-90be-7feced8a9fcc)

---

## 意图 1：正常 turn 完成后 StreamDelta 拼接内容与 MessagePersisted 和存储消息完全一致

**场景**
用户发消息，LLM 流式回复，前端通过 EventBus 逐字追加显示。流式内容、持久化事件、磁盘存储三者必须完全一致，任何一处不一致都会导致用户看到的内容与实际存储的内容不同。

**前提**
- MockLlmExecutor 预设：返回 `ContentComplete { content: "你好，我是助手" }`
- 使用 TempDir + AppStorage，conversation_id 为 `"conv-stream-test"`
- 用户消息为 `"请介绍一下你自己"`

**操作**
- driver 执行一轮正常对话

**断言**

事件序列：
- EventBus 中 `StreamStarted` 出现在所有 `StreamDelta` 之前
- 所有 `StreamDelta` 事件的 `content` 字段按出现顺序拼接后等于 `"你好，我是助手"`
- `StreamDone` 出现在最后一个 `StreamDelta` 之后，在 `TurnCompleted` 之前
- `MessagePersisted` 的 `role` 为 `"assistant"`，`content.text` 等于 `"你好，我是助手"`
- `TurnCompleted` 的 `outcome` 序列化值为 `"Success"`
- `AgentIdle` 是 EventBus 中最后一个事件

存储状态：
- `storage.get_messages("conv-stream-test")` 返回列表长度为 2
- 第 1 条 `role` 为 `"user"`，`content["text"]` 为 `"请介绍一下你自己"`
- 第 2 条 `role` 为 `"assistant"`，`content["text"]` 为 `"你好，我是助手"`（与 StreamDelta 拼接结果完全一致）

---

## 意图 2：流式输出中途取消，已输出内容落盘，StreamDone 不出现，后续可发新消息

**场景**
用户在 AI 流式输出一半时停止，已到达的内容不能丢失，`StreamDone` 不应出现（流未正常完成），对话状态干净，下一条消息可正常发送。

**前提**
- MockLlmExecutor 预设：先发出 delta `"你好"`，检测到 cancel 后返回 `Cancelled`
- 使用 TempDir + AppStorage，conversation_id 为 `"conv-cancel-stream-test"`
- 用户消息为 `"请详细介绍"`
- turn 开始后 50ms 触发取消

**操作**
- driver 执行对话，50ms 后触发取消

**断言**

事件序列：
- EventBus 中 `TurnCompleted` 的 `outcome` 序列化值为 `"Cancelled"`
- EventBus 中 `StreamDone` 不出现
- EventBus 中最后一个事件为 `AgentIdle`

存储状态：
- `storage.get_messages("conv-cancel-stream-test")` 返回列表长度为 2
- assistant 消息 `content["text"]` 等于 `"你好"`（已流出内容被保存，不丢失）

后续可用性：
- 取消后用同一 conversation_id 再执行一轮，新 turn 的 `TurnCompleted` 的 `outcome` 序列化值为 `"Success"`

---

## 意图 3：同一 turn 内所有 streaming 事件 run_id 与 request 一致

**场景**
前端通过 `runId` 将流式 delta 归属到正确的 turn，run_id 不一致会导致内容追加到错误的对话气泡。

**前提**
- MockLlmExecutor 预设：返回 `ContentComplete { content: "ABC" }`
- request 中 `run_id` 为 `"run-stream-001"`

**操作**
- driver 执行一轮正常对话

**断言**

事件序列：
- EventBus 中所有 `StreamStarted`、`StreamDelta`、`StreamDone`、`TurnCompleted` 事件的 `run_id` 字段均等于 `"run-stream-001"`
- 不存在 `run_id` 不等于 `"run-stream-001"` 的上述事件
