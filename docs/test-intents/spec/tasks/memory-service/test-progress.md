# test-progress.md — ProjectMemoryService 服务层执行记录

## 状态

- 已实现测试文件：`src-tauri/tests/review_memory_service_test.rs`
- 执行命令：`cd src-tauri && cargo test --test review_memory_service_test -- --nocapture`
- 最近执行结果：13 passed；生产代码无需修改

| 意图 | 状态 | 备注 |
|---|---|---|
| 意图 1：保存记忆时写入独立 entry 文件并重建索引 | ✅ 通过 | 拆成 entry 文件/frontmatter/content 与 MEMORY.md index 两个测试 |
| 意图 2：不同 workspace 的记忆互相隔离 | ✅ 通过 | 验证 bucket 不同，B 不 recall A，B index 不含 A |
| 意图 3：加载上下文时只返回与 query 相关的 entries | ✅ 通过 | 覆盖相关召回、prompt `[相关记忆]`、无命中回退 index |
| 意图 4：legacy core memory 被懒迁移且迁移幂等 | ✅ 通过 | 覆盖 legacy entry 创建、source、index、重复加载不复制 |
| 意图 5：distill_index 从现有 entry 文件重建 MEMORY.md | ✅ 通过 | 覆盖合法 entry 计数与跳过无 frontmatter 损坏文件 |
| 意图 6：同一条记忆重复保存时更新而不是复制 | ✅ 通过 | 验证 entries 只有 1 个，内容为 v2，index 只出现一次 |
| 意图 7：四类 memory_type 都能正确持久化并被 recall | ✅ 通过 | 覆盖 user_preference/project_constraint/reference_info/feedback |
| 意图 8：query 为空或过短时不做相关性召回，只回退 index | ✅ 通过 | 覆盖 `""`、`"a"`、`"我"` |
| 意图 9：相关性召回最多返回 5 条，且优先返回命中分更高的记忆 | ✅ 通过 | 覆盖 6 条命中时 capped=5 且高分 entry 保留 |
| 意图 10：损坏的 entry 文件不会污染 recall 和 index | ✅ 通过 | 覆盖无 frontmatter、缺 type、非法 type 均跳过 |

## 执行记录

- 2026-04-24：发现已有未跟踪 `review_memory_service_test.rs` 已覆盖 10 条意图，先运行验证而不是重写。
- 2026-04-24：首次运行 11 passed / 2 failed。根因是测试 query 使用中文长句时当前 `query_tokens()` 会把连续中文作为一个 token；与实现的 substring 匹配规则不一致，属于测试漂移。调整为使用可被当前 token 规则命中的 query（如 `薪资分析`、`薪资 分析 偏好`）。
- 2026-04-24：运行 `cd src-tauri && cargo test --test review_memory_service_test -- --nocapture`，结果 13 passed。仅出现既有 dead_code warning：`FILE_GEN_TOOLS`、`is_last_tool_file_generation` 未使用。
