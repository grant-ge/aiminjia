# Lotus Backend Architecture Migration Master Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 将 lotus-app 后端从 Tauri command-centric 架构重构为 runtime-first 架构，并保持现有用户能力可用。

**Architecture:** 采用 0~4 期直接替换式重构：先审计现状与事件协议，再抽 `SessionRuntime`，再统一 Tool/Permission/Store，再引入 Task/Agent Runtime，最后完成 Store 领域拆分与 Transport 解耦。所有实施以 TDD 为主线：先写真实失败测试与 golden trace，再做最小实现，再收敛旧路径。

**Tech Stack:** Rust, Tauri, React/TypeScript, file-based storage, Python bridge, Playwright, cargo test, Vitest/前端回归检查

---
## 当前实际进度回填（2026-04-10）

| Phase | 当前状态 | 已落地重点 | 主要未完成项 |
|------|----------|------------|--------------|
| Phase 0 | 部分完成 | `runtime_audit`、golden trace 测试、legacy 事件基线采样 | `docs/architecture-audit/*` 审计文档尚未补齐 |
| Phase 1 | 大部分完成 | `SessionId/RunId/...`、`TurnState`、`RuntimeEventBus`、`TauriEventAdapter`、`SessionRuntime`、`QueryEngine`、`chat.rs` 薄 adapter | 真实发送主循环尚未完全迁入 runtime |
| Phase 2 | 进行中 | runtime store 契约、`ToolDispatcher`、`PermissionPipeline`、`LegacyToolAdapter`、1 个 runtime builtin 样板 | `PluginContext` 仍偏宽，旧工具 trait 仍未彻底退出 |
| Phase 3 | 大部分完成 | `TaskRuntime` / `AgentRuntime`、child run、取消链路、Python `RunId` scope、recovery input | background/message bridge 与工作流清理仍需继续收口 |
| Phase 4 | 进行中 | transport host/adapter、chat transport 重定向、domain facade、Stage C 最小 resume/worktree/team | `file_store` 仍未完全退场，非 chat commands 尚未全部 transport 化 |

### 当前已落地的主目录
- `src-tauri/src/runtime/`
- `src-tauri/src/runtime_audit/`
- `src-tauri/src/transport/`
- `src-tauri/tests/*runtime*`, `*transport*`, `*tool*`, `*agent*`

### 当前回归基线
- 最近一次完整后端回归命令：`cd /Users/a20250311/IdeaProjects/lotus-app/src-tauri && cargo test --tests --no-fail-fast`
- 当前结果：通过
- 验证日期：2026-04-10

---


## File Structure

### Plan files
- `docs/superpowers/plans/2026-04-10-lotus-backend-master-plan.md`
- `docs/superpowers/plans/2026-04-10-phase-0-baseline-audit-plan.md`
- `docs/superpowers/plans/2026-04-10-phase-1-session-runtime-plan.md`
- `docs/superpowers/plans/2026-04-10-phase-2-tool-permission-store-plan.md`
- `docs/superpowers/plans/2026-04-10-phase-3-task-agent-plan.md`
- `docs/superpowers/plans/2026-04-10-phase-4-store-transport-plan.md`

### Core spec references
- `docs/architecture-blueprint.md`
- `docs/phase-0-baseline-audit.md`
- `docs/phase-1-session-runtime.md`
- `docs/phase-2-tool-permission-store.md`
- `docs/phase-3-task-agent.md`
- `docs/phase-4-store-transport-subagent-c.md`

---

## Cross-phase guardrails

- 所有测试示例都必须是真失败测试：要么 unresolved import，要么缺少真实 API，要么行为断言与当前实现冲突；禁止用 `assert!(true)`、`assert_eq!(vec.len(), N)` 这类占位断言冒充 TDD。
- Phase 0 的 golden trace 必须来自真实 legacy emit 路径采样，后续各期拿它做兼容回归。
- Phase 1 就引入 `SessionId` / `RunId` / `AgentId` / `ToolCallId`；前两期 `SessionId` 值直接复用 `conversation_id`，但 runtime 真相源只能是 `SessionId`。
- Phase 2 工具迁移顺序固定：先 `RuntimeTool + LegacyToolAdapter`，再切主链路，最后再处理旧 `ToolPlugin` trait 的弃用。
- Phase 3 Python 改造必须同时覆盖 `RunId` scope、recovery input 来源、`loaded:{conversation_id}:*` 到 `loaded:{run_id}:*` 的迁移。

---

### Task 1: 建立计划执行顺序

**Files:**
- Read: `docs/architecture-blueprint.md`
- Read: `docs/phase-0-baseline-audit.md`
- Read: `docs/phase-1-session-runtime.md`
- Read: `docs/phase-2-tool-permission-store.md`
- Read: `docs/phase-3-task-agent.md`
- Read: `docs/phase-4-store-transport-subagent-c.md`
- Test: `src-tauri` 下现有测试与后续新增 runtime tests

- [x] **Step 1: 确认分期依赖关系**

```text
执行顺序固定：
P0 -> P1 -> P2 -> P3 -> P4

硬依赖：
- P1 依赖 P0 的事件契约和职责地图
- P2 依赖 P1 的 RunId / RuntimeEvent / QueryEngine 主链路
- P3 依赖 P2 的 ToolDispatcher / AgentInvocationStore
- P4 依赖 P3 的 TaskRuntime / AgentRuntime / Python RunId scope
```

- [x] **Step 2: 建立总体验收表**

```text
每期都必须有：
- 真实失败测试
- 事件序列回放（golden trace）
- 旧路径 kill list 验证
- rollback 可行性检查
```

- [x] **Step 3: 运行当前测试基线**

Run: `cd /Users/a20250311/IdeaProjects/lotus-app/src-tauri && cargo test --lib`
Expected: 当前基线通过，或记录现有失败并冻结为已知基线

- [x] **Step 4: 记录当前前端协议基线**

```text
legacy 事件基线：
- streaming:delta
- streaming:done
- tool:executing
- tool:completed
- message:updated
- agent:idle
```

- [ ] **Step 5: Commit**

```bash
git add docs/superpowers/plans/*.md
git commit -m "docs: add master backend migration plan"
```

### Task 2: 逐期执行规则

**Files:**
- Modify: `docs/superpowers/plans/2026-04-10-phase-0-baseline-audit-plan.md`
- Modify: `docs/superpowers/plans/2026-04-10-phase-1-session-runtime-plan.md`
- Modify: `docs/superpowers/plans/2026-04-10-phase-2-tool-permission-store-plan.md`
- Modify: `docs/superpowers/plans/2026-04-10-phase-3-task-agent-plan.md`
- Modify: `docs/superpowers/plans/2026-04-10-phase-4-store-transport-plan.md`

- [x] **Step 1: 为每期固定 TDD 执行模板**

```text
每一期都按以下顺序执行：
1. 先写失败测试 / trace fixture
2. 运行测试确认失败
3. 写最小实现
4. 再跑测试确认通过
5. 删除旧路径 / 收紧兼容层
6. 再跑回归
7. 提交
```

- [x] **Step 2: 固定失败测试质量门槛**

```text
失败测试必须满足至少一项：
- unresolved import / missing module
- missing function / missing trait impl
- 真实行为断言失败

以下示例禁止出现：
- assert!(true)
- assert_eq!(vec.len(), N)
- let cancelled = true; assert!(cancelled)
```

- [x] **Step 3: 为每期固定切换策略**

```text
切换策略：直接替换，不做灰度
失败策略：立即回滚到旧主路径
```

- [x] **Step 4: 为每期固定完成门槛**

```text
没有以下证据不得宣称完成：
- 相关测试通过
- golden trace 对齐
- kill list 已执行
- rollback 路径已验证
```

- [ ] **Step 5: Commit**

```bash
git add docs/superpowers/plans/*.md
git commit -m "docs: define tdd execution rules for backend migration"
```

## Definition of Done

- 分期顺序、硬依赖和验收规则已经固定。
- 所有 phase 计划都符合真实 TDD、真实 golden trace、可回滚、可替换这四个基准。
