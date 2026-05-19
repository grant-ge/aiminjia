# test-progress.md — chat-turn-boundary

| 意图 | 状态 | 备注 |
|------|------|------|
| 1. 达到 max_iterations → outcome=MaxIterationsReached | 待执行 | |
| 2. CancellationToken 触发 → outcome=Cancelled | 待执行 | |
| 3. 正常完成 → outcome=Success | 待执行 | |
| 4. token 超 80% 阈值时触发 compaction | 待执行 | |
| 5. token 未超阈值时不触发 compaction | 待执行 | |
| 6. 工具调用一轮后继续 turn → outcome=Success | 待执行 | |
