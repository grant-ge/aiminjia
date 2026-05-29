# 剩余非云端关键差距收口总纲（Plan-U）

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 把 lotus-app 相对 `claude-code-best` 仍然存在、且与真实使用直接相关的剩余非云端关键差距拆成可执行批次：MCP 真闭环、权限治理、长会话上下文预处理管道、记忆 runtime-native、subagent worker runtime、PluginContext bridge 退出热路径。

**Architecture:** 以 `claude-code-best` 的五层架构、权限模型、agentic loop、project memory、sub-agent runtime 为对标基线；lotus 本批次只做本地桌面场景必须收口的运行时链路，不补云端能力，不做泛产品面扩张。`Plan-U` 本身只负责总纲、边界和依赖关系，具体实现拆到 `Plan-U1` 到 `Plan-U6`。

**Tech Stack:** Rust, Tauri v2, TypeScript/React, Zustand

**Worktree branch:** pzc

---

## 用户裁剪范围（2026-04-19）

- 不做远程登录、远程会话、托管同步、云端设置恢复等链路。
- 不做泛产品能力扩张；只收口当前桌面本地链路里“已经露出入口，但实际边界不稳”的问题。
- 不重复覆盖已单列或已基本收口的方向：`Plan-V`（安全边界）、`Plan-AA`（Prompt Caching）、`Plan-AC`（CLAUDE.md 加载）、`Plan-AE`（本地/工作区/会话模型分层）、`Plan-AH` / `Plan-AI`（工具合约与提示词收口）。

## 为什么改成多计划

- 这些差距分布在 `runtime/mcp`、`permission pipeline`、`chat turn driver`、`memory`、`subagent runtime`、`plugin bridge` 六条不同写集。
- 如果继续塞回单一 `Plan-U`，执行时会把验证口、依赖顺序和回归范围搅在一起，最后很难判断哪条链路真正关掉了。
- 拆分后可以让每条链路有独立验收条件，同时仍保留一个总纲把顺序和非目标钉住。

## 现状速览

| Gap | 当前症状 | 主要文件 | 对应计划 |
|---|---|---|---|
| U1 | MCP 已有配置面板与连接动作，但 `PendingMcpConnection` 仍是占位实现，连接成功不代表可执行 | `src-tauri/src/runtime/mcp/connection.rs`、`src-tauri/src/runtime/mcp/manager.rs`、`src-tauri/src/transport/tauri_commands/mcp.rs`、`src/components/settings/McpTab.tsx` | `Plan-U1` |
| U2 | 权限模型仍是 `Default/DontAsk` + 扁平 `tool:scope` 持久化；Ask UI 也只有 allow/deny/cancel，没有 remember / destination / mode 语义 | `src-tauri/src/runtime/tools/permission.rs`、`src-tauri/src/runtime/store/permission_store.rs`、`src/components/common/PermissionAskDialog.tsx`、`src/App.tsx` | `Plan-U2` |
| U3 | 主循环只有 `microcompact + auto_compact + chars/4` 观测，没有完整预处理管道与统一恢复顺序 | `src-tauri/src/runtime/chat/chat_turn_driver.rs`、`src-tauri/src/runtime/chat/compaction.rs`、`src-tauri/src/llm/context_decay.rs` | `Plan-U3` |
| U4 | 记忆仍停留在 `core_memory + legacy memory tools` 混合态，没有 runtime-native 本地记忆主线 | `src-tauri/src/plugin/builtin/tools/memory_*.rs`、`src-tauri/src/llm/tool_executor/memory.rs`、`src-tauri/src/runtime/chat/chat_turn_driver.rs` | `Plan-U4` |
| U5 | subagent 仍自己跑一套 legacy loop，没有复用主 turn driver / shared runtime contract | `src-tauri/src/llm/sub_agent.rs`、`src-tauri/src/runtime/agent/agent_runtime.rs`、`src-tauri/src/runtime/query_engine.rs` | `Plan-U5` |
| U6 | `PluginContext` 与 `ToolRegistry::execute()/to_runtime_dispatcher()` 仍在热路径桥接 request-scoped / legacy tools | `src-tauri/src/plugin/context.rs`、`src-tauri/src/plugin/registry.rs`、`src-tauri/src/runtime/tools/legacy_adapter.rs` | `Plan-U6` |

## 执行顺序与依赖

1. `Plan-U1`：先把 MCP 假闭环收口，避免产品继续暴露“能连不能用”的入口。
2. `Plan-U2`：统一权限治理和 Ask 语义，给 MCP / subagent / request-scoped tool 共用。
3. `Plan-U3`：补齐长会话预处理管道，稳定主循环的上下文治理。
4. `Plan-U4`：在新上下文链路上接入本地记忆 runtime。
5. `Plan-U6`：先让 `PluginContext` 退出热路径，为 subagent runtime-first 清桥。
6. `Plan-U5`：最后把 subagent 升级成一等 worker runtime，复用前面收口结果。

### 依赖说明

- `Plan-U4` 建议在 `Plan-U3` 之后执行，因为记忆注入要挂在新的上下文预处理链上。
- `Plan-U5` 依赖 `Plan-U2` + `Plan-U6`；否则 worker 的权限、工具池和桥接边界仍会分叉。
- 若 `Plan-AI` 仍未执行，先冻结 `analysis mode` 的分叉逻辑，再推进 `Plan-U3/U6`，避免形成双轨预处理与双轨 tool surface。

## 子计划清单

### `Plan-U1` — MCP 真闭环与工具暴露收口

文件：`docs/superpowers/plans/2026-04-19-plan-u1-mcp-runtime-closure.md`

- 目标：把 MCP 从“可配置/可连接但不可执行”的占位实现收口为真实 transport + runtime tool 注册链路。
- 验收口：只有真实握手成功且拿到 tool list 的 server 才能进入 tool pool。

### `Plan-U2` — 权限治理与 Ask/Remember 语义统一

文件：`docs/superpowers/plans/2026-04-19-plan-u2-permission-governance.md`

- 目标：把当前扁平权限模型升级为本地多层规则 + 完整 Ask/remember 语义。
- 验收口：主线程、MCP、subagent 对同一权限规则做出一致裁决。

### `Plan-U3` — 长会话上下文预处理管道补齐

文件：`docs/superpowers/plans/2026-04-19-plan-u3-context-pipeline.md`

- 目标：把主循环改造成可预测、可回归测试的预处理管道，而不是 scattered compact 逻辑。
- 验收口：正常回合和 prompt-too-long 恢复路径复用同一 pipeline 顺序。

### `Plan-U4` — 本地记忆 Runtime-Native 化

文件：`docs/superpowers/plans/2026-04-19-plan-u4-memory-runtime-native.md`

- 目标：把 `core_memory + legacy memory tools` 过渡到本地文件式记忆主线。
- 验收口：记忆加载、召回、保存都有 runtime-native 入口，不再依赖 legacy `memory_*` ToolPlugin。

### `Plan-U5` — Subagent 一等 Worker Runtime 收口

文件：`docs/superpowers/plans/2026-04-19-plan-u5-subagent-worker-runtime.md`

- 目标：让 subagent 复用共享 runtime contract，而不是继续维护一套独立 legacy loop。
- 验收口：worker 的权限、取消、转录、背景运行与主 runtime 一致。

### `Plan-U6` — PluginContext 热路径退出与 Request-Scoped Tool 运行时化

文件：`docs/superpowers/plans/2026-04-19-plan-u6-plugin-context-bridge-exit.md`

- 目标：让 request-scoped tool 与 transport 主路径不再依赖全能 `PluginContext`。
- 验收口：生产热路径里不再新增 `PluginContext` 依赖，`ToolRegistry::execute()` 退回纯 legacy 岛。

## 批次完成定义

当以下条件全部成立时，`Plan-U` 可视为关闭：

- MCP 设置面板不再把占位 transport 当作可用能力。
- Ask / Allow / Deny / Remember / Mode 语义在主线程、MCP、subagent 三条链路一致。
- chat loop 的上下文预处理顺序有稳定的回归测试锁定。
- 记忆系统走本地 runtime-native 主线，不再依赖 legacy `memory_*` 工具族。
- subagent 与 request-scoped tool 不再通过 `PluginContext` 拼装隐式全能上下文。

## 非目标

- 新增云端身份、远程 worker、团队记忆、远端同步。
- 扩展大量不影响当前本地桌面主链路的产品能力。
- 为了对标而复制 `claude-code-best` 的所有产品面；本批次只收口当前 lotus 已经暴露给用户的关键运行时边界。
