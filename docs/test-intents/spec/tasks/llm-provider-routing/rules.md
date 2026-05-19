# rules.md — llm-provider-routing LLM Provider 路由与降级测试意图

来源：[LUT-4](mention://issue/8fb70292-f4aa-4ec9-8c10-2ec6bcb05c76)

---

## 意图 1：API key 无效（401）时用户看到错误提示，assistant 消息不落盘

**场景**
用户发消息，provider 返回 401 认证失败。前端展示错误提示，对话历史中不产生空的 assistant 消息。

**前提**
- MockLlmExecutor 预设：`run_llm_step` 返回 Err，错误内容包含 `"API error (401): unauthorized"`
- 使用隔离存储，conversation_id 为 `"conv-401-test"`
- 用户消息为 `"你好"`

**操作**
- driver 执行一轮对话，LLM 返回 401 错误

**断言**
- EventBus 中出现 `StreamError` 事件，`error` 字段包含 `"401"` 或 `"unauthorized"`
- EventBus 中 `StreamError` 出现在 `AgentIdle` 之前
- `messages.jsonl` 存在，文件共 1 行，该行为合法 JSON，`role` 字段值为 `"user"`（无 assistant 消息写入）

---

## 意图 2：provider 限流（429）时用户看到错误提示，assistant 消息不落盘

**场景**
provider 配额耗尽返回 429，用户收到错误提示，不无限等待，存储中不产生空的 assistant 消息。

**前提**
- MockLlmExecutor 预设：`run_llm_step` 返回 Err，错误内容包含 `"API error (429): rate limit exceeded"`
- 使用隔离存储，conversation_id 为 `"conv-429-test"`
- 用户消息为 `"你好"`

**操作**
- driver 执行一轮对话，LLM 返回 429 错误

**断言**
- EventBus 中出现 `StreamError` 事件，`error` 字段包含 `"429"` 或 `"rate limit"`
- `messages.jsonl` 存在，文件共 1 行，`role` 字段值为 `"user"`

---

## 意图 3：上下文过长触发 compaction 后 turn 正常完成，assistant 消���落盘

**场景**
历史消息超出上下文窗口，系统自动压缩历史后继续完成 turn，用户感受到的是对话正常完成，assistant 消息正常写入。

**前提**
- MockLlmExecutor 预设：第 1 次返回 PromptTooLong 错误，第 2 次返回 `ContentComplete { content: "压缩后的回复" }`
- MockCompactClient 预设：返回摘要 `"历史已压缩"`
- 使用隔离存储，conversation_id 为 `"conv-compact-test"`

**操作**
- driver 执行一轮对话，触发 PromptTooLong 后自动 compaction 并重试

**断言**
- EventBus 中出现 `TurnStageChanged` 事件，`stage.kind` 字段值为 `"compacting"`
- EventBus 中 `TurnCompleted` 的 `outcome` 字段值为 `"Success"`
- `messages.jsonl` 存在，文件共 2 行，每行均为合法 JSON
- 第 2 行 `role` 字段值为 `"assistant"`，`content.text` 字段值为 `"压缩后的回复"`

---

## 意图 4：正常 turn 完成后 StreamDelta 拼接内容与存储消息完全一致

**场景**
前端依赖 StreamDelta 事件逐字追加显示回复，存储中的消息必须与流式内容完全一致，不能丢字或重复。

**前提**
- MockLlmExecutor 预设：返回 `ContentComplete { content: "你好，我是 AI 助手" }`
- 使用隔离存储，conversation_id 为 `"conv-delta-test"`
- 用户消息为 `"请介绍一下你自己"`

**操作**
- driver 执行一轮正常对话

**断言**
- EventBus 中所有 `StreamDelta` 事件的 `content` 字段按出现顺序拼接后等于 `"你好，我是 AI 助手"`
- `StreamDone` 出现在最后一个 `StreamDelta` 之后
- `messages.jsonl` 存在，文件共 2 行，每行均为合法 JSON
- 第 1 行 `role` 字段值为 `"user"`，`content.text` 字段值为 `"请介绍一下你自己"`
- 第 2 行 `role` 字段值为 `"assistant"`，`content.text` 字段值为 `"你���，我是 AI 助手"`
