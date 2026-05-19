# rules.md — cancellation-e2e 取消语义端到端测试意图

来源：[LUT-6](mention://issue/61eb2d45-0626-4b6a-840e-0209133260d6)

---

## 意图 1：用户点击停止后已流出内容落盘，对话后续可继续使用

**场景**
用户在 AI 流式输出过程中点击停止，已到达的内容被保存，对话状态干净，可以继续发新消息。

**前提**
- 使用有效 API key，新建对话
- 用户消息为 `"请详细介绍一下自己，至少写300字"`
- 确认 AI 已开始流式输出内容

**操作**
- 用户发送消息
- 在 AI 流式输出过程中点击「停止」按钮

**验收标准**
- EventBus 中 `TurnCompleted` 的 `outcome` 字段值为 `"Cancelled"`
- EventBus 中 `StreamDone` 不出现
- EventBus 中最后一个事件类型为 `AgentIdle`，`scope` 字段值为 `"primary"`
- `messages.jsonl` 存在，文件共 2 行，每行均为合法 JSON
- 第 1 行 `role` 字段值为 `"user"`，`content.text` 字段值为 `"请详细介绍一下自己，至少写300字"`
- 第 2 行 `role` 字段值为 `"assistant"`，`content.text` 不为空（已流出内容被保存）
- 停止后在同一对话发送 `"你好"`，新 turn 的 `TurnCompleted` 的 `outcome` 字段值为 `"Success"`

---

## 意图 2：工具执行过程中用户点击停止，消息历史中无孤儿 ToolUse 记录

**场景**
工具正在执行时用户停止，已开始的工具调用必须有对应的 tool_result，消息文件中不允许出现没有结果的孤儿工具调用记录。

**前提**
- 使用会触发工具调用的 prompt，且工具执行耗时较长
- 用户消息为 `"请帮我执行一个耗时的 bash 命令：sleep 30"`
- 确认工具已开始执行

**操作**
- 用户发送消息
- 在工具执行过程中点击「停止」按钮

**验收标准**
- EventBus 中 `TurnCompleted` 的 `outcome` 字段值为 `"Cancelled"`
- EventBus 中 `ToolCallCompleted` 事件出现 1 次，`is_error` 字段值为 `true`
- `messages.jsonl` 存在，每行均为合法 JSON
- 文件中每条 `role` 为 `"assistant"` 且含 `tool_calls` 字段的记录，均有对应 `role` 为 `"tool"` 的记录（无孤儿 ToolUse）

---

## 意图 3：连续快速点击停止两次，消息记录不重复，TurnCompleted 只出现一次

**场景**
用户快速多次点击停止，系统幂等处理，消息文件不产生重复记录，TurnCompleted 只出现一次。

**前提**
- 使用有效 API key，新建对话
- 用户消息为 `"请详细介绍一下自己，至少写300字"`
- 确认 AI 已开始流式输出

**操作**
- 用户发送消息
- 在 AI 流式输出过程中快速连续点击「停止」按钮两次（间隔 ≤ 200ms）

**验收标准**
- EventBus 中 `TurnCompleted` 事件恰好出现 1 次，`outcome` 字段值为 `"Cancelled"`
- EventBus 中 `AgentIdle` 事件恰好出现 1 次
- `messages.jsonl` 存在，文件共 2 行，每行均为合法 JSON
- 第 2 行 `role` 字段值为 `"assistant"`，`content.text` 不为空（不重复写入）
