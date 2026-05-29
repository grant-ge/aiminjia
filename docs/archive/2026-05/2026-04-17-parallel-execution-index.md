# 架构对齐计划索引 — 并行执行指南

**日期**：2026-04-17  
**基于 Roadmap**：`docs/2026-04-17-architecture-roadmap.md`

---

## 计划拆分总览

共 **8 个实施计划**，分 4 批并行执行。

---

## 第一批（立即可并行，互不依赖）

> **Sprint 0 正在执行中，以下三个计划可同时开 worktree 并行推进。**

### Plan-A：P0 Bug 修复
**文件**：`docs/superpowers/plans/2026-04-17-plan-a-p0-bugfix.md`（待写）  
**Worktree**：`branch: fix/p0-bugfix`  
**内容**：
- P0-A：Cancel 后 synthetic tool_result 注入（对话 cancel 后 API 不崩溃）
- P0-B：权限 Ask 路径接通（后端 AskRequired → RuntimeEvent → 前端弹框）
- P0-C 正确性修复（6 项）：
  - `run_registry.rs` std::Mutex → tokio::sync::Mutex（panic 风险）
  - `useChat.ts` stop+retry 竞态（generation token）
  - `python/session.rs` session key 改为 per-run（数据污染）
  - `python/sandbox.rs` path 边界绕过修复
  - `context_builder.rs` build_env_info 阻塞 tokio 线程
  - `chat.rs` Executor AppHandle 抽为 trait 注入

**依赖**：无  
**可并行**：✅ 与 Sprint 0、Plan-B、Plan-D 完全独立

---

### Plan-B：Session State Owner + Turn 健壮性
**文件**：`docs/superpowers/plans/2026-04-17-plan-b-session-state.md`（待写）  
**Worktree**：`branch: feat/session-state-owner`  
**内容**：
- QueryEngine 持有 `read_file_state: FileStateCache`（跨 turn 复用，对标 claude-code-best）
- QueryEngine 持有 `total_usage`（token 用量跨 turn 累积）
- Turn 内部多处 cancel checkpoint（streaming 中、工具前、工具后）
- Turn state 改为不可变更新（每次 continue 创建新 State object）

**依赖**：Sprint 0 Task 1.2（FileStateCache 类型定义）  
**可并行**：✅ 与 Plan-A、Plan-C、Plan-D 独立（改不同文件）

---

### Plan-D：工具结果预算
**文件**：`docs/superpowers/plans/2026-04-17-plan-d-tool-result-budget.md`（待写）  
**Worktree**：`branch: feat/tool-result-budget`  
**内容**：
- 每个 RuntimeTool 声明 `max_result_size_chars`
- `ToolResult` 超限截断逻辑
- TurnDriver 维护 per-turn `content_budget` 全局追踪
- 超限工具结果附通知（模型知道被截断）

**依赖**：Sprint 0 Task 2.1（RuntimeTool trait 谓词方法，同期扩展声明字段）  
**可并行**：✅ 与 Plan-A、Plan-B、Plan-C 独立

---

## 第二批（Sprint 0 完成后可并行）

### Plan-C：bash / file 基础工具集
**文件**：`docs/superpowers/plans/2026-04-17-plan-c-bash-file-tools.md`（待写）  
**Worktree**：`branch: feat/bash-file-tools`  
**内容**：
- `BashTool`：命令执行，绑定 CancellationToken，timeout 后台化，`check_permissions`
- `WriteFileTool`：文件写入，更新 FileStateCache
- `EditFileTool`：基于 diff 的文件编辑，写入文件历史
- `GrepTool`（完整版）：regex 支持，`is_concurrency_safe = true`

**依赖**：Sprint 0 全部（Task 1.2 FileStateCache + Task 2.1 谓词 + Task 3.1 check_permissions）+ Plan-A（Ask 路径，bash 需要权限确认）  
**可并行**：✅ 与 Plan-E 独立（改不同文件）

---

### Plan-E：核心工具完整迁移（python / report / chart）
**文件**：`docs/superpowers/plans/2026-04-17-plan-e-tool-migration.md`（待写）  
**Worktree**：`branch: feat/core-tool-migration`  
**内容**：
- `ExecutePythonRuntimeTool` 完整实现（`PythonExecution` trait 注入，脱离 PluginContext）
- `GenerateReportRuntimeTool` 完整实现（`ReportCapability` trait 注入）
- `GenerateChartRuntimeTool` 完整实现（`ChartCapability` trait 注入）
- 参照：`docs/2026-04-16-execute-python-migration-boundary.md`

**依赖**：Sprint 0 全部 + Plan-B（Session state，python 需要跨 turn session）  
**可并行**：✅ 与 Plan-C 独立

---

## 第三批（第二批完成后）

### Plan-F：设计债系统清理
**文件**：`docs/superpowers/plans/2026-04-17-plan-f-design-debt.md`（待写）  
**Worktree**：`branch: refactor/design-debt`  
**内容**（按独立性分为可并行的子组）：

| 子任务 | 文件 | 可独立 commit |
|--------|------|-------------|
| D1：Schema/注册/执行一致性校验 | `registry.rs` | ✅ |
| D4：权限管线重复逻辑 → 责任链 | `permission.rs` | ✅ |
| D8：Prompt 构建统一入口 | `context_builder.rs` + `prompts.rs` | ✅ |
| D11：settings 每步重读 → turn 入口一次 | `chat.rs` | ✅ |
| F1：chatStore 拆分 | `src/stores/` | ✅ |
| F3：useTauriEvent listener 泄露 | `src/hooks/` | ✅ |
| R2：SessionId newtype 全面推广 | 全局搜替 | ✅ |

**依赖**：Plan-A/B/C/D/E 完成（稳定后再统一清理）  
**可并行**：✅ 子任务之间互相独立，可多人各取一个 commit

---

### Plan-G：MCP 支持
**文件**：`docs/superpowers/plans/2026-04-17-plan-g-mcp.md`（待写）  
**Worktree**：`branch: feat/mcp-support`  
**内容**：
- MCP client 连接管理（server 生命周期）
- MCP 工具动态发现 → 注册到 ToolRegistry
- TOOL_CATALOG 改为动态可注册（D10）
- MCP 工具权限集成（capability scope = `mcp`）

**依赖**：Plan-D（工具结果预算）+ Plan-F（TOOL_CATALOG 动态化）  
**可并行**：✅ 与 Plan-H 独立

---

## 第四批（独立，可在第三批同期规划）

### Plan-H：Subagent 状态隔离
**文件**：`docs/superpowers/plans/2026-04-17-plan-h-subagent-isolation.md`（待写）  
**Worktree**：`branch: feat/subagent-isolation`  
**内容**：
- `AgentContext` 独立（独立 file state、独立 messages buffer）
- subagent 的 `setAppState` 变隔离写（父代理不受影响）
- 父子 cancel 级联（parent abort → child abort）
- subagent 结果汇报协议

**依赖**：Plan-B（Session state owner）  
**可并行**：✅ 与 Plan-G 独立

---

## 并行执行图

```
现在
├── Sprint 0（进行中）────────────────────────────────────────────┐
├── Plan-A（P0 bug，立即）                                        │
├── Plan-B（Session state，等 Sprint 0 Task 1.2）                 │
└── Plan-D（工具结果预算，等 Sprint 0 Task 2.1）                  │
                                                                  ↓
                                                        Sprint 0 全部完成
                                                                  │
                                    ┌─────────────────────────────┤
                                    ▼                             ▼
                               Plan-C                        Plan-E
                          (bash/file 工具)              (python/report/chart)
                               │                             │
                               └──────────┬──────────────────┘
                                          ▼
                                Plan-F（设计债清理）
                                Plan-G（MCP）────────┐
                                Plan-H（Subagent）───┘
```

---

## 哲学级专项（Φ1-Φ4）

> 这四项不是修修补补，需要各自独立 brainstorm → spec → plan 周期。**不在当前 8 个计划范围内，第三批完成后再立项。**

| 专项 | 依赖 | 简述 |
|------|------|------|
| Φ1：安全边界统一 | Plan-A（Ask 路径） | 删多层检查，permission pipeline 为唯一边界 |
| Φ2：Hook 系统 | Plan-F 完成后 | PreToolUse/PostToolUse/Stop 钩子架构 |
| Φ3：Context Window 主动管理 | Plan-B + Plan-D | token 预算 + auto-compact 策略 |
| Φ4：Subagent 并发基础设计 | Plan-H | 并发 agent 资源调度和结果聚合 |

---

## 每个计划的写作状态

| 计划 | 状态 | 计划文件 |
|------|------|---------|
| Sprint 0（工具系统基础） | ✅ 已有计划 | `2026-04-17-atomic-tool-capability-upgrade-plan.md` |
| Plan-A（P0 bug） | ❌ 待写 | `2026-04-17-plan-a-p0-bugfix.md` |
| Plan-B（Session state） | ❌ 待写 | `2026-04-17-plan-b-session-state.md` |
| Plan-C（bash/file 工具） | ❌ 待写 | `2026-04-17-plan-c-bash-file-tools.md` |
| Plan-D（工具结果预算） | ❌ 待写 | `2026-04-17-plan-d-tool-result-budget.md` |
| Plan-E（核心工具迁移） | ❌ 待写 | `2026-04-17-plan-e-tool-migration.md` |
| Plan-F（设计债清理） | ❌ 待写 | `2026-04-17-plan-f-design-debt.md` |
| Plan-G（MCP） | ❌ 待写 | `2026-04-17-plan-g-mcp.md` |
| Plan-H（Subagent 隔离） | ❌ 待写 | `2026-04-17-plan-h-subagent-isolation.md` |
