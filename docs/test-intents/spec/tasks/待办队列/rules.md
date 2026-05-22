# rules.md — pending-queue（待办队列）意图测试规格

## 测试范围

覆盖用户在 AI 还在执行当前 turn 时继续发消息的队列化行为：消息正确进入 pending 队列、当前 turn 结束后队列被自动 drain 成下一轮 turn 的输入、用户取消时队列被清空且不残留。不包含队列消息的具体渲染样式或动画。

## 待覆盖的主要场景

- 场景 1：当前 turn 处于 Running 状态时用户发消息，消息进入 pending 队列而非立刻起新 turn
- 场景 2：当前 turn 完成（AgentIdle）后，pending 队列里的消息被合并/按序 drain 出来，触发下一轮 turn
- 场景 3：pending 队列里多条消息按到达顺序合并，不丢、不乱序
- 场景 4：用户在 Running 状态点取消，pending 队列被清空，cancel 之后再起的 turn 不会读到旧 pending
- 场景 5：应用崩溃 / 重启后，pending 队列里未消费的消息按设计选择"恢复"或"丢弃"（与持久化策略一致）
- 场景 6：pending 队列空时 AgentIdle 不会触发空 turn

---

## 意图 1：AI 仍在 Running 时用户发消息，消息进入 pending 队列而非立刻起新 turn

**场景**
用户提一个比较慢的问题，AI 还在流式输出中，用户又想到一句补充想立刻发出去。系统应当把这条新消息排进 pending 队列、UI 上明确显示「等待中」之类的待发态，而不是直接起一个新 turn 把正在跑的覆盖掉。

**前提**
- 应用已启动并登录
- 新建一个空对话，记录 conv_id 与 session_id
- 当前对话 LLM 配置正常（能正常出回复）
- 选一句能让 AI 至少花 5 秒以上才能答完的问题作为「慢问题」（例如 `"请用中文写一段大约 500 字的产品介绍稿。"`）

**操作**
1. 在输入框输入「慢问题」，点击发送
2. 在 AI 还在流式输出（屏幕上 AI 气泡内文字仍在持续增长）期间，立即在输入框输入 `"补充一句：风格要正式。"` 并点击发送
3. 屏幕截图记录此刻 UI 状态
4. 调用 Tauri 命令 `pending_snapshot_for_session(sessionId)` 读取队列快照（可通过 devtools 控制台 `await window.__TAURI_INTERNALS__.invoke('pending_snapshot_for_session', { sessionId: '<session_id>' })` 触发）

**验收标准**
- 第 2 步发送后，UI 上出现一个独立的「等待中 / Pending」气泡或 chip，文本内容包含 `"补充一句：风格要正式。"`
- 此时 AI 仍在继续输出第 1 步「慢问题」的回复（屏幕上 AI 气泡文字继续增长），未被截断
- `pending_snapshot_for_session` 返回的数组长度为 1，唯一一项的 `text` 字段值为 `"补充一句：风格要正式。"`，`source` 字段值为 `"app"`
- 此刻 `~/.renlijia/users/{scope}/conversations/{conv_id}/messages.jsonl` 记录条数不增加（未把 pending 写入持久化的对话历史）

---

## 意图 2：当前 turn 结束后，pending 中的消息自动 drain 触发下一轮 turn

**场景**
用户在 AI 输出过程中追加了一条消息进队列，AI 把当前 turn 答完后，系统应当自动把队列里的消息当作新一轮用户输入接着发出去，用户不需要再点一次发送。

**前提**
- 应用已启动并登录
- 接着「意图 1」的状态：当前 turn 还在 Running，pending 队列里恰好有一条 `"补充一句：风格要正式。"`，conv_id / session_id 已记录

**操作**
1. 静等当前 turn 自然结束（AI 气泡停止增长，输入框重新可用）
2. 等待 30 秒，让 pending drain 完成
3. 调用 `pending_snapshot_for_session(sessionId)` 读取队列快照
4. 打开 `~/.renlijia/users/{scope}/conversations/{conv_id}/messages.jsonl`

**验收标准**
- 当前 turn 结束后 5 秒内，UI 上原本的「等待中」气泡消失，对话区出现一条新的用户消息 bubble，内容为 `"补充一句：风格要正式。"`
- 紧接着对话区出现新的 assistant 流式输出气泡（说明 drain 触发了新一轮 turn）
- `pending_snapshot_for_session` 返回数组长度为 0
- `messages.jsonl` 共 4 条记录：第 1 条 `role` 为 `"user"`、`content.text` 为「慢问题」；第 2 条 `role` 为 `"assistant"`、`content.text` 不为空；第 3 条 `role` 为 `"user"`、`content.text` 为 `"补充一句：风格要正式。"`；第 4 条 `role` 为 `"assistant"`、`content.text` 不为空
- 第 3 条 user 记录的 timestamp 字段晚于第 2 条 assistant 记录的 timestamp 字段

---

## 意图 3：在 Running 中点取消，pending 队列被清空，后续不会自动发出

**场景**
用户在 AI 输出中追加了几条想说的话进 pending 队列，但又改主意了，直接点了停止。系统应当连同 pending 队列一并清掉，让对话回到干净状态——不应当 turn 取消之后系统又把那几条排队消息自动发出去。

**前提**
- 应用已启动并登录
- 新建一个空对话，记录 conv_id 与 session_id
- AI 正在输出一段较长的回复（例如先发送 `"请写一段 500 字的产品介绍"` 让 AI 流式输出）

**操作**
1. 在 AI 还在流式输出期间，依次输入并发送 `"备注 1"`、`"备注 2"`、`"备注 3"`
2. 调用 `pending_snapshot_for_session(sessionId)` 确认队列里此刻有 3 条
3. 点击当前 AI 气泡上的「停止」按钮（或对话区底部的停止按钮）
4. 等待 5 秒
5. 再次调用 `pending_snapshot_for_session(sessionId)` 读取队列快照
6. 等待 30 秒，确认无新 turn 被触发
7. 打开 `~/.renlijia/users/{scope}/conversations/{conv_id}/messages.jsonl`

**验收标准**
- 第 2 步快照返回数组长度为 3，三项 `text` 字段按顺序为 `"备注 1"`、`"备注 2"`、`"备注 3"`
- 点击「停止」后，UI 上「等待中」相关的 3 个 pending 气泡 / chip 全部消失
- 第 5 步快照返回数组长度为 0
- 第 6 步等待结束时，对话区没有任何新的 user / assistant 消息 bubble 自动出现
- `messages.jsonl` 中不存在 `content.text` 为 `"备注 1"` / `"备注 2"` / `"备注 3"` 的记录
- 此后用户在输入框输入新消息并发送，能正常起新 turn（说明取消 + 清空 pending 没把对话搞坏）
