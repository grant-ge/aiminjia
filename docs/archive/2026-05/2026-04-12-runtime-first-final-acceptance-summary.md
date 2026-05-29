# 2026-04-12 Runtime-First 改造总体验收摘要

## 状态

- 结论：已通过当前验收范围
- 日期：2026-04-12
- 范围：`lotus-app` 后端 runtime-first 迁移核心 + 前后端事件联调闭环

## 本次验收覆盖的核心范围

- 后端主链路：`SessionRuntime` / `QueryEngine` / `RuntimeEventBus`
- 宿主适配：`TauriEventAdapter` / `transport/tauri_commands`
- Tool / Task / Agent 迁移核心的 review/TDD 回归
- 前端事件消费闭环：`src/lib/tauri.ts`、`src/hooks/useStreaming.ts`、`src/stores/chatStore.ts`

## 最终结论

在本轮验收范围内，runtime-first 改造已经形成可工作的闭环：

- 后端 runtime 事件可以通过 `TauriEventAdapter` 映射到 legacy host 事件
- 前端已经真实消费 `task:status-changed`
- `agent:idle` 已按 `scope` / `agentId` 区分 child 与 primary，不再误清 parent conversation
- Rust 侧严格 review/TDD 用例已经全部转绿
- 前端 contract / integration / store 测试已经通过

因此，截至 2026 年 4 月 12 日，当前这批 runtime-first 核心改造没有阻塞验收的已知问题。

## 本次实际复验证据

### 2026-04-12 执行并通过的 targeted suites

```bash
cd /Users/a20250311/IdeaProjects/lotus-app
pnpm exec vitest run src/lib/tauri.events.test.ts src/hooks/useStreaming.integration.test.tsx src/stores/chatStore.test.ts
```

结果：

- 3 files
- 32 tests
- 全部通过

```bash
cd /Users/a20250311/IdeaProjects/lotus-app/src-tauri
cargo test --test tauri_event_adapter_test -- --nocapture
cargo test review_ --tests --no-fail-fast
```

结果：

- `tauri_event_adapter_test` 全部通过
- Rust 侧 `review_` 套件全部通过

### 作为更大基线沿用的历史证据

```bash
cd /Users/a20250311/IdeaProjects/lotus-app/src-tauri
cargo test --tests --no-fail-fast
```

说明：

- 这套更大基线此前已通过
- 本次 2026-04-12 的收尾以 targeted acceptance suites 为主，没有重新全量回归一次所有 Rust tests

## 本轮验收通过的关键点

### 1. runtime -> transport -> front-end 闭环成立

- 后端 runtime 事件不再停留在 store / audit 层
- `TauriEventAdapter` 已把关键 runtime 事件映射为宿主可消费协议
- 前端 listener 与 store 已接上，真实形成消费链路

### 2. task terminal signal 已进入前端状态层

- `task:status-changed` 已有 typed wrapper
- `useStreaming()` 已注册监听
- `chatStore` 已有 task state 落点

### 3. child/background agent 不再误伤主会话状态

- `agent:idle` payload 已包含 `scope`
- 前端不再把 child idle 直接当成 primary run complete

### 4. 严格 TDD review 的 blocking findings 已关闭

- runtime transport bus wiring
- task terminal notification 映射
- task runtime event emission
- sub-agent background caller wiring
- 前端事件联调相关 finding

## 关键代码与文档落点

### 后端

- `/Users/a20250311/IdeaProjects/lotus-app/src-tauri/src/transport/tauri_event_adapter.rs`
- `/Users/a20250311/IdeaProjects/lotus-app/src-tauri/tests/tauri_event_adapter_test.rs`
- `/Users/a20250311/IdeaProjects/lotus-app/src-tauri/tests/review_*.rs`

### 前端

- `/Users/a20250311/IdeaProjects/lotus-app/src/lib/tauri.ts`
- `/Users/a20250311/IdeaProjects/lotus-app/src/hooks/useStreaming.ts`
- `/Users/a20250311/IdeaProjects/lotus-app/src/stores/chatStore.ts`
- `/Users/a20250311/IdeaProjects/lotus-app/src/hooks/useStreaming.integration.test.tsx`
- `/Users/a20250311/IdeaProjects/lotus-app/src/lib/tauri.events.test.ts`
- `/Users/a20250311/IdeaProjects/lotus-app/src/stores/chatStore.test.ts`

### 审计 / 计划 / review

- `/Users/a20250311/IdeaProjects/lotus-app/docs/architecture-blueprint.md`
- `/Users/a20250311/IdeaProjects/lotus-app/docs/reviews/2026-04-10-runtime-first-strict-tdd-review.md`
- `/Users/a20250311/IdeaProjects/lotus-app/docs/reviews/2026-04-12-front-end-event-integration-review.md`
- `/Users/a20250311/IdeaProjects/lotus-app/docs/2026-04-12-review-plan-status-matrix.md`

## 非阻塞遗留项

以下项目不阻塞当前验收，但仍值得后续继续治理：

- Rust 编译 warning 仍需要逐步清理
- 更深一层的 store/file-store 退场与彻底瘦身仍可继续做工程化优化
- 如果后续要验证更高层真实 Tauri host UI 行为，建议另开专项计划，不复用本轮验收文档

## 建议作为当前权威入口的文档

如果现在只看 3 份文档，建议优先看：

1. `/Users/a20250311/IdeaProjects/lotus-app/docs/2026-04-12-runtime-first-final-acceptance-summary.md`
2. `/Users/a20250311/IdeaProjects/lotus-app/docs/2026-04-12-review-plan-status-matrix.md`
3. `/Users/a20250311/IdeaProjects/lotus-app/docs/README-architecture-plan.md`
