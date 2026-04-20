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

## 2026-04-19 计划批次

> **注：** `Plan-U` 已在 2026-04-19 重新定义为“剩余非云端关键差距总纲”；原 TurnError / compact 历史修复以 Git 历史为准。除 `Plan-U` 外，本批次其余已关闭条目维持原状态。

### 后端计划（Rust）

| 计划文件 | 目标 | 状态 | 优先级 |
|---------|------|------|--------|
| [2026-04-19-plan-u-critical-fixes.md](./2026-04-19-plan-u-critical-fixes.md) | U：剩余非云端关键差距总纲（原 TurnError / compact 历史修复以 Git 记录为准） | 📝 当前总纲（2026-04-19） | 🔴 Critical |
| [2026-04-19-plan-v-security-hardening.md](./2026-04-19-plan-v-security-hardening.md) | V：安全加固（Hook 沙箱 + MCP 权限 + Python read + Bash + dontAsk） | ✅ 已关闭（2026-04-19） | 🔴 Critical |
| [2026-04-19-plan-aa-prompt-caching.md](./2026-04-19-plan-aa-prompt-caching.md) | AA：Prompt Caching（system prompt + tools list 添加 cache_control breakpoint） | ✅ 已关闭（2026-04-19） | 🔴 P0（成本） |
| [2026-04-19-plan-ab-small-fixes.md](./2026-04-19-plan-ab-small-fixes.md) | AB：小修复包（core_memory 接通 S4 路径 + SSE error 不静默 + sub_agent 迁 RuntimeTool） | ✅ 已关闭（2026-04-19） | 🔴 P0 |
| [2026-04-19-plan-ac-claude-md-loading.md](./2026-04-19-plan-ac-claude-md-loading.md) | AC：CLAUDE.md 项目记忆文件读取（目录遍历 + mtime 缓存 + dynamic context 注入） | ✅ 已关闭（2026-04-19） | 🟠 P1 |
| [2026-04-19-plan-ad-token-and-thinking.md](./2026-04-19-plan-ad-token-and-thinking.md) | AD：Token 感知 + Extended Thinking 主动启用（chars/4 估算 + ThinkingConfig） | ✅ 已关闭（2026-04-19） | 🟠 P1-P2 |
| [2026-04-19-plan-ae-config-layers.md](./2026-04-19-plan-ae-config-layers.md) | AE：配置分层 + Per-conversation 模型 Override（.lotus/settings.json + ConversationMeta.model_override） | ✅ 已关闭（2026-04-19） | 🟠 P2 |
| [2026-04-19-plan-w-runtime-recovery.md](./2026-04-19-plan-w-runtime-recovery.md) | W：运行时恢复语义（错误分类 + max_output_tokens 循环 + stop_hook 前移） | ✅ 已关闭（2026-04-19） | 🟠 Important |
| [2026-04-19-plan-x-tool-system.md](./2026-04-19-plan-x-tool-system.md) | X：工具系统改进（Tool Pool 分区排序 + preserveToolUseResults + 超时声明） | ✅ 已关闭（2026-04-19） | 🟠 Important |
| [2026-04-19-plan-y-storage-improvements.md](./2026-04-19-plan-y-storage-improvements.md) | Y：存储层改进（async write + 级联清理 + 孤儿文件 GC） | ✅ 已关闭（2026-04-19） | 🟠 Important |

### 2026-04-19 剩余非云端关键差距（待执行）

| 计划文件 | 目标 | 状态 | 依赖 |
|---------|------|------|------|
| [2026-04-19-plan-u1-mcp-runtime-closure.md](./2026-04-19-plan-u1-mcp-runtime-closure.md) | U1：MCP 真闭环与工具暴露收口 | 📝 待执行 | Plan-U |
| [2026-04-19-plan-u2-permission-governance.md](./2026-04-19-plan-u2-permission-governance.md) | U2：权限治理与 Ask/Remember 语义统一 | 📝 待执行 | Plan-U |
| [2026-04-19-plan-u3-context-pipeline.md](./2026-04-19-plan-u3-context-pipeline.md) | U3：长会话上下文预处理管道补齐 | 📝 待执行 | Plan-U / Plan-AI（若 analysis mode 仍在） |
| [2026-04-19-plan-u4-memory-runtime-native.md](./2026-04-19-plan-u4-memory-runtime-native.md) | U4：本地记忆 Runtime-Native 化 | 📝 待执行 | Plan-U / Plan-U3 |
| [2026-04-19-plan-u6-plugin-context-bridge-exit.md](./2026-04-19-plan-u6-plugin-context-bridge-exit.md) | U6：PluginContext 热路径退出与 Request-Scoped Tool 运行时化 | 📝 待执行 | Plan-U / Plan-AI（若 tool surface 仍双轨） |
| [2026-04-19-plan-u5-subagent-worker-runtime.md](./2026-04-19-plan-u5-subagent-worker-runtime.md) | U5：Subagent 一等 Worker Runtime 收口 | 📝 待执行 | Plan-U / Plan-U2 / Plan-U6 |


### 前端计划（TypeScript/React）

| 计划文件 | 目标 | 状态 | 优先级 |
|---------|------|------|--------|
| [2026-04-19-plan-z-gui-desktop-ux.md](./2026-04-19-plan-z-gui-desktop-ux.md) | Z：GUI 体验改进（时间戳 + 重新生成 + 侧边栏搜索 + TaskStatus 语义标签 + 图片预览） | ✅ 已关闭（2026-04-19） | 🟡 P1-P2 |
| [2026-04-19-plan-af-frontend-gaps.md](./2026-04-19-plan-af-frontend-gaps.md) | AF：前端缺口修复（对话搜索 + export 入口 + 孤儿 invoke 清理；AF4 已确认无需修改） | ✅ 已关闭（2026-04-19） | 🟡 P2 |

> **已取消**：Z1（Permission Ask 非模态重设计）、Z4（代码语法高亮）— 维持现状。

## 2026-04-18 计划批次（已关闭）

### 后端计划（Rust）

| 计划文件 | 目标 | 状态 |
|---------|------|------|
| [2026-04-18-plan-i-runtime-parity.md](./2026-04-18-plan-i-runtime-parity.md) | I：SessionRuntime cancel root owner + subagent transcript parity | ✅ 已关闭（2026-04-19） |
| [2026-04-18-plan-j-python-analysis-parity.md](./2026-04-18-plan-j-python-analysis-parity.md) | J：ExecutePythonRuntimeTool analysis 模式 Python binary 缺口 | ✅ 已关闭（2026-04-19） |
| [2026-04-18-plan-k-autocompact.md](./2026-04-18-plan-k-autocompact.md) | K：LLM 辅助自动 compact + compact_boundary 持久化 | ✅ 已关闭（2026-04-19） |
| [2026-04-18-plan-l-input-schema-validation.md](./2026-04-18-plan-l-input-schema-validation.md) | L：执行时 Input Schema Validation（safeParse + validateInput） | ✅ 已关闭（2026-04-19） |
| [2026-04-18-plan-m-hook-system.md](./2026-04-18-plan-m-hook-system.md) | M：Hook 系统（PreToolUse / PostToolUse / Stop hooks） | ✅ 已关闭（2026-04-19） |
| [2026-04-18-plan-n-tool-execution.md](./2026-04-18-plan-n-tool-execution.md) | N：工具执行改进（sibling cascade + interruptBehavior + contextModifier） | ✅ 已关闭（2026-04-19） |
| [2026-04-18-plan-o-queryengine-session-state.md](./2026-04-18-plan-o-queryengine-session-state.md) | O：QueryEngine 跨 turn 会话状态 + Turn 终态枚举 + maxBudgetUsd | ✅ 已关闭（2026-04-19） |

### 前端计划（TypeScript/React）

| 计划文件 | 目标 | 状态 |
|---------|------|------|
| [2026-04-18-plan-p-permission-ask-frontend.md](./2026-04-18-plan-p-permission-ask-frontend.md) | P：Permission Ask 全链路前端（弹窗 + approve/deny/cancel） | ✅ 已关闭（2026-04-19） |
| [2026-04-18-plan-q-turn-outcome-frontend.md](./2026-04-18-plan-q-turn-outcome-frontend.md) | Q：Turn 终态 + token 用量前端展示（`turn:completed` 消费） | ✅ 已关闭（2026-04-19） |
| [2026-04-18-plan-r-mcp-config-panel.md](./2026-04-18-plan-r-mcp-config-panel.md) | R：MCP 服务器配置面板（含后端 R0 配置持久化 + Tauri commands） | ✅ 已关闭（2026-04-19） |
| [2026-04-18-plan-s-frontend-tool-visibility.md](./2026-04-18-plan-s-frontend-tool-visibility.md) | S：前端工具状态可视化（Tool Error 展示 + Task Status 子任务列表） | ✅ 已关闭（2026-04-19） |
| [2026-04-18-plan-t-subagent-transcript-frontend.md](./2026-04-18-plan-t-subagent-transcript-frontend.md) | T：Subagent Transcript 前端展示（SubAgentResultCard + 折叠 transcript viewer） | ✅ 已关闭（2026-04-19） |

### 已并入并关闭的小缺口

| 缺口 | 合并目标 | 状态 |
|------|---------|------|
| `streaming:error` errorType 扩展（budget/iterations） | Plan-Q 前端联调节 | ✅ 已随 Plan-Q 关闭 |
| Compact 状态反馈前端 | Plan-K 前端联调节 | ✅ 已随 Plan-K 关闭 |
| Hook deny 前端反馈 | Plan-M 前端联调节 | ✅ 已随 Plan-M 关闭 |
| Permission dontAsk 持久化 | Plan-P P5 节 | ✅ 已随 Plan-P 关闭 |

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
