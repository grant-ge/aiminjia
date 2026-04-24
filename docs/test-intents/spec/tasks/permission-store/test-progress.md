# test-progress.md — 权限记住决策与存储执行记录

## 状态

| 意图 | 状态 | 备注 |
|---|---|---|
| 意图 1：Session 记住后本会话直接 Allow，且不写入磁盘 | ⬜ 待执行 | |
| 意图 2：Workspace 记住后 workspace 级 Allow 规则被记录 | ⬜ 待执行 | |
| 意图 3：User 记住后 user 级 Allow 规则被记录 | ⬜ 待执行 | |
| 意图 4：记住 Deny 后后续直接 Deny | ⬜ 待执行 | |
| 意图 5：记住规则按 tool_name + scope 精确匹配 | ⬜ 待执行 | |
| 意图 6：多 scopes 工具的所有 scopes 都被记录 | ⬜ 待执行 | |
| 意图 7：Ask 默认记住目标为 Session | ⬜ 待执行 | |
| 意图 8：Workspace/User 持久化后可跨 PermissionStore 实例读取 | ⬜ 待执行 | |
| 意图 9：同一 tool_name + scope 后写规则覆盖前写规则 | ⬜ 待执行 | |
| 意图 10：Workspace 与 User 冲突规则优先级明确 | ⬜ 待执行 | |

## 执行记录

<!-- 执行后在这里记录：通过/失败、遇到的坑、结论 -->
