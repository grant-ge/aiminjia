# test-progress.md — subagent 生命周期执行记录

## 状态

| 意图 | 状态 | 备注 |
|---|---|---|
| 意图 1：spawn 后状态为 Running，持有独立 ID | ⬜ 待执行 | |
| 意图 2：complete 后状态变为 Completed | ⬜ 待执行 | |
| 意图 3：cancel 后状态变为 Cancelled | ⬜ 待执行 | |
| 意图 4：fail 后状态变为 Failed | ⬜ 待执行 | |
| 意图 5：查询不存在的 run_id 返回 missing | ⬜ 待执行 | |
| 意图 6：background 完成后发出 AgentIdle 事件 | ⬜ 待执行 | |
| 意图 7：转录在完成后可按 transcript_ref 读取 | ⬜ 待执行 | |
| 意图 8：通过 child_run_id 关联读取 transcript_ref | ⬜ 待执行 | |
| 意图 9：resume 后能恢复已有 invocation 的 handle | ⬜ 待执行 | |
| 意图 10：resume 不存在的 agent_id 返回错误 | ⬜ 待执行 | |

## 执行记录

<!-- 执行后在这里记录：通过/失败、遇到的坑、结论 -->
