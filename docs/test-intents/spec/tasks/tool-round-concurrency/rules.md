# rules.md — tool round 并发策略测试意图

`ToolRoundDriver` 只能并发真正安全的工具，不能把所有工具一股脑并发。安全工具可以并发，非安全工具必须串行。

---

## 意图 1：同一 turn 内两个 concurrency-safe 工具并发执行，两个 ToolCallExecuting 事件出现时间差不超过 500ms

**场景**
LLM 在同一轮返回两个标记为 concurrency-safe 的工具调用，系统应并发执行，不应等第一个完成后再开始第二个。否则父代理会把本可并行的工作慢一倍。

**前提**
- 使用有效 API key，注册了两个执行各需约 1 秒的 concurrency-safe 工具（如两个执行 `sleep 1` 的 bash 命令工具）
- 新建对话

**操作**
- 在输入框输入能让 LLM 同时调用这两个工具的消息，点击发送
- 等待两个工具执行完成，turn 结束

**验收标准**
- EventBus 中 `ToolCallExecuting` 事件出现 2 次
- EventBus 中 `ToolCallCompleted` 事件出现 2 次
- 两个 `ToolCallExecuting` 事件的出现时间差 ≤ 500ms（说明并发启动，而非串行）
- turn 总耗时 ≤ 单个工具耗时 × 1.5（说明并发执行，而非串行叠加）
- `messages.jsonl` 中包含两条 role 为 `"tool"` 的记录

---

## 意图 2：同一 turn 内一个 safe 工具和一个 unsafe 工具，unsafe 工具必须等 safe 工具结束后才开始

**场景**
如果同一轮里同时出现一个 safe 工具和一个 unsafe 工具，系统不能把它们并发起来。非安全工具必须等前一个结束后再开始，防止并发副作用。

**前提**
- 使用有效 API key，注册了一个 concurrency-safe 工具和一个 concurrency-unsafe 工具，各执行约 1 秒
- 新建对话

**操作**
- 在输入框输入能让 LLM 同时调用这两个工具的消息，点击发��
- 等待两个工具执行完成，turn 结束

**验收标准**
- EventBus 中 `ToolCallExecuting` 事件出现 2 次
- EventBus 中 `ToolCallCompleted` 事件出现 2 次
- unsafe 工具的 `ToolCallExecuting` 事件，晚于 safe 工具的 `ToolCallCompleted` 事件出现（即 unsafe 工具在 safe 工具结束后才启动）
- turn 总耗时 ≥ 单个工具耗时 × 1.8（说明串行执行，两个工具没有重叠）
- `messages.jsonl` 中包含两条 role 为 `"tool"` 的记录，均为成功结果
