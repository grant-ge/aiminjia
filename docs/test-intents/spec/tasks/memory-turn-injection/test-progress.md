# test-progress.md — memory Turn 注入层执行记录

## 状态

- 已实现测试文件：`src-tauri/tests/review_memory_turn_injection_test.rs`
- 执行命令：`cd src-tauri && cargo test --test review_memory_turn_injection_test -- --nocapture`
- 最近执行结果：9 passed；生产代码无需修改

| 意图 | 状态 | 备注 |
|---|---|---|
| 意图 1：每个 turn 开始时，用当前用户消息作为 query 加载 project memory | ✅ 通过 | 覆盖 workspace_path 与 query 精确等于 ChatTurnRequest.content |
| 意图 2：project memory 命中时，被注入 dynamic_context 的 `[项目记忆]` 区块 | ✅ 通过 | 覆盖 header、内容、动态上下文头、project memory 在 env_info 前 |
| 意图 3：project memory 不混入 messages 历史 | ✅ 通过 | dynamic_context 含 memory，messages 不含 `[项目记忆]` 与 memory 内容 |
| 意图 4：project memory 为空时才回退加载 legacy core memory | ✅ 通过 | load_core_memory 调用 1 次，dynamic_context 只含 `[核心记忆]` |
| 意图 5：project memory 非空时不再加载 legacy core memory | ✅ 通过 | load_core_memory 调用 0 次，dynamic_context 只含 `[项目记忆]` |
| 意图 6：多轮工具调用中 project memory 只在 turn 开始加载一次 | ✅ 通过 | 3 次 run_llm_step 共享同一份 project memory，load_project_memory 仅 1 次 |
| 意图 7：load_project_memory 失败时不阻断 turn | ✅ 通过 | turn 成功，run_llm_step 被调用，错误文本不进入 dynamic_context |
| 意图 8：project memory 渲染内容为空时视为空上下文 | ✅ 通过 | 不注入 `[项目记忆]`/`[核心记忆]` 空标题，保留动态上下文头 |
| 意图 9：project memory 与 RENLIJIA.md / env_info 保持独立区块 | ✅ 通过 | project memory/env_info 在 dynamic_context，RENLIJIA.md 在 messages，互不混入 |

## 执行记录

- 2026-04-24：新增 `review_memory_turn_injection_test.rs`，用 mock `RuntimeLlmExecutor` 捕获 `load_project_memory` 参数、`dynamic_context` 与 `messages`。
- 2026-04-24：首次编译失败，原因是测试使用了不存在的 `TurnError::Other`；改为现有 `TurnError::PersistenceError` 表达 memory 加载失败。
- 2026-04-24：运行 `cd src-tauri && cargo test --test review_memory_turn_injection_test -- --nocapture`，结果 9 passed。仅出现既有 dead_code warning：`FILE_GEN_TOOLS`、`is_last_tool_file_generation` 未使用。
