# rules.md — llm-provider-routing LLM Provider 路由与降级测试意图

来源：[LUT-4](mention://issue/8fb70292-f4aa-4ec9-8c10-2ec6bcb05c76)

---

## 意图 1：API key 无效（401）时 EventBus 发出 StreamError，消息不落盘

**场景**
用户发消息，provider 返回 401 认证失败。前端应收到错误事件展示提示，assistant 消息不应被写入存储（没有内容可写）。

**前提**
- MockLlmExecutor 预设：`run_llm_step` 返回 `Err`，错误内容包含 `"API error (401): unauthorized"`
- 使用 TempDir + AppStorage，conversation_id 为 `"conv-401-test"`
- 用户消息为 `"你好"`

**操作**
- driver 执行一轮对话，LLM 返回 401 错误

**断言**

事件序列：
- EventBus 中出现 `StreamError` 事件，`error` 字段包含 `"401"` 或 `"unauthorized"`
- EventBus 中出现 `AgentIdle` 事件，`scope` 序列化值为 `"primary"`
- `StreamError` 出现在 `AgentIdle` 之前
- EventBus 中不出现 `TurnCompleted` 的 `outcome` 为 `"Success"` 的事件

存储状态：
- `storage.get_messages("conv-401-test")` 返回列表长度为 1（仅 user 消息，无 assistant 消息）

---

## 意图 2：provider 限流（429）时 EventBus 发出 StreamError，消息不落盘

**场景**
provider 配额耗尽返回 429，用户应收到错误提示而非无限等待，存储中不产生空的 assistant 消息。

**前提**
- MockLlmExecutor 预设：`run_llm_step` 返回 `Err`，错误内容包含 `"API error (429): rate limit exceeded"`
- 使用 TempDir + AppStorage，conversation_id 为 `"conv-429-test"`
- 用户消息为 `"你好"`

**操作**
- driver 执行一轮对话，LLM 返回 429 错误

**断言**

事件序列：
- EventBus 中出现 `StreamError` 事��，`error` 字段包含 `"429"` 或 `"rate limit"`
- EventBus 中出现 `AgentIdle` 事件
- `StreamError` 出现在 `AgentIdle` 之前

存储状态：
- `storage.get_messages("conv-429-test")` 返回列表长度为 1（仅 user 消息）

---

## 意图 3：上下文过长触发 compaction 后 turn 以 Success 完成，assistant 消息落盘

**场景**
历史消息超出上下文窗口，系统自动压缩历史后继续完成 turn，用户感受到的是对话正常完成。

**前提**
- MockLlmExecutor 预设：第 1 次返回 PromptTooLong 错误，第 2 次返回 `ContentComplete { content: "压缩后的回复" }`
- MockCompactClient 预设：返回摘要 `"历史已压缩"`
- 使用 TempDir + AppStorage，conversation_id 为 `"conv-compact-test"`

**操作**
- driver 执行一轮对话，触发 PromptTooLong 后自动 compaction 并重试

**断言**

事件序列：
- EventBus 中出现 `TurnStageChanged`，`stage.kind` 序列化值为 `"compacting"`
- EventBus 中 `TurnCompleted` 的 `outcome` 序列化值为 `"Success"`
- EventBus 中出现 `MessagePersisted`，`role` 为 `"assistant"`，`content.text` 为 `"压缩后的回复"`

存储状态：
- `storage.get_messages("conv-compact-test")` 返回列表长度为 2
- 第 2 条 `role` 为 `"assistant"`，`content["text"]` 为 `"压缩后的回复"`

---

## 意图 4：正常 turn 所有 StreamDelta 拼接后等于 LLM 完整输出，MessagePersisted 内容一致

**场景**
前端依赖 `StreamDelta` 事件逐字追加显示回复，同时存储中的消息必须与流式内容完全一致，不能丢字或重复。

**前提**
- MockLlmExecutor 预设：返回 `ContentComplete { content: "你好，我是 AI 助手" }`
- 使用 TempDir + AppStorage，conversation_id 为 `"conv-delta-test"`

**操作**
- driver 执行一轮正常对话

**断言**

事件序列：
- EventBus 中所有 `StreamDelta` 事件的 `content` 字段按出现顺序拼接后等于 `"你好，我是 AI 助手"`
- `StreamDone` 出现在最后一个 `StreamDelta` 之后
- `MessagePersisted` 的 `content.text` 等于 `"你好，我是 AI 助手"`

存储状态：
- `storage.get_messages("conv-delta-test")` 第 2 条 `content["text"]` 等于 `"你好，我是 AI 助手"`（与 StreamDelta 拼接结果完全一致）
