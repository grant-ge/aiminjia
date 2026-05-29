# 2026-04-12 Review / Plan 当前状态总表

## 说明

这份矩阵用于回答两个问题：

- 哪些文档仍然是架构基线，需要继续保留
- 哪些计划 / review 已经完成并应视为历史记录

状态定义：

- `已冻结`：作为长期参考基线保留，不再按“待执行计划”理解
- `已关闭`：计划或 review 已完成其职责，当前不再是活动文档
- `已完成`：本轮新增的收尾 / 验收文档，当前有效

## 证据命令索引

### E1 - 前端联调回归

```bash
cd /Users/a20250311/IdeaProjects/lotus-app
pnpm exec vitest run src/lib/tauri.events.test.ts src/hooks/useStreaming.integration.test.tsx src/stores/chatStore.test.ts
```

### E2 - Rust transport / review 回归

```bash
cd /Users/a20250311/IdeaProjects/lotus-app/src-tauri
cargo test --test tauri_event_adapter_test -- --nocapture
cargo test review_ --tests --no-fail-fast
```

### E3 - 历史全量 Rust 基线

```bash
cd /Users/a20250311/IdeaProjects/lotus-app/src-tauri
cargo test --tests --no-fail-fast
```

## 状态矩阵

| 类别 | 文档 | 当前状态 | 当前定位 | 证据 |
| --- | --- | --- | --- | --- |
| 蓝图 | `/Users/a20250311/IdeaProjects/lotus-app/docs/architecture-blueprint.md` | 已冻结 | runtime-first 改造总蓝图，继续作为目标架构基线 | E1 / E2 |
| 分期设计 | `/Users/a20250311/IdeaProjects/lotus-app/docs/phase-0-baseline-audit.md` | 已冻结 | 审计方法与 baseline 文档，保留为历史设计依据 | E2 / E3 |
| 分期设计 | `/Users/a20250311/IdeaProjects/lotus-app/docs/phase-1-session-runtime.md` | 已冻结 | SessionRuntime / QueryEngine / event adapter 设计基线 | E2 |
| 分期设计 | `/Users/a20250311/IdeaProjects/lotus-app/docs/phase-2-tool-permission-store.md` | 已冻结 | Tool / permission / minimal store 设计基线 | E2 |
| 分期设计 | `/Users/a20250311/IdeaProjects/lotus-app/docs/phase-3-task-agent.md` | 已冻结 | Task / Agent / child run 设计基线 | E2 |
| 分期设计 | `/Users/a20250311/IdeaProjects/lotus-app/docs/phase-4-store-transport-subagent-c.md` | 已冻结 | store 拆分 / transport 解耦 / stage C 基线 | E1 / E2 |
| 计划索引 | `/Users/a20250311/IdeaProjects/lotus-app/docs/superpowers/plans/README.md` | 已完成 | 当前 plans 总入口与快照索引 | E1 / E2 |
| 总计划 | `/Users/a20250311/IdeaProjects/lotus-app/docs/superpowers/plans/2026-04-10-lotus-backend-master-plan.md` | 已关闭 | 历史主计划，实施结果已落到验收摘要 | E1 / E2 / E3 |
| Phase 0 计划 | `/Users/a20250311/IdeaProjects/lotus-app/docs/superpowers/plans/2026-04-10-phase-0-baseline-audit-plan.md` | 已关闭 | 审计基线已补齐，不再作为待执行项 | E2 / E3 |
| Phase 1 计划 | `/Users/a20250311/IdeaProjects/lotus-app/docs/superpowers/plans/2026-04-10-phase-1-session-runtime-plan.md` | 已关闭 | runtime identity / event adapter 主链路已验收 | E2 |
| Phase 2 计划 | `/Users/a20250311/IdeaProjects/lotus-app/docs/superpowers/plans/2026-04-10-phase-2-tool-permission-store-plan.md` | 已关闭 | 本轮验收范围内相关链路已关闭 | E2 |
| Phase 3 计划 | `/Users/a20250311/IdeaProjects/lotus-app/docs/superpowers/plans/2026-04-10-phase-3-task-agent-plan.md` | 已关闭 | task / agent / background 链路已完成本轮验收 | E1 / E2 |
| Phase 4 计划 | `/Users/a20250311/IdeaProjects/lotus-app/docs/superpowers/plans/2026-04-10-phase-4-store-transport-plan.md` | 已关闭 | transport / store facade 当前验收范围已关闭 | E1 / E2 |
| 专项计划 | `/Users/a20250311/IdeaProjects/lotus-app/docs/superpowers/plans/2026-04-12-front-end-event-integration-plan.md` | 已关闭 | 前后端事件联调已完成并回填 | E1 / E2 |
| 严格 review | `/Users/a20250311/IdeaProjects/lotus-app/docs/reviews/2026-04-10-runtime-first-strict-tdd-review.md` | 已关闭 | 历史 findings 已由后续修复与回归关闭 | E2 |
| 联调 review | `/Users/a20250311/IdeaProjects/lotus-app/docs/reviews/2026-04-12-front-end-event-integration-review.md` | 已关闭 | 前端事件消费链路 findings 已关闭 | E1 / E2 |
| 验收摘要 | `/Users/a20250311/IdeaProjects/lotus-app/docs/2026-04-12-runtime-first-final-acceptance-summary.md` | 已完成 | 当前最权威的收尾结论 | E1 / E2 |
| 状态矩阵 | `/Users/a20250311/IdeaProjects/lotus-app/docs/2026-04-12-review-plan-status-matrix.md` | 已完成 | 当前最权威的文档状态总表 | E1 / E2 |

## 当前建议阅读顺序

1. `/Users/a20250311/IdeaProjects/lotus-app/docs/2026-04-12-runtime-first-final-acceptance-summary.md`
2. `/Users/a20250311/IdeaProjects/lotus-app/docs/2026-04-12-review-plan-status-matrix.md`
3. `/Users/a20250311/IdeaProjects/lotus-app/docs/README-architecture-plan.md`
4. `/Users/a20250311/IdeaProjects/lotus-app/docs/reviews/2026-04-12-front-end-event-integration-review.md`
5. `/Users/a20250311/IdeaProjects/lotus-app/docs/reviews/2026-04-10-runtime-first-strict-tdd-review.md`
