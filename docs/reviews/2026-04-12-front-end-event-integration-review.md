# 2026-04-12 前端事件消费链路 / Tauri Host 联调 Review

## 状态

- 状态：已关闭
- 结论：本轮联调 findings 已修复并在 2026-04-12 回归通过

## 范围

本轮 review 聚焦 runtime-first 改造之后的更高一层联调边界：

- 前端事件消费链路：`/Users/a20250311/IdeaProjects/lotus-app/src/hooks/useStreaming.ts`
- 前端 Tauri IPC 定义：`/Users/a20250311/IdeaProjects/lotus-app/src/lib/tauri.ts`
- 后端真实事件发射：`/Users/a20250311/IdeaProjects/lotus-app/src-tauri/src/transport/tauri_event_adapter.rs`
- 后台子 agent 完成路径：`/Users/a20250311/IdeaProjects/lotus-app/src-tauri/src/runtime/agent/agent_runtime.rs`

## 本轮新增 TDD

新增并保留的联调用例：

- `/Users/a20250311/IdeaProjects/lotus-app/src/hooks/useStreaming.integration.test.tsx`

首次执行命令：

```bash
cd /Users/a20250311/IdeaProjects/lotus-app
pnpm install
pnpm exec vitest run src/hooks/useStreaming.integration.test.tsx
```

首次结果：2 个 failing tests，均可稳定复现。

## 2026-04-12 验收复查结果

Claude 已按这轮联调 review 完成修复，我重新验收后的结果如下：

```bash
cd /Users/a20250311/IdeaProjects/lotus-app
pnpm exec vitest run src/lib/tauri.events.test.ts src/hooks/useStreaming.integration.test.tsx src/stores/chatStore.test.ts

cd /Users/a20250311/IdeaProjects/lotus-app/src-tauri
cargo test --test tauri_event_adapter_test -- --nocapture
cargo test review_ --tests --no-fail-fast
```

当前结果：

- 前端 contract / integration / store tests 通过
- `tauri_event_adapter_test` 通过
- Rust 侧 `review_` 回归继续保持通过

因此，本轮 2 个联调 finding 已修复。

## Findings

### Finding 1 - 前端完全没有消费 `task:status-changed`，runtime task terminal signal 在 UI 边界直接丢失

状态：`已修复`

- 严重级别：`P1`
- 影响范围：TaskRuntime → Tauri host → 前端事件消费链路
- 真实使用路径：后端已经通过 `TauriEventAdapter` 发出 `task:status-changed`，但前端既没有 event constant，也没有 listener，更没有 store 消费路径

#### 代码定位

- `/Users/a20250311/IdeaProjects/lotus-app/src-tauri/src/transport/tauri_event_adapter.rs:83`
- `/Users/a20250311/IdeaProjects/lotus-app/src/lib/tauri.ts:23`
- `/Users/a20250311/IdeaProjects/lotus-app/src/hooks/useStreaming.ts:38`
- `/Users/a20250311/IdeaProjects/lotus-app/src/hooks/useStreaming.integration.test.tsx:59`

#### 问题说明

后端现在已经把 `TaskStatusChanged` 映射成 legacy 事件：

- 事件名：`task:status-changed`
- payload：`conversationId / taskId / status / runId`

但前端：

- `TAURI_EVENTS` 没有 `TASK_STATUS_CHANGED`
- `src/lib/tauri.ts` 没有 `onTaskStatusChanged()` wrapper
- `useStreaming()` 没有注册 task terminal listener

这意味着 runtime/task 层已经具备事件语义，但在真实 UI 边界完全断掉。

#### 原失败测试

- `/Users/a20250311/IdeaProjects/lotus-app/src/hooks/useStreaming.integration.test.tsx:59`

断言：

- `useStreaming()` 挂载后，前端应当对 `task:status-changed` 建立监听
- 历史结果：没有任何监听注册

#### 修复后验证

- `src/lib/tauri.ts` 已新增 `TASK_STATUS_CHANGED` 和 `onTaskStatusChanged()`
- `src/hooks/useStreaming.ts` 已注册 task terminal listener
- `src/stores/chatStore.ts` 已新增 `taskStates` / `upsertConversationTaskState()`
- `src/hooks/useStreaming.integration.test.tsx` 已验证事件可写入 store

#### 通过条件

- 前端定义并注册 `task:status-changed`
- 至少存在一条明确消费路径：store / UI / notification / task panel 之一
- `pnpm exec vitest run src/hooks/useStreaming.integration.test.tsx` 转绿

---

### Finding 2 - background child agent 的 `agent:idle` 会被前端误判成整个会话已完成，导致主会话 UI 被提前清空

状态：`已修复`

- 严重级别：`P1`
- 影响范围：background sub-agent、busy state、streaming UI、主会话联调行为
- 真实使用路径：后台 child run 完成后，后端会发 `agent:idle`；前端 `useStreaming()` 收到任何 `agent:idle` 都会直接 `removeBusyConversation + clearConversationStreamState`

#### 代码定位

- `/Users/a20250311/IdeaProjects/lotus-app/src-tauri/src/runtime/agent/agent_runtime.rs:88`
- `/Users/a20250311/IdeaProjects/lotus-app/src-tauri/src/transport/tauri_event_adapter.rs:75`
- `/Users/a20250311/IdeaProjects/lotus-app/src/lib/tauri.ts:67`
- `/Users/a20250311/IdeaProjects/lotus-app/src/hooks/useStreaming.ts:303`
- `/Users/a20250311/IdeaProjects/lotus-app/src/hooks/useStreaming.integration.test.tsx:70`

#### 问题说明

后端 `complete_background_run()` 明确会在 child/background run 完成时发 `AgentIdle`。

并且 adapter payload 已经带了：

- `conversationId`
- `agentId`
- `runId`

但前端 `AgentIdlePayload` 只声明了 `conversationId`，`useStreaming()` 也只按 `conversationId` 处理：

- 清 busy
- 清 streaming state

结果就是：

- 只要某个 child/background agent 完成
- 前端就会把整个 parent conversation 当成“整轮 finished”
- 主 agent 还在跑时，UI 也会被提前清空

#### 原失败测试

- `/Users/a20250311/IdeaProjects/lotus-app/src/hooks/useStreaming.integration.test.tsx:70`

断言：

- child/background agent 的 `agent:idle` 不应直接清空 parent conversation 的 busy / streaming 状态
- 历史结果：当前会直接清空

#### 修复后验证

- 后端 adapter payload 已补 `scope`
- `src/lib/tauri.ts` 已补 `AgentIdlePayload.scope`
- `src/hooks/useStreaming.ts` 已区分 `child` 与 `primary`
- `src/hooks/useStreaming.integration.test.tsx` 已验证 child idle 不再误清 parent conversation

#### 通过条件

- 前端能区分“child agent idle”与“primary run complete”
- 或后端改成不同事件语义，不再复用同一个 `agent:idle`
- `pnpm exec vitest run src/hooks/useStreaming.integration.test.tsx` 转绿

## 备注

我顺手跑了：

```bash
pnpm exec vitest run src/stores/chatStore.test.ts src/hooks/useStreaming.integration.test.tsx
```

其中 `src/stores/chatStore.test.ts` 那条旧用例语义偏差也已一并收敛；当前这轮联调 review 的主问题都已关闭。
