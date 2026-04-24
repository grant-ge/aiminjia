# test-progress.md — 权限管线决策执行记录

## 状态

| 意图 | 状态 | 备注 |
|---|---|---|
| 意图 1：无 scope 工具始终被允许 | ⬜ 待执行 | |
| 意图 2：需要 workspace 但无 workspace 时被 Deny | ⬜ 待执行 | |
| 意图 3：需要 workspace 且有 workspace 时被 Allow | ⬜ 待执行 | |
| 意图 4：python:exec 在没有 workspace 时被 Deny | ⬜ 待执行 | |
| 意图 5：需要 browser 但无 browser 时被 Deny | ⬜ 待执行 | |
| 意图 6：network scope 始终被允许 | ⬜ 待执行 | |
| 意图 7：未知 scope 在 CapabilityPipeline 中被 Deny | ⬜ 待执行 | |
| 意图 8：mcp scope 在 StorePolicyPipeline 中升级为 Ask | ⬜ 待执行 | |
| 意图 9：已持久化 Allow 时直接放行绕过 capability 检查 | ⬜ 待执行 | |
| 意图 10：已持久化 Deny 时直接拒绝 | ⬜ 待执行 | |
| 意图 11：DontAsk 模式下不出现权限确认弹窗 | ⬜ 待执行 | |
| 意图 12：Plan 模式下写操作工具的 Ask 自动变为 Deny | ⬜ 待执行 | |

## 执行记录

<!-- 执行后在这里记录：通过/失败、遇到的坑、结论 -->
