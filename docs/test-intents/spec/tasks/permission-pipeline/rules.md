# rules.md — 权限管线决策测试意图

权限管线决定一个工具调用能不能执行：Allow、Deny 还是需要问用户（Ask）。

---

## 意图 1：没有 capability_scope 的工具始终被允许

**场景**
内置工具（如 write_memory、search_memory）不涉及外部资源，不需要权限检查。

**前提**
- 工具定义的 capability_scope 为空列表
- 使用 CapabilityPermissionPipeline
- 不提供任何 capability context

**操作**
- 调用 authorize()

**断言**
- 结果为 Allow
- 补充验证：为同一工具提供 workspace capability 再次调用，结果仍为 Allow

---

## 意图 2：需要 workspace 的工具在没有 workspace 时被 Deny

**场景**
用户没有选择工作区就触发了文件操作，工具应该被拒绝并给出说明。

**前提**
- 工具 id 为 `"file_write"`，capability_scope 为 `["workspace:write"]`
- ToolExecutionContext 中没有 workspace capability
- 使用 CapabilityPermissionPipeline

**操作**
- 调用 authorize()

**断言**
- 结果为 Deny
- Deny 消息包含字符串 `"workspace"`
- Deny 消息包含字符串 `"file_write"`

---

## 意图 3：需要 workspace 的工具在有 workspace 时被 Allow

**场景**
用户已选择工作区，文件操作应该被允许。

**前提**
- 工具定义包含 capability_scope `["workspace:write"]`
- ToolExecutionContext 中有 workspace capability（TempDir 路径）
- 使用 CapabilityPermissionPipeline

**操作**
- 调用 authorize()

**断言**
- 结果为 Allow

---

## 意图 4：python:exec scope 在没有 workspace 时被 Deny

**场景**
Python 执行需要 workspace 作为沙箱边界，缺少 workspace 时不能执行。

**前提**
- 工具定义包含 capability_scope `["python:exec"]`
- ToolExecutionContext 中没有 workspace capability
- 使用 CapabilityPermissionPipeline

**操作**
- 调用 authorize()

**断言**
- 结果为 Deny
- Deny 消息包含字符串 `"workspace"`

---

## 意图 5：需要 browser 的工具在没有 browser capability 时被 Deny

**场景**
用户没有打开 browser connector，浏览相关工具应该被拒绝。

**前提**
- 工具 id 为 `"browse_page"`，capability_scope 为 `["browser"]`
- ToolExecutionContext 中没有 browser capability
- 使用 CapabilityPermissionPipeline

**操作**
- 调用 authorize()

**断言**
- 结果为 Deny
- Deny 消息包含字符串 `"browser"`
- Deny 消息包含字符串 `"browse_page"`

---

## 意图 6：network scope 始终被允许

**场景**
网络访问不在本地 capability 层校验，任何工具的 network scope 都直接放行。

**前提**
- 工具定义包含 capability_scope `["network"]`
- 不提供任何 capability context
- 使用 CapabilityPermissionPipeline

**操作**
- 调用 authorize()

**断言**
- 结果为 Allow

---

## 意图 7：未知 scope 在 CapabilityPipeline 中被 Deny（fail-closed）

**场景**
工具声明了一个系统不认识的 scope，安全起见应该拒绝，而不是默认放行。

**前提**
- 工具定义包含 capability_scope `["custom:unknown"]`
- 使用 CapabilityPermissionPipeline

**操作**
- 调用 authorize()

**断言**
- 结果为 Deny
- Deny 消息包含字符串 `"custom:unknown"`

---

## 意图 8：mcp scope 在 StorePolicyPipeline 中升级为 Ask（而非 Deny）

**场景**
MCP 工具是动态注册的外部工具，系统不认识其 scope，但不应直接拒绝——应让用户决定是否授权。

**前提**
- 工具 id 为 `"mcp__demo__action"`，capability_scope 为 `["mcp"]`
- PermissionStore 中没有任何记录
- 使用 StorePolicyPipeline

**操作**
- 调用 authorize()

**断言**
- 结果为 Ask，不是 Deny
- Ask 消息包含字符串 `"mcp__demo__action"`
- Ask 消息包含字符串 `"external server"` 或 `"MCP"`
- `ask.suggestions` 包含 `"Allow once"` 和 `"Deny"`
- `ask.remember_options` 包含 `Session`、`Workspace`、`User` 三项
- `ask.default_destination` 为 `Session`

---

## 意图 9：StorePolicyPipeline 中已持久化 Allow 时直接放行，即使没有 capability

**场景**
用户曾经授权过该工具，即使当前会话没有加载 workspace 也应该放行——持久化授权不应因 capability 缺失失效。

**前提**
- 工具定义包含 capability_scope `["workspace:write"]`
- PermissionStore 中有该工具该 scope 的 AlwaysAllow 规则（来源 Workspace）
- ToolExecutionContext 中**没有** workspace capability

**操作**
- 调用 authorize()

**断言**
- 结果为 Allow，不是 Deny
- （对比：同样条件下使用 CapabilityPermissionPipeline 会返回 Deny）

---

## 意图 10：StorePolicyPipeline 中已持久化 Deny 时直接拒绝

**场景**
用户曾经明确拒绝了某工具，之后每次调用都应该继续被拒绝，不再 Ask。

**前提**
- 工具 id 为 `"mcp__demo__action"`，scope 为 `"mcp"`
- PermissionStore 中有该工具该 scope 的 AlwaysDeny 规则

**操作**
- 调用 authorize()

**断言**
- 结果为 Deny，不是 Ask
- Deny 消息包含字符串 `"mcp__demo__action"`
- Deny 消息包含字符串 `"stored policy"` 或 `"denied by"`

---

## 意图 11：DontAsk 模式下不出现权限确认弹窗，Ask 自动变为 Deny

**场景**
在全自动运行模式下，所有需要用户确认的操作应该自动被拒绝，不阻塞流程。

**前提**
- 权限管线本来会返回 Ask（工具有 mcp scope，store 无记录）
- 当前对话的 PermissionMode 为 DontAsk

**操作**
- 执行一次完整 turn，LLM 调用该 mcp 工具

**断言**
- EventBus 中不包含 `PermissionAskRequired` 事件
- LLM 收到的 tool_result 为错误类型（is_error = true）
- tool_result 内容包含字符串 `"dontAsk"` 或 `"requires permission"`

---

## 意图 12：Plan 模式下写操作工具的 Ask 自动变为 Deny

**场景**
在只读规划阶段，任何需要用户确认的工具都不应执行，应直接告知 LLM 被拒绝。

**前提**
- 权限管线本来会返回 Ask
- 当前对话的 PermissionMode 为 Plan

**操作**
- 执行一次完整 turn，LLM 调用该工具

**断言**
- EventBus 中不包含 `PermissionAskRequired` 事件
- tool_result 为错误类型（is_error = true）
- tool_result 内容包含字符串 `"plan"` 或 `"read-only"`
