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
> 当前状态：✅ 已关闭（P1 全部 commits 已合并，B1-B4 已收口）

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

| 目标 | 计划文件 | 状态 |
|------|---------|------|
| PluginContext 退出主路径 + CancellationToken 级联 | [p4a-plugin-context-cancellation-plan.md](./2026-04-14-p4a-plugin-context-cancellation-plan.md) | ✅ 已关闭（2026-04-14，precompute key helper 已提取，legacy spawn 注释化说明取消边界） |
| AppStorage → ConversationStore facade 迁移 | [p4b-storage-facade-plan.md](./2026-04-14-p4b-storage-facade-plan.md) | ✅ 已关闭（2026-04-14，conversation_service.rs 完成，delete_conversation 待后续）|
| PolicyEngine（allow/deny/ask + 持久化）+ Python 安全收口 | [p4c-policy-engine-python-sandbox-plan.md](./2026-04-14-p4c-policy-engine-python-sandbox-plan.md) | ✅ 已关闭（2026-04-15，PermissionStore + StorePolicyPipeline + validate_code deprecated）|
| AgentRuntime 持久化（FileAgentInvocationStore） | — | ✅ 评估确认已实现 |

---

## 进行中计划（2026-04-18）

### 后端计划（Rust）

| 计划文件 | 目标 | 状态 | 依赖 |
|---------|------|------|------|
| [2026-04-18-plan-i-runtime-parity.md](./2026-04-18-plan-i-runtime-parity.md) | I：SessionRuntime cancel root owner + subagent transcript parity | 🔄 未执行 | — |
| [2026-04-18-plan-j-python-analysis-parity.md](./2026-04-18-plan-j-python-analysis-parity.md) | J：ExecutePythonRuntimeTool analysis 模式 Python binary 缺口 | 🔄 未执行 | — |
| [2026-04-18-plan-k-autocompact.md](./2026-04-18-plan-k-autocompact.md) | K：LLM 辅助自动 compact + compact_boundary 持久化 | 🔄 未执行 | — |
| [2026-04-18-plan-l-input-schema-validation.md](./2026-04-18-plan-l-input-schema-validation.md) | L：执行时 Input Schema Validation（safeParse + validateInput） | 🔄 未执行（reviewed 2026-04-18） | — |
| [2026-04-18-plan-m-hook-system.md](./2026-04-18-plan-m-hook-system.md) | M：Hook 系统（PreToolUse / PostToolUse / Stop hooks） | 🔄 未执行（reviewed 2026-04-18） | — |
| [2026-04-18-plan-n-tool-execution.md](./2026-04-18-plan-n-tool-execution.md) | N：工具执行改进（sibling cascade + interruptBehavior + contextModifier） | 🔄 未执行（reviewed 2026-04-18） | — |
| [2026-04-18-plan-o-queryengine-session-state.md](./2026-04-18-plan-o-queryengine-session-state.md) | O：QueryEngine 跨 turn 会话状态 + Turn 终态枚举 + maxBudgetUsd | 🔄 未执行（reviewed 2026-04-18） | — |

### 前端计划（TypeScript/React）

| 计划文件 | 目标 | 状态 | 依赖 |
|---------|------|------|------|
| [2026-04-18-plan-p-permission-ask-frontend.md](./2026-04-18-plan-p-permission-ask-frontend.md) | P：Permission Ask 全链路前端（弹窗 + approve/deny/cancel） | 🔄 未执行 | 后端 A2 ✅ 已就绪 |
| [2026-04-18-plan-q-turn-outcome-frontend.md](./2026-04-18-plan-q-turn-outcome-frontend.md) | Q：Turn 终态 + token 用量前端展示（`turn:completed` 消费） | 🔄 未执行 | Plan-O 后端 |
| [2026-04-18-plan-r-mcp-config-panel.md](./2026-04-18-plan-r-mcp-config-panel.md) | R：MCP 服务器配置面板（含后端 R0 配置持久化 + Tauri commands） | 🔄 未执行 | Plan-G ✅ 后端已就绪；需 R0 后端前置 |
| [2026-04-18-plan-s-frontend-tool-visibility.md](./2026-04-18-plan-s-frontend-tool-visibility.md) | S：前端工具状态可视化（Tool Error 展示 + Task Status 子任务列表） | 🔄 未执行 | 无 |
| [2026-04-18-plan-t-subagent-transcript-frontend.md](./2026-04-18-plan-t-subagent-transcript-frontend.md) | T：Subagent Transcript 前端展示（SubAgentResultCard + 折叠 transcript viewer） | 🔄 未执行 | Plan-I 后端 |

### 小缺口（合并进对应计划执行）

| 缺口 | 合并目标 | 说明 |
|------|---------|------|
| `streaming:error` errorType 扩展（budget/iterations） | Plan-Q 前端联调节 | 扩展 TypeScript 联合类型 + toast 路由 |
| Compact 状态反馈前端 | Plan-K 前端联调节 | `compact:started` 事件监听 + toast |
| Hook deny 前端反馈 | Plan-M 前端联调节 | 复用 `tool:completed` error 路径 |
| Permission dontAsk 持久化 | Plan-P P5 节 | 依赖 Plan-P 弹窗先完成 |

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
