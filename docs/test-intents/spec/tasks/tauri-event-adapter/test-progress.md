# test-progress.md — tauri-event-adapter 前后端事件协议映射

## 状态

- rules.md 已创建并经过 Opus review 修订
- 暂未执行测试代码实现

## 执行记录

| 意图 | 状态 | 备注 |
|---|---|---|
| 意图 1：StreamDelta → streaming:delta | 未实现 |  |
| 意图 2：StreamDone → streaming:done | 未实现 |  |
| 意图 3：StreamError → streaming:error | 未实现 | 新增 |
| 意图 4：ToolCallExecuting → tool:executing | 未实现 | 补 input 字段断言 |
| 意图 5：ToolCallCompleted（成功）→ tool:completed | 未实现 | 补 id/role/content/{} 断言 |
| 意图 6：ToolCallCompleted（失败）→ isError/success 反转 | 未实现 | 新增 |
| 意图 7：PermissionAskRequired → permission:ask | 未实现 | 补 conversationId/runId |
| 意图 8：PermissionAskRequired DontAsk mode | 未实现 | 新增 |
| 意图 9：AgentIdle Primary scope → "primary" | 未实现 | 拆分 |
| 意图 10：AgentIdle Child scope → "child" | 未实现 | 拆分 |
| 意图 11：MessagePersisted → message:updated | 未实现 | 补 runId/createdAt |
| 意图 12：RunStarted/RunCancelled/StreamStarted/OrphanedPermissionDetected → None | 未实现 | 补 StreamStarted |
| 意图 13：TurnCompleted → turn:completed | 未实现 | 补 permissionDenialCount |
| 意图 14：TaskStatusChanged → task:status-changed | 未实现 | 新增 |
