# rules.md — persistence-crash-recovery 持久化原子性与崩溃恢复测试意图

来源：[LUT-8](mention://issue/8cb3deac-6e6a-478e-96f2-cdaa609f0f53)

**验证方式：agent 跑（端到端产品验收）**

启动真实应用进程，用真实用户操作触发持久化，通过 kill -9 / 文件破坏等手段制造异常，重启后读取真实存储文件验证结果。不 mock 存储层，不 mock LLM（可使用沙盒 API key），agent 直接读 `~/.renlijia/` 目录下的文件作为判定依据。

涉及核心模块：
- `storage/fs_atomic.rs` — tmp+rename 原子写，写失败不破坏已有文件
- `storage/message_write_queue.rs` — 异步写队列（channel 容量 128）
- `storage/file_store/messages.rs` — 分片 JSONL（v1，`_current` 跟踪分片）+ 单文件（v2，`messages.jsonl`）
- `storage/aijia_home.rs` — 存储根目录 `~/.renlijia/`，`turn_stages/` + `interrupted_turns/` 路径
- `runtime/run_registry.rs` — 纯内存 HashMap，重启后自动清空
- `runtime/chat/turn_stage.rs` — `TurnStageEmitter` 每次状态转换原子写 `turn_stages/{conv_id}.json`；`run_recovery_sweep()` 启动时扫描孤儿文件

**存储路径速查：**
```
~/.renlijia/
├── users/{scope}/conversations/{conv_id}/
│   ├── conv.json              # 对话元数据
│   ├── messages.jsonl         # v2 单文件（新对话）
│   ├── messages.1.jsonl       # v1 分片（老对话）
│   └── _current               # v1 分片指针，内容格式 "shard:next_seq"
├── turn_stages/{conv_id}.json      # 活跃 turn 的阶段快照（turn 结束时删除��
└── interrupted_turns/{conv_id}.json # 崩溃哨兵（startup sweep 生成）
```

---

## 模块 1：崩溃后对话历史完整性 + 恢复 Banner

### 意图 1.1：完整响应后 kill -9，重启后消息内容完整、interrupted_turns 哨兵存在、前端显示恢复 banner

**场景**
用户发一条消息，等 AI 完整回复后，进程被 kill -9。重启应用，打开同一对话，应看到历史消息完整，前端顶部出现「上次对话未完成」banner（含「重试」「关闭」按钮）。

**操作**
1. 启动 lotus-app，完成登录，记录当前用户的 scope（格式 `t_{tenantId}__u_{userId}`，可从 `~/.renlijia/users/` 目录名获取）
2. 新建对话，发送消息：`"帮我写一首关于春天的五言绝句"`，等待 AI 完整回复
3. 记录 conv_id（从 `~/.renlijia/users/{scope}/conversations/` 目录中找最新目录）
4. 执行 `kill -9 $(pgrep -f "aijia\|lotus")` 强制终止进程
5. 重新启动 lotus-app，打开步骤 3 记录的对话

**判定（全部满足才算 PASS）**
- `~/.renlijia/users/{scope}/conversations/{conv_id}/messages.jsonl`（或 `messages.1.jsonl`）存在，且包含 2 条记录（user + assistant）
- assistant 消息 content 中包含汉字，无乱码，无 JSON 解析错误
- `~/.renlijia/turn_stages/{conv_id}.json` 不存在（kill 前 turn 已正常完成，无孤儿文件）OR 存在并被 sweep 处理后 `interrupted_turns/{conv_id}.json` 出现
- 前端对话顶部出现 banner，包含「重试」和「关闭」两个按钮（如有 banner 触发条件则验证；如 turn 正常完成后无 banner 则验证发送按钮可用即可）

---

### 意图 1.2：流式输出中途 kill -9，interrupted_turns 哨兵被正确生成，已写入消息无乱码

**场景**
AI 正在流式输出回复时进程被 kill -9。重启后，`run_recovery_sweep` 应将 `turn_stages/{conv_id}.json` 转换为 `interrupted_turns/{conv_id}.json`，前端打开对话时显示中断 banner。

**操作**
1. 发送一条能触发较长回复的消息：`"请详细解释量子纠缠的原理，用500字以上"`
2. 在 AI **开始流式输出但尚未完成时**（观察到前端有内容在逐字出现），执行 `kill -9`
3. 在 kill 前先确认 `turn_stages/` 目录下有对应的 `{conv_id}.json` 文件（可在步骤 2 触发后立即检查）
4. 重启应用，打开该对话

**判定（全部满足才算 PASS）**
- 重启后 `~/.renlijia/turn_stages/{conv_id}.json` 不存在（已被 sweep 移除）
- `~/.renlijia/interrupted_turns/{conv_id}.json` 存在，且可被 `cat | python3 -m json.tool` 解析为合法 JSON
- `interrupted_turns/{conv_id}.json` 中 `conversationId` 字段与步骤 1 的对话 ID 一致
- 前端对话顶部出现「上次对话未完成」banner
- 已写入的部分消息（如有）内容为合法 UTF-8 中文，无乱码字符（用 `cat messages.jsonl | python3 -c "import sys,json; [json.loads(l) for l in sys.stdin if l.strip()]"` 验证每行合法 JSON）

---

### 意图 1.3：点击 banner「关闭」后哨兵文件被删除，对话可正常发消息

**场景**
存在 `interrupted_turns/{conv_id}.json` 哨兵的前提下，用户点击 banner「关闭」按钮，哨兵应被删除，对话恢复正常可用状态。

**前提**：意图 1.2 执行后对话处于中断状态，banner 可见

**操作**
1. 在有 banner 的对话中点击「关闭」按钮
2. 检查文件系统：`ls ~/.renlijia/interrupted_turns/{conv_id}.json`
3. 在该对话发送一条新消息：`"你好"`

**判定**
- 步骤 2：文件不存在（已被 `dismiss_interrupted_turn` 删除）
- banner 从 UI 消失
- 步骤 3 的消息正常发出并收到回复，消息被追加到 `messages.jsonl`

---

### 意图 1.4：点击 banner「重试」后原消息重新发出，历史消息不受影响

**场景**
中断 banner 的「重试」按钮应取最后一条用户消息重新发送，不污染历史记录。

**前提**：意图 1.2 执行后 banner 可见，记录中断前最后一条用户消息内容

**操作**
1. 在有 banner 的对话中点击「重试」
2. 等待新回复完成
3. 检查 `messages.jsonl` 中的消息数量和内容

**判定**
- banner 消失
- 能收到新的 AI 回复
- `messages.jsonl` 中原有消息仍存在（历史不被清除）
- `interrupted_turns/{conv_id}.json` 不存在（dismiss 已清理）

---

## 模块 2：写操作失败时的用户感知与数据保护

### 意图 2.1：存储目录改为只读后发消息，UI 出现错误提示，进程不退出

**场景**
`chmod 555` 锁定存储目录后，新消息无法写入，应用应给出可理解的错误提示而非静默失败或崩溃。

**操作**
1. 记录当前 scope，确保有一个已有历史消息的对话
2. 执行 `chmod 555 ~/.renlijia/users/{scope}/conversations/`
3. 在该对话发送消息：`"测试消息"`
4. 观察 UI 反应
5. 执行 `chmod 755 ~/.renlijia/users/{scope}/conversations/`（恢复权限，避免影响后续测试）

**判定**
- 步骤 3 触发后，UI 出现错误提示（toast/错误文字/红色提示，内容包含「失败」「错误」或等价信息）；不是白屏、不是 panic 弹窗
- 应用进程仍在运行（`pgrep -f "aijia\|lotus"` 有返回）
- 新消息**未**被写入 `messages.jsonl`（文件 mtime 未更新）

---

### 意图 2.2：恢复权限后发消息正常，历史消息完整未损坏

**场景**
权限恢复后，应用应能正常写入新消息，之前的历史消息不因权限操作而损坏。

**前提**：意图 2.1 已完成，权限已恢复为 755

**操作**
1. 在同一对话发送消息：`"权限恢复后的消息"`
2. 等待回复完成
3. 执行 `cat ~/.renlijia/users/{scope}/conversations/{conv_id}/messages.jsonl | python3 -c "import sys,json; msgs=[json.loads(l) for l in sys.stdin if l.strip()]; print(len(msgs), 'messages')"`

**判定**
- 步骤 2 收到正常回复（无报错）
- 步骤 3 输出的消息数量 = 意图 2.1 前的消息数 + 2（新的 user + assistant）
- 每行 JSON 均可解析（无损坏行）

---

## 模块 3：重启后会话不被锁定（run_registry 纯内存自清空）

### 意图 3.1：运行中 kill -9 后重启，发送按钮可用，可立即发新消息

**场景**
`RuntimeRunRegistry` 是纯内存结构，重启后天然为空。进行中的 run 被 kill 后重启，前端不应残留「处理中」状态。

**操作**
1. 发送需要较长时间的消息：`"请逐步分析：1+1=? 请用10步来解释"`，在 AI **开始回复但尚未完成时** kill -9
2. 重启应用，打开该对话
3. 观察发送按钮状态
4. 发送新消息：`"你好"`

**判定**
- 步骤 3：发送按钮可点击（无常驻 loading spinner）
- 步骤 4：消息正常发出并收到回复
- 没有「当前对话正在处理中」的阻塞提示

---

### 意图 3.2：两个对话同时 kill -9 后重启，两个对话均可发消息

**场景**
多个 session 同时中断，重启后 registry 清空，所有对话均不被锁定。

**操作**
1. 打开对话 A，发送长消息（不等完成）
2. 立刻打开对话 B，发送长消息（不等完成）
3. 在两个对话都在流式输出时 kill -9
4. 重启，分别打开对话 A 和对话 B

**判定**
- 对话 A 和对话 B 的发送按钮均可点击
- 分别向两个对话各发一条新消息，均正常收到回复

---

## 模块 4：并发写入时的消息完整性

### 意图 4.1：快速连续发送 5 条消息，全部消息可读，文件每行为合法 JSON

**场景**
`MessageWriteQueue` 的 channel 容量 128，快速发送不应触发 Full 错误。每条消息应独立完整写入。

**操作**
1. 新建一个对话，记录 conv_id
2. 快速连续发送 5 条消息（每条发出后不等回复，直接发下一条）：
   - `"第一条消息"`
   - `"第二条消息"`
   - `"第三条消息"`
   - `"第四条消息"`
   - `"第五条消息"`
3. 等待所有回复完成（约 30 秒）
4. 执行：`cat ~/.renlijia/users/{scope}/conversations/{conv_id}/messages.jsonl | python3 -c "import sys,json; lines=[l for l in sys.stdin if l.strip()]; print(len(lines),'lines'); [json.loads(l) for l in lines]"`

**判定**
- 步骤 3 全部收到回复，UI 无错误提示
- 步骤 4 输出的行数 ≥ 10（5 条 user + 5 条 assistant）
- 所有行均为合法 JSON（python3 无异常）
- `messages.jsonl` 中无内容交织行（每行只有一个 `id` 字段）

---

### 意图 4.2：并发写入中途 kill -9，已完成的消息行可独立解析，未完成行不破坏文件

**场景**
`append_jsonl` 使用换行分隔追加，原子性由文件系统保证单行 write。kill 后已完成的行应仍可解析。

**操作**
1. 连续快速发送 3 条消息后，在 AI 回复流式输出过程中 kill -9
2. 重启后执行：
   ```bash
   python3 -c "
   import json, sys
   ok, bad = 0, 0
   for line in open('~/.renlijia/users/{scope}/conversations/{conv_id}/messages.jsonl'):
       line = line.strip()
       if not line: continue
       try: json.loads(line); ok+=1
       except: bad+=1; print('BAD LINE:', repr(line[:80]))
   print(f'{ok} ok, {bad} bad')
   "
   ```

**判定**
- `bad == 0`（无损坏行）
- `ok >= 1`（至少有一条完整消息）

---

## 模块 5：存储文件损坏后的启动容错

### 意图 5.1：截断 messages.jsonl 后启动，应用不 panic，其他对话正常

**场景**
单个对话的消息文件被截断，不应影响整体应用启动和其他对话的使用。

**操作**
1. 确保有至少 2 个对话（对话 A 有历史消息，对话 B 正常可用）
2. 备份：`cp ~/.renlijia/users/{scope}/conversations/{conv_A_id}/messages.jsonl /tmp/messages.jsonl.bak`
3. 截断：`truncate -s $(( $(wc -c < ~/.renlijia/users/{scope}/conversations/{conv_A_id}/messages.jsonl) / 2 )) ~/.renlijia/users/{scope}/conversations/{conv_A_id}/messages.jsonl`
4. 重启应用
5. 打开对话 B，发送消息：`"你好"`
6. 打开对话 A，观察展示

**判定**
- 步骤 4：应用正常启动，无崩溃弹窗（进程存活）
- 步骤 5：对话 B 正常收到回复
- 步骤 6：对话 A 要么显示部分可读消息，要么显示「加载失败」提示；不显示乱码；不导致整个应用不可用
- 恢复：`cp /tmp/messages.jsonl.bak ~/.renlijia/users/{scope}/conversations/{conv_A_id}/messages.jsonl`

---

### 意图 5.2：损坏 conv.json 后启动，该对话不出现或提示错误，其他对话正常

**场景**
单个对话的元数据文件损坏，应被隔离处理，不影响其他对话和整体运行。

**操作**
1. 确保有至少 2 个对话
2. 备份：`cp ~/.renlijia/users/{scope}/conversations/{conv_A_id}/conv.json /tmp/conv.json.bak`
3. 损坏：`echo "notjson" > ~/.renlijia/users/{scope}/conversations/{conv_A_id}/conv.json`
4. 重启应用，观察对话列表和对话 B 可用性
5. 恢复：`cp /tmp/conv.json.bak ~/.renlijia/users/{scope}/conversations/{conv_A_id}/conv.json`

**判定**
- 应用正常启动，无崩溃弹窗
- 对话 B 仍然出现在列表，可正常使用
- 对话 A 要么不出现在列表，要么点击时显示错误提示；不引发整体崩溃

---

### 意图 5.3：turn_stages 目录下存在损坏的 .json 文件，启动不 panic，正常对话可用

**场景**
`run_recovery_sweep` 对单个损坏文件只 log+删除，不 panic，不阻塞启动。

**操作**
1. 确保应用已关闭
2. 写入损坏文件：`echo "{bad json" > ~/.renlijia/turn_stages/fake-conv-id.json`
3. 同时写入一个合法的 turn_stage 文件（格式参考 `turn_stages/{real_conv_id}.json`，可从运行中的 app 复制）：
   ```bash
   cat > ~/.renlijia/turn_stages/valid-conv-123.json << 'EOF'
   {"schemaVersion":1,"conversationId":"valid-conv-123","runId":"run-test","stage":{"kind":"completing"},"stageStartedAtMs":1700000000000,"turnStartedAtMs":1700000000000,"lastHeartbeatAtMs":1700000000000}
   EOF
   ```
4. 启动应用

**判定**
- 应用正常启动，无崩溃弹窗
- 启动后 `~/.renlijia/turn_stages/fake-conv-id.json` 不存在（被 sweep 删除）
- 启动后 `~/.renlijia/interrupted_turns/valid-conv-123.json` 存在（合法文件被正常处理）
- 现有对话正常可用

---

### 意图 5.4：删除 _current 文件（保留 messages.1.jsonl），历史消息仍可读

**场景**
`read_shard_meta` 在 `_current` 缺失时 fallback 到 `shard=1:next_seq=1`，历史消息仍可通过 `messages.1.jsonl` 读取。仅影响 v1 分片格式的老对话。

**操作**（仅在确认对话使用 v1 分片格式时执行，即目录下有 `messages.1.jsonl` 而无 `messages.jsonl`）
1. 找到一个 v1 格式对话（有 `messages.1.jsonl` 和 `_current`，无 `messages.jsonl`）
2. 备份并删除：`cp ~/.renlijia/users/{scope}/conversations/{conv_id}/_current /tmp/_current.bak && rm ~/.renlijia/users/{scope}/conversations/{conv_id}/_current`
3. 重启应用，打开该对话

**判定**
- 历史消息正常展示（不为空，无乱码）
- 应用不崩溃
- 恢复：`cp /tmp/_current.bak ~/.renlijia/users/{scope}/conversations/{conv_id}/_current`

---

## 执行说明

所有意图的判定证据均来自：
1. **文件系统**：直接 `cat` / `ls` / `python3` 检查 `~/.renlijia/` 目录下的真实文件
2. **前端 UI**：观察 banner、发送按钮状态、错误提示
3. **进程状态**：`pgrep` 确认进程存活

每条意图至少跑两次以确认稳定复现。BLOCKED 条件：环境无法满足前置操作（如无法写入 `turn_stages/`），记录阻塞原因，不计入失败率。
