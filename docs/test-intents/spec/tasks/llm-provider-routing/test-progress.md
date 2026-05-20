# test-progress.md — llm-provider-routing

| 意图 | 状态 | 备注 |
|------|------|------|
| 1. 薪酬关键词 → TaskType::Analysis | 待执行 | |
| 2. 「分析」单词 → TaskType::General | 待执行 | |
| 3. 代码关键词 → TaskType::CodeGen | 待执行 | |
| 4. 用最后一条 user 消息推断 TaskType | 待执行 | |
| 5. 空消息列表 → TaskType::General | 待执行 | |
| 6. auto_model_routing=false 始终返回 primary_model | 待执行 | |
| 7. Analysis 任务 use_tools 始终为 true | 待执行 | |
| 8. use_cloud=true 返回 provider=lotus | 待执行 | |
| 9. use_cloud+Reasoning → model_type=reasoner | 待执行 | |
| 10. HTTP 429 被识别为 retryable | 待执行 | |
| 11. HTTP 401 不被识别为 retryable | 待执行 | |
| 12. session key revoked → is_auth_revoked=true | 待执行 | |
| 13. 普通 401 → is_auth_revoked=false | 待执行 | |
