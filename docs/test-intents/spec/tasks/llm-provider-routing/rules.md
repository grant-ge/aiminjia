# rules.md — llm-provider-routing LLM Provider 路由与降级测试意图

来源：[LUT-4](mention://issue/8fb70292-f4aa-4ec9-8c10-2ec6bcb05c76)

---

## 意图 1：API key 无效时 EventBus 中出现 StreamError，turn 以错误结束

**场景**
用户发消息，provider 返回 401 认证失败。系统应通过 EventBus 告知前端出错，而不是静默卡死。

**前提**
- MockLlmExecutor 预设：`run_llm_step` 返回 `Err`，错误内容为 `"API error (401): unauthorized"`
- 不注册任何工具

**操作**
- driver 执行一轮对话，LLM 返回 401 错误

**断言**
- EventBus 中出现 `StreamError` 事件
- `StreamError` 事件的 `error` 字段包含 `"401"` 或 `"unauthorized"`
- EventBus 中出现 `AgentIdle` 事件（turn 结束，前端解除 loading）
- `StreamError` 出现在 `AgentIdle` 之前

---

## 意图 2：provider 限流（429）时 EventBus 中出现 StreamError，turn 以错误结束

**场景**
provider 配额耗尽返回 429，用户应收到错误提示而非无限等待。

**前提**
- MockLlmExecutor 预设：`run_llm_step` 返回 `Err`，错误内容为 `"API error (429): rate limit exceeded"`

**操作**
- driver 执行一轮对话，LLM 返回 429 错误

**断言**
- EventBus 中出现 `StreamError` 事件
- `StreamError` 事件的 `error` 字段包含 `"429"` 或 `"rate limit"`
- EventBus 中出现 `AgentIdle` 事件

---

## 意图 3：上下文过长触发 compaction 后 turn 以 Success 完成

**场景**
历史消息超出上下文窗口，系统自动压缩历史后继续完成 turn，用户感受到的是对话正常完成，不是报错。

**前提**
- MockLlmExecutor 预设：第 1 次返回 PromptTooLong 错误，第 2 次返回 `ContentComplete { content: "压缩后的回复" }`
- MockCompactClient 预设：返回摘要 `"历史已压缩"`

**操作**
- driver 执行一轮对话，触发 PromptTooLong 后自动 compaction 并重试

**断言**
- EventBus 中出现 `TurnStageChanged`，`stage.kind` 序列化值为 `"compacting"`
- EventBus 中 `TurnCompleted` 的 `outcome` 序列化值为 `"Success"`
- EventBus 中出现 `MessagePersisted`，`role` 为 `"assistant"`，`content.text` 包含 `"压缩后的回复"`

---

## 意图 4：正常 turn 所有 StreamDelta 拼接后等于 LLM 完整输出内容

**场景**
前端依赖 `StreamDelta` 事件逐字追加显示回复。所有 delta 拼接后必须等于 LLM 的完整输出，不能丢字、不能重复。

**前提**
- MockLlmExecutor 预设：返回 `ContentComplete { content: "你好，我是 AI 助手" }`

**操作**
- driver 执行一轮正常对话

**断言**
- EventBus 中所有 `StreamDelta` 事件的 `content` 字段按出现顺序拼接后等于 `"你好，我是 AI 助手"`
- `StreamDone` 出现在最后一个 `StreamDelta` 之后
