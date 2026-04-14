# Lotus Backend Migration Plans Index

## 当前建议先看（2026-04-12）

- `/Users/a20250311/IdeaProjects/lotus-app/docs/2026-04-12-runtime-first-final-acceptance-summary.md`
- `/Users/a20250311/IdeaProjects/lotus-app/docs/2026-04-12-review-plan-status-matrix.md`
- `/Users/a20250311/IdeaProjects/lotus-app/docs/2026-04-12-runtime-gap-problem-statement.md`
- `/Users/a20250311/IdeaProjects/lotus-app/docs/reviews/2026-04-12-front-end-event-integration-review.md`

## 总计划
- [2026-04-10-lotus-backend-master-plan.md](./2026-04-10-lotus-backend-master-plan.md)

## 分期计划
- [2026-04-10-phase-0-baseline-audit-plan.md](./2026-04-10-phase-0-baseline-audit-plan.md)
- [2026-04-10-phase-1-session-runtime-plan.md](./2026-04-10-phase-1-session-runtime-plan.md)
- [2026-04-10-phase-2-tool-permission-store-plan.md](./2026-04-10-phase-2-tool-permission-store-plan.md)
- [2026-04-10-phase-3-task-agent-plan.md](./2026-04-10-phase-3-task-agent-plan.md)
- [2026-04-10-phase-4-store-transport-plan.md](./2026-04-10-phase-4-store-transport-plan.md)

## 专项联调计划
- [2026-04-12-front-end-event-integration-plan.md](./2026-04-12-front-end-event-integration-plan.md)

## 架构专项（2026-04-13 启动）
- [2026-04-12-workspace-first-file-runtime-plan.md](./2026-04-12-workspace-first-file-runtime-plan.md) — 专项 1：Workspace-First 文件能力模型
- [2026-04-13-atomic-tool-runtime-plan.md](./2026-04-13-atomic-tool-runtime-plan.md) — 专项 2：Atomic Tool 工具体系（A1-A5）
  - 问题定义：`docs/2026-04-13-atomic-tool-problem-statement.md`
- [2026-04-13-chat-runtime-first-closure-plan.md](./2026-04-13-chat-runtime-first-closure-plan.md) — 专项 4：聊天主链路 Runtime-First 收口

## 阅读顺序
1. 先看总蓝图：`/Users/a20250311/IdeaProjects/lotus-app/docs/architecture-blueprint.md`
2. 再看总计划：`2026-04-10-lotus-backend-master-plan.md`
3. 然后按 Phase 0 → 4 顺序执行

## 计划特征
- 全部为文件级实施计划
- 每一期都包含 TDD 路径，且测试示例要求是真失败测试
- 每一期默认直接替换，不做灰度
- 每一期都要求 rollback、golden trace、commit
- Phase 0 的 golden trace 明确要求来自真实 legacy emit 路径采样

## 当前进度快照（2026-04-12）
- Phase 0：已关闭（审计文档与 golden trace 已补齐）
- Phase 1：已关闭（runtime identity / `SessionRuntime` / `TauriEventAdapter` 已落地并验收）
- Phase 2：已关闭（按当前 runtime-first 验收范围，tool / permission / minimal store 主链路已关闭）
- Phase 3：已关闭（task / agent / background 主链路已完成本轮验收）
- Phase 4：已关闭（transport / domain facade / stage-c 当前验收范围已关闭）
- 专项联调计划（2026-04-12）：已关闭（前端 `task:status-changed` 与 `agent:idle scope` 已对接，Vitest/Rust 回归通过）
- 当前总体验收入口：`/Users/a20250311/IdeaProjects/lotus-app/docs/2026-04-12-runtime-first-final-acceptance-summary.md`
- 当前状态总表：`/Users/a20250311/IdeaProjects/lotus-app/docs/2026-04-12-review-plan-status-matrix.md`
- 最近一次 Rust targeted 回归：`/Users/a20250311/IdeaProjects/lotus-app/src-tauri` 下执行 `cargo test --test tauri_event_adapter_test -- --nocapture` 与 `cargo test review_ --tests --no-fail-fast`，结果通过
- 最近一次前端联调回归：`/Users/a20250311/IdeaProjects/lotus-app` 下执行 `pnpm exec vitest run src/lib/tauri.events.test.ts src/hooks/useStreaming.integration.test.tsx src/stores/chatStore.test.ts`，结果通过
