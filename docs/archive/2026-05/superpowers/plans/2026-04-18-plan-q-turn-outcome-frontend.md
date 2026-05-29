# Turn 终态 + Token 用量前端展示（Plan-Q）

> **For agentic workers:** REQUIRED SUB-SKILL: Use `superpowers:subagent-driven-development` (recommended) or `superpowers:executing-plans` to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 消费 `turn:completed` 事件，为非正常终态（MaxIterationsReached / BudgetExceeded / ExecutionError）提供即时用户反馈，彻底消除"等 200s watchdog 超时"的沉默卡死体验；可选展示 token/cost 摘要。

**Architecture:**
- Q1 在 `tauri.ts` 定义 `TurnCompletedPayload` 类型与 `onTurnCompleted` 监听器，注册 `TAURI_EVENTS.TURN_COMPLETED`
- Q2 在 `useStreaming.ts` 订阅事件：非正常终态 push toast，同时主动清理 busy/stream 状态（替代 watchdog 在这条路径上的兜底）
- Q3（可选）在 `streamingStore` 存储最近一次 turn outcome，并在消息列表底部显示 token/cost 徽章
- Q4 review 约束测试：确保 `TURN_COMPLETED` 常量值稳定，`onTurnCompleted` 签名向后兼容

**Tech Stack:** React, TypeScript, Zustand, Tauri v2, Vitest

**Worktree branch:** `pzc`

**前置依赖:** Plan-O 后端实现 `turn:completed` 事件（Rust 侧 `RuntimeEventKind::TurnCompleted` + `TauriEventAdapter` 映射）。Plan-Q 可以在 Plan-O 合并前以 mock 形式独立开发和测试。

---

## 背景与动机

当前前端处理非正常 turn 终态的唯一机制是 `useStreaming.ts` 中 200 秒 watchdog：

```
STALE_STREAM_TIMEOUT_MS = 200_000
WATCHDOG_INTERVAL_MS    = 10_000
```

一旦 agent 因迭代上限或预算耗尽而提前退出，后端不再发 `streaming:delta`，watchdog 要等满 200 秒才介入。用户体验为：发送消息 → 界面卡住 → 3 分钟后弹出"响应超时"警告。

Plan-O 将从后端发出 `turn:completed` 携带真实终态枚举，本计划负责前端消费。

---

## Payload 类型规范

与 Plan-O 的 Rust `TurnOutcome` 枚举对应关系：

| Rust 枚举变体 | TypeScript 字符串字面量 |
|---|---|
| `TurnOutcome::Success` | `'Success'` |
| `TurnOutcome::MaxIterationsReached` | `'MaxIterationsReached'` |
| `TurnOutcome::BudgetExceeded` | `'BudgetExceeded'` |
| `TurnOutcome::ExecutionError` | `'ExecutionError'` |

Rust 端用 `#[serde(rename_all = "camelCase")]` 序列化结构体字段，但枚举变体名保持 PascalCase（`serde` 默认行为）。TS 侧字符串字面量需与 Rust serde 输出精确匹配。

---

## 改造视角

### Q1：TAURI_EVENTS 常量 + Payload 类型 + `onTurnCompleted` 封装

**当前状态**：`tauri.ts` 的 `TAURI_EVENTS` 对象没有 `TURN_COMPLETED` 条目，没有对应的 payload 类型，没有 `onTurnCompleted` 监听器封装。

**目标状态**：

```typescript
// TAURI_EVENTS 新增
TURN_COMPLETED: 'turn:completed'

// 新增 payload 类型
export type TurnOutcome =
  | 'Success'
  | 'MaxIterationsReached'
  | 'BudgetExceeded'
  | 'ExecutionError'

export interface TurnCompletedPayload {
  /** 对话 ID，与其他事件字段保持一致 */
  conversationId: string
  /** turn 终态枚举，与 Rust TurnOutcome serde 输出对应 */
  outcome: TurnOutcome
  /** 本次 turn 总 token 输入数（可选，后端不一定填充） */
  totalInputTokens?: number
  /** 本次 turn 总 token 输出数（可选） */
  totalOutputTokens?: number
  /** 本次 turn 估算美元成本（可选，后端可能为 null） */
  totalCostUsd?: number | null
  /** 触发终止的权限拒绝次数（可选，用于诊断） */
  permissionDenialCount?: number
}

// 新增监听器
export function onTurnCompleted(
  handler: (payload: TurnCompletedPayload) => void,
): Promise<() => void>
```

**改造范围**：仅 `src/lib/tauri.ts`，新增约 35 行，不修改任何现有内容。

---

### Q2：`useStreaming.ts` 订阅 turn:completed + 非正常终态 toast

**当前状态**：`useStreaming.ts` 不监听 `turn:completed`，非正常终态只靠 watchdog 兜底。

**目标状态**：

新增 `useTauriEvent` 调用块，逻辑如下：

```typescript
// --- turn:completed -----------------------------------------------
useTauriEvent(() =>
  onTurnCompleted(({ conversationId, outcome, totalInputTokens, totalOutputTokens, totalCostUsd }: TurnCompletedPayload) => {
    console.log('[turn:completed]', conversationId, outcome, { totalInputTokens, totalOutputTokens, totalCostUsd })

    // 1. 主动清理 busy/stream 状态，无需等 watchdog
    //    仅对非 Success 终态清理（Success 路径已有 streaming:done / agent:idle 覆盖）
    if (outcome !== 'Success') {
      flushConversationDeltas(conversationId)
      delete lastActivityRef.current[conversationId]
      const store = useChatStore.getState()
      store.clearConversationStreamState(conversationId)
      store.removeBusyConversation(conversationId)
    }

    // 2. 按终态推送 toast
    switch (outcome) {
      case 'MaxIterationsReached':
        useNotificationStore.getState().push({
          level: 'warning',
          title: i18n.t('turnOutcome.maxIterationsTitle'),
          message: i18n.t('turnOutcome.maxIterationsDesc'),
          actions: [],
          dismissible: true,
          autoHide: 12,
          context: 'toast',
        })
        break
      case 'BudgetExceeded':
        useNotificationStore.getState().push({
          level: 'warning',
          title: i18n.t('turnOutcome.budgetExceededTitle'),
          message: i18n.t('turnOutcome.budgetExceededDesc'),
          actions: [],
          dismissible: true,
          autoHide: 12,
          context: 'toast',
        })
        break
      case 'ExecutionError':
        useNotificationStore.getState().push({
          level: 'error',
          title: i18n.t('turnOutcome.executionErrorTitle'),
          message: i18n.t('turnOutcome.executionErrorDesc'),
          actions: [],
          dismissible: true,
          autoHide: 10,
          context: 'toast',
        })
        break
      case 'Success':
        // 可选：仅在 cost 有值时显示 info 摘要
        if (totalCostUsd != null && totalCostUsd > 0) {
          const tokens = (totalInputTokens ?? 0) + (totalOutputTokens ?? 0)
          useNotificationStore.getState().push({
            level: 'info',
            title: i18n.t('turnOutcome.successSummaryTitle'),
            message: i18n.t('turnOutcome.successSummaryDesc', {
              tokens,
              cost: totalCostUsd.toFixed(4),
            }),
            actions: [],
            dismissible: true,
            autoHide: 6,
            context: 'toast',
          })
        }
        break
    }
  }),
)
```

**i18n 键新增**（`zh-CN.json` 和 `en-US.json` 的 `turnOutcome` 命名空间）：

| key | zh-CN | en-US |
|---|---|---|
| `turnOutcome.maxIterationsTitle` | 已达最大迭代次数 | Max iterations reached |
| `turnOutcome.maxIterationsDesc` | Agent 已达到本轮最大迭代次数上限，本轮已停止。如需继续，请发送新消息。 | The agent reached its maximum iteration limit for this turn. Send a new message to continue. |
| `turnOutcome.budgetExceededTitle` | 已超出预算上限 | Budget limit exceeded |
| `turnOutcome.budgetExceededDesc` | 本轮 Token 用量已超出预设预算，已自动停止。 | This turn exceeded the configured token budget and was stopped automatically. |
| `turnOutcome.executionErrorTitle` | 执行出现错误 | Execution error |
| `turnOutcome.executionErrorDesc` | Agent 执行过程中出现错误，本轮已中止。请重试或检查设置。 | The agent encountered an error and this turn was aborted. Please retry or check settings. |
| `turnOutcome.successSummaryTitle` | Turn 完成 | Turn complete |
| `turnOutcome.successSummaryDesc` | 共消耗 {{tokens}} tokens，估算费用 ${{cost}}。 | Used {{tokens}} tokens, estimated cost ${{cost}}. |

**改造范围**：`src/hooks/useStreaming.ts`（新增 1 个 `useTauriEvent` 块，约 50 行），`src/i18n/zh-CN.json`、`src/i18n/en-US.json`（各新增 8 个 key）。不修改现有监听器。

---

### Q3（可选）：token/cost 状态存储 + 消息列表底部徽章

**当前状态**：`streamingStore.ts` / `ConversationStreamState` 不保存 turn 终态数据。

**目标状态**（选做，可在 Q2 完成后独立追加）：

在 `streamingStore.ts` 的 `ConversationStreamState` 中新增字段：

```typescript
export interface TurnSummary {
  outcome: TurnOutcome
  totalInputTokens?: number
  totalOutputTokens?: number
  totalCostUsd?: number | null
  completedAt: number
}

export interface ConversationStreamState {
  // ...现有字段不变...
  lastTurnSummary?: TurnSummary   // 最近一次 turn 的终态摘要
}
```

在 `streamingStore.ts` 新增 action：

```typescript
setLastTurnSummary: (convId: string, summary: TurnSummary) => void
```

在 `useStreaming.ts` 的 `turn:completed` 处理器末尾调用 `useChatStore.getState().setLastTurnSummary(conversationId, { ... })`。

在 `MessageList.tsx` 或 `ChatArea.tsx` 底部渲染轻量的 `TurnSummaryBadge` 组件（仅在 `lastTurnSummary` 存在且非 Success、或 cost > 0 时显示）：

```tsx
// 组件位置：src/components/chat/TurnSummaryBadge.tsx
// 显示规则：
//   - MaxIterationsReached / BudgetExceeded → 橙色小徽章 + 终态文字
//   - ExecutionError → 红色小徽章
//   - Success + cost → 灰色小徽章 "~$0.0012 · 1234 tokens"
//   - Success + no cost → 不渲染
```

**改造范围**：`src/stores/streamingStore.ts`（小幅扩展），`src/hooks/useStreaming.ts`（Q2 基础上追加 1 行），`src/components/chat/TurnSummaryBadge.tsx`（新建），`src/components/chat/MessageList.tsx` 或 `src/components/layout/ChatArea.tsx`（引入徽章）。

---

### Q4：review 约束测试

**范围**：两个测试文件：

1. `src/lib/tauri.events.test.ts` 扩展——验证 `TURN_COMPLETED` 常量值、`TurnCompletedPayload` 类型形态、`onTurnCompleted` 注册正确的事件名。
2. `src/hooks/useStreaming.integration.test.tsx` 扩展——验证：
   - `turn:completed` listener 被注册
   - `MaxIterationsReached` payload → toast 被推送（level = warning）
   - `BudgetExceeded` payload → toast 被推送（level = warning）
   - `ExecutionError` payload → toast 被推送（level = error）
   - `MaxIterationsReached` payload → 对应 conversation busy 状态被清除
   - `Success` + `totalCostUsd = 0.001` → info toast 被推送
   - `Success` + `totalCostUsd = null` → 不推送 toast

---

## 执行约束

1. 严格按 Q1 → Q2 → Q4（→ Q3 可选）执行，每个 Task 独立 commit。
2. 每个 Task 遵循 TDD：先写失败测试 → 确认失败 → 最小实现 → 确认通过 → commit。
3. 每个 Task 完成后立即停下汇报，不得连续推进。
4. Q1 不修改任何现有 payload 类型或监听器，只新增。
5. Q2 的 `Success` cost toast 默认关闭（`totalCostUsd` 为 null 时不显示），避免骚扰。
6. Q3 整体为可选，不应阻塞 Q4。
7. `tauri.ts` 中禁止直接使用字符串字面量注册事件；必须通过 `TAURI_EVENTS` 常量。

---

## 每个 Task 详细步骤

### Q1 步骤

- [ ] 在 `src/lib/tauri.events.test.ts` 中新增：
  - 断言 `TAURI_EVENTS.TURN_COMPLETED === 'turn:completed'`
  - 断言 `TurnCompletedPayload` 的结构字段（outcome, conversationId, totalCostUsd 可选）
  - 断言 `onTurnCompleted` 调用后 `listen` 被以 `'turn:completed'` 调用
- [ ] 运行测试，确认新增测试**失败**
- [ ] 在 `src/lib/tauri.ts` 中：
  1. `TAURI_EVENTS` 对象末尾新增 `TURN_COMPLETED: 'turn:completed'`
  2. 新增 `TurnOutcome` 类型和 `TurnCompletedPayload` 接口（放在 `TaskStatusChangedPayload` 之后）
  3. 新增 `onTurnCompleted` 函数（放在 `onTaskStatusChanged` 之后）
- [ ] 运行测试，确认全部通过
- [ ] `git commit`：`feat(tauri): add TurnCompleted event constant, payload type, and listener - Q1`

### Q2 步骤

- [ ] 在 `src/hooks/useStreaming.integration.test.tsx` 中新增 Q4 描述的 7 条测试
- [ ] 运行测试，确认新增测试**失败**
- [ ] 在 `src/i18n/zh-CN.json` 和 `src/i18n/en-US.json` 中各新增 `turnOutcome` 命名空间（8 个 key）
- [ ] 在 `src/hooks/useStreaming.ts` 中：
  1. 从 `@/lib/tauri` 引入 `onTurnCompleted` 和 `TurnCompletedPayload`
  2. 在 `onTaskStatusChanged` useTauriEvent 块之后新增 `turn:completed` useTauriEvent 块
- [ ] 运行测试，确认全部通过
- [ ] 运行前端完整测试 `pnpm test`，确认无回归
- [ ] `git commit`：`feat(streaming): subscribe turn:completed for non-success outcome toasts - Q2`

### Q3 步骤（可选）

- [ ] 在 `src/stores/streamingStore.ts` 中新增 `TurnSummary` 接口、`lastTurnSummary?` 字段、`setLastTurnSummary` action
- [ ] 在 `src/hooks/useStreaming.ts` 的 `turn:completed` 处理器末尾调用 `setLastTurnSummary`
- [ ] 新建 `src/components/chat/TurnSummaryBadge.tsx`
- [ ] 在 `src/components/chat/MessageList.tsx` 底部引入 `TurnSummaryBadge`
- [ ] 为 `TurnSummaryBadge` 写基础 Vitest 快照或行为测试
- [ ] `git commit`：`feat(chat): add TurnSummaryBadge for token cost and outcome display - Q3`

### Q4 步骤

- [ ] （Q4 测试已在 Q2 步骤中一并编写，Q4 作为 review 节点：回顾所有新增测试，确认覆盖充分）
- [ ] 运行关键集成测试套件：
  ```bash
  pnpm exec vitest run src/lib/tauri.events.test.ts src/hooks/useStreaming.integration.test.tsx
  ```
- [ ] 运行全量前端测试：`pnpm test`
- [ ] `git commit`：`test(turn-outcome): review constraints for turn:completed frontend integration - Q4`

---

## 回归验证命令

```bash
# 关键集成测试（每个 Task 完成后执行）
pnpm exec vitest run src/lib/tauri.events.test.ts src/hooks/useStreaming.integration.test.tsx src/stores/chatStore.test.ts

# 全量前端测试（Q2、Q3 完成后执行）
pnpm test

# ESLint 检查
pnpm lint
```

---

## 与 Plan-O 的接口合同

本计划假设 Plan-O 后端发出如下格式的 Tauri 事件（Rust serde 序列化结果）：

```json
{
  "conversationId": "conv-abc",
  "outcome": "MaxIterationsReached",
  "totalInputTokens": 12400,
  "totalOutputTokens": 3100,
  "totalCostUsd": 0.0031,
  "permissionDenialCount": 0
}
```

`outcome` 字段使用 PascalCase（Rust serde 默认枚举变体名），不加额外映射。如 Plan-O 实际采用 snake_case（`max_iterations_reached`），需同步更新 `TurnOutcome` 类型定义和 i18n 测试用例。
