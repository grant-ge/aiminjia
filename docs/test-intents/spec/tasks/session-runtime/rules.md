# rules.md — session-runtime 主链路事件序列测试意图

`SessionRuntime` 是整个系统的顶层调度入口，驱动 RunStarted → Turn → MessagePersisted → StreamDone → TurnCompleted → AgentIdle 的完整事件生命周期。

> **注意**：工具调用、取消、最大迭代场景见 `chat-turn-boundary/rules.md`。

---

## 意图 1：正常 turn 完成后 EventBus 中包含完整的事件序列且顺序正确

**场景**
用户发一条普通文字消息，LLM 直接回复文本，没有工具调用。系统应按固定顺序发出覆盖整个生命周期的事件——遗漏任何一个或顺序错误都会导致前端状态机卡死或 UI 不响应。

**前提**
- 使用有效 API key，新建对话
- 用户消息为 `"你好"`
- 无工具注册，LLM 预期直接返回文本回复

**操作**
- 用户在对话框输入消息并发送，等待 AI 完整回复

**验收标准**
- EventBus 中按顺序出现以下事件类型：`RunStarted`、`StreamStarted`、`StreamDelta`、`MessagePersisted`、`StreamDone`、`TurnCompleted`、`AgentIdle`
- `RunStarted` 是第一个事件，`AgentIdle` 是最后一个事件
- `StreamDone` 出现在 `TurnCompleted` 之前
- `TurnCompleted` 出现在 `AgentIdle` 之前
- EventBus 中不出现 `ToolCallExecuting` 或 `ToolCallCompleted`

---

## 意图 2：同一 turn 内所有事件携带相同的 run_id

**场景**
前端通过 `runId` 字段区分不同的 turn 状态，如果事件的 `run_id` 不一致，前端的流式内容会追加到错误的会话位置。

**前提**
- 使用有效 API key，新建对话
- 用户消息为 `"���好"`

**操作**
- 用户发送消息，等待 AI 完整回复

**验收标准**
- 同一 turn 内所有 EventBus 事件携带相同的 `run_id`
- `RunStarted` 事件的 `run_id` 与 `AgentIdle` 事件的 `run_id` 相同
- `TurnCompleted` 事件的 `run_id` 与 `RunStarted` 事件的 `run_id` 相同
