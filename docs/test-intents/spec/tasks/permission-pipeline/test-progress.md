# test-progress.md — 权限管线决策执行记录

## 状态

- 已实现测试文件：`src-tauri/tests/review_permission_pipeline_test.rs`
- 执行命令：`cd src-tauri && cargo test --test review_permission_pipeline_test -- --nocapture`
- 最近执行结果：12 passed；生产代码无需修改
- 注：意图 11/12 的核心规则先在 `apply_permission_mode` 层固化（Ask → Deny），未扩展到完整 turn 事件/LLM tool_result 断言

| 意图 | 状态 | 备注 |
|---|---|---|
| 意图 1：没有 capability_scope 的工具始终被允许 | ✅ 通过 | 验证无 capability 与有 workspace capability 都 Allow |
| 意图 2：需要 workspace 的工具在没有 workspace 时被 Deny | ✅ 通过 | Deny message 包含 workspace 与 tool id |
| 意图 3：需要 workspace 的工具在有 workspace 时被 Allow | ✅ 通过 | 使用 TempDir workspace capability |
| 意图 4：python:exec scope 在没有 workspace 时被 Deny | ✅ 通过 | Deny message 包含 workspace |
| 意图 5：需要 browser 的工具在没有 browser capability 时被 Deny | ✅ 通过 | Deny message 包含 browser 与 tool id |
| 意图 6：network scope 始终被允许 | ✅ 通过 | 无 capability context 也 Allow |
| 意图 7：未知 scope 在 CapabilityPipeline 中被 Deny（fail-closed） | ✅ 通过 | Deny message 包含 custom scope |
| 意图 8：mcp scope 在 StorePolicyPipeline 中升级为 Ask（而非 Deny） | ✅ 通过 | 覆盖 message/suggestions/rememberOptions/defaultDestination |
| 意图 9：StorePolicyPipeline 中已持久化 Allow 时直接放行，即使没有 capability | ✅ 通过 | 对比 CapabilityPermissionPipeline 同条件会 Deny |
| 意图 10：StorePolicyPipeline 中已持久化 Deny 时直接拒绝 | ✅ 通过 | 返回 Deny，不进入 Ask |
| 意图 11：DontAsk 模式下不出现权限确认弹窗，Ask 自动变为 Deny | ✅ 部分通过 | 已覆盖 `apply_permission_mode` 将 Ask 转 Deny 且消息含 dontAsk/requires permission；完整 turn 事件链待后续如需补强 |
| 意图 12：Plan 模式下写操作工具的 Ask 自动变为 Deny | ✅ 部分通过 | 已覆盖 `apply_permission_mode` 将 Ask 转 Deny 且消息含 plan/read-only；完整 turn 事件链待后续如需补强 |

## 执行记录

- 2026-04-24：新增 `review_permission_pipeline_test.rs`，按 rules.md 覆盖 CapabilityPermissionPipeline、StorePolicyPipeline 与 permission mode transform。
- 2026-04-24：运行 `cd src-tauri && cargo test --test review_permission_pipeline_test -- --nocapture`，结果 12 passed。仅出现既有 dead_code warning：`FILE_GEN_TOOLS`、`is_last_tool_file_generation` 未使用。
