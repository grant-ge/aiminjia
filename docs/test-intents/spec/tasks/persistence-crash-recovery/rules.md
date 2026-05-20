# rules.md — persistence-crash-recovery 持久化原子性与崩溃恢复测试意图

来源：[LUT-8](mention://issue/8cb3deac-6e6a-478e-96f2-cdaa609f0f53)

验证方式：**agent 跑**（启动真实应用，操作真实 `~/.renlijia/` 目录，不 mock 存储层）

存储路径速查：
```
~/.renlijia/
├── users/{scope}/conversations/{conv_id}/
│   ├── conv.json
│   ├── messages.jsonl         # v2 单文件（新对话）
│   ├── messages.1.jsonl       # v1 分片（老对话）
│   └── _current               # v1 分片指针，格式 "shard:next_seq"
├── turn_stages/{conv_id}.json      # 活跃 turn 阶段快照（turn 正常结束时删除）
└── interrupted_turns/{conv_id}.json # 崩溃哨兵（startup sweep 生成）
```

scope 格式为 `t_{tenantId}__u_{userId}`，可从 `~/.renlijia/users/` 目录名获取。

---

## 意图 1：turn 正常完成后 kill -9，messages.jsonl 包含完整的 user 和 assistant 消息

**场景**
用户发消息并等待 AI 完整回复后进程被 kill -9，重启后该对话的消息文件应包含两条完整记录，无乱码。

**前提**
- lotus-app 已启动并登录，记录当前用户 scope
- 新建对话，发送消息 `"帮我写一首关于春天的五言绝句"`，等待 AI 完整回复
- 记录该对话的 conv_id（`~/.renlijia/users/{scope}/conversations/` 下最新目录名）

**操作**
1. 执行 `kill -9 $(pgrep -f "aijia")` 强制终止进程
2. 重启应用，等待启动完成
3. 读取 `~/.renlijia/users/{scope}/conversations/{conv_id}/messages.jsonl`，对每行执行 `python3 -c "import json,sys; [json.loads(l) for l in sys.stdin if l.strip()]"`

**验收标准**
- `messages.jsonl` 存在
- 文件共 2 行（1 条 user + 1 条 assistant），每行均为合�� JSON，无解析错误
- 第 1 行 `role` 字段值为 `"user"`，`content.text` 字段值为 `"帮我写一首关于春天的五言绝句"`
- 第 2 行 `role` 字段值为 `"assistant"`，`content.text` 字段值不为空且为合法 UTF-8 中文

---

## 意图 2：turn 正常完成后 kill -9，turn_stages 目录下无该对话的孤儿文件

**场景**
turn 正常完成时 `mark_turn_complete()` 会删除 `turn_stages/{conv_id}.json`。kill -9 后不应残留孤儿文件。

**前提**
- 同意图 1，在 AI 完整回复后才 kill -9

**操作**
1. 重启后检查 `~/.renlijia/turn_stages/` 目录

**验收标准**
- `~/.renlijia/turn_stages/{conv_id}.json` 不存在

---

## 意图 3：流式输出中途 kill -9，重启后 interrupted_turns 哨兵文件存在且字段完整

**场景**
AI 正在流式输出时进程被 kill -9，`run_recovery_sweep` 应在下次启动时将 `turn_stages/{conv_id}.json` 转换为 `interrupted_turns/{conv_id}.json`。

**前提**
- 新建对话，发送能触发较长回复的消息 `"请详细解释量子纠缠的原理，至少用300字"`
- 在前端可见 AI 正在逐字输出（流式输出尚未完成）时执行 kill -9

**操作**
1. 重启应用，等待启动完成
2. 读取 `~/.renlijia/interrupted_turns/{conv_id}.json`，执行 `python3 -m json.tool` 解析

**验收标准**
- `~/.renlijia/interrupted_turns/{conv_id}.json` 存在
- 文件内容为合法 JSON
- JSON 中 `conversationId` 字段值等于该对话的 conv_id
- JSON 中 `runId` 字段存在且不为空字符串
- JSON 中 `lastStage` 字段存在，其 `kind` 字段值为 `"streaming"` 或 `"waitingLlm"` 或 `"completing"` 之一（取决于 kill 时刻）
- `~/.renlijia/turn_stages/{conv_id}.json` 不存在（sweep 已将其删除）

---

## 意图 4：流式输出中途 kill -9，重启后前端对话顶部出现中断 banner

**场景**
`interrupted_turns/{conv_id}.json` 存在时，前端打开该对话应展示「上次对话未完成」banner，含「重试」「关闭」两个按钮。

**前提**
- 意图 3 已执行，`interrupted_turns/{conv_id}.json` 存在

**操作**
1. 重启应用，打开该对话

**验收标准**
- 对话顶部出现 banner，包含文字（中文描述中断状态）
- banner 中有「重试」按钮
- banner 中有「关闭」按钮

---

## 意图 5：点击 banner「关闭」后 interrupted_turns 哨兵文件被删除

**场景**
`dismissInterruptedTurn` Tauri 命令被调用后，`interrupted_turns/{conv_id}.json` 应被删除。

**前提**
- 意图 4 已执行，banner 可见

**操作**
1. 点击 banner 上的「关闭」按钮
2. 检查 `~/.renlijia/interrupted_turns/{conv_id}.json`

**验收标准**
- `~/.renlijia/interrupted_turns/{conv_id}.json` 不存在（文件被删除）
- banner 从 UI 消失

---

## 意图 6：点击 banner「关闭」后对话可正常发送新消息

**场景**
关闭 banner 后对话应恢复正常可用状态，可以发送新消息。

**前提**
- 意图 5 已执行，banner 已关闭

**操作**
1. 在该对话发送消息 `"你好"`
2. 等待回复完成
3. 读取 `messages.jsonl` 末尾两行

**验收标准**
- 收到 AI 回复（无错误提示）
- `messages.jsonl` 末尾新增 2 行，分别为 `role == "user"` 和 `role == "assistant"`
- 新增的 user 消息 `content.text == "你好"`

---

## 意图 7：存储目录改为只读后发消息，UI 出现错误提示，进程不退出

**场景**
`chmod 555` 锁定对话存储目录后，新消息无法写入，应出现可理解的错误提示而非静默失败或崩溃。

**前提**
- lotus-app 已启动，有一个已有历史消息的对话
- 执行 `chmod 555 ~/.renlijia/users/{scope}/conversations/`

**操作**
1. 在该对话发送消息 `"测试权限错误"`
2. 执行 `chmod 755 ~/.renlijia/users/{scope}/conversations/`（测试完毕立刻恢复）

**验收标准**
- 步骤 1 触发后 UI 出现错误提示（toast 或提示文字，内容包含「失败」「错误」或等价词）
- 应用进程仍在运行（`pgrep -f "aijia"` 有返回）
- `messages.jsonl` 的 mtime 未更新（新消息未被写入文件）

---

## 意图 8：恢复权限后发消息正常写入，历史消息行均为合法 JSON

**场景**
权限恢复后应用应能正常写入，之前的历史消息不因 chmod 操作而损坏。

**前提**
- 意图 7 已执行，权限已恢复为 755
- 记录恢复权限前 `messages.jsonl` 的行数为 N

**操作**
1. 在该对话发送消息 `"权限已恢复"`，等待回复
2. 对 `messages.jsonl` 每行执行 `python3 -c "import json,sys; [json.loads(l) for l in sys.stdin if l.strip()]"`

**验收标准**
- 收到 AI 回复，无错误提示
- `messages.jsonl` 行数为 N + 2（新增 user + assistant 各一行）
- 所有行均为合法 JSON（无解析错误）

---

## 意图 9：流式输出中途 kill -9 后重启，发送按钮可用、无常驻 loading spinner

**场景**
`RuntimeRunRegistry` 是纯内存结构，重启后天然为空。中断的 run 不应使前端发送按钮常驻 loading 状态。

**前提**
- 新建对话，发送消息 `"请逐步用10步分析 1+1=2 的逻辑"`
- 在 AI 开始回复但尚未完成时 kill -9

**操作**
1. 重启应用，打开该对话

**验收标准**
- 发送按钮可点击（无常驻 loading spinner）
- 无「当前对话正在处理中，请稍候」类阻塞提示

---

## 意图 10：重启后对之前中断的对话可正常发送新消息并收到回复

**场景**
`RuntimeRunRegistry` 清空后，同一对话应可立即发起新 run。

**前提**
- 意图 9 已执行，应用已重启，发送按钮可用

**操作**
1. 在该对话发送消息 `"你好"`，等待回复

**验收标准**
- 收到 AI 回复（无错误提示、无超时）
- 回复出现在对话历史中，`content.text` 不为空

---

## 意图 11：快速连续发送 5 条消息，messages.jsonl 中每行均为合法 JSON 且无内容交织

**场景**
`MessageWriteQueue` channel 容量 128，快速发 5 条不会触发 Full 错误，每条消息独立落盘。

**前提**
- 新建对话，记录 conv_id

**操作**
1. 快速连续发送 5 条消息（每条发出后不等回复）：`"第一条"`、`"第二条"`、`"第三条"`、`"第四条"`、`"第五条"`
2. 等待所有回复完成
3. 对 `messages.jsonl` 逐行执行 `python3 -c "import json,sys; lines=[l.strip() for l in sys.stdin if l.strip()]; [json.loads(l) for l in lines]; print(len(lines))"`

**验收标准**
- 无错误提示
- 步骤 3 输出行数 ≥ 10（5 条 user + 5 条 assistant）
- 所有行均为合法 JSON（无解析错误）
- 每行只包含一个 `id` 字段（无内容交织行）

---

## 意图 12：截断 messages.jsonl 后重启，其他对话不受影响且应用不崩溃

**场景**
单个对话的消息文件损坏不应影响整体应用启动和其他对话的正常使用。

**前提**
- 确保有对话 A（待损坏）和对话 B（正常使用）
- 备份：`cp ~/.renlijia/users/{scope}/conversations/{conv_A_id}/messages.jsonl /tmp/messages_bak.jsonl`
- 执行：`truncate -s $(( $(wc -c < ~/.renlijia/users/{scope}/conversations/{conv_A_id}/messages.jsonl) / 2 )) ~/.renlijia/users/{scope}/conversations/{conv_A_id}/messages.jsonl`

**操作**
1. 重启应用
2. 在对话 B 发送消息 `"你好"`，等待回复
3. 打开对话 A，观察展示内容
4. 恢复：`cp /tmp/messages_bak.jsonl ~/.renlijia/users/{scope}/conversations/{conv_A_id}/messages.jsonl`

**验收标准**
- 步骤 1：应用正常启动，无崩溃弹窗（进程存活）
- 步骤 2：对话 B 正常收到回复
- 步骤 3：对话 A 要么展示部分可读消息，要么展示错误提示；不展示乱码；不导致应用整体不可用

---

## 意图 13：turn_stages 目录下存在格式损坏的 .json 文件，重启应用不崩溃，合法孤儿文件正常被 sweep

**场景**
`run_recovery_sweep` 对单个损坏文件只 log+删除，不中断 sweep 流程，其余合法文件正常处理。

**前提**
- 确保应用已关闭
- 写入损坏文件：`echo "{bad json" > ~/.renlijia/turn_stages/fake-conv-damage.json`
- 写入合法孤儿文件：
  ```
  echo '{"schemaVersion":1,"conversationId":"test-conv-valid","runId":"run-test","stage":{"kind":"completing"},"stageStartedAtMs":1700000000000,"turnStartedAtMs":1700000000000,"lastHeartbeatAtMs":1700000000000}' > ~/.renlijia/turn_stages/test-conv-valid.json
  ```

**操作**
1. 启动应用，等待启动完成
2. 检查 `~/.renlijia/turn_stages/` 目录
3. 检查 `~/.renlijia/interrupted_turns/` 目录

**验收标准**
- 步骤 1：应用正常启动，无崩溃弹窗
- 步骤 2：`fake-conv-damage.json` 不存在（已被 sweep 删除）；`test-conv-valid.json` 不存在（已被 sweep 处理并删除）
- 步骤 3：`test-conv-valid.json` 存在，内容为合法 JSON，`conversationId` 字段值为 `"test-conv-valid"`
