# rules.md — chat-turn-boundary Chat Turn 主循环边界测试意图

来源：[LUT-5](mention://issue/646d0af5-f4ca-4a90-9774-ba041ca55a23)

---

## 意图 1：LLM 持续工具调用达到 max_iterations 时 turn 终止，工具调用记录落盘

**场景**
LLM 一直返回工具调用不给最终回复，达到上限后 turn 主动终止，工具调用的消息记录被持久化。

**前提**
- `TurnConfigOverrides { max_iterations: Some(2) }` 注入
- MockLlmExecutor 预设：每轮返回包含 `"dummy_tool"` 的 `ToolCalls`，永不返回 `ContentComplete`
- 注册 `"dummy_tool"`，始终返回成功
- 使用隔离存储，conversation_id 为 `"conv-maxiter-test"`
- 用户消息为 `"请帮我持续调用工具"`

**操作**
- driver 执行对话，LLM 持续返回工具调用直到 max_iterations 触达

**断言**
- EventBus 中 `TurnCompleted` 的 `outcome` 字段值为 `{"MaxIterationsReached":{"iterations":2}}`
- EventBus 中 `ToolCallExecuting` 事件恰好出现 2 次
- EventBus 中最后一个事件的类型为 `AgentIdle`，`scope` 字段值为 `"primary"`
- `messages.jsonl` 存在，文件行数 ≥ 2，每行均为合法 JSON
- 第 1 行 `role` 字段值为 `"user"`，`content.text` 字段值为 `"请帮我持续调用工具"`

---

## 意图 2：用户取消后 already 流出内容落盘，StreamDone 不出现，后续可发新消息

**场景**
用户点击停止，turn 安全退出，已流出内容被持久化，StreamDone 不出现（流未正常完成），下一条消息可正常发送。

**前提**
- MockLlmExecutor 预设：先发出 delta `"你好"`，检测到 cancel 后返回 `Cancelled`
- 使用隔离存储，conversation_id 为 `"conv-cancel-test"`
- 用户消息为 `"请介绍一下自己"`
- turn 开始后 20ms 触发取消

**操作**
- driver 执行对话，20ms 后触发取消

**断言**
- EventBus 中 `TurnCompleted` 的 `outcome` 字段值为 `"Cancelled"`
- EventBus 中 `StreamDone` 不出现
- EventBus 中最后一个事件类型为 `AgentIdle`
- `messages.jsonl` 存在，文件共 2 行，每行均为合法 JSON
- 第 1 行 `role` 字段值为 `"user"`，`content.text` 字段值为 `"请介绍一下自己"`
- 第 2 行 `role` 字段值为 `"assistant"`，`content.text` 字段值为 `"你好"`（已流出内容被保存）
- 取消后用同一 conversation_id 再执行一轮，新 turn 的 `TurnCompleted` 的 `outcome` 字段值为 `"Success"`

---

## 意图 3：正常 turn 完成后消息落盘，事件序列完整

**场景**
用户正常发消息，LLM 回复文本，turn 以 Success 结束，user 和 assistant 消息均写入存储。

**前提**
- MockLlmExecutor 预设：返回 `ContentComplete { content: "你好" }`
- 使用隔离存储，conversation_id 为 `"conv-success-test"`
- 用户消息为 `"你好吗"`

**操作**
- driver 执行一轮正常对话

**断言**
- EventBus 中 `TurnCompleted` 的 `outcome` 字段值为 `"Success"`
- `StreamDone` 出现在 `TurnCompleted` 之前
- `TurnCompleted` 出现在 `AgentIdle` 之前
- `AgentIdle` 的 `scope` 字段值为 `"primary"`
- `messages.jsonl` 存在，文件共 2 行，每行均为合法 JSON
- 第 1 行 `role` 字段值为 `"user"`，`content.text` 字段值为 `"你好吗"`
- 第 2 行 `role` 字段值为 `"assistant"`，`content.text` 字段值为 `"你好"`

---

## 意图 4：上下文过长触发 compaction 后 turn 正常完成，assistant 消息落盘

**场景**
对话历史超出上下文窗口，系统自动触发 compaction 压缩历史后继续执行，用户感受到 turn 成功，消息正常写入。

**前提**
- MockLlmExecutor 预设：第 1 次返回 PromptTooLong，第 2 次返回 `ContentComplete { content: "好的" }`
- MockCompactClient 预设：返回摘要 `"历史已压缩"`
- 使用隔离存储，conversation_id 为 `"conv-compact-test"`

**操作**
- driver 执行对话，触发 PromptTooLong 后自动 compaction 并重试

**断言**
- EventBus 中出现 `TurnStageChanged` 事件，`stage.kind` 字段值为 `"compacting"`
- EventBus 中 `TurnCompleted` 的 `outcome` 字段值为 `"Success"`
- `messages.jsonl` 存在，文件共 2 行，每行均为合法 JSON
- 第 2 行 `role` 字段值为 `"assistant"`，`content.text` 字段值为 `"好的"`

---

## 意图 5：前一个 turn 报错，下一个 turn 正常执行，两轮消息均完整落盘

**场景**
前一次对话出错，不污染下一次对话状态，用户重新发消息能正常收到回复，两轮消息均在存储中。

**前提**
- 使用隔离存储，conversation_id 为 `"conv-recovery-test"`
- turn 1：用户消息 `"第一条"`，MockLlmExecutor 返回 Err（LLM 报错）
- turn 2：用户消息 `"第二条"`，MockLlmExecutor 返回 `ContentComplete { content: "正常回复" }`

**操作**
- 先执行 turn 1（预期出错）
- 再执行 turn 2

**断言**
- turn 2 的 `TurnCompleted` 的 `outcome` 字段值为 `"Success"`
- `messages.jsonl` 存在，文件共 3 行，每行均为合法 JSON
- 第 1 行 `role` 字段值为 `"user"`，`content.text` 字段值为 `"第一条"`
- 第 2 行 `role` 字段值为 `"user"`，`content.text` 字段值为 `"第二条"`
- 第 3 行 `role` 字段值为 `"assistant"`，`content.text` 字段值为 `"正常回复"`
