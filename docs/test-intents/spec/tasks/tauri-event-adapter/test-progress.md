# test-progress.md — tauri-event-adapter 前后端事件协议映射

## 状态

- rules.md 已创建并经过 Opus review 修订
- 已实现测试文件：`src-tauri/tests/review_tauri_event_adapter_test.rs`
- 执行命令：`cd src-tauri && cargo test --test review_tauri_event_adapter_test -- --nocapture`
- 最近执行结果：14 passed；生产代码无需修改

## 执行记录

| 意图 | 状态 | 备注 |
|---|---|---|
| 意图 1：StreamDelta → streaming:delta | ✅ 通过 | 覆盖 conversationId/delta/runId |
| 意图 2：StreamDone → streaming:done | ✅ 通过 | 覆盖 conversationId/runId |
| 意图 3：StreamError → streaming:error | ✅ 通过 | 覆盖 error/rawError/conversationId/runId |
| 意图 4：ToolCallExecuting → tool:executing | ✅ 通过 | 覆盖 toolName/toolId/input/conversationId/runId |
| 意图 5：ToolCallCompleted（成功）→ tool:completed | ✅ 通过 | 覆盖 id/role/content/toolResult/success/runId |
| 意图 6：ToolCallCompleted（失败）→ isError/success 反转 | ✅ 通过 | 覆盖 durationMs 为 null |
| 意图 7：PermissionAskRequired → permission:ask | ✅ 通过 | 覆盖 suggestions/rememberOptions/defaultDestination/mode/conversationId/runId |
| 意图 8：PermissionAskRequired DontAsk mode | ✅ 通过 | mode 精确为 `dontAsk` |
| 意图 9：AgentIdle Primary scope → "primary" | ✅ 通过 | 覆盖 agentId/conversationId/runId |
| 意图 10：AgentIdle Child scope → "child" | ✅ 通过 | 覆盖 child scope 与 agentId |
| 意图 11：MessagePersisted → message:updated | ✅ 通过 | 覆盖 messageId/id/role/conversationId/runId/createdAt 存在 |
| 意图 12：RunStarted/RunCancelled/StreamStarted/OrphanedPermissionDetected → None | ✅ 通过 | 四类内部事件均不映射 |
| 意图 13：TurnCompleted → turn:completed | ✅ 通过 | 覆盖 outcome/token/cost/permissionDenialCount/conversationId/runId |
| 意图 14：TaskStatusChanged → task:status-changed | ✅ 通过 | 覆盖 taskId/status/subject/activeForm/owner/conversationId/runId |

## 执行记录详情

- 2026-04-24：新增 `review_tauri_event_adapter_test.rs`，按 rules.md 14 条意图逐条覆盖 `map_runtime_event`。
- 2026-04-24：运行 `cd src-tauri && cargo test --test review_tauri_event_adapter_test -- --nocapture`，结果 14 passed。仅出现既有 dead_code warning：`FILE_GEN_TOOLS`、`is_last_tool_file_generation` 未使用。
- 结论：当前 `map_runtime_event` 实现已满足 tauri-event-adapter spec；本次只固化回归测试，未修改生产代码。
