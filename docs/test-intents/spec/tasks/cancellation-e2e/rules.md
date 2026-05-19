# rules.md — cancellation-e2e 取消语义端到端测试意图

来源：[LUT-6](mention://issue/61eb2d45-0626-4b6a-840e-0209133260d6)

---

## 意图 1：用户触发取消后，turn 正常结束，流式内容不丢失，对话后续可用

**场景**
用户点击「停止」，系统停止流式输出，已到达的内容被持久化，对话状态干净，下一条消息可以正常发送。

**前提**
- MockLlmExecutor 预设：先发出 delta `"你好"`，检测到 cancel 后返回 `Cancelled`
- 使用 TempDir + AppStorage，conversation_id 为 `"conv-cancel-test"`
- 用户消息内容为 `"请介绍一下自己"`
- turn 开始后 20ms 触发取消

**操作**
- driver 执行对话，20ms 后触发取消，等待 turn 返回

**断言**

事件序列：
- EventBus 中 `TurnCompleted` 的 `outcome` 序列化值为 `"Cancelled"`
- EventBus 中 `StreamDone` **不出现**（流未正常完成）
- EventBus 中最后一个事件为 `AgentIdle`，`scope` 序列化值为 `"primary"`
- `TurnCompleted` 出现在 `AgentIdle` 之前，两者之间无其他 `TurnCompleted`

存储状态：
- `storage.get_messages("conv-cancel-test")` 返回列表长度为 2（user + assistant 各 1 条）
- assistant 消息的 `role` 等于 `"assistant"`
- assistant 消息的 `content["text"]` 等于 `"你好"`（已流出的内容被保存，不丢失）
- user 消息的 `content["text"]` 等于 `"请介绍一下自己"`

后续可用性：
- 取消后用同一 conversation_id 再发一条消息 `"你好"`，新 turn 的 `TurnCompleted` 的 `outcome` 序列化值为 `"Success"`（不被取消状态阻塞）

---

## 意图 2：工具执行中途取消，工具调用有合成结果，不产生孤儿 ToolUse 记录

**场景**
工具正在执行时用户停止，已开始的工具调用必须有对应的 tool_result（合成取消结果），消息历史中不允许出现没有 tool_result 的 ToolUse 记录。

**前提**
- 注册 `"long_tool"`，执行时等待 cancel 信号
- MockLlmExecutor 预设：第 1 轮返回包含 `"long_tool"` 的 `ToolCalls`
- 工具开始执行后 30ms 触发取消

**操作**
- driver 执行对话，工具执行中途触发取消

**断言**

事件序列：
- EventBus 中 `TurnCompleted` 的 `outcome` 序列化值为 `"Cancelled"`
- EventBus 中 `ToolCallExecuting` 事件出现 1 次
- EventBus 中 `ToolCallCompleted` 事件出现 1 次，`is_error` 字段为 `true`
- `ToolCallCompleted` 出现在 `TurnCompleted` 之前

存储状态：
- `storage.get_messages` 返回的消息列表中，不存在没有对应 tool_result 的 assistant ToolUse 记录（消息历史闭合）

---

## 意图 3：连续两次触发取消，TurnCompleted 只出现一次，reason 为第一次的 UserCancel

**场景**
用户快速多次点击停止，系统不应 panic，第一次取消的 reason 应被保留，后续幂等忽略。

**前提**
- MockLlmExecutor 预设：检测到 cancel 后返回 `Cancelled`
- 20ms 后触发第一次取消（reason: `UserCancel`），立即再触发第二次（reason: `Interrupt`）

**操作**
- driver 执行对话，连续触发两次取消

**断言**

事件序列：
- EventBus 中 `TurnCompleted` 事件恰好出现 1 次
- `TurnCompleted` 的 `outcome` 序列化值为 `"Cancelled"`
- EventBus 中 `AgentIdle` 恰好出现 1 次

存储状态：
- `storage.get_messages` 返回列表长度为 2（user + assistant），数据完整不重复
