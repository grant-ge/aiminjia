# MCP 集成说明

本文档说明 lotus-app 当前已经落地的 MCP（Model Context Protocol）运行时集成边界，以及接入真实 MCP server 时应遵守的约束。

## 当前状态

当前代码库已经具备以下能力：

- `runtime/mcp/` 下有独立的 MCP 运行时层，不依赖 `tauri::*`
- MCP server 工具可以动态注册为一等 `RuntimeTool`
- 动态工具会同步进入 `TOOL_CATALOG`，并在 disconnect / refresh 时清理
- 工具名统一使用 fully-qualified 形式：`mcp__<server>__<tool>`
- `McpServerManager` 已在 Tauri 启动时通过 `app.manage(...)` 注入应用状态

当前还**没有**以下能力：

- 没有内置的 stdio / HTTP / SSE 真实传输实现
- 没有从配置文件自动加载 MCP server 的 loader
- 没有前端 MCP server 管理面板

也就是说，runtime 主链路已经对齐，但真实 server 接入仍需要宿主层补一个 `McpConnection` 实现。

## 代码落点

- `src-tauri/src/runtime/mcp/types.rs`
  - `McpServerConfig`
  - `McpToolDefinition`
  - `build_mcp_tool_name(...)`
- `src-tauri/src/runtime/mcp/connection.rs`
  - `McpConnection`
  - `McpError`
  - `McpResult`
- `src-tauri/src/runtime/mcp/runtime_tool.rs`
  - `McpRuntimeTool`
- `src-tauri/src/runtime/mcp/manager.rs`
  - `McpServerManager`
  - `McpServerStatus`
- `src-tauri/src/plugin/registry.rs`
  - `register_mcp_server(...)`
  - `unregister_runtime_tools(...)`
- `src-tauri/src/lib.rs`
  - 启动时构造并 `app.manage(Arc<McpServerManager>)`

## 架构约束

### 1. 工具名必须 fully-qualified

所有 MCP 工具在 lotus 内部都必须使用：

```text
mcp__<server>__<tool>
```

目的：

- 避免与 builtin / 其他 server 的工具撞名
- 让 permission、catalog、tool pool、analytics 共享同一个稳定 id
- 让 disconnect / refresh 时可以精确清理动态工具

注意：`McpRuntimeTool::execute()` 真正发给远端 server 的仍是原始 `tool_name`，不是 fully-qualified 名。

### 2. catalog / runtime tool pool 必须双向同步

MCP tool connect 时：

1. `McpServerManager::connect(...)`
2. `ToolRegistry::register_mcp_server(...)`
3. runtime tool 注册进 dispatcher
4. schema 注册进 `TOOL_CATALOG`

MCP tool disconnect / refresh / unregister 时：

1. `ToolRegistry::unregister_runtime_tools(...)`
2. 同时移除 runtime tool
3. 同时移除 `TOOL_CATALOG` entry

不能只删其中一边，否则会留下 stale tool pool。

### 3. manager 不得持锁跨 `await`

`McpServerManager` 只在拿到连接和旧 tool ids 时短暂持锁；真正的：

- `connect()`
- `disconnect()`
- `list_tools()`
- `register_mcp_server()`

都在锁外执行，避免把 server 网络/进程等待时间带进 `RwLock` 临界区。

### 4. 权限范围用 `mcp`

`McpToolDefinition::to_tool_definition()` 会自动附加 capability scope `mcp`。

当前语义是：

- lotus 本地 permission pipeline 允许已注册的 `mcp` 工具执行
- 更细粒度的权限与审计由远端 MCP server 自己负责

这意味着如果后续要做更严格的本地审批，需要在现有 `mcp` scope 基础上继续扩展，而不是回退到 legacy plugin 路径。

## 生命周期

### 注册但不连接

```rust
manager.register(connection).await?;
```

这一步只把 server 放进 manager，不会把工具暴露给 dispatcher。

### 连接并注册工具

```rust
let tool_ids = manager.connect("my-server").await?;
```

效果：

- 必要时先连接远端 server
- 调 `list_tools()`
- 为每个 tool 创建 `McpRuntimeTool`
- 注册到 `ToolRegistry`
- 同步到 `TOOL_CATALOG`
- 返回这次注册出的 fully-qualified tool ids

### 刷新工具集

```rust
let tool_ids = manager.refresh("my-server").await?;
```

效果：

- 清理旧 tool ids
- 重新读取远端工具列表
- 注册新 tool ids

### 断开连接

```rust
manager.disconnect("my-server").await?;
```

效果：

- 从 runtime tool pool 移除该 server 注册过的所有工具
- 从 `TOOL_CATALOG` 移除对应 entry
- 断开远端连接

### 注销 server

```rust
manager.unregister("my-server").await?;
```

效果：

- 从 manager 中删除 server
- 清理 runtime tool 与 catalog entry
- 如仍连接，则主动断开

## 接入真实 MCP server 的最小步骤

### 第一步：实现 `McpConnection`

你需要提供一个实现了 `McpConnection` trait 的连接对象，负责：

- 建连
- 断连
- `list_tools()`
- `call_tool(name, arguments)`
- 暴露 `McpServerConfig`

测试里已有多个 mock 参考：

- `src-tauri/tests/mcp_runtime_tool_test.rs`
- `src-tauri/tests/mcp_registry_integration_test.rs`
- `src-tauri/tests/mcp_server_manager_test.rs`
- `src-tauri/tests/mcp_e2e_workflow_test.rs`

### 第二步：把连接注册进 `McpServerManager`

```rust
let manager = app.state::<Arc<McpServerManager>>().inner().clone();
manager.register(connection.clone()).await?;
manager.connect(connection.server_name()).await?;
```

### 第三步：让 LLM 看见这些工具

一旦 `connect(...)` 完成：

- 工具已经在 runtime dispatcher 中可执行
- `TOOL_CATALOG` 中已经有 schema
- `QueryEngine` / tool dispatch 主链路会像 builtin tool 一样看到这些工具

不需要再额外走 legacy plugin 注册。

## 当前限制与下一步

当前实现是“runtime 层打通”，还没有做这些外围能力：

1. 真实传输实现（stdio / HTTP / SSE）
2. 配置文件加载器
3. 前端 server 管理 UI
4. 更细粒度的本地审批 / 审计
5. MCP tool progress 的事件流式映射

如果后续继续对标 `claude-code-best`，下一批更像是“宿主层和产品层补齐”，而不是再回头重写当前 runtime/mcp 主体。
