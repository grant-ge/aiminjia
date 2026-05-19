# rules.md — chat-turn-boundary Chat Turn 主循环边界测试意图

来源：[LUT-5](mention://issue/646d0af5-f4ca-4a90-9774-ba041ca55a23)

---

## 意图 1：LLM 持续工具调用达到 max_iterations 时 outcome 为 MaxIterationsReached，assistant 消息落盘

**场景**
LLM 一直返回工具调用不给最终回复，达到上限后 turn 主动终止，assistant 消息被持久化，前端可据此展示「已达到最大轮次」提示。

**前提**
- `TurnConfigOverrides { max_iterations: Some(2) }` 注入
- MockLlmExecutor 预设：每轮返回 `ToolCalls { tool_calls: [{ name: "dummy_tool" }] }`，永不返回 `ContentComplete`
- 注册 `"dummy_tool"`，始终返回成功
- 使用 TempDir + AppStorage，conversation_id 为 `"conv-maxiter-test"`

**操作**
- driver 执行对话，LLM 持续返回工具调用直到 max_iterations 触达

**断言**

事件序列：
- EventBus 中 `TurnCompleted` 的 `outcome` 序列化值为 `{"MaxIterationsReached":{"iterations":2}}`
- EventBus 中 `ToolCallExecuting` 事件恰好出现 2 次
- EventBus 中最后一个事件为 `AgentIdle`，`scope` 序列化值为 `"primary"`

存储状态：
- `storage.get_messages("conv-maxiter-test")` 返回列表长度 ≥ 2（含 user 消息和至少 1 条 assistant 记录）
- 最后一条 assistant 消息 `role` 为 `"assistant"`

---

## 意图 2：用户取消后 outcome 为 Cancelled，已流出内容落盘，后续可发新消息

**场景**
用户点击「停止」，turn 安全退出，已流出内容被持久化，对话状态干净，下一条消息可正常发送。

**前提**
- MockLlmExecutor 预设：先发出 delta `"你好"`，检测到 cancel 后返回 `Cancelled`
- 使用 TempDir + AppStorage，conversation_id 为 `"conv-cancel-test"`
- turn 开始后 20ms 触发取消

**操作**
- driver 执行对话，20ms 后触发取消

**断言**

事件序列：
- EventBus 中 `TurnCompleted` 的 `outcome` 序列化值为 `"Cancelled"`
- EventBus 中 `StreamDone` 不出现
- EventBus 中最后一个事件为 `AgentIdle`

存储状态：
- `storage.get_messages("conv-cancel-test")` 返回列表长度为 2
- assistant 消息 `content["text"]` 等于 `"你好"`（已流出内容被保存）

后续可用性：
- 取消后用同一 conversation_id 再执行一轮，新 turn 的 `TurnCompleted` 的 `outcome` 序列化值为 `"Success"`

---

## 意图 3：正常 turn 完成后 outcome 为 Success，assistant 消息落盘，事件序列完整

**场景**
用户正常发消息，LLM 回复文本，turn 以 Success 结束，消息被持久化。

**前提**
- MockLlmExecutor 预设：返回 `ContentComplete { content: "你好" }`
- 使用 TempDir + AppStorage，conversation_id 为 `"conv-success-test"`
- 用户消息为 `"你好吗"`

**操作**
- driver 执行一轮正常对话

**断言**

事件序列：
- EventBus 中 `TurnCompleted` 的 `outcome` 序列化值为 `"Success"`
- `StreamDone` 出现在 `TurnCompleted` 之前
- `TurnCompleted` 出现在 `AgentIdle` 之前
- `AgentIdle` 的 `scope` 序列化值为 `"primary"`

存储状态：
- `storage.get_messages("conv-success-test")` 返回列表长度为 2
- 第 1 条 `role` 为 `"user"`，`content["text"]` 为 `"你好吗"`
- 第 2 条 `role` 为 `"assistant"`，`content["text"]` 为 `"你好"`

---

## 意图 4：上下文过长触发 compaction 后 turn 以 Success 完成，compacting 阶段事件出现

**场景**
对话历史超出上下文窗口，系统自动触发 compaction 压缩历史后继续执行，用户感受到 turn 成功，不是报错中止。

**前提**
- MockLlmExecutor 预设：第 1 次返回 PromptTooLong，第 2 次返回 `ContentComplete { content: "好的" }`
- MockCompactClient 预设：返回摘要 `"历史已压缩"`
- 使用 TempDir + AppStorage，conversation_id 为 `"conv-compact-test"`

**操作**
- driver 执行对话，触发 PromptTooLong 后自动 compaction 并重试

**断言**

事件序列：
- EventBus 中出现 `TurnStageChanged`，`stage.kind` 序列化值为 `"compacting"`
- EventBus 中 `TurnCompleted` 的 `outcome` 序列化值为 `"Success"`

存储状态：
- `storage.get_messages("conv-compact-test")` 第 2 条 `role` 为 `"assistant"`，`content["text"]` 为 `"好的"`

---

## 意图 5：前一个 turn 报错，下一个 turn 正常执行，存储中两轮消息均完整

**场景**
前一次对话出错，不应污染下一次对话的状态，用户重新发消息能正常收到回复，存储中两轮消息共存且无损坏。

**前提**
- 使用 TempDir + AppStorage，conversation_id 为 `"conv-recovery-test"`
- turn 1：用户消息 `"第一条"`，MockLlmExecutor 返回 `Err`（LLM 报错）
- turn 2：用户消息 `"第二条"`，MockLlmExecutor 返回 `ContentComplete { content: "正常回复" }`

**操作**
- 先执行 turn 1（预期出错）
- 再执行 turn 2

**断言**

事件序列：
- turn 2 的 `TurnCompleted` 的 `outcome` 序列化值为 `"Success"`

存储状态：
- `storage.get_messages("conv-recovery-test")` 返回列表长度为 3（turn 1 的 user + turn 2 的 user + assistant）
- 最后一条 `role` 为 `"assistant"`，`content["text"]` 为 `"正常回复"`
