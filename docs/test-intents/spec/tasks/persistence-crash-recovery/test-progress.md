# test-progress.md — persistence-crash-recovery

| 意图 | 验证方式 | 状态 | 备注 |
|------|---------|------|------|
| 1.1 崩溃后历史完整 + interrupted_turns 哨兵 | cargo test | 待执行 | |
| 1.2 流式中途崩溃，已写入行可独立解析 | cargo test | 待执行 | |
| 1.3 turn_stages 损坏文件只 log 不 panic | cargo test | 待执行 | |
| 1.4 正常 turn 完成后无哨兵 | cargo test | 待执行 | |
| 2.1 write_atomic .tmp 孤儿后下次写入正常 | cargo test | 待执行 | |
| 2.2 _current .tmp 回退恢复分片元数据 | cargo test | 待执行 | |
| 3.1 registry 重建后 is_session_busy 为 false | cargo test | 待执行 | |
| 3.2 cancelled run 后同 session 可重新 reserve | cargo test | 待执行 | |
| 4.1 5 条消息串行写入全部可读无混叠 | cargo test | 待执行 | |
| 4.2 append_jsonl 追加不破坏已有行 | cargo test | 待执行 | |
| 4.3 channel 满时 try_send 返回 Full 错误 | cargo test | 待执行 | |
| 5.1 messages.jsonl 截断后不 panic 返回部分消息 | cargo test | 待执行 | |
| 5.2 _current 不存在时 fallback 读 shard | cargo test | 待执行 | |
| 5.3 turn_stages 目录不存在时 sweep 返回零结果 | cargo test | 待执行 | |
| 5.4 v2 为空时 fallback 读 v1 shard | cargo test | 待执行 | 已有 cargo test 用例覆盖（`get_messages_v2_falls_back_to_legacy_shards_when_single_file_is_empty`） |
