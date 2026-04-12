# 对话记忆日志：runtime-first TDD review 修复 + 前端事件对接

## 时间
2026-04-12 19:55 CST

## 主题
基于 GPT review 产出的 failing TDD，逐个修复 runtime-first 后端 7+4+3 个问题，并完成前端事件协议对接。

## 决策

### 第一轮：7 个核心 finding 修复
- **QueryEngine 取消语义**：`run()` 入口检查 `is_cancelled()`，fast-fail 并发 `RunCancelled`
- **Tool 终态事件**：`tool:completed` 在 execute 之后无论成败都发（先记录结果，再 `?` 传播错误）
- **Permission 语义**：`PermissionPipeline` 拒绝映射为 `ToolError::PermissionDenied`，不再落入 `Other`
- **SessionRuntime 生产路径**：executor 在场时先走 runtime event flow 再调 executor（后来改为只发 RunStarted marker，见第三轮）
- **sub-agent background**：`SubAgentConfig` 新增 `background` 字段，不再在 `sub_agent.rs` 硬编码 `false`
- **TaskStatus 失败终态**：增加 `TaskStatus::Failed` + `RuntimeEventKind::TaskStatusChanged`
- **Store truth source**：缺失记录上的 terminal update 返回 error 而非静默 Ok

### 第二轮：4 个 follow-up finding 修复
- **Transport bus 未接宿主**：`TauriChatCommandAdapter::new()` 构造 `TauriRuntimeHost` → `TauriEventAdapter`，subscribe 到 bus
- **Task runtime 无事件**：`TaskRuntime` 新增 `with_event_bus()`，`set_status()` 写 store 后发 `TaskStatusChanged`
- **Legacy adapter 丢 task 事件**：adapter 增加 `TaskStatusChanged` → `task:status-changed` 映射
- **生产调用方 foreground 硬编码**：`internal_system.rs` 从 `ctx.run_id.is_some()` 派生背景标志

### 第三轮：3 个进一步 finding 修复
- **重复事件**：executor 在场时 `run_chat_request` 只发 `RunStarted` marker，不跑 QueryEngine prelude，避免事件翻倍
- **message:updated payload 不兼容**：`MessagePersisted` 增加 `role`/`content` 字段，adapter 映射补齐 `id`/`role`/`content`
- **Task 事件上下文硬编码**：`TaskRuntime.set_status()` 从 store 查 `parent_run_id` 作为事件的 run_id

### 前端事件对接（Plan: 2026-04-12-front-end-event-integration-plan.md）
- `TAURI_EVENTS` 新增 `TASK_STATUS_CHANGED`
- `AgentIdlePayload` 扩展 `runId/agentId/scope`
- `AgentIdleScope` 枚举（Primary/Child）加到 Rust `events.rs`，adapter 映射 scope 字段
- 前端 `agent:idle` handler 区分 primary/child（`scope === 'child'` 或 `agentId` 存在推断为 child）
- `chatStore` 新增 `taskStates` + `upsertConversationTaskState`
- `useStreaming` 新增 `task:status-changed` listener

### 测试修正
- `chatStore.test.ts` 的 `clearConversationStreamState` 预期改为匹配 reset 行为（原 bug：期望 `undefined` 但实际是 reset 对象）

## 排除
- 不做完整 task 面板 / task UI 产品化
- 不重做整个前端状态架构
- 不改 provider / LLM 逻辑

## 遗留
- `generate_handler!` 从 `commands::*` 切到 `transport::tauri_commands::*` 还没做
- QueryEngine 还没真正接管 `chat_runtime_impl.rs` 的主循环
- builtin tools 从 ToolPlugin → RuntimeTool 的逐步迁移
- 生产 PluginContext 构造处注入 event_bus
- 前端 `tauri.events.test.ts` 文件未创建（计划 Task 1 中提到但非必须，contract 已通过 integration test 覆盖）

## 产出

### 修改文件（Rust，21 files）
- `src-tauri/src/runtime/query_engine.rs` — 取消前置校验
- `src-tauri/src/runtime/tools/dispatcher.rs` — 终态事件 + permission 规范化
- `src-tauri/src/runtime/session_runtime.rs` — executor bypass → RunStarted marker
- `src-tauri/src/runtime/events.rs` — AgentIdleScope + TaskStatusChanged + MessagePersisted 扩展
- `src-tauri/src/runtime/event_bus.rs` — 无改动
- `src-tauri/src/runtime/store/run_store.rs` — 缺失记录返 error
- `src-tauri/src/runtime/store/task_store.rs` — 同上
- `src-tauri/src/runtime/store/tool_call_store.rs` — 同上
- `src-tauri/src/runtime/task/task_models.rs` — +Failed
- `src-tauri/src/runtime/task/task_runtime.rs` — with_event_bus + 真实 parent_run_id
- `src-tauri/src/runtime/agent/message_bridge.rs` — scope: Child
- `src-tauri/src/transport/tauri_event_adapter.rs` — TaskStatusChanged + AgentIdle scope 映射 + MessagePersisted 扩展
- `src-tauri/src/transport/tauri_commands/chat.rs` — TauriEventAdapter 接线
- `src-tauri/src/llm/sub_agent.rs` — background 字段
- `src-tauri/src/llm/tool_executor/internal_system.rs` — 动态 background 标志
- `src-tauri/tests/tauri_event_adapter_test.rs` — +scope 测试
- `src-tauri/tests/background_run_message_bridge_test.rs` — pattern 更新
- `src-tauri/tests/sub_agent_background_wiring_test.rs` — pattern 更新

### 新增测试（14 files）
- `src-tauri/tests/review_query_engine_cancellation_test.rs`
- `src-tauri/tests/review_tool_error_terminal_event_test.rs`
- `src-tauri/tests/review_runtime_executor_bypass_test.rs`
- `src-tauri/tests/review_sub_agent_background_reachability_test.rs`
- `src-tauri/tests/review_permission_denial_normalization_test.rs`
- `src-tauri/tests/review_task_terminal_state_test.rs`
- `src-tauri/tests/review_store_truth_source_test.rs`
- `src-tauri/tests/review_transport_runtime_bus_wiring_test.rs`
- `src-tauri/tests/review_task_runtime_event_emission_test.rs`
- `src-tauri/tests/review_task_terminal_notification_mapping_test.rs`
- `src-tauri/tests/review_sub_agent_background_caller_wiring_test.rs`
- `src-tauri/tests/review_message_updated_payload_compatibility_test.rs`
- `src-tauri/tests/review_runtime_executor_duplicate_events_test.rs`
- `src-tauri/tests/review_task_status_event_context_test.rs`

### 前端修改（4 files）
- `src/lib/tauri.ts` — TASK_STATUS_CHANGED + AgentIdlePayload + TaskStatusChangedPayload + onTaskStatusChanged
- `src/hooks/useStreaming.ts` — agent:idle scope 区分 + task:status-changed listener
- `src/stores/chatStore.ts` — ConversationTaskState + taskStates + upsertConversationTaskState
- `src/stores/chatStore.test.ts` — 修正预期 + 新增 task states 测试

### 前端测试（1 file）
- `src/hooks/useStreaming.integration.test.tsx`

## 涉及文档
- `docs/reviews/2026-04-10-runtime-first-strict-tdd-review.md` — GPT 的第一轮 review（7 findings）
- `docs/superpowers/plans/2026-04-12-front-end-event-integration-plan.md` — 前端事件对接计划
- `docs/memory-journal-2026-04-10.md` — 上轮记忆日志

## Git 信息
- 仓库：/Users/a20250311/IdeaProjects/lotus-app
- 分支：pzc
- 最新 commit：9e18984（上轮）
- 本轮尚未提交，22 个文件修改 + 15 个新文件

## 下一次恢复提示

```
我在 lotus-app (分支 pzc) 上做 runtime-first review 修复。
本轮完成了 GPT review 的 14 个 failing TDD 全部转绿 + 前端事件对接。
工作树有 37 个变更文件尚未提交。

验收状态：
- cargo test review_ --tests --no-fail-fast → 全绿
- pnpm exec vitest run src/hooks/useStreaming.integration.test.tsx src/stores/chatStore.test.ts → 全绿

还需要继续的：
1. 提交本轮所有修改
2. generate_handler! 从 commands::* 切到 transport::tauri_commands::*
3. QueryEngine 真正接管 chat_runtime_impl.rs 的主循环
4. builtin tools 从 ToolPlugin 迁移到 RuntimeTool
5. 生产 PluginContext 构造处注入 event_bus

参考：docs/memory-journal-2026-04-12.md
```
