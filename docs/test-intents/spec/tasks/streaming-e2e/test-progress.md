# test-progress.md — streaming-e2e

| 意图 | 状态 | 备注 |
|------|------|------|
| 1. 正常 turn EventBus 包含 StreamStarted→Delta→Done 且顺序正确 | 待执行 | |
| 2. 正常 turn MessagePersisted 包含完整 assistant 消息 | 待执行 | |
| 3. 正常 turn assistant 消息写入存储文件 | 待执行 | |
| 4. 取消后已输出内容被持久化 | 待执行 | |
| 5. 取消后 StreamDone 不出现，TurnCompleted+AgentIdle 出现 | 待执行 | |
| 6. 同一 turn 所有 streaming 事件携带相同 run_id | 待执行 | |
