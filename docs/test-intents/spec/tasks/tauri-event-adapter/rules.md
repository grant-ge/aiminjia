# rules.md — tauri-event-adapter 前后端事件协议映射测试意图

`map_runtime_event` 是 RuntimeEvent → 前端 legacy event 的唯一映射层，是前后端协议边界。  
字段名拼写或事件名错误会静默破坏所有前端响应，因此每个映射关系都需要精确验证。

---

## 意图 1：StreamDelta 映射为 streaming:delta，payload 包含 delta 内容与 runId

**场景**
流式文本的每个分片都通过 `streaming:delta` 事件传到前端，前端靠 `delta` 字段拼接最终文本。字段缺失会导致流式显示空白。

**前提**
- 构造一个 `RuntimeEvent`，`kind = StreamDelta { content: "Hello" }`，`session_id = "conv-123"`，`run_id = "run-456"`

**操作**
- 调用 `map_runtime_event(&event)`

**断言**
- 返回 `Some(LegacyEvent { name: "streaming:delta", .. })`
- `payload["conversationId"] == "conv-123"`
- `payload["delta"] == "Hello"`
- `payload["runId"] == "run-456"`

---

## 意图 2：StreamDone 映射为 streaming:done，payload 包含 conversationId 与 runId

**场景**
前端监听 `streaming:done` 来标记流式结束、触发 UI 状态变更。缺少这个事件会导致流式 loading 永远不消失。

**前提**
- 构造一个 `RuntimeEvent`，`kind = StreamDone`，`session_id = "conv-123"`，`run_id = "run-456"`

**操作**
- 调用 `map_runtime_event(&event)`

**断言**
- 返回 `Some(LegacyEvent { name: "streaming:done", .. })`
- `payload["conversationId"] == "conv-123"`
- `payload["runId"] == "run-456"`

---

## 意图 3：StreamError 映射为 streaming:error，payload 包含 error 与 rawError

**场景**
LLM 调用出错时，前端需要通过 `streaming:error` 展示错误提示。`error` 是用户可见的消息，`rawError` 是原始错误（可选）。

**前提**
- 构造一个 `RuntimeEvent`，`kind = StreamError { error: "LLM 超时", raw_error: Some("upstream timeout") }`，`session_id = "conv-123"`，`run_id = "run-456"`

**操作**
- 调用 `map_runtime_event(&event)`

**断言**
- 返回 `Some(LegacyEvent { name: "streaming:error", .. })`
- `payload["error"] == "LLM 超时"`
- `payload["rawError"] == "upstream timeout"`
- `payload["conversationId"] == "conv-123"`
- `payload["runId"] == "run-456"`

---

## 意图 4：ToolCallExecuting 映射为 tool:executing，payload 包含 toolName、toolId 与 input

**场景**
前端用 `tool:executing` 事件展示工具调用动画，依赖 `toolName` 显示工具名称、`toolId` 关联后续 completed 事件。任一字段错误会导致工具进度条展示异常。

**前提**
- 构造一个 `RuntimeEvent`，`kind = ToolCallExecuting { tool_call_id: "tc-001", tool_name: "file_write", input: {} }`，`session_id = "conv-123"`，`run_id = "run-456"`

**操作**
- 调用 `map_runtime_event(&event)`

**断言**
- 返回 `Some(LegacyEvent { name: "tool:executing", .. })`
- `payload["toolName"] == "file_write"`
- `payload["toolId"] == "tc-001"`
- `payload["input"] == {}`
- `payload["conversationId"] == "conv-123"`
- `payload["runId"] == "run-456"`

---

## 意图 5：ToolCallCompleted（成功）映射为 tool:completed，payload 结构完整

**场景**
前端通过 `tool:completed` 更新工具结果展示，依赖 `toolResult.isError` 决定成功/失败样式，依赖 `toolResult.content` 显示输出内容。顶层 `id` 字段是消息 upsert 的幂等键，顶层 `content` 是空对象（与 toolResult.content 不同）。

**前提**
- 构造一个 `RuntimeEvent`，`kind = ToolCallCompleted { tool_call_id: "tc-001", tool_name: "file_write", is_error: false, content: "写入成功", msg_id: "msg-789", duration_ms: Some(42) }`，`session_id = "conv-123"`，`run_id = "run-456"`

**操作**
- 调用 `map_runtime_event(&event)`

**断言**
- 返回 `Some(LegacyEvent { name: "tool:completed", .. })`
- `payload["id"] == "msg-789"`（顶层，用于消息 upsert）
- `payload["role"] == "tool"`
- `payload["content"] == {}`（顶层 content 是空对象，不是工具输出）
- `payload["toolResult"]["toolCallId"] == "tc-001"`
- `payload["toolResult"]["name"] == "file_write"`
- `payload["toolResult"]["isError"] == false`
- `payload["toolResult"]["content"] == "写入成功"`
- `payload["toolResult"]["durationMs"] == 42`
- `payload["success"] == true`（legacy 兼容字段，与 `!is_error` 一致）
- `payload["runId"] == "run-456"`

---

## 意图 6：ToolCallCompleted（失败）时 isError 与 success 字段反转

**场景**
工具执行失败时，前端通过 `toolResult.isError == true` 和 `success == false` 切换到错误样式展示。如果这两个字段没有同步反转，UI 会显示错误的状态。

**前提**
- 构造一个 `RuntimeEvent`，`kind = ToolCallCompleted { tool_call_id: "tc-002", tool_name: "execute_python", is_error: true, content: "语法错误：第 3 行", msg_id: "msg-999", duration_ms: None }`

**操作**
- 调用 `map_runtime_event(&event)`

**断言**
- 返回 `Some(LegacyEvent { name: "tool:completed", .. })`
- `payload["toolResult"]["isError"] == true`
- `payload["success"] == false`
- `payload["toolResult"]["content"] == "语法错误：第 3 行"`
- `payload["toolResult"]["durationMs"]` 为 JSON `null`

---

## 意图 7：PermissionAskRequired 映射为 permission:ask，payload 包含完整确认信息

**场景**
前端弹出权限确认对话框所需的所有字段（工具名、提示消息、建议选项、记住选项、默认记住目标）都必须完整传达，缺少任一字段会导致 UI 无法正确渲染确认对话框。

**前提**
- 构造一个 `RuntimeEvent`，`kind = PermissionAskRequired { tool_call_id: "tc-003", tool_name: "browse", message: "是否允许浏览网页？", suggestions: ["允许一次", "总是允许"], mode: PermissionMode::Default, remember_options: [Session, Workspace], default_destination: Some(Session) }`，`session_id = "conv-123"`，`run_id = "run-456"`

**操作**
- 调用 `map_runtime_event(&event)`

**断言**
- 返回 `Some(LegacyEvent { name: "permission:ask", .. })`
- `payload["toolName"] == "browse"`
- `payload["toolCallId"] == "tc-003"`
- `payload["message"] == "是否允许浏览网页？"`
- `payload["suggestions"]` 长度为 2，包含 `"允许一次"` 和 `"总是允许"`
- `payload["rememberOptions"]` 包含 `"session"` 和 `"workspace"`
- `payload["defaultDestination"] == "session"`
- `payload["mode"] == "default"`
- `payload["conversationId"] == "conv-123"`
- `payload["runId"] == "run-456"`

---

## 意图 8：PermissionAskRequired 在 DontAsk 模式下 payload["mode"] 为 "dontAsk"

**场景**
前端通过 `mode` 字段区分正常确认（default）与静默拒绝（dontAsk）场景，以决定是否渲染对话框。序列化值必须精确匹配前端判断逻辑。

**前提**
- 构造一个 `RuntimeEvent`，`kind = PermissionAskRequired { ..., mode: PermissionMode::DontAsk, ... }`（其他字段同意图 7）

**操作**
- 调用 `map_runtime_event(&event)`

**断言**
- 返回 `Some(LegacyEvent { name: "permission:ask", .. })`
- `payload["mode"] == "dontAsk"`

---

## 意图 9：AgentIdle（Primary scope）映射为 agent:idle，scope 为 "primary"

**场景**
主代理完成一轮对话后发出 `AgentIdle` 事件，前端以此判断 AI 已停止响应、可以接收新输入。`scope` 字段必须为 `"primary"` 才能正确触发前端主输入框的解锁逻辑。

**前提**
- 构造一个 `RuntimeEvent`，`kind = AgentIdle { agent_id: "agent-run-001", scope: AgentIdleScope::Primary }`，`session_id = "conv-123"`，`run_id = "run-456"`

**操作**
- 调用 `map_runtime_event(&event)`

**断言**
- 返回 `Some(LegacyEvent { name: "agent:idle", .. })`
- `payload["scope"] == "primary"`
- `payload["agentId"] == "agent-run-001"`
- `payload["conversationId"] == "conv-123"`
- `payload["runId"] == "run-456"`

---

## 意图 10：AgentIdle（Child scope）映射为 agent:idle，scope 为 "child"

**场景**
子代理完成任务后发出 `AgentIdle`，前端以此更新子代理状态展示。`scope` 字段必须为 `"child"` 以区分于主代理的 idle，避免错误解锁主输入框。

**前提**
- 构造一个 `RuntimeEvent`，`kind = AgentIdle { agent_id: "agent-child-002", scope: AgentIdleScope::Child }`，`session_id = "conv-123"`，`run_id = "run-456"`

**操作**
- 调用 `map_runtime_event(&event)`

**断言**
- 返回 `Some(LegacyEvent { name: "agent:idle", .. })`
- `payload["scope"] == "child"`
- `payload["agentId"] == "agent-child-002"`

---

## 意图 11：MessagePersisted 映射为 message:updated，payload 包含 id、role、runId

**场景**
前端通过 `message:updated` 事件将服务端持久化的消息 upsert 到本地消息列表。`messageId` 和 `id` 是幂等键（两个字段同值），`role` 决定渲染样式，`runId` 关联当前 turn。

**前提**
- 构造一个 `RuntimeEvent`，`kind = MessagePersisted { message_id: "msg-001", role: "assistant", content: { "text": "你好" } }`，`session_id = "conv-123"`，`run_id = "run-456"`

**操作**
- 调用 `map_runtime_event(&event)`

**断言**
- 返回 `Some(LegacyEvent { name: "message:updated", .. })`
- `payload["messageId"] == "msg-001"`
- `payload["id"] == "msg-001"`（legacy 兼容字段，与 messageId 相同）
- `payload["role"] == "assistant"`
- `payload["conversationId"] == "conv-123"`
- `payload["runId"] == "run-456"`
- `payload["createdAt"]` 存在（动态时间戳，只验证字段存在，不验证值）

---

## 意图 12：RunStarted / RunCancelled / StreamStarted / OrphanedPermissionDetected 不映射为任何前端事件

**场景**
这四个 RuntimeEvent 是内部调度信号，没有对应的前端 legacy event。如果错误地映射出前端事件，会触发无意义的前端状态变更。其中 `StreamStarted` 在每次 S4 turn 都会发出，静默丢弃是刻意的设计。

**前提**
- 分别构造四个 `RuntimeEvent`：`kind = RunStarted`、`kind = RunCancelled`、`kind = StreamStarted`、`kind = OrphanedPermissionDetected { count: 1 }`

**操作**
- 对四个 event 分别调用 `map_runtime_event`

**断言**
- 四者均返回 `None`

---

## 意图 13：TurnCompleted 映射为 turn:completed，payload 包含 outcome、token 统计与 permissionDenialCount

**场景**
前端通过 `turn:completed` 事件展示 token 用量和响应结果，`outcome` 字段决定是否显示取消或超限提示，`permissionDenialCount` 用于统计本次 turn 中被拒绝的权限请求数。

**前提**
- 构造一个 `RuntimeEvent`，`kind = TurnCompleted { outcome: ChatTurnOutcome::Success, total_input_tokens: 100, total_output_tokens: 50, total_cost_usd: Some(0.002), permission_denial_count: 3 }`，`session_id = "conv-123"`，`run_id = "run-456"`

**操作**
- 调用 `map_runtime_event(&event)`

**断言**
- 返回 `Some(LegacyEvent { name: "turn:completed", .. })`
- `payload["outcome"] == "Success"`
- `payload["totalInputTokens"] == 100`
- `payload["totalOutputTokens"] == 50`
- `payload["totalCostUsd"] == 0.002`
- `payload["permissionDenialCount"] == 3`
- `payload["conversationId"] == "conv-123"`
- `payload["runId"] == "run-456"`

---

## 意图 14：TaskStatusChanged 映射为 task:status-changed，payload 包含 taskId 与 status

**场景**
前端通过 `task:status-changed` 事件更新任务列表状态展示。`taskId`、`status`、`subject` 是必填字段，`activeForm` 和 `owner` 可选。

**前提**
- 构造一个 `RuntimeEvent`，`kind = TaskStatusChanged { task_id: "task-001", status: "in_progress", subject: "分析数据", active_form: Some("正在分析数据"), owner_agent_id: Some("agent-run-001") }`，`session_id = "conv-123"`，`run_id = "run-456"`

**操作**
- 调用 `map_runtime_event(&event)`

**断言**
- 返回 `Some(LegacyEvent { name: "task:status-changed", .. })`
- `payload["taskId"] == "task-001"`
- `payload["status"] == "in_progress"`
- `payload["subject"] == "分析数据"`
- `payload["activeForm"] == "正在分析数据"`
- `payload["owner"] == "agent-run-001"`
- `payload["conversationId"] == "conv-123"`
- `payload["runId"] == "run-456"`
