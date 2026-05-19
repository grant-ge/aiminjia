# rules.md — 权限管线决策测试意图

权限管线决定一个工具调用能不能执行：Allow、Deny 还是需要问用户（Ask）。

> **注意**：意图 1–10 原本测试的是 `authorize()` 函数内部返回值（空 scope Allow、workspace Deny/Allow、python:exec Deny、browser Deny、network Allow、unknown scope Deny、mcp Ask、持久化 Allow/Deny），属于 Rust 单元测试的范畴，由 `src-tauri/tests/` 覆盖，不在意图测试框架的产品视角范围内，已删除。下面保留的两条意图来自原意图 11、12，涉及完整 turn 触发权限判断，属于可观察的产品行为。

---

## 意图 1：DontAsk 模式下，LLM 调用 MCP 工具不会弹出权限确认弹窗，工具结果报告执行被拒

**场景**
在全自动运行模式下，所有需要用户确认的工具调用应自动被拒绝，不弹窗、不阻塞对话流程。

**前提**
- 应用已配置有效 API key，对话可正常发起
- 已注册至少一个 MCP 工具（`~/.renlijia/mcp_servers.json` 中有有效 server 配置），且 `permissions.json` 中没有该工具的持久化授权记录
- 新建对话，将该对话的权限模式设置为 DontAsk

**操作**
- 在输入框输入能让 LLM 调用该 MCP 工具的消息，点击发送
- 等待 turn 结束

**验收标准**
- 整个 turn 过程中，应用界面不出现权限确认弹窗（即不触发 `permission:ask` 事件）
- LLM 收到的 tool_result 为错误类型（`is_error = true`）
- tool_result 文本内容包含 `"dontAsk"` 或 `"requires permission"` 字样，向 LLM 说明执行被拒原因
- 对话历史中可���看到 LLM 的回复（turn 未卡死）

---

## 意图 2：Plan 模式下，LLM 调用需要用户确认的工具时不弹窗，工具结果报告执行被拒

**场景**
在只读规划阶段，任何需要用户确认的工具都不应执行，应直接告知 LLM 被拒绝，不弹窗打断用户思路。

**前提**
- 应用已配置有效 API key，对话可正常发起
- 注册了一个在当前 `permissions.json` 中无持久化授权记录的工具（MCP 工具或其他需要 Ask 的工具）
- 新建对话，将该对话的权限模式设置为 Plan

**操作**
- 在输入框输入能让 LLM 调用该工具的消息，点击发送
- 等待 turn 结束

**验收标准**
- 整个 turn 过程中，应用界面不出现权限确认弹窗（即不触发 `permission:ask` 事件）
- LLM 收到的 tool_result 为错误类型（`is_error = true`）
- tool_result 文本内容包含 `"plan"` 或 `"read-only"` 字样
- 对话历史中可以看到 LLM 的回复（turn 未卡死）
