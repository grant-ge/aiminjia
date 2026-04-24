# rules.md — 权限 Ask 全链路交互测试意图

工具触发 Ask 后的完整用户交互链路：事件发出 → 前端展示 → 用户选择 → 工具重试或拒绝。

---

## 意图 1：工具触发 Ask 时前端收到 permission:ask 事件，事件携带完整信息

**场景**
LLM 调用了一个需要用户确认的工具，前端需要收到通知来展示确认对话框。

**前提**
- 工具名 `mcp__demo__action`，scope `mcp`，PermissionStore 中无记录
- RuntimeChatTurnDriver 配置了 permission control plane
- 当前 PermissionMode 为 Default

**操作**
- driver 处理一轮包含该工具的调用，LLM 返回 ToolUse

**断言**
- EventBus 中包含 PermissionAskRequired 事件
- 事件中 `tool_name == "mcp__demo__action"`
- 事件中 `message` 不为空字符串
- 事件中 `suggestions` 长度 >= 2，包含 `"Allow once"` 和 `"Deny"`
- 事件中 `remember_options` 包含 `Session`、`Workspace`、`User` 三项
- 事件中 `mode` 等于 `Default`
- 事件中 `default_destination` 为 `Session`

---

## 意图 2：用户选择 Allow 后工具被重新执行，结果正常返回给 LLM

**场景**
用户确认允许，工具应该被重新执行，结果正常返回给 LLM，不是错误。

**前提**
- 工具名 `mcp__demo__action` 触发 AskRequired
- Mock 工具在重新执行时返回成功内容 `"执行完成"`
- control plane 模拟用户响应 Allow

**操作**
- driver 等待 control plane 响应，收到 Allow

**断言**
- EventBus 中包含 `ToolCallCompleted` 事件，`is_error == false`
- EventBus 中只有 1 个 `PermissionAskRequired` 事件（Allow 后不再 Ask）
- turn 继续执行，LLM 收到 `tool_result` 内容包含 `"执行完成"`

---

## 意图 3：用户选择 Deny 后工具返回错误 tool_result，turn 继续

**场景**
用户拒绝，LLM 需要知道这个工具被拒绝了，以便调整后续行为。

**前提**
- 工具名 `mcp__demo__action` 触发 AskRequired
- control plane 模拟用户响应 Deny，消息为 `"用户拒绝"`

**操作**
- driver 等待 control plane 响应，收到 Deny

**断言**
- EventBus 中包含 `ToolCallCompleted` 事件，`is_error == true`
- LLM 消息历史中包含 role 为 `tool` 的消息
- 该 tool_result 消息内容包含 `"用户拒绝"` 或拒绝相关字符串
- turn 继续执行，LLM 后续可以响应（不终止 turn）

---

## 意图 4：用户直接关闭确认框（Cancel）等同于 Deny

**场景**
用户不做选择直接关闭对话框，系统不能永久等待，应视为拒绝。

**前提**
- 工具触发 AskRequired
- control plane 模拟 Cancel 响应

**操作**
- driver 收到 Cancel 响应

**断言**
- EventBus 中包含 `ToolCallCompleted` 事件，`is_error == true`
- tool_result 内容不为空（有说明文字）
- turn 继续执行，不挂起

---

## 意图 5：一轮内多个工具触发 Ask，按顺序逐个处理

**场景**
LLM 一次调用了两个工具，都需要确认，系统需要按顺序逐个处理。

**前提**
- 工具 A（`mcp__demo__action1`）和工具 B（`mcp__demo__action2`）都触发 AskRequired
- control plane 按顺序：先对工具 A 响应 Allow，再对工具 B 响应 Deny

**操作**
- driver 处理该轮结果

**断言**
- EventBus 中有 2 个 `PermissionAskRequired` 事件
- 两个事件的 `tool_name` 分别为 `"mcp__demo__action1"` 和 `"mcp__demo__action2"`
- 工具 A 对应的 `ToolCallCompleted` 事件 `is_error == false`
- 工具 B 对应的 `ToolCallCompleted` 事件 `is_error == true`

---

## 意图 6：Ask 等待中 turn 被取消，driver 正常退出不死锁

**场景**
用户在等待权限确认的过程中直接取消了整个对话，系统不能死锁。

**前提**
- 工具触发 AskRequired，control plane 不响应（模拟挂起）
- 在 driver 等待期间，CancellationToken 被触发

**操作**
- driver 进入等待，随后取消 token

**断言**
- driver 退出等待，函数正常返回
- 不 panic，不死锁
- turn 最终结束（可以是 cancelled 状态）

---

## 意图 7：PermissionAskRequired 事件映射为前端 permission:ask 事件，payload 完整

**场景**
前端订阅的是字符串事件名 `"permission:ask"`，后端 RuntimeEvent 必须被正确翻译，payload 字段完整。

**前提**
- 构造 PermissionAskRequired RuntimeEvent，携带：
  - `tool_name = "mcp__demo__action"`
  - `message = "需要确认"`
  - `suggestions = ["Allow once", "Deny"]`
  - `mode = Default`

**操作**
- 将该事件传入 TauriEventAdapter 处理

**断言**
- 前端收到的事件名为 `"permission:ask"`
- payload 中 `tool_name == "mcp__demo__action"`
- payload 中 `message == "需要确认"`
- payload 中 `suggestions` 长度为 2
- payload 中 `mode` 字段存在
