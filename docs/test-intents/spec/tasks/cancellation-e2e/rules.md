# rules.md — cancellation-e2e 取消语义端到端测试意图

来源：[LUT-6](mention://issue/61eb2d45-0626-4b6a-840e-0209133260d6)

---

## 意图 1：用户触发取消后 TurnCompleted 的 outcome 为 Cancelled，AgentIdle 在 5s 内发出

**场景**
用户点击「停止」，系统应在合理时间内停止，发出 `TurnCompleted(Cancelled)` 告知前端解除 loading，发出 `AgentIdle` 告知系统空闲。

**前提**
- MockLlmExecutor 预设：检测到 cancel 信号后返回 `Cancelled`
- turn 开始后 20ms 触发取消

**操作**
- driver 开始执行对话，20ms 后触发取消

**断言**
- turn 在 5s 内结束，不挂起
- EventBus 中 `TurnCompleted` 的 `outcome` 序列化值为 `"Cancelled"`
- EventBus 中 `AgentIdle` 事件存在
- `TurnCompleted` 出现在 `AgentIdle` 之前

---

## 意图 2：工具执行中途取消，EventBus 中 ToolCallCompleted 存在且 is_error 为 true

**场景**
工具正在执行时用户停止，已开始的工具调用不应残留为没有结果的孤儿——系统注入合成的取消结果，标记为错误。

**前提**
- 注册 `"long_tool"`，执行时等待 cancel 信号再返回
- MockLlmExecutor 预设：第 1 轮返回包含 `"long_tool"` 的 `ToolCalls`
- 工具开始执行后 30ms 触发取消

**操作**
- driver 执行对话，工具执行中途触发取消

**断言**
- EventBus 中 `TurnCompleted` 的 `outcome` 序列化值为 `"Cancelled"`
- EventBus 中 `ToolCallCompleted` 事件存在
- `ToolCallCompleted` 事件的 `is_error` 字段为 `true`

---

## 意图 3：取消后立刻执行新 turn，新 turn 正常完成不受阻塞

**场景**
取消后系统状态干净，下一条消息能正常发起新 turn，不被残留状态阻塞。

**前提**
- turn 1：取消场景，同意图 1
- turn 2：MockLlmExecutor 预设返回 `ContentComplete { content: "好的" }`，使用新的 run_id

**操作**
- 执行 turn 1 并等待取消完成
- 立即执行 turn 2

**断言**
- turn 2 的 `TurnCompleted` 的 `outcome` 序列化值为 `"Success"`
- EventBus 中 turn 2 的 `MessagePersisted` 的 `content.text` 等于 `"好的"`

---

## 意图 4：连续两次触发取消，turn 正常结束，TurnCompleted 只出现一次

**场景**
用户快速多次点击停止，系统不应 panic 或重复结束，第一次取消生效，后续幂等忽略。

**前提**
- MockLlmExecutor 预设：检测到 cancel 信号后返回 `Cancelled`
- 准备连续两次取消：第 1 次原因为 `UserCancel`，第 2 次原因为 `Interrupt`，间隔 ≤ 10ms

**操作**
- driver 开始执行对话
- 20ms 后连续触发两次取消

**断言**
- turn 正常返回，不 panic
- EventBus 中 `TurnCompleted` 事件恰好出现 1 次
- `TurnCompleted` 的 `outcome` 序列化值为 `"Cancelled"`
