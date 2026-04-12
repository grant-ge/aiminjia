# Front-End Event Integration Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 把 runtime-first 后端新增出来的 `task / child-agent / richer agent idle payload` 语义，完整对接到前端事件消费层，避免“后端事件已发出，但 UI 边界直接丢失或误判”的联调问题。

**Architecture:** 后端 runtime event protocol 是真相源；`transport/tauri_event_adapter.rs` 负责 legacy event 兼容；前端 `src/lib/tauri.ts` 负责 typed wrapper；`src/hooks/useStreaming.ts` 负责消费与状态落点；`chatStore` / `notificationStore` 负责持久化 UI 可见状态。

**Tech Stack:** Rust, Tauri event adapter, TypeScript, React hooks, Zustand, Vitest, cargo test

## 当前实际状态（2026-04-12）

- 状态：已完成
- 后端 runtime-first 核心 review 已保持转绿：`cargo test review_ --tests --no-fail-fast` 通过
- 前端联调问题已修复：
  - 前端已消费 `task:status-changed`
  - background child agent 的 `agent:idle` 不再误清 parent conversation 的 busy / streaming 状态
- 已新增并保留测试：
  - `/Users/a20250311/IdeaProjects/lotus-app/src/lib/tauri.events.test.ts`
  - `/Users/a20250311/IdeaProjects/lotus-app/src/hooks/useStreaming.integration.test.tsx`
- 最终验证命令：
  - `cd /Users/a20250311/IdeaProjects/lotus-app && pnpm exec vitest run src/lib/tauri.events.test.ts src/hooks/useStreaming.integration.test.tsx src/stores/chatStore.test.ts`
  - `cd /Users/a20250311/IdeaProjects/lotus-app/src-tauri && cargo test --test tauri_event_adapter_test -- --nocapture`
  - `cd /Users/a20250311/IdeaProjects/lotus-app/src-tauri && cargo test review_ --tests --no-fail-fast`

---

## Phase constraints

- 不开新“大阶段”重构，只做前后端事件协议补齐与消费层修复。
- 不要求现在就做完整 task 面板，但 `task:status-changed` 必须有明确消费落点，不能继续悬空。
- `agent:idle` 必须区分 `primary run complete` 与 `child/background idle`，不能再只按 `conversationId` 粗暴清状态。
- 这轮以 **联调 TDD 绿灯** 为完成标准，不以“代码看起来对了”为标准。

---

## Task 1: 冻结前后端事件契约，补齐 typed IPC wrapper

**Files:**
- Modify: `src/lib/tauri.ts`
- Modify: `src/hooks/useStreaming.integration.test.tsx`
- Create: `src/lib/tauri.events.test.ts`
- Optional Modify: `src/types/message.ts`（如果需要补充运行时事件类型）

- [x] **Step 1: 先写失败测试，锁定前端必须显式暴露新事件和 richer payload**

```ts
import { describe, expect, it } from 'vitest'
import { TAURI_EVENTS } from '@/lib/tauri'

describe('tauri event contract', () => {
  it('exposes task status changed event constant', () => {
    expect(TAURI_EVENTS.TASK_STATUS_CHANGED).toBe('task:status-changed')
  })

  it('agent idle payload keeps scope fields for child/primary discrimination', () => {
    type Payload = import('@/lib/tauri').AgentIdlePayload
    const payload: Payload = {
      conversationId: 'conv-1',
      runId: 'run-1',
      agentId: 'agent-1',
      scope: 'child',
    }
    expect(payload.scope).toBe('child')
  })
})
```

- [x] **Step 2: 运行测试确认失败**

Run: `cd /Users/a20250311/IdeaProjects/lotus-app && pnpm exec vitest run src/lib/tauri.events.test.ts src/hooks/useStreaming.integration.test.tsx`

Expected:
- `TASK_STATUS_CHANGED` 不存在
- `AgentIdlePayload` 没有 `runId / agentId / scope`
- `useStreaming.integration.test.tsx` 仍然失败

- [x] **Step 3: 写最小 typed contract**

```ts
export const TAURI_EVENTS = {
  // ...
  TASK_STATUS_CHANGED: 'task:status-changed',
} as const

export interface TaskStatusChangedPayload {
  conversationId: string
  taskId: string
  status: string
  runId: string
}

export interface AgentIdlePayload {
  conversationId: string
  runId?: string
  agentId?: string
  scope?: 'primary' | 'child'
}

export function onTaskStatusChanged(
  handler: (payload: TaskStatusChangedPayload) => void,
): Promise<() => void> {
  return listen<TaskStatusChangedPayload>(TAURI_EVENTS.TASK_STATUS_CHANGED, (event) => {
    handler(event.payload)
  })
}
```

- [x] **Step 4: 运行测试确认 contract 层通过**

Run: `cd /Users/a20250311/IdeaProjects/lotus-app && pnpm exec vitest run src/lib/tauri.events.test.ts`

Expected: PASS

---

## Task 2: 给 `task:status-changed` 一个明确前端消费落点

**Files:**
- Modify: `src/hooks/useStreaming.ts`
- Modify: `src/stores/chatStore.ts`
- Modify: `src/stores/chatStore.test.ts`
- Modify: `src/hooks/useStreaming.integration.test.tsx`

- [x] **Step 1: 先写失败测试，要求 `useStreaming()` 真的注册并消费 task terminal event**

在现有 `/Users/a20250311/IdeaProjects/lotus-app/src/hooks/useStreaming.integration.test.tsx` 基础上补强：

```tsx
it('registers a frontend listener for runtime task terminal notifications', async () => {
  render(<HookHarness />)
  await waitForListeners()

  expect(tauriEventMock.listeners.has('task:status-changed')).toBe(true)
})
```

再补 store 落点测试，例如：

```ts
it('records task terminal state per conversation', () => {
  const store = useChatStore.getState()
  store.upsertConversationTaskState('conv-1', {
    taskId: 'task-1',
    status: 'completed',
    runId: 'run-1',
  })
  expect(store.taskStates['conv-1']?.[0]?.status).toBe('completed')
})
```

- [x] **Step 2: 运行测试确认失败**

Run: `cd /Users/a20250311/IdeaProjects/lotus-app && pnpm exec vitest run src/hooks/useStreaming.integration.test.tsx src/stores/chatStore.test.ts`

Expected:
- 当前没有 `task:status-changed` listener
- `chatStore` 没有 task state 容器和 action

- [x] **Step 3: 选择最小但真实的 UI 落点**

本计划建议的最小落点不是只弹 toast，而是：

```ts
interface ConversationTaskState {
  taskId: string
  status: string
  runId: string
}

taskStates: Record<string, ConversationTaskState[]>
upsertConversationTaskState(...)
```

然后 `useStreaming()`：

```ts
onTaskStatusChanged(({ conversationId, taskId, status, runId }) => {
  useChatStore.getState().upsertConversationTaskState(conversationId, {
    taskId,
    status,
    runId,
  })
})
```

如果产品上暂时没有 task UI，可选附加：
- 对 `failed / cancelled` 推送 toast
- 对 `completed` 仅落 store，不强制打扰用户

- [x] **Step 4: 更新 store 测试与 hook 集成测试**

要求：
- `task:status-changed` 被注册
- 事件到达后，状态进入 `chatStore`
- 不引入 activeConversation 假设，仍按 `conversationId` 归属

- [x] **Step 5: 运行测试确认通过**

Run: `cd /Users/a20250311/IdeaProjects/lotus-app && pnpm exec vitest run src/hooks/useStreaming.integration.test.tsx src/stores/chatStore.test.ts`

Expected: PASS

---

## Task 3: 修正 `agent:idle` 语义，避免 child/background run 误清 parent conversation

**Files:**
- Modify: `src/lib/tauri.ts`
- Modify: `src/hooks/useStreaming.ts`
- Modify: `src/hooks/useStreaming.integration.test.tsx`
- Modify: `src-tauri/src/transport/tauri_event_adapter.rs`
- Modify: `src-tauri/tests/tauri_event_adapter_test.rs`
- Optional Modify: `src-tauri/src/transport/tauri_commands/chat/chat_support.rs`

- [x] **Step 1: 先写失败测试，明确 child idle 与 primary idle 不能再混用**

前端集成失败测试已经存在：

```tsx
it('does not clear the parent conversation when a child/background agent becomes idle', async () => {
  // child run idle should not remove parent busy/stream state
})
```

再补一条 Rust 侧 adapter 测试，锁定 payload 至少带 `agentId / runId / scope`：

```rust
#[test]
fn maps_agent_idle_with_scope_metadata() {
    let event = RuntimeEvent::new(
        SessionId::new("conv-1"),
        RunId::new("run-parent"),
        RuntimeEventKind::AgentIdle { agent_id: AgentId::new("agent-child") },
    );
    let mapped = map_runtime_event(&event).unwrap();
    assert_eq!(mapped.payload.get("agentId").and_then(|v| v.as_str()), Some("agent-child"));
    assert!(mapped.payload.get("scope").is_some());
}
```

- [x] **Step 2: 运行测试确认失败**

Run:
- `cd /Users/a20250311/IdeaProjects/lotus-app && pnpm exec vitest run src/hooks/useStreaming.integration.test.tsx`
- `cd /Users/a20250311/IdeaProjects/lotus-app/src-tauri && cargo test tauri_event_adapter_test -- --nocapture`

Expected:
- 前端仍会误清 parent conversation
- Rust payload 还没有显式 `scope`

- [x] **Step 3: 明确事件语义，禁止前端再靠猜**

本计划推荐的契约：

```ts
type AgentIdleScope = 'primary' | 'child'
```

后端：
- background child run 发 `agent:idle` 时，payload 带 `scope: "child"`
- primary agent loop 正常结束时，payload 带 `scope: "primary"`

前端：

```ts
onAgentIdle(({ conversationId, scope }) => {
  if (scope === 'child') {
    // 只更新 child/task/subagent 状态，不清主会话 streaming
    return
  }

  store.removeBusyConversation(conversationId)
  store.clearConversationStreamState(conversationId)
})
```

- [x] **Step 4: 兼容旧 payload**

为了避免一次性打断旧路径：
- 当前阶段允许 `scope` 缺省
- 缺省时按 `primary` 处理
- 但所有 runtime-first 新路径必须补上 `scope`

- [x] **Step 5: 运行测试确认通过**

Run:
- `cd /Users/a20250311/IdeaProjects/lotus-app && pnpm exec vitest run src/hooks/useStreaming.integration.test.tsx`
- `cd /Users/a20250311/IdeaProjects/lotus-app/src-tauri && cargo test tauri_event_adapter_test -- --nocapture`

Expected: PASS

---

## Task 4: 做一轮前后端联调回归，锁定最终验收

**Files:**
- Modify: `docs/reviews/2026-04-12-front-end-event-integration-review.md`
- Optional Modify: `docs/reviews/2026-04-10-runtime-first-strict-tdd-review.md`

- [x] **Step 1: 运行前端专项回归**

```bash
cd /Users/a20250311/IdeaProjects/lotus-app
pnpm exec vitest run \
  src/lib/tauri.events.test.ts \
  src/hooks/useStreaming.integration.test.tsx \
  src/stores/chatStore.test.ts
```

Expected: PASS

- [x] **Step 2: 运行 Rust 侧兼容回归**

```bash
cd /Users/a20250311/IdeaProjects/lotus-app/src-tauri
cargo test review_ --tests --no-fail-fast
cargo test tauri_event_adapter_test -- --nocapture
```

Expected: PASS

- [x] **Step 3: 回填 review 文档**

要求：
- 把 `/Users/a20250311/IdeaProjects/lotus-app/docs/reviews/2026-04-12-front-end-event-integration-review.md` 中的 2 个 finding 标记为已修复
- 记录最终通过命令和日期

---

## Not Doing

- 这轮不做完整 task 页面 / task 面板产品化
- 这轮不重做整个前端状态架构
- 这轮不改 provider / LLM 逻辑
- 这轮不引入新的 transport 宿主

## Definition of Done

- 前端存在明确的 `task:status-changed` typed wrapper、listener 和状态落点
- `agent:idle` 不再误清 background child run 对应的 parent conversation
- `src/hooks/useStreaming.integration.test.tsx` 全绿
- Rust 侧 `cargo test review_ --tests --no-fail-fast` 保持全绿
- `/Users/a20250311/IdeaProjects/lotus-app/docs/reviews/2026-04-12-front-end-event-integration-review.md` 已回填为已修复状态
