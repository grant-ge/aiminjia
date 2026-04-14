# Lotus-App 后端改造文档索引

这组文档用于把 lotus-app 后端逐步改造成更接近 `claude-code-best` 的 runtime-first 架构。

## 当前建议优先看（2026-04-12）

1. [2026-04-12-runtime-first-final-acceptance-summary.md](./2026-04-12-runtime-first-final-acceptance-summary.md)
   - 当前最权威的总体验收摘要
   - 汇总运行结果、关键通过点、非阻塞遗留项

2. [2026-04-12-review-plan-status-matrix.md](./2026-04-12-review-plan-status-matrix.md)
   - 当前所有 blueprint / plans / reviews 的状态总表
   - 区分哪些已冻结、哪些已关闭、哪些仍是有效入口

3. [2026-04-12-runtime-gap-problem-statement.md](./2026-04-12-runtime-gap-problem-statement.md)
   - 当前新收敛出的 4 个专项问题定义
   - 包含问题、目标、验收标准，供后续 Claude 出专项计划使用

4. [reviews/2026-04-12-front-end-event-integration-review.md](./reviews/2026-04-12-front-end-event-integration-review.md)
   - 前后端事件联调 review
   - `task:status-changed` 与 `agent:idle scope` 验收记录

5. [reviews/2026-04-10-runtime-first-strict-tdd-review.md](./reviews/2026-04-10-runtime-first-strict-tdd-review.md)
   - runtime-first 核心严格 TDD review 历史记录
   - 适合回看当时的问题来源与关闭背景

## 阅读顺序

1. [architecture-blueprint.md](./architecture-blueprint.md)
   - 总蓝图
   - 目标分层
   - 10 个架构约束
   - 5 个关键决策
   - 分期概览

2. [phase-0-baseline-audit.md](./phase-0-baseline-audit.md)
   - 现状审计
   - 职责地图
   - 状态矩阵
   - 事件协议矩阵
   - golden trace 基线

3. [phase-1-session-runtime.md](./phase-1-session-runtime.md)
   - 身份模型
   - TurnState
   - SessionRuntime / QueryEngine
   - RuntimeEventBus / TauriEventAdapter

4. [phase-2-tool-permission-store.md](./phase-2-tool-permission-store.md)
   - Tool Runtime
   - Permission Pipeline
   - ToolExecutionContext
   - 最小 Run/Task/ToolCall Store

5. [phase-3-task-agent.md](./phase-3-task-agent.md)
   - TaskRuntime
   - AgentRuntime
   - SubAgent 阶段 A/B
   - Skill / Workflow 归属

6. [phase-4-store-transport-subagent-c.md](./phase-4-store-transport-subagent-c.md)
   - Store 领域拆分
   - Tauri transport 解耦
   - SubAgent 阶段 C

## 改造总原则

- 渐进重构，业务不停机
- 前两期前端事件协议保持兼容
- 第 1 期就引入 `SessionId / RunId / AgentId / ToolCallId`
- 第 1 期开始，核心 runtime 禁止直接依赖 Tauri 类型
- 第 2 期就落最小持久化，不把 store 全部后置
- SubAgent 分三段演进，不一步追平 claude-code-best

## 建议执行方式

- 先按第 0 期做现状审计和 golden trace
- 审计完成后再进入第 1 期实施计划
- 每一期都先确认：Compatibility Boundary / Kill List / Truth Source / Not Doing

## 当前文档位置

- 文档目录：`/Users/a20250311/IdeaProjects/lotus-app/docs/`
