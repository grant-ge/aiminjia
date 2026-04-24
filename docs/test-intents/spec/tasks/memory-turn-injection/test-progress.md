# test-progress.md — memory Turn 注入层执行记录

## 状态

| 意图 | 状态 | 备注 |
|---|---|---|
| 意图 1：每个 turn 用当前用户消息作为 query 加载 project memory | ⬜ 待执行 | |
| 意图 2：project memory 命中时注入 dynamic_context 的 `[项目记忆]` 区块 | ⬜ 待执行 | |
| 意图 3：project memory 不混入 messages 历史 | ⬜ 待执行 | |
| 意图 4：project memory 为空时才回退加载 legacy core memory | ⬜ 待执行 | |
| 意图 5：project memory 非空时不再加载 legacy core memory | ⬜ 待执行 | |
| 意图 6：多轮工具调用中 project memory 只在 turn 开始加载一次 | ⬜ 待执行 | |
| 意图 7：load_project_memory 失败时不阻断 turn | ⬜ 待执行 | |
| 意图 8：project memory 渲染内容为空时视为空上下文 | ⬜ 待执行 | |
| 意图 9：project memory 与 RENLIJIA.md / env_info 保持独立区块 | ⬜ 待执行 | |

## 执行记录

<!-- 执行后在这里记录：通过/失败、遇到的坑、结论 -->
