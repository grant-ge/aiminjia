# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## 项目概述

Lotus-App（品牌名 AIjia）是一个 Tauri v2 桌面应用，运行时为 `WebView(React/TS) + Tauri Host(Rust) + 子进程(Python/Playwright)`，主要提供 AI 驱动的数据分析和工作助手功能。

## 常用命令

### 开发

```bash
# 启动 Tauri 开发模式（前端 + 后端热重载）
pnpm tauri:dev

# 仅启动前端 Vite 开发服务器
pnpm dev
```

### 构建

```bash
# 构建生产包（TypeScript 检查 + Vite build + Tauri bundle）
pnpm tauri:build

# 仅构建前端
pnpm build
```

### 测试

```bash
# 前端单测（Vitest）
pnpm test

# 前端关键集成测试（事件联调回归）
pnpm exec vitest run src/lib/tauri.events.test.ts src/hooks/useStreaming.integration.test.tsx src/stores/chatStore.test.ts

# Rust 全部测试
cd src-tauri && cargo test

# Rust 单个测试文件（集成测试在 tests/ 目录）
cd src-tauri && cargo test --test tauri_event_adapter_test -- --nocapture

# Rust review_ 系列回归测试（验证架构约束）
cd src-tauri && cargo test review_ --tests --no-fail-fast

# Rust 按名称过滤单测
cd src-tauri && cargo test <test_name> -- --nocapture
```

### 代码检查

```bash
# 前端 ESLint
pnpm lint
```

## 后端架构（Rust）

### 分层结构（从上到下）

```
transport/tauri_commands/       ← L1: Transport Adapter（Tauri IPC 入口，禁止包含业务逻辑）
runtime/                        ← L2: Session/Query Runtime（核心编排层）
  session_runtime.rs            ← 驱动一次完整 agentic turn
  query_engine.rs               ← 会话级编排，transport-neutral
  tools/                        ← L3: Tool Runtime（工具注册、路由、权限、执行）
  agent/                        ← L4: Task/Agent Runtime（子代理、任务生命周期）
  store/                        ← L5: State Store（repository trait + file-based 实现）
llm/                            ← L6: Infra Adapter（LLM provider、tool_executor）
python/                         ← L6: Infra Adapter（Python 沙箱执行）
storage/                        ← L6: Infra Adapter（文件持久化、workspace 管理）
plugin/                         ← 遗留工具插件系统（正在向 RuntimeTool 迁移）
```

**核心约束：`src-tauri/src/runtime/` 下的模块禁止 `use tauri::*`，通过 `RuntimeHost` trait 注入宿主能力。**

### 消息主链路

```
invoke('send_message')
  → transport/tauri_commands/chat.rs::TauriChatCommandAdapter::send_message()
    → SessionRuntime::run_chat_request()
      → RuntimeChatTurnDriver::run_chat_turn()
        → QueryEngine / ToolDispatcher
          → RuntimeTool / LegacyToolAdapter
      → RuntimeEventBus
        → TauriEventAdapter → app.emit() 发 legacy events 给前端
      → runtime/store/ 持久化
```

### ID 模型

系统内流转的核心标识：`SessionId` > `RunId` > `AgentId` / `ToolCallId`。新增运行态逻辑必须优先使用这套 ID，不再用裸 `conversation_id` 字符串。

### 工具系统（双轨）

- **RuntimeTool**（新）：在 `runtime/tools/dispatcher.rs` 注册，通过 `ToolExecutionContext` + `CapabilityContext` 获取能力，是长期目标路径
- **LegacyToolAdapter**（旧）：将 `plugin/tool_trait.rs` 的 `ToolPlugin` 适配为 `RuntimeTool`，桥接层，不应新增
- 工具实现主体在 `llm/tool_executor/`（upload/load/execute_python/report/chart 等）和 `plugin/builtin/tools/`（browse/extract 等）
- **MCP 工具**（新）：位于 `runtime/mcp/`，通过 `McpConnection -> McpRuntimeTool -> ToolRegistry` 动态注册；对外工具名必须是 `mcp__<server>__<tool>`，disconnect / refresh 时必须同步清理 runtime tool pool 与 `TOOL_CATALOG`

### 事件协议

后端内部发 `RuntimeEvent`，通过 `transport/tauri_event_adapter.rs` 映射为前端 legacy events：

| RuntimeEventKind | 前端 Tauri Event |
|---|---|
| StreamDelta | `streaming:delta` |
| StreamDone | `streaming:done` |
| ToolCallExecuting | `tool:executing` |
| ToolCallCompleted | `tool:completed` |
| PermissionAskRequired | `permission:ask` |
| MessagePersisted | `message:updated` |
| AgentIdle | `agent:idle` |
| TaskStatusChanged | `task:status-changed` |

### MCP 集成

- `src-tauri/src/runtime/mcp/types.rs`：MCP server 配置、tool definition、fully-qualified 命名规则
- `src-tauri/src/runtime/mcp/connection.rs`：MCP 连接抽象，测试和真实传输都走这一层
- `src-tauri/src/runtime/mcp/runtime_tool.rs`：把远端 MCP tool 包装成 `RuntimeTool`
- `src-tauri/src/runtime/mcp/manager.rs`：管理 server 注册 / connect / refresh / disconnect / unregister 生命周期
- Tauri 启动时会在 `src-tauri/src/lib.rs` 中 `app.manage(Arc<McpServerManager>)`
- 当前仓库已具备 runtime 层 MCP 支持，但**还没有** end-user 配置加载器和前端管理面板；若要接真实 server，需要先实现 `McpConnection`，再由宿主层把连接注册到 `McpServerManager`

### Python 沙箱

- 配置入口：`python/sandbox.rs` — `SandboxConfig::for_workspace()` 设置允许路径（写死为 workspace 的 7 个子目录）
- 执行入口：`python/runner.rs` — `PythonRunner`
- 沙箱通过 `_safe_open` 限制写路径，通过 `validate_code()` 静态检查危险模式

## 前端架构（React/TypeScript）

### 关键模块

- `src/lib/tauri.ts` — 所有 Tauri IPC 的类型化封装（invoke + listen），是前后端接口的唯一真相源
- `src/stores/` — Zustand store（chatStore 是核心，管理会话消息、流式状态、工具执行状态）
- `src/hooks/useStreaming.ts` — 订阅 `streaming:delta`/`streaming:done` 事件并更新 chatStore
- `src/hooks/useTauriEvent.ts` — 通用 Tauri 事件订阅 hook

### 事件订阅原则

前端通过 `src/lib/tauri.ts` 中的 `TAURI_EVENTS` 常量订阅事件，不直接使用字符串字面量。

## 存储结构

所有运行时数据持久化到 workspace 目录（`AppStorage`，基于 JSON 文件）：

- `conversations/{id}/` — 对话数据（`conv.json`、`messages.*.jsonl`、`file_index.json`）
- `workspace/uploads/` — 用户上传文件的副本
- `workspace/exports/` / `reports/` / `charts/` / `analysis/` — 生成物
- `shared/memory/` — 跨对话记忆

## 重要架构决策与约束

1. **Tauri command 层只做参数接收 → 转发 Runtime**，不含业务逻辑（见 `docs/architecture-blueprint.md`）
2. **不接受只改 prompt（base.md/daily.md）来修复能力问题**；能力边界应由 runtime/tool/capability/sandbox 保证
3. **新工具应实现 `RuntimeTool` trait**，不应新增 `ToolPlugin` 实现
4. **`CapabilityContext`（`runtime/tools/capability.rs`）是工具获取系统能力的窄接口**，不应扩大它来传入 `LlmGateway`、`AuthManager` 等编排层对象

## 进行中的架构专项

当前有 4 个进行中的架构专项，定义在 `docs/2026-04-12-runtime-gap-problem-statement.md`：

1. **Workspace-First 文件能力模型**（计划：`docs/superpowers/plans/2026-04-12-workspace-first-file-runtime-plan.md`）
2. **Atomic Tool 工具体系**
3. **Prompt Slimming 提示词职责回收**
4. **Skill 本地导入/打包导入模型统一**

架构总蓝图：`docs/architecture-blueprint.md`；分期计划索引：`docs/superpowers/plans/README.md`

## 集成测试文件命名惯例

`src-tauri/tests/` 下：
- `review_*.rs` — 架构约束回归测试，验证各期实施后约束不被破坏
- `*_integration_test.rs` — 跨模块集成测试
- `*_test.rs` — 针对单个功能的集成测试
