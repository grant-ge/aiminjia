# rules.md — chat-turn-boundary Chat Turn 主循环边界测试意图

来源：[LUT-5](mention://issue/646d0af5-f4ca-4a90-9774-ba041ca55a23)

---

## 意图 1：LLM 持续工具调用达到 max_iterations 时 TurnCompleted 的 outcome 为 MaxIterationsReached

**场景**
LLM 一直返回工具调用不给最终回复，达到上限后 turn 主动终止，前端可据此展示「已达到最大轮次」提示。

**前提**
- `TurnConfigOverrides { max_iterations: Some(2) }` 注入
- MockLlmExecutor 预设：每轮返回 `ToolCalls`，永不返回 `ContentComplete`
- 注册名为 `"dummy_tool"` 的 Mock 工具，始终返回成功

**操作**
- driver 执行对话，LLM 持续返回工具调用直到 max_iterations 触达

**断言**
- EventBus 中 `TurnCompleted` 的 `outcome` 序列化值为 `{"MaxIterationsReached":{"iterations":2}}`
- EventBus 中 `ToolCallExecuting` 事件恰好出现 2 次
- `AgentIdle` 是 EventBus 中最后一个事件

---

## 意图 2：用户取消后 TurnCompleted 的 outcome 为 Cancelled，AgentIdle 仍然发出

**场景**
用户点击「停止」，turn 安全退出，前端收到 `TurnCompleted(Cancelled)` 解除 loading，`AgentIdle` 告知系统空闲。

**前提**
- MockLlmExecutor 预设：检测到 cancel 信号后返回 `Cancelled`
- turn 开始后 20ms 触发取消

**操作**
- driver 开始执行对话，20ms 后用户触发取消

**断言**
- turn 在 5s 内结束，不挂起
- EventBus 中 `TurnCompleted` 的 `outcome` 序列化值为 `"Cancelled"`
- EventBus 中 `AgentIdle` 事件存在
- `TurnCompleted` 出现在 `AgentIdle` 之前

---

## 意图 3：正常 turn 完成后 TurnCompleted 的 outcome 为 Success，AgentIdle 是最后事件

**场景**
用户正常发消息，LLM 回复文本，turn 以 Success 结束，EventBus 事件序列完整。

**前提**
- MockLlmExecutor 预设：返回 `ContentComplete { content: "你好" }`
- 不注册任何工具

**操作**
- driver 执行一轮正常对话

**断言**
- EventBus 中 `TurnCompleted` 的 `outcome` 序列化值为 `"Success"`
- `StreamDone` 出现在 `TurnCompleted` 之前
- `TurnCompleted` 出现在 `AgentIdle` 之前
- `AgentIdle` 的 `scope` 序列化值为 `"primary"`

---

## 意图 4：上下文过长触发 compaction 后 turn 继续，EventBus 中出现 compacting 阶段

**场景**
对话历史超出上下文窗口，系统自动触发 compaction 压缩历史后继续执行，用户感受到 turn 成功，不是报错中止。

**前提**
- MockLlmExecutor 预设：第 1 次返回 PromptTooLong，第 2 次返回 `ContentComplete { content: "好的" }`
- MockCompactClient 预设：返回摘要 `"历史已压缩"`

**操作**
- driver 执行对话，触发 PromptTooLong 后自动 compaction 并重试

**断言**
- EventBus 中出现 `TurnStageChanged`，`stage.kind` 序列化值为 `"compacting"`
- EventBus 中 `TurnCompleted` 的 `outcome` 序列化值为 `"Success"`

---

## 意图 5：前一个 turn 报错，下一个 turn 正常执行不受影响

**场景**
前一次对话出错，不应污染下一次对话的状态，用户重新发消息能正常收到回复。

**前提**
- 同一 conversation_id
- turn 1：MockLlmExecutor 返回 `Err`（LLM 报错）
- turn 2：MockLlmExecutor 返回 `ContentComplete { content: "正常回复" }`

**操作**
- 先执行 turn 1（预期出错）
- 再执行 turn 2

**断言**
- turn 2 的 `TurnCompleted` 的 `outcome` 序列化值为 `"Success"`
- turn 2 的 `MessagePersisted` 事件中 `content.text` 等于 `"正常回复"`
