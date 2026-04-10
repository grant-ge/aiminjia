# Lotus Backend Migration Plans Index

## 总计划
- [2026-04-10-lotus-backend-master-plan.md](./2026-04-10-lotus-backend-master-plan.md)

## 分期计划
- [2026-04-10-phase-0-baseline-audit-plan.md](./2026-04-10-phase-0-baseline-audit-plan.md)
- [2026-04-10-phase-1-session-runtime-plan.md](./2026-04-10-phase-1-session-runtime-plan.md)
- [2026-04-10-phase-2-tool-permission-store-plan.md](./2026-04-10-phase-2-tool-permission-store-plan.md)
- [2026-04-10-phase-3-task-agent-plan.md](./2026-04-10-phase-3-task-agent-plan.md)
- [2026-04-10-phase-4-store-transport-plan.md](./2026-04-10-phase-4-store-transport-plan.md)

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

## 当前进度快照（2026-04-10）
- Phase 0：部分完成（事件基线/trace 已落地，审计文档未补齐）
- Phase 1：大部分完成（identity/runtime/event/transport 主骨架已落地）
- Phase 2：进行中（tool/store 主链路已切入，context/legacy trait 还没收尾）
- Phase 3：大部分完成（task/agent/python run scope 已落地）
- Phase 4：进行中（transport/domain facade/stage-c 最小模型已落地）
- 最近一次后端回归：`/Users/a20250311/IdeaProjects/lotus-app/src-tauri` 下执行 `cargo test --tests --no-fail-fast`，结果通过
