# rules.md — session-runtime 主链路事件序列测试意图

`SessionRuntime::run_chat_request` 是整个系统的顶层调度入口，驱动 RunStarted → Turn → MessagePersisted → StreamDone → TurnCompleted → AgentIdle 的完整事件生命周期。

---

## 意图 1：正常 turn 完成后 EventBus 中包含完整的事件序列且顺序正确

**场景**
用户发一条普通文字消息，LLM 直接回复文本，没有工具调用。系统应按固定顺序发出覆盖整个生命周期的事件——遗漏任何一个或顺序错误都会导致前端状态机卡死或 UI 不响应。

**前提**
- 使用 `SessionRuntime::with_llm_executor(QueryEngine::default(), RuntimeEventBus::new(), executor)` 构造 runtime
- Mock LLM（MockLlmExecutor）预设：第 1 轮返回 `ContentComplete { content: "你好！" }`
- 不注册任何工具

**操作**
1. 调用 `session_runtime.run_chat_request(request)` 并等待 `Ok`
2. 读取 `session_runtime.recorded_events()` 的 kind 标签序列

**断言**
- 事件序列中包含（按出现顺序）：`RunStarted`、`StreamStarted`、`StreamDelta`、`MessagePersisted`、`StreamDone`、`TurnCompleted`、`AgentIdle`
- `RunStarted` 是第一个事件
- `AgentIdle` 是最后一个事件
- `StreamDone` 出现在 `TurnCompleted` 之前
- `TurnCompleted` 出现在 `AgentIdle` 之前
- 不出现 `ToolCallExecuting` 或 `ToolCallCompleted`

---

## 意图 2：同一 turn 内所有事件携带相同的 run_id

**场景**
前端通过 `runId` 字段区分不同的 turn 状态，如果事件的 `run_id` 不一致，前端的流式内容会追加到错误的会话位置。

**前提**
- 同意图 1 的构造方式
- Mock LLM 预设：第 1 轮返回 `ContentComplete { content: "OK" }`

**操作**
1. 调用 `run_chat_request(request)` 并等待完成
2. 读取所有 recorded events 的 `run_id` 字段

**断言**
- 所有事件携带的 run_id 相同（均等于 request 中的 run_id）
- `RunStarted` 事件的 run_id 与 `AgentIdle` 事件的 run_id 相同

---

## 意图 3：LLM 调用工具后下一轮收到 tool_result，turn 继续推进直至完成

**场景**
工具调用是 agentic turn 的核心路径。LLM 先调用工具，工具返回结果后 LLM 必须收到这个结果并继续生成，最终完成 turn。如果 tool_result 没被正确注入下一轮，turn 就会提前结束或无限循环。

**前提**
- 注册一个名为 `dummy_tool` 的 Mock RuntimeTool，执行后返回 `Ok("工具结果")`
- Mock LLM 预设：
  - 第 1 轮返回 `ToolCalls { tool_calls: [{ name: "dummy_tool", input: {} }] }`
  - 第 2 轮返回 `ContentComplete { content: "分析完成" }`

**操作**
1. 调用 `run_chat_request(request)` 并等待完成
2. 读取 recorded events 的 kind 标签序列

**断言**
- 事件序列中包含 `ToolCallExecuting` 且紧随其后有 `ToolCallCompleted`
- `ToolCallExecuting` 事件恰好出现 1 次，`ToolCallCompleted` 事件恰好出现 1 次
- `TurnCompleted` 的 `outcome` 字段序列化值为 `"Success"`
- 事件序列中包含 `StreamDone` 和 `AgentIdle`

---

## 意图 4：CancellationToken 触发后 turn 正常退出，不挂起

**场景**
用户点击"停止"后，系统必须能安全退出正在进行的 turn，不能死锁或永远阻塞。

**前提**
- Mock LLM 预设：第 1 轮无限延迟（在 `run_llm_step` 内等待 cancel 信号后返回 `Cancelled`）
- 在 turn 开始后 10ms，通过 `session_runtime.cancel_session(session_id, CancellationReason::UserCancel)` 触发取消

**操作**
1. 同时启动 `run_chat_request` 和一个 10ms 后触发取消的异步任务
2. 等待 `run_chat_request` 返回（或超时 5s）

**断言**
- `run_chat_request` 在 5s 内返回（不挂起）
- recorded events 中包含 `TurnCompleted`，其 `outcome` 字段序列化值为 `"Cancelled"`
- recorded events 中包含 `AgentIdle`（即使取消，也要发送 idle 信号）

---

## 意图 5：达到最大迭代次数时 turn 以 MaxIterationsReached 结束

**场景**
LLM 持续调用工具而不给出最终回复时，系统必须在达到上限后主动终止，避免无限消耗 token 和资源。

**前提**
- `max_iterations = 2`（通过 `TurnConfigOverrides` 注入）
- Mock LLM 每轮都返回 `ToolCalls { tool_calls: [{ name: "dummy_tool" }] }`（永不给 ContentComplete）
- 注册 `dummy_tool` 始终返回成功

**操作**
1. 调用 `run_chat_request(request)` 并等待完成

**断言**
- recorded events 中 `TurnCompleted` 的 `outcome` 字段序列化值为 `"MaxIterationsReached"`
- `ToolCallExecuting` 事件恰好出现 2 次
- 最后一个事件是 `AgentIdle`
