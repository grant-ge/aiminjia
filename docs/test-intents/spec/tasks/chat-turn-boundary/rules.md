# rules.md — chat-turn-boundary Chat Turn 主循环边界测试意图

来源：[LUT-5](mention://issue/646d0af5-f4ca-4a90-9774-ba041ca55a23)

涉及核心模块：`runtime/chat/chat_turn_driver.rs`、`runtime/chat/turn_outcome.rs`、`llm/context_decay.rs`（`CONTEXT_OVERFLOW_THRESHOLD = 0.8`）

`ChatTurnOutcome` 序列化标签（无 `rename_all`，直接用变体名）：
- 成功：`"Success"`
- 取消：`"Cancelled"`
- 达到最大迭代：`"MaxIterationsReached"`（含 `iterations` 字段）
- 预算超限：`"BudgetExceeded"`（含 `reason`、`total_cost_usd`）
- 执行错误：`"ExecutionError"`（含 `message`）

---

## 意图 1：达到 max_iterations 上限时 TurnCompleted 的 outcome 为 MaxIterationsReached

**场景**
LLM 持续返回 ToolCalls 不给 ContentComplete，达到 `max_iterations` 时 turn 应主动终止，`TurnCompleted` 的 outcome 携带迭代次数。

**前提**
- 使用 `RuntimeChatTurnDriver::with_llm_executor(QueryEngine::default(), RuntimeEventBus::new(), executor)`
- `TurnConfigOverrides { max_iterations: Some(2), ..Default::default() }` 注入
- MockLlmExecutor 每轮返回 `LlmStepResult::ToolCalls { tool_calls: [{ name: "dummy_tool", input: {} }], .. }`，永不返回 ContentComplete
- 注册名为 `"dummy_tool"` 的 Mock RuntimeTool，始终返回 `Ok("done")`

**操作**
1. 调用 `driver.run_chat_turn(&mut state, &request)` 并等待完成
2. 从 EventBus 收集所有事件，找到 `TurnCompleted` 事件

**断言**
- `TurnCompleted` 事件存在
- `event.outcome` 序列化为 `{"MaxIterationsReached": {"iterations": 2}}`
- `ToolCallExecuting` 事件恰好出现 2 次
- `AgentIdle` 是最后一个事件

---

## 意图 2：CancellationToken 触发后 TurnCompleted 的 outcome 为 Cancelled

**场景**
用户点击停止时，turn 应安全退出，`TurnCompleted` 的 outcome 为 `Cancelled`，不挂起。

**前提**
- 同意图 1 构造 driver，不注入 max_iterations 限制
- MockLlmExecutor 第 1 轮：在执行中等待 cancel 信号，收到后返回 `LlmStepResult::Cancelled`
- turn 开始后 10ms，通过 `cancel_token.cancel_with_reason(CancellationReason::UserCancel)` 触发取消

**操作**
1. 同时启动 `run_chat_turn` 和 10ms 后触发取消的异步任务
2. 等待 `run_chat_turn` 返回（设 5s 超时）

**断言**
- `run_chat_turn` 在 5s 内返回（不挂起）
- `TurnCompleted` 事件的 `outcome` 序列化值等于 `"Cancelled"`
- `AgentIdle` 事件存在

---

## 意图 3：正常 turn 完成后 TurnCompleted 的 outcome 为 Success

**场景**
LLM 正常回复文本，turn 以 Success 结束。

**前提**
- MockLlmExecutor 预设：第 1 轮返回 `LlmStepResult::ContentComplete { content: "你好！", .. }`
- 不注册任何工具

**操作**
1. 调用 `run_chat_turn(&mut state, &request)` 并等待完成

**断言**
- `TurnCompleted` 事件的 `outcome` 序列化值等于 `"Success"`
- `StreamDone` 在 `TurnCompleted` 之前出现
- `AgentIdle` 是最后一个事件

---

## 意图 4：上下文 token 数超过 CONTEXT_OVERFLOW_THRESHOLD（80%）时触发 compaction

**场景**
`CONTEXT_OVERFLOW_THRESHOLD = 0.8`，当预估 token 数超过 context_window × 0.8 时，preprocess 触发 compaction。

**前提**
- 使用 `RuntimeChatTurnDriver::with_compact_client(mock_compact_client)`
- 在 AppStorage 中预置超过当前 provider context_window × 80% token 的历史消息
- MockCompactClient 接收到调用后返回压缩摘要 `"compressed summary"`

**操作**
1. 调用 `run_chat_turn` 并等待完成
2. 检查 MockCompactClient 是否被调用

**断言**
- MockCompactClient 的 `compact_summary` 被调用恰好 1 次
- EventBus 中出现 `TurnStageChanged`，其 `stage.kind` 序列化值为 `"compacting"`
- turn 最终以 `"Success"` outcome 完成（compaction 后对话继续）

---

## 意图 5：上下文 token 数未超过阈值时不触发 compaction

**场景**
短对话不应触发 compaction，避免不必要的 LLM 调用。

**前提**
- 同意图 4 构造，但历史消息 token 数明显低于 context_window × 80%（如只有 2 条短消息）

**操作**
1. 调用 `run_chat_turn` 并等待完成

**断言**
- MockCompactClient 的 `compact_summary` 未被调用（调用次数为 0）
- turn 以 `"Success"` 完成

---

## 意图 6：LLM 工具调用一轮后继续 turn，最终以 Success 完成

**场景**
工具调用是 agentic turn 核心路径：第 1 轮 ToolCalls，第 2 轮 ContentComplete，turn 应正常走完两轮。

**前提**
- 注册 `"dummy_tool"`，始终返回 `Ok("工具结果")`
- MockLlmExecutor：第 1 轮返回 `ToolCalls { tool_calls: [{ name: "dummy_tool", input: {} }], .. }`，第 2 轮返回 `ContentComplete { content: "分析完成", .. }`

**操作**
1. 调用 `run_chat_turn` 并等待完成

**断言**
- `ToolCallExecuting` 事件恰好出现 1 次
- `ToolCallCompleted` 事件恰好出现 1 次
- `TurnCompleted` 的 `outcome` 序列化值等于 `"Success"`
- `AgentIdle` 是最后一个事件
