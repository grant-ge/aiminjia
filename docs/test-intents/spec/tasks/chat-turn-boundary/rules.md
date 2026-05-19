# rules.md — chat-turn-boundary Chat Turn 主循环边界测试意图

来源：[LUT-5](mention://issue/646d0af5-f4ca-4a90-9774-ba041ca55a23)

验证方式：cargo test（使用 MockLlmExecutor + TurnConfigOverrides，不调真实 provider）

---

## 意图 1：LLM 持续工具调用达到 max_iterations 时 TurnCompleted 的 outcome 为 MaxIterationsReached

**场景**
LLM 一直返回工具调用不给最终回复，达到 `max_iterations` 上限后 turn 主动终止，前端收到终止信号可展示「已达到最大轮次」提示。

**前提**
- `TurnConfigOverrides { max_iterations: Some(2), ..Default::default() }` 注入
- MockLlmExecutor 每轮返回 `LlmStepResult::ToolCalls { tool_calls: [{ name: "dummy_tool", .. }], .. }`，永不返回 ContentComplete
- 注册 `"dummy_tool"`，始终返回 `Ok("done")`

**操作**
1. 调用 `run_chat_turn` 并等待完成

**断言**
- `run_chat_turn` 返回 `Ok`
- EventBus 中 `TurnCompleted` 的 `outcome` 序列化值等于 `{"MaxIterationsReached":{"iterations":2}}`
- EventBus 中 `ToolCallExecuting` 事件恰好出现 2 次
- `AgentIdle` 是 EventBus 中最后一个事件

---

## 意图 2：用户取消后 TurnCompleted 的 outcome 为 Cancelled，AgentIdle 仍然发出

**场景**
用户点击「停止」，turn 应安全退出，前端收到 `TurnCompleted(Cancelled)` 解除 loading 状态，`AgentIdle` 告知系统空闲。

**前提**
- MockLlmExecutor：`run_llm_step` 在收到 cancel 信号后返回 `Ok(LlmStepResult::Cancelled)`
- turn 开始后 20ms 触发 `cancel_token.cancel_with_reason(CancellationReason::UserCancel)`

**操作**
1. 同时启动 `run_chat_turn` 和 20ms 后触发取消的异步任务，等待 `run_chat_turn` 返回（设 5s 超时）

**断言**
- `run_chat_turn` 在 5s 内返回���不挂起
- EventBus 中 `TurnCompleted` 的 `outcome` 序列化值等于 `"Cancelled"`
- EventBus 中 `AgentIdle` 事件存在

---

## 意图 3：正常 turn 完成后 TurnCompleted 的 outcome 为 Success，AgentIdle 是最后事件

**场景**
用户正常发消息，LLM 回复文本，turn 应以 Success 结束，EventBus 事件序列完整。

**前提**
- MockLlmExecutor：第 1 轮返回 `ContentComplete { content: "你好", .. }`
- 不注册任何工具

**操作**
1. 调用 `run_chat_turn` 并等待完成

**断言**
- EventBus 中 `TurnCompleted` 的 `outcome` 序列化值等于 `"Success"`
- `StreamDone` 出现在 `TurnCompleted` 之前
- `TurnCompleted` 出现在 `AgentIdle` 之前
- `AgentIdle` 的 `scope` 序列化值为 `"primary"`

---

## 意图 4：PromptTooLong 触发 compaction 后 turn 继续，EventBus 中出现 compacting 阶段事件

**场景**
对话历史过长超出上下文窗口，系统自动触发 compaction，压缩后继续执行，用户感受到 turn 成功完成，不是报错中止。

**前提**
- MockLlmExecutor：第 1 次 `run_llm_step` 返回 `Err(TurnError::PromptTooLong("too long".to_string()))`，第 2 次返回 `Ok(ContentComplete { content: "好的", .. })`
- MockCompactClient 返回摘要 `"历史已压缩"`

**操作**
1. 调用 `run_chat_turn` 并等待完成

**断言**
- EventBus 中出现 `TurnStageChanged`，`stage.kind` 序列化值为 `"compacting"`
- EventBus 中 `TurnCompleted` 的 `outcome` 序列化值等于 `"Success"`

---

## 意图 5：前一个 turn 报错，下一个 turn 正常执行不受影响

**场景**
前一次 LLM 报错，不应污染下一个 turn 的状态，用户重新发消息能正常收到回复。

**前提**
- 同一个 AppStorage 和 conversation_id
- turn 1：MockLlmExecutor 返回 `Err(TurnError::LlmError("error".to_string()))`
- turn 2：MockLlmExecutor 返回 `Ok(ContentComplete { content: "正常回复", .. })`

**操作**
1. 执行 turn 1，等待返回（预期 Err）
2. 执行 turn 2，等待完成

**断言**
- turn 2 的 `run_chat_turn` 返回 `Ok`
- EventBus 中 turn 2 的 `TurnCompleted` 的 `outcome` 序列化值等于 `"Success"`
- turn 2 的 `MessagePersisted` 事件中 `content.text` 等于 `"正常回复"`
