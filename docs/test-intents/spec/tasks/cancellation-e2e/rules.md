# rules.md — cancellation-e2e 取消语义端到端测试意图

来源：[LUT-6](mention://issue/61eb2d45-0626-4b6a-840e-0209133260d6)

---

## 意图 1：用户触发取消后已流出内容落盘，StreamDone 不出现，后续可发新消息

**场景**
用户点击停止，已到达的内容被持久化，StreamDone 不出现，对话状态干净，下一条消息可正常发送。

**前提**
- MockLlmExecutor 预设：先发出 delta `"你好"`，检测到 cancel 后返回 `Cancelled`
- 使用隔离存储，conversation_id 为 `"conv-cancel-test"`
- 用户消息为 `"请介绍一下自己"`
- turn 开始后 20ms 触发取消

**操作**
- driver 执行对话，20ms 后触发取消

**验收标准**
- EventBus 中 `TurnCompleted` 的 `outcome` 字段值为 `"Cancelled"`
- EventBus 中 `StreamDone` 不出现
- EventBus 中最后一个事件类型为 `AgentIdle`，`scope` 字段值为 `"primary"`
- `messages.jsonl` 存在，文件共 2 行，每行均为合法 JSON
- 第 1 行 `role` 字段值为 `"user"`，`content.text` 字段值为 `"请介绍一下自己"`
- 第 2 行 `role` 字段值为 `"assistant"`，`content.text` 字段值为 `"你好"`（已流出内容被保存，不丢失）
- 取消后用同一 conversation_id 再执行一轮，新 turn 的 `TurnCompleted` 的 `outcome` 字段值为 `"Success"`

---

## 意图 2：工具执行中途取消，工具调用有合成结果，messages.jsonl 中无孤儿 ToolUse 记录

**场景**
工具正在执行时用户停止，已开始的工具调用必须有对应的 tool_result（合成取消结果），消息文件中不允许出现没有 tool_result 的 ToolUse 记录。

**前提**
- 注册 `"long_tool"`，执行时等待 cancel 信号
- MockLlmExecutor 预设：第 1 轮返回包含 `"long_tool"` 的 `ToolCalls`
- 使用隔离存储，conversation_id 为 `"conv-tool-cancel-test"`
- 工具开始执行后 30ms 触发取消

**操作**
- driver 执行对话，工具执行中途触发取消

**验收标准**
- EventBus 中 `TurnCompleted` 的 `outcome` 字段值为 `"Cancelled"`
- EventBus 中 `ToolCallCompleted` 事件出现 1 次，`is_error` 字段值为 `true`
- `messages.jsonl` 存在，每行均为合法 JSON
- 文件中每条 `role` 为 `"assistant"` 且含 `tool_calls` 字段的记录，均有对应 `role` 为 `"tool"` 的记录（无孤儿 ToolUse）

---

## 意图 3：连续两次触发取消，messages.jsonl 中消息记录不重复，TurnCompleted 只出现一次

**场景**
用户快速多次点击停止，系统不应 panic，消息文件不产生重复记录，TurnCompleted 只出现一次。

**前提**
- MockLlmExecutor 预设：先发出 delta `"你好"`，检测到 cancel 后返回 `Cancelled`
- 使用隔离存储，conversation_id 为 `"conv-double-cancel-test"`
- 用户消息为 `"请介绍一下自己"`
- 20ms 后触发第一次取消，立即再触发第二次取消

**操作**
- driver 执行对话，连续触发两次取消

**验收标准**
- EventBus 中 `TurnCompleted` 事件恰好出现 1 次，`outcome` 字段值为 `"Cancelled"`
- EventBus 中 `AgentIdle` 事件恰好出现 1 次
- `messages.jsonl` 存在，文件共 2 行，每行均为合法 JSON
- 第 2 行 `role` 字段值为 `"assistant"`，`content.text` 字段值为 `"你好"`（不重复写入）
