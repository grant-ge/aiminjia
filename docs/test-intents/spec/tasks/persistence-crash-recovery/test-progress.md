# test-progress.md — persistence-crash-recovery

验证方式：**agent 跑（端到端产品验收）** — 启动真实应用进程，操作真实存储文件，不 mock 存储层。

| 意图 | 验证方式 | 状态 | 备注 |
|------|---------|------|------|
| 1.1 完整响应后 kill -9，历史完整 + banner | agent 跑 | 待执行 | |
| 1.2 流式中途 kill -9，interrupted_turns 哨兵生成 | agent 跑 | 待执行 | |
| 1.3 点击「关闭」后哨兵删除，对话可用 | agent 跑 | 待执行 | |
| 1.4 点击「重试」后消息重发，历史不损坏 | agent 跑 | 待执行 | |
| 2.1 chmod 555 后发消息，UI 报错不崩溃 | agent 跑 | 待执行 | |
| 2.2 恢复权限后正常写入，历史完整 | agent 跑 | 待执行 | |
| 3.1 运行中 kill -9，重启后发送按钮可用 | agent 跑 | 待执行 | |
| 3.2 两个对话同时 kill -9，重启后均可用 | agent 跑 | 待执行 | |
| 4.1 快速连续 5 条消息，全部可读无混叠 | agent 跑 | 待执行 | |
| 4.2 并发写入中途 kill -9，已完成行可解析 | agent 跑 | 待执行 | |
| 5.1 截断 messages.jsonl，启动不 panic | agent 跑 | 待执行 | 执行前备份文件 |
| 5.2 损坏 conv.json，启动不 panic 其他对话正常 | agent 跑 | 待执行 | 执行前备份文件 |
| 5.3 turn_stages 下有损坏文件，启动不 panic | agent 跑 | 待执行 | |
| 5.4 删除 _current，v1 历史消息仍可读 | agent 跑 | 待执行 | 仅适用于 v1 分片格式对话 |
