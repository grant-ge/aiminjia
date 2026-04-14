# Lotus Backend 改造计划索引

## 架构总蓝图

- `docs/architecture-blueprint.md` — 长期目标架构
- `docs/2026-04-14-backend-architecture-gap-assessment.md` — 当前差距全景报告（包含问题清单和优先级路线图）

---

## 执行顺序与依赖关系

```
P0（立即修复）
  ↓ 无需等待，可并行
P1（chat runtime-first 收口）← 唯一必须先关的主 blocker
  ↓ P1 关闭后
P2（WF + AT 专项）← 各自独立，可并行
  ↓
P3（PS + SK 专项）← 各自独立，可并行
  ↓
P4（基础设施收尾）← 依赖 P1-P3 的成果
```

**关键约束：各阶段不混做。P1 未关闭前不启动 P2。**

---

## P0：立即修复（安全/功能损坏）

> **目标**：修复安全漏洞和高危 bug，不涉及架构改造。
> **关闭条件**：5 个修复点全部合并，相关测试通过。

| 计划文件 | 覆盖问题 | 状态 |
|---------|---------|------|
| [2026-04-14-p0-immediate-fixes-plan.md](./2026-04-14-p0-immediate-fixes-plan.md) | M1 Claude provider tool calling 损坏、PY2 env var 泄漏、PY3 pickle RCE、S1 AgentRuntime for_test、WF2 wiring 顺序 | ✅ 已关闭（2026-04-14）|

---

## P1：Chat Runtime-First 主链路收口

> **目标**：拿回真实 send_message 生产路径的可信 ownership。
> **关闭条件**：B1-B4 全部修复，4 条 gating tests 全绿，closure review 标记为已关闭。

| 计划文件 | 覆盖问题 | 状态 |
|---------|---------|------|
| [2026-04-14-p1-chat-runtime-first-final-closure-plan.md](./2026-04-14-p1-chat-runtime-first-final-closure-plan.md) | B1 legacy executor owner、B2 gating 不足、B3 wiring 顺序、B4 双 RunId + 孤立 QueryEngine | ✅ 已关闭（2026-04-14）|

### P1 历史计划（参考，已部分执行）

这些计划已完成部分改造，是 P1 收口计划的前置工作：

| 计划文件 | 内容 | 状态 |
|---------|------|------|
| [2026-04-13-chat-runtime-first-closure-plan.md](./2026-04-13-chat-runtime-first-closure-plan.md) | chat runtime-first 专项（Phase 1） | ✅ 部分完成 |
| [2026-04-14-chat-runtime-closure-red-lights.md](./2026-04-14-chat-runtime-closure-red-lights.md) | T1+T4 红灯转绿（P1-B） | ✅ 已执行 |
| [2026-04-14-p1-a-chat-tool-dispatch-runtime-plan.md](./2026-04-14-p1-a-chat-tool-dispatch-runtime-plan.md) | T2 工具回合 runtime dispatcher 收口（P1-A） | ✅ 已执行 |

> **评审文档**：`docs/reviews/2026-04-14-chat-runtime-first-closure-review.md`
> 当前状态：❌ 未关闭（B1-B4 仍未收口）

---

## P2：四大专项 · WF + AT

> 在 P1 关闭后启动，各自独立执行。

| 计划文件 | 专项 | 状态 |
|---------|------|------|
| [2026-04-12-workspace-first-file-runtime-plan.md](./2026-04-12-workspace-first-file-runtime-plan.md) | WF：Workspace-First 文件能力模型 | ✅ 已关闭（评估确认已实现：4 个原子工具 + 沙箱授权目录 + workspace-first 主链路）|
| [2026-04-13-atomic-tool-runtime-plan.md](./2026-04-13-atomic-tool-runtime-plan.md) | AT：Atomic Tool 工具体系 | ✅ 已关闭（评估确认已实现：ToolCatalog + DAILY_ALLOWED_TOOLS 18 个 + CapabilityPermissionPipeline）|

---

## P3：四大专项 · PS + SK

> 在 P2 启动后可并行，各自独立执行。

| 计划文件 | 专项 | 状态 |
|---------|------|------|
| [2026-04-14-prompt-slimming-plan.md](./2026-04-14-prompt-slimming-plan.md) | PS：Prompt Slimming 提示词职责回收 | ✅ 已关闭（2026-04-14）|
| *(待创建)* | SK：Skill 本地导入/打包导入模型统一 | ✅ 已关闭（评估确认已实现，无需计划）|

---

## P4：基础设施收尾

> 依赖 P1-P3 的成果，最后执行。

| 目标 | 前置依赖 | 状态 |
|------|---------|------|
| PluginContext 退出主路径 → per-call CapabilityContext | P1 + AT | ⬜ |
| AppStorage → repository facade 继续迁移 | P1 | ⬜ |
| CapabilityPermissionPipeline → 真正 policy engine（allow/deny/ask + 持久化） | AT | ⬜ |
| 取消传播：CancellationToken 级联，废弃 fire-and-forget | P1 | ⬜ |
| AgentRuntime 持久化（FileAgentInvocationStore） | P1 | ⬜ |
| Python 安全模型收口（废弃静态检查沙箱，改用权限系统） | policy engine | ⬜ |

---

## 历史计划（已关闭）

| 计划文件 | 内容 | 状态 |
|---------|------|------|
| [2026-04-10-lotus-backend-master-plan.md](./2026-04-10-lotus-backend-master-plan.md) | 总体后端迁移计划 | ✅ 已关闭 |
| [2026-04-10-phase-0-baseline-audit-plan.md](./2026-04-10-phase-0-baseline-audit-plan.md) | Phase 0 基线审计 | ✅ 已关闭 |
| [2026-04-10-phase-1-session-runtime-plan.md](./2026-04-10-phase-1-session-runtime-plan.md) | Phase 1 SessionRuntime | ✅ 已关闭 |
| [2026-04-10-phase-2-tool-permission-store-plan.md](./2026-04-10-phase-2-tool-permission-store-plan.md) | Phase 2 tool/permission/store | ✅ 已关闭 |
| [2026-04-10-phase-3-task-agent-plan.md](./2026-04-10-phase-3-task-agent-plan.md) | Phase 3 task/agent | ✅ 已关闭 |
| [2026-04-10-phase-4-store-transport-plan.md](./2026-04-10-phase-4-store-transport-plan.md) | Phase 4 store/transport | ✅ 已关闭 |
| [2026-04-12-front-end-event-integration-plan.md](./2026-04-12-front-end-event-integration-plan.md) | 前端事件联调 | ✅ 已关闭 |
