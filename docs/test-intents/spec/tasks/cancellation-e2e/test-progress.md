# test-progress.md — cancellation-e2e

| 意图 | 状态 | 备注 |
|------|------|------|
| 1. parent cancel 传播到 child | 待执行 | |
| 2. child cancel 不传播到 parent | 待执行 | |
| 3. 三层嵌套 parent cancel 传播到 grandchild | 待执行 | |
| 4. 从已取消 parent 创建 child 立即 cancelled | 待执行 | |
| 5. child_token_ignoring_reason 忽略指定 reason | 待执行 | |
| 6. cancel 幂等，多次调用 reason 不变 | 待执行 | |
| 7. registry register 后 get 返回相同 token | 待执行 | |
| 8. registry unregister 后 get 返回 None | 待执行 | |
| 9. cancel_team 取消 team 所有 token 并清除注册 | 待执行 | |
| 10. 不同 team_name 互相隔离 | 待执行 | |
