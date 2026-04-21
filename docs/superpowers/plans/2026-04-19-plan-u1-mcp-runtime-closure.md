# MCP 真闭环与工具暴露收口（Plan-U1）

> **For agentic workers:** REQUIRED SUB-SKILL: `superpowers:test-driven-development` — transport、manager、UI 状态必须先有回归测试再改实现。 REQUIRED SUB-SKILL: `superpowers:verification-before-completion` — 关闭任务前必须跑通 Rust 集成测试与前端状态测试。

**Goal:** 把 lotus-app 当前“可配置、可连接、不可执行”的 MCP 占位链路收口为真实可用的本地 MCP 运行时；没有真实 transport / handshake / tool list 的 server 不得暴露为可用能力。

**Architecture:** 对标 `claude-code-best` 的真实 MCP client/runtime boundary，lotus 本期只覆盖本地桌面必须的 transport 与 tool registration：先做 local `stdio`，保留 `http/sse` 扩展点，但不把未实现 transport 暴露给 UI。所有 MCP 工具继续走 `RuntimeTool + ToolDispatcher` 主路径，不回退 legacy plugin。

**Tech Stack:** Rust, Tauri v2, React/TypeScript

**Worktree branch:** pzc

---

## 背景与现状

| 文件 | 现状 |
|---|---|
| `src-tauri/src/runtime/mcp/connection.rs` | `PendingMcpConnection::connect()` 仅把 `connected=true`，`list_tools()` 恒空，`call_tool()` 恒返回 `ToolExecutionFailed` |
| `src-tauri/src/runtime/mcp/manager.rs` | `connect()` 只要 `connect()` 不报错就会继续 `register_mcp_server()`，缺少“握手成功且拿到工具”这一层语义 |
| `src-tauri/src/transport/tauri_commands/mcp.rs` | `connect_mcp_server` 直接返回 `Vec<String>`，无法表达 `configured / unsupported / failed / ready` 等状态 |
| `src/components/settings/McpTab.tsx` | 点击连接后会 toast “connect success”，用户很容易误以为 server 已经可用 |

### 这条链路为什么不合理

- 它把“配置存在”和“真实可执行”混成了同一件事。
- UI 会向用户承诺一条还没有 transport 实现的能力。
- 权限、tool pool、subagent 后续都无法基于可靠的 MCP 能力边界工作。

## 范围

- 纳入：
  - 本地 transport 真实现，优先 `stdio`
  - server 状态机、错误状态、tool list 注册/卸载
  - 设置面板与后端返回状态语义对齐
- 不纳入：
  - 托管 MCP registry / proxy / OAuth
  - 组织级 server 同步
  - MCP 市场、发现页等产品型扩展

## 任务拆分

### U1-1：让 `PendingMcpConnection` 退出生产路径

- [ ] 新建真实 transport 实现（优先 `StdioMcpConnection`），把 handshake / initialize / `list_tools` / `call_tool` 跑通。
- [ ] `PendingMcpConnection` 仅保留为 test double 或显式“未实现”状态，不再作为设置面板默认注册对象。
- [ ] `McpServerConfig.transport_type` 的解析改为 factory；未实现 transport 直接返回结构化 `UnsupportedTransport`。

### U1-2：重做 `McpServerManager` 的 server 状态机

- [ ] 区分 `configured / connecting / ready / failed / disconnected` 五种状态。
- [ ] 只有 `ready` 且 `list_tools()` 成功后才注册 runtime tools。
- [ ] `disconnect / unregister` 必须反向清理 runtime tool ids、错误状态与缓存的 tool list。
- [ ] 失败状态保留 `last_error`，便于 UI 与日志诊断。

### U1-3：把设置面板从“乐观成功”改成“真实状态反馈”

- [ ] `connect_mcp_server` 返回结构化 DTO（状态、tool_count、last_error），不再只返回 `Vec<String>`。
- [ ] `McpTab` / `McpServerList` 统一展示真实状态；只有 `ready` 才显示“已连接”。
- [ ] 对未实现 transport 或握手失败显示错误，而不是 success toast。
- [ ] 无工具 server 不应被视为可用能力，除非明确声明这是合法场景并在 UI 中标注。

### U1-4：回归测试与验收

- [ ] 为 transport handshake、tool registration、disconnect cleanup、unsupported transport 写 Rust 集成测试。
- [ ] 增加 UI 层状态渲染测试：`ready`、`failed`、`unsupported` 三种状态都能正确反馈。
- [ ] 验证没有真实 tool list 时，主 tool pool / schema surface 中不出现对应 `mcp__*` 工具。

## 验收标准

- 用户在设置里看到“已连接”时，对应 MCP 工具必须真的能执行。
- 未实现 transport 的 server 不再通过“连接成功”误导用户。
- `mcp__*` 工具只在真实 `ready` 状态下进入 tool pool。
- 整条链路不引入任何远程托管依赖。
