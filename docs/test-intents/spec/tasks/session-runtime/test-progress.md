# test-progress.md — session-runtime 主链路事件序列

## 状态

- rules.md 已创建
- 暂未执行测试代码实现

## 执行记录

| 意图 | 状态 | 备注 |
|---|---|---|
| 意图 1：正常 turn 完成后 EventBus 中包含完整的事件序列且顺序正确 | 未实现 |  |
| 意图 2：同一 turn 内所有事件携带相同的 run_id | 未实现 |  |
| 意图 3：LLM 调用工具后下一轮收到 tool_result，turn 继续推进直至完成 | 未实现 |  |
| 意图 4：CancellationToken 触发后 turn 正常退出，不挂起 | 未实现 |  |
| 意图 5：达到最大迭代次数时 turn 以 MaxIterationsReached 结束 | 未实现 |  |
