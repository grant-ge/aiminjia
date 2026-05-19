# test-progress.md — persistence-crash-recovery

---

## cargo test（集成测试）

| 意图 | 状态 | 备注 |
|------|------|------|
| 1.1 崩溃后历史完整 + interrupted_turns 哨兵 | 待执行 | |
| 1.2 流式中途崩溃，已写入行可独立解析 | 待执行 | |
| 1.3 turn_stages 损坏文件只 log 不 panic | 待执行 | |
| 1.4 正常 turn 完成后无哨兵 | 待执行 | |
| 2.1 write_atomic .tmp 孤儿后下次写入正常 | 待执行 | |
| 2.2 _current .tmp 回退恢复分片元数据 | 待执行 | |
| 3.1 registry 重建后 is_session_busy 为 false | 待执行 | |
| 3.2 cancelled run 后同 session 可重新 reserve | 待执行 | |
| 4.1 5 条消息串行写入全部可读无混叠 | 待执行 | |
| 4.2 append_jsonl 追加不破坏已有行 | 待执行 | |
| 4.3 channel 满时 try_send 返回 Full 错误 | 待执行 | |
| 5.1 messages.jsonl 截断后不 panic 返回部分消息 | 待执行 | |
| 5.2 _current 不存在时 fallback 读 shard | 待执行 | |
| 5.3 turn_stages 目录不存在时 sweep 返回零结果 | 待执行 | |
| 5.4 v2 为空时 fallback 读 v1 shard | 待执行 | 已有 cargo test 覆盖（`get_messages_v2_falls_back_to_legacy_shards_when_single_file_is_empty`） |

---

## agent 跑（端到端产品验收）

启动真实应用进程，操作真实存储文件，不 mock 存储层。

| 意图 | 状态 | 备注 |
|------|------|------|
| 1. kill -9 后 messages.jsonl 有完整 2 行 | 待执行 | |
| 2. 正常完成后 kill -9，turn_stages 无孤儿文件 | 待执行 | |
| 3. 流式中途 kill -9，interrupted_turns 哨兵字段完整 | 待执行 | |
| 4. 重启后前端出现中断 banner | 待执行 | |
| 5. 点击「关闭」后哨兵文件被删除 | 待执行 | |
| 6. 关闭 banner 后可正常发消息 | 待执行 | |
| 7. chmod 555 后发消息报错、进程不退出 | 待执行 | |
| 8. 恢复权限后写入正常，历史行合法 JSON | 待执行 | |
| 9. 流式中途 kill -9 后重启，发送按钮可用 | 待执行 | |
| 10. 重启后可对中断对话发新消息 | 待执行 | |
| 11. 快速连续 5 条消息，每行合法 JSON 无交织 | 待执行 | |
| 12. 截断 messages.jsonl 后重启，其他对话正常 | 待执行 | 执行前备份文件 |
| 13. turn_stages 有损坏文件，重启不崩溃，合法孤儿被 sweep | 待执行 | |
