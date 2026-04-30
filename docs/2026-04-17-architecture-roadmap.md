# lotus-app 架构演进 Roadmap

**日期**：2026-04-17  
**基于**：
- `docs/2026-04-17-full-gap-assessment.md` — 功能差距 + 哲学差距
- `docs/2026-04-17-design-debt-assessment.md` — 设计问题审计
- `docs/superpowers/plans/2026-04-17-atomic-tool-capability-upgrade-plan.md` — 当前实施计划

---

## 优先级定义

| 级别 | 定义 |
|------|------|
| **P0** | 当前存在 bug 或安全漏洞，影响生产正确性 |
| **P1** | 核心能力缺失或架构缺陷，阻碍功能演进 |
| **P2** | 优化项，影响稳定性、可维护性、扩展性 |
| **Φ** | 哲学级，需独立设计新子系统，不可通过修补解决 |

---

## 当前进行中

### Sprint 0（正在执行）：工具系统基础补齐

**计划文件**：`docs/superpowers/plans/2026-04-17-atomic-tool-capability-upgrade-plan.md`

| Task | 内容 | 状态 |
|------|------|------|
| 1.1 | Tool Pool 按名排序（prompt cache 稳定） | ⏳ |
| 1.2 | CapabilityContext 扩展（FileStateCache / FileReadingLimits / NotificationSink） | ⏳ |
| 1.3 | ReadWorkspaceFile 使用文件状态缓存 | ⏳ |
| 2.1 | RuntimeTool 新增 is_concurrency_safe / is_read_only / is_destructive 谓词 | ⏳ |
| 2.2 | workspace 工具声明并发安全性 | ⏳ |
| 2.3 | ToolDispatcher dispatch_batch 并发分区调度 | ⏳ |
| 3.1 | RuntimeTool check_permissions 动态权限钩子 | ⏳ |
| 3.2 | ExecutePythonRuntimeTool 迁移骨架 | ⏳ |
| 3.3 | GenerateReport / GenerateChart 迁移骨架 | ⏳ |

---

## 第一阶段：P0 修复（立即，不依赖 Sprint 0 完成）

> **原则**：可与 Sprint 0 并行推进，互不干扰。

### 🔴 P0-A：Cancel 后 synthetic tool_result 注入

**问题**：ESC 发生在工具执行中途时，消息历史出现 `tool_use` 无对应 `tool_result` → Anthropic API 400 → 对话崩溃。

**涉及文件**：
- `src-tauri/src/runtime/chat/chat_turn_driver.rs` — cancel 检测后注入 synthetic result
- `src-tauri/src/runtime/chat/tool_result_collector.rs` — 补全未完成的工具结果

**验收条件**：Turn 被 cancel 后，下次发消息不会触发 API 400。

---

### 🔴 P0-B：权限 Ask 路径接通

**问题**：`ToolDispatchOutcome::AskRequired` 在 `query_engine.rs` 被注释为 `FIXME(S6)`，直接转错误。用户永远看不到权限确认对话框。

**涉及文件**：
- `src-tauri/src/runtime/query_engine.rs` — 处理 AskRequired，发 RuntimeEvent
- `src-tauri/src/transport/tauri_event_adapter.rs` — 新增 Ask 事件映射
- `src/` 前端 — 接收 Ask 事件，显示确认对话框，返回决策

**验收条件**：工具执行遇到 Ask 时，前端弹出确认框，用户选择后继续/拒绝。

---

### 🔴 P0-C：设计债正确性修复（可单独 commit）

| 问题 | 位置 | 修复 |
|------|------|------|
| R1：`std::Mutex::unwrap()` 在 async 路径 | `run_registry.rs:28-117` | 改 `tokio::sync::Mutex` + 移除 unwrap |
| F2：stop+retry 竞态（旧 token 混入新响应） | `useChat.ts:174` + `useStreaming.tsx:155` | 引入 generation token 校验 |
| P1：Python session 与 RunId 模型不对齐 | `python/session.rs:61-63` | session key 改为 per-run |
| P2：Sandbox path 边界绕过 | `python/sandbox.rs:267` | `startswith('/workspace/')` 修复 |
| D2：`build_env_info` 阻塞 tokio | `context_builder.rs:115` | 改 `tokio::process::Command` |
| D3：Executor 持有 `tauri::AppHandle` | `chat.rs:124` | AppHandle 职责抽为 trait 注入 |

---

## 第二阶段：核心能力补齐（P1，Sprint 0 完成后）

### Sprint 1：Session 状态 Owner + Turn 健壮性

**目标**：QueryEngine 拥有跨 turn 的 session 状态，Turn 内部多处 cancel checkpoint。

| 子任务 | 内容 |
|--------|------|
| 1a | QueryEngine 持有 `read_file_state`（FileStateCache）跨 turn 复用 |
| 1b | QueryEngine 持有 `total_usage`（token 用量跨 turn 累积） |
| 1c | Turn 内部多处 cancel checkpoint（streaming 中、工具前、工具后） |
| 1d | Turn state 改为不可变更新（每次 continue 创建新 State object） |

---

### Sprint 2：bash / file 基础工具

**目标**：实现 claude-code-best 同等的基础工具集，支持 bash 执行和文件读写。

| 工具 | 内容 |
|------|------|
| `bash` | 命令执行，绑定 CancellationToken，timeout 后台化 |
| `read_file` | 完整版（复用 FileStateCache，支持 offset/limit） |
| `write_file` | 文件写入，更新 FileStateCache |
| `edit_file` | 基于 diff 的文件编辑 |
| `grep` | 内容搜索（完整版，支持 regex） |

**注意**：`bash` 工具实现 `check_permissions`（依赖 Sprint 0 Task 3.1 完成）。

---

### Sprint 3：工具结果预算

**目标**：防止超大工具结果撑爆上下文窗口。

| 子任务 | 内容 |
|--------|------|
| 3a | 每个 RuntimeTool 声明 `max_result_size_chars` |
| 3b | `ToolResult` 增加截断逻辑 |
| 3c | TurnDriver 维护 per-turn `content_budget` 全局追踪 |
| 3d | 超限工具结果自动截断 + 附通知（模型知道结果被截断） |

---

### Sprint 4：核心旧工具完整迁移（P0-C 骨架的完整版）

**目标**：Sprint 0 Task 3.2-3.3 建立了骨架，本 sprint 做完整实现。

| 工具 | 关键依赖 |
|------|---------|
| `execute_python`（完整） | `PythonExecution` trait 注入，脱离 PluginContext |
| `generate_report`（完整） | `ReportCapability` trait 注入 |
| `generate_chart`（完整） | `ChartCapability` trait 注入 |

**参照**：`docs/2026-04-16-execute-python-migration-boundary.md`

---

## 第三阶段：架构健壮化（P1/P2，可并行规划）

### Sprint 5：设计债系统性清理

| 问题 | 内容 |
|------|------|
| D1：Schema/注册/执行一致性 | 工具注册时做 catalog 一致性校验 |
| D4：权限管线重复逻辑 | 提取 scope 匹配逻辑，pipeline 改责任链 |
| D8：Prompt 构建统一 | `context_builder.rs` 成为唯一 prompt 构建入口 |
| D11：settings 每步重读 | turn 入口读一次，注入 TurnConfig |
| F1：chatStore 拆分 | `SessionStore` + `StreamingStore` |
| F3：listener 泄露修复 | `useTauriEvent` 异常路径 + error boundary |
| R2：SessionId newtype 全面推广 | 消除裸 `String` 作为 session id |

---

### Sprint 6：MCP 支持

**目标**：接入 MCP 工具生态。

| 子任务 | 内容 |
|--------|------|
| 6a | MCP client 连接管理（server 生命周期） |
| 6b | MCP 工具动态发现 → 注册到 ToolRegistry |
| 6c | TOOL_CATALOG 改为动态可注册（依赖 D10 修复） |
| 6d | MCP 工具权限集成（capability scope = `mcp`） |

---

### Sprint 7：Subagent 状态隔离

**目标**：subagent 有独立状态，不污染父代理。

| 子任务 | 内容 |
|--------|------|
| 7a | `AgentContext` 独立（独立 file state、独立 messages buffer） |
| 7b | subagent 的 `setAppState` 变隔离写 |
| 7c | 父子 cancel 级联（parent abort → child abort） |
| 7d | subagent 结果汇报协议 |

---

## 第四阶段：哲学级子系统重设计（Φ，独立立项）

> **这四项不是"修修补补"，是全新子系统设计。每项需要独立 brainstorm → spec → plan 周期。**

### Φ1：安全边界统一化

**目标**：permission pipeline 成为唯一安全边界，删除多套重叠检查的安全假象。

**依赖**：P0-B（Ask 路径）完成后。

**主要工作**：
- 删除 `validate_code()`，将危险 pattern 检测迁入 `ExecutePythonRuntimeTool.check_permissions`
- 修复 sandbox `_safe_open` 边界漏洞
- 审计所有安全检查点，确保全部流经 permission pipeline
- 文档化"permission pipeline 是唯一安全边界"的架构约束

---

### Φ2：Hook 系统

**目标**：系统行为在运行时可扩展，不需要修改源码。

**核心设计**：
- Hook 注册接口（TOML 配置 or API）
- `PreToolUse` hook：可修改输入、可中断执行
- `PostToolUse` hook：可审计结果、可触发后续动作
- `Stop` hook：agent 停止时触发
- Hook 执行上下文（安全沙箱，不能访问全局状态）

---

### Φ3：Context Window 主动管理

**目标**：上下文窗口作为有限资源，主动管理而非被动截断。

**核心设计**（部分与 Sprint 1/3 协同）：
- Token 预算计数器（per turn）
- 工具结果预算（与 Sprint 3 协同）
- FileStateCache turn-level 去重（与 Sprint 1 协同）
- auto-compact 策略：summarize vs truncate vs evict 的决策算法
- 模型侧通知："以下内容已被压缩，原始内容可通过 X 访问"

---

### Φ4：Subagent 并发基础设计

**目标**：系统原生支持多 agent 并发，不是后加功能。

**依赖**：Sprint 7（subagent 状态隔离）完成后。

**主要工作**：
- 并发 agent 的资源竞争策略（工具调用��队 vs 并行）
- agent 间通信协议（结果共享 vs 消息传递）
- 并发 agent 的 context budget 分配
- 父 agent 对子 agent 结果的聚合机制

---

## 全局时序视图

```
现在
 │
 ├─ Sprint 0（进行中）        工具系统基础补齐
 ├─ P0-A/B（立即）            Cancel 修复 + Ask 路径接通
 └─ P0-C（立即，逐个 commit） 正确性 bug 修复
 │
 ▼ Sprint 0 完成后
 │
 ├─ Sprint 1                  Session state + Turn 健壮性
 ├─ Sprint 2                  bash/file 工具
 └─ Sprint 3                  工具结果预算
 │
 ▼
 ├─ Sprint 4                  核心工具完整迁移
 ├─ Sprint 5                  设计债清理
 └─ Sprint 6                  MCP 支持
 │
 ▼
 ├─ Sprint 7                  Subagent 隔离
 └─ Φ1                        安全边界统一（可与 Sprint 7 并行规划）
 │
 ▼
 ├─ Φ2                        Hook 系统
 ├─ Φ3                        Context Window 主动管理
 └─ Φ4                        Subagent 并发基础设计
```

---

## 附：文档索引

| 文档 | 内容 |
|------|------|
| `docs/2026-04-17-full-gap-assessment.md` | 功能差距（20项）+ 哲学差距（4项） |
| `docs/2026-04-17-design-debt-assessment.md` | 设计问题审计（27项） |
| `docs/2026-04-17-atomic-tool-vs-claude-code-best-gap.md` | 工具层专项差距 |
| `docs/superpowers/plans/2026-04-17-atomic-tool-capability-upgrade-plan.md` | Sprint 0 实施计划（TDD，8 Task） |
| `docs/superpowers/specs/2026-04-17-atomic-tool-capability-upgrade-design.md` | Sprint 0 设计文档 |
| `docs/2026-04-16-execute-python-migration-boundary.md` | execute_python 迁移边界分析 |
