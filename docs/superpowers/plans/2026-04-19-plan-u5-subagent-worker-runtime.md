# Subagent 一等 Worker Runtime 收口（Plan-U5）

> **For agentic workers:** REQUIRED SUB-SKILL: `superpowers:subagent-driven-development` — 子任务执行与 review 必须按 worker 边界拆开。 REQUIRED SUB-SKILL: `superpowers:verification-before-completion` — 关闭任务前必须证明 worker 与主 runtime 在权限、取消、转录上的语义一致。

**Goal:** 把 lotus 当前 subagent 从一套自维护的 legacy loop 收口为一等 worker runtime：共享 turn driver、权限、事件、转录与取消语义，不再长期维护一套平行实现。

**Architecture:** 对标 `claude-code-best/docs/agent/sub-agents.mdx` 的 worker runtime 思路，但保持本地桌面范围：不做 worktree、远程 worker、云端协调器。worker 只复用共享 runtime contract、允许受限工具池、独立 transcript 与 background 生命周期。

**Boundary update (2026-04-20):** U5 直接走 runtime-only / worker-runtime-first，不再为 `PluginContext`、旧工具桥接或其它 legacy subagent loop 保留兼容层。`sub_agent.rs` 退化为薄入口，真正的 loop owner 必须下沉到 `runtime/agent/worker_runtime.rs`。

**Tech Stack:** Rust, async runtime, Tauri event bus

**Worktree branch:** pzc

---

## 依赖

- 依赖 `Plan-U2`：worker 的 Ask / mode 语义必须与主线程共享。
- 依赖 `Plan-U6`：在 `PluginContext` 热路径退场前，subagent 很难真正 runtime-first。

## 背景与现状

| 文件 | 现状 |
|---|---|
| `src-tauri/src/llm/sub_agent.rs` | subagent 自己维护消息数组、流式循环、tool dispatch 和 Ask 冒泡，几乎平行复制了一套小型主循环 |
| `src-tauri/src/runtime/agent/agent_runtime.rs` | 目前更像 invocation/transcript store，不是完整 worker runtime owner |
| `src-tauri/src/runtime/query_engine.rs` | 主 runtime 已经有工具 round、AskRequired、事件总线语义，但 subagent 没有完整复用 |
| `src-tauri/src/plugin/registry.rs` | subagent 仍通过 `to_runtime_dispatcher(sub_plugin_ctx)` 组装工具池，带着明显 bridge 痕迹 |

### 当前问题

- 主线程和 subagent 各自维护一套 loop，行为容易漂移。
- Ask、cancel、background completion、tool result 合成逻辑在两边重复。
- 这会让 worker 越来越像“半独立产品”，而不是共享 runtime 的一个执行单元。

## 范围

- 纳入：
  - worker request / worker runtime / transcript 主线
  - allowed tools、Ask 冒泡、取消级联、background completion 统一
  - 共享 turn driver 与事件语义
- 不纳入：
  - worktree 隔离
  - 远程 worker / coordinator / team runtime
  - 额外的多模型编排产品面

## 任务拆分

### U5-1：定义 worker runtime contract

- [x] 新建 `WorkerTurnRequest` / `WorkerRunConfig` 等结构，让 worker 复用主 turn driver 所需的最小配置。
- [x] worker 不再直接在 `llm/sub_agent.rs` 内手写一套流式 LLM loop，而是由 `runtime/agent/worker_runtime.rs` 成为 loop owner，`sub_agent.rs` 只保留入口封装。
- [x] `allowed_tools`、`parent_run_id`、`background`、`transcript_ref` 等 worker 专属约束通过显式字段表达。

### U5-2：统一权限与 Ask 冒泡

- [x] worker 的 `AskRequired` 直接复用主 runtime 的 permission control plane，不再自带一套 pending ask 处理分支。
- [x] worker 的 mode、remember、destination 语义全部继承 `Plan-U2` 的统一实现。
- [x] MCP 与 request-scoped tool 在 worker 里仍经过同一权限管线。

### U5-3：统一取消、背景运行与转录

- [x] 父 run 的取消必须级联到 worker；worker 自身取消也要正确回写 invocation status。
- [x] background worker 完成后，摘要、结果引用、`AgentIdle` 事件都走 `AgentRuntime + RuntimeEventBus` 主线。
- [x] transcript 持久化继续保留，但挂到统一 worker lifecycle，而不是 scattered completion hooks。

### U5-4：工具池与消息边界对齐

- [x] worker 的 tool pool 由共享 runtime registry + `ToolRoundDriver` 组装并执行，不再在 `sub_agent.rs` 内重复筛 schema + 重建 dispatch 语义。
- [x] assistant/tool_result 消息结构必须与主 turn driver 保持兼容，避免子任务 transcript 成为特例格式。
- [x] 保留前端 transcript viewer 所需结构，但不新增新的 envelope 分叉。

### U5-5：回归测试

- [x] 覆盖 allowed tool filter、AskRequired 冒泡、cancel cascade、background completion、transcript parity。
- [x] 增加 review test，锁定“worker 不再维护独立 legacy loop”这一约束。
- [x] 验证主线程与 worker 对同一工具错误、权限、取消事件的表现一致。

## 验收标准

- `llm/sub_agent.rs` 不再长期承载一套独立的主循环实现。
- worker 与主线程共享权限、取消、事件、tool result 语义。
- background worker 结束后，UI 能收到统一、稳定的 runtime 事件。
- 整条 worker runtime 仍是本地-only，不引入远程调度前提。
