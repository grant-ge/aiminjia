# rules.md — llm-provider-routing LLM Provider 路由与降级测试意图

来源：[LUT-4](mention://issue/8fb70292-f4aa-4ec9-8c10-2ec6bcb05c76)

验证方式：cargo test（使用 MockLlmExecutor，不调真实 provider）

---

## 意图 1：API key 无效时 EventBus 发出 StreamError，turn 终止

**场景**
用户发消息，provider 返回认证失败（401）。系统应通过 EventBus 发出 `StreamError` 事件，让前端展示错误提示，而不是静默卡死。

**前提**
- MockLlmExecutor 预设：`run_llm_step` 返回 `Err(TurnError::LlmError("API error (401): unauthorized".to_string()))`
- 不注册任何工具

**操作**
1. 调用 `driver.run_chat_turn(&mut state, &request)` 并等待返回

**断言**
- `run_chat_turn` 返回 `Err`，错误字符串包含 `"401"` 或 `"unauthorized"`
- EventBus 中出现 `StreamError` 事件
- `StreamError` 事件的 `error` 字段包含 `"401"` 或 `"unauthorized"`
- EventBus 中出现 `AgentIdle` 事件（turn 结束后必须发出，让前端解除 loading 状态）

---

## 意图 2：provider 限流（429）时 EventBus 发出 StreamError，turn 终止

**场景**
provider 返回 429（配额耗尽），用户应看到错误提示而非无限等待。

**前提**
- MockLlmExecutor 预设：`run_llm_step` 返回 `Err(TurnError::LlmError("API error (429): rate limit exceeded".to_string()))`

**操作**
1. 调用 `run_chat_turn` 并等待返回

**断言**
- `run_chat_turn` 返回 `Err`
- EventBus 中出现 `StreamError` 事件，`error` 字段包含 `"429"` 或 `"rate limit"`
- EventBus 中出现 `AgentIdle` 事件

---

## 意图 3：上下文过长（PromptTooLong）触发 compaction 后 turn 以 Success 完成

**场景**
消息历史超出 provider 上下文窗口限制，系统触发 compaction 压缩历史，压缩后继续完成 turn，用户感受到的是 turn 成功，不是报错。

**前提**
- MockLlmExecutor：第 1 次 `run_llm_step` 返回 `Err(TurnError::PromptTooLong("context too long".to_string()))`，第 2 次返回 `Ok(LlmStepResult::ContentComplete { content: "压缩后回复", .. })`
- MockCompactClient：返回摘要字符串 `"压缩摘要"`

**操作**
1. 调用 `run_chat_turn` 并等待完成

**断言**
- `run_chat_turn` 返回 `Ok`
- EventBus 中出现 `TurnStageChanged`，`stage.kind` 序列化值为 `"compacting"`
- EventBus 中出现 `TurnCompleted`，`outcome` 序列化值为 `"Success"`
- EventBus 中出现 `MessagePersisted`，`role == "assistant"`，`content.text` 包含 `"压缩后回复"`

---

## 意图 4：正常 turn 中 StreamDelta 内容拼接后等于 LLM 完整输出

**场景**
LLM 流式输出，所有 `StreamDelta` 的 `content` 拼接后必须完整还原 LLM 的回复内容，前端依赖此进行逐字追加显示。

**前提**
- MockLlmExecutor 返回 `ContentComplete { content: "你好，我是 AI 助手", .. }`

**操作**
1. 调用 `run_chat_turn` 并等待完成
2. 从 EventBus 收集所有 `StreamDelta` 事件，将 `content` 字段按顺序拼接

**断言**
- 所有 `StreamDelta.content` 拼接后等于 `"你好，我是 AI 助手"`
- `StreamDone` 事件出现在最后一个 `StreamDelta` 之后
