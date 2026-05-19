# rules.md — cancellation-e2e 取消语义端到端测试意图

来源：[LUT-6](mention://issue/61eb2d45-0626-4b6a-840e-0209133260d6)

验证方式：cargo test（使用 MockLlmExecutor + CancellationToken，不调真实 provider）

---

## 意图 1：用户点击停止后 TurnCompleted 的 outcome 为 Cancelled，AgentIdle 在 5s 内发出

**场景**
用户在 turn 执行中点击「停止」，系统应在合理时间内停止，发出 `TurnCompleted(Cancelled)` 告知前端解除 loading，发出 `AgentIdle` 告知系统空闲。

**前提**
- MockLlmExecutor：`run_llm_step` 在检测到 cancel 后返回 `Ok(LlmStepResult::Cancelled)`
- 构造独立 `CancellationToken`，注入 turn

**操作**
1. 启动 `run_chat_turn`，同时 20ms 后调用 `cancel_token.cancel_with_reason(CancellationReason::UserCancel)`
2. 等待 `run_chat_turn` 返回，设 5s 超时

**断言**
- `run_chat_turn` 在 5s 内返回（不挂起）
- EventBus 中 `TurnCompleted` 的 `outcome` 序列化值等于 `"Cancelled"`
- EventBus 中 `AgentIdle` 事件存在
- `TurnCompleted` 在 `AgentIdle` 之前出现

---

## 意图 2：工具执行中途取消，turn 以 Cancelled 结束，不产生孤儿工具调用

**场景**
工具正在执行时用户停止，已开始的工具调用不应残留在消息历史中（不允许有没有对应 tool_result 的 ToolUse 记录）。

**前提**
- 注册 `"long_tool"`，执行时等待 cancel 信号再返回
- MockLlmExecutor：第 1 轮返回 `ToolCalls { tool_calls: [{ name: "long_tool" }], .. }`
- 工具开始执行后 30ms 触发取消

**操作**
1. 启动 `run_chat_turn`，同时 30ms 后触发取消
2. 等待 `run_chat_turn` 返回

**断言**
- EventBus 中 `TurnCompleted` 的 `outcome` 序列化值等于 `"Cancelled"`
- EventBus 中 `ToolCallCompleted` 事件存在（合成的取消结果被注入）
- `ToolCallCompleted` 事件的 `is_error` 字段为 `true`（工具被取消视为错误结果）

---

## 意图 3：取消后立刻发新消息，新 turn 正常执行不受阻塞

**场景**
取消后 `RuntimeRunRegistry` 清空，下一条消息能正常发起新 turn，不被残留状态阻塞。

**前提**
- turn 1：取消场景（同意图 1）
- turn 2：MockLlmExecutor 返回 `ContentComplete { content: "好的", .. }`，使用新的 run_id

**操作**
1. 执行 turn 1 并等待取消完成
2. 立即执行 turn 2，等待完成

**断言**
- turn 2 的 `run_chat_turn` 返回 `Ok`
- EventBus 中 turn 2 的 `TurnCompleted` 的 `outcome` 序列化值等于 `"Success"`

---

## 意图 4：连续两次触发取消，第二次取消幂等不产生额外错误

**场景**
用户快速多次点击停止，系统不应报错或 panic，第一次取消生效，第二次被幂等忽略。

**前提**
- 同意图 1 构造，turn 执行中
- 准备两次取消调用

**操作**
1. 启动 `run_chat_turn`
2. 20ms 后调用 `cancel_token.cancel_with_reason(CancellationReason::UserCancel)`（第一次）
3. 立即再调用一次 `cancel_token.cancel_with_reason(CancellationReason::Interrupt)`（第二次）
4. 等待 `run_chat_turn` 返回

**断言**
- `run_chat_turn` 正常返回，不 panic
- EventBus 中 `TurnCompleted` 的 `outcome` 序列化值等于 `"Cancelled"`
- EventBus 中 `TurnCompleted` 事件恰好出现 1 次（幂等，不重复）
