# test-progress.md — 权限记住决策与存储执行记录

## 状态

- 已实现测试文件：`src-tauri/tests/review_permission_store_test.rs`
- 执行命令：`cd src-tauri && cargo test --test review_permission_store_test -- --nocapture`
- 最近执行结果：10 passed；生产代码无需修改
- 当前实现优先级：`session > workspace > user`；因此 workspace 与 user 冲突时 workspace 覆盖 user

| 意图 | 状态 | 备注 |
|---|---|---|
| 意图 1：Session 记住后本会话直接 Allow，且不写入磁盘 | ✅ 通过 | 验证 session Allow 生效，workspace/user 文件不含该规则 |
| 意图 2：Workspace 记住后 workspace 级 Allow 规则被记录 | ✅ 通过 | 读取 workspace snapshot，source 为 Workspace |
| 意图 3：User 记住后 user 级 Allow 规则被记录 | ✅ 通过 | 读取 user snapshot，source 为 User |
| 意图 4：记住 Deny 后后续直接 Deny | ✅ 通过 | StorePolicyPipeline 返回 Deny |
| 意图 5：记住规则按 tool_name + scope 精确匹配 | ✅ 通过 | 其他 tool/scope 不被误放行 |
| 意图 6：多 scopes 工具的所有 scopes 都被记录 | ✅ 通过 | `mcp` 与 `custom:data` 均写入 workspace 规则 |
| 意图 7：Ask 默认记住目标为 Session | ✅ 通过 | rememberOptions 包含 Session/Workspace/User |
| 意图 8：Workspace/User 持久化后可跨 PermissionStore 实例读取 | ✅ 通过 | 重建 store 后仍可 Allow |
| 意图 9：同一 tool_name + scope 后写规则覆盖前写规则 | ✅ 通过 | 后写 AlwaysDeny 覆盖 AlwaysAllow |
| 意图 10：Workspace 与 User 冲突规则优先级明确 | ✅ 通过 | 当前为 workspace 覆盖 user，结果 Deny |

## 执行记录

- 2026-04-24：新增 `review_permission_store_test.rs`，按 rules.md 10 条意图覆盖 `persist_permission_decision`、`PermissionStore` 分层存储与 `StorePolicyPipeline` 生效行为。
- 2026-04-24：首次测试因测试代码使用不存在的 `PermissionStore::debug_snapshot()` 编译失败；改为读取文件型 store 持久化 snapshot 来验证 source，不新增生产 API。
- 2026-04-24：运行 `cd src-tauri && cargo test --test review_permission_store_test -- --nocapture`，结果 10 passed。仅出现既有 dead_code warning：`FILE_GEN_TOOLS`、`is_last_tool_file_generation` 未使用。
