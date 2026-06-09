# rules.md — 崩溃恢复

本 task 测的产品承诺：**应用突然崩了再开，用户的对话历史、正在写的消息草稿、正在跑的任务都还在，不丢东西**。

L4 端到端验证：通过外部 `kill -9` 模拟 crash，重启应用后核验状态。

⚠️ **本 task 所有意图都需要 kill 应用进程**——agent 跑前要先在对话里和作者确认「现在 kill 不会影响你的未保存工作」，得到肯定回复后再执行。这是 §3.5 命令白/黑名单对崩溃恢复 task 的合法例外。

---

## 意图-崩溃恢复-001: 收完一轮 AI 回复后崩了再开，对话历史还在

**场景**
用户发了一条消息、AI 完整回复完，此时应用突然崩了。用户再启动应用进入这个对话，刚才那一来一回的消息原样还在。

**操作步骤**
1. 应用探活：`tauri-pilot aijia health-check`
2. 推断 scope（形如 `t_{tenantId}__u_{userId}`）：从 `tauri-pilot aijia where --json` 取
3. 新建一个对话：`tauri-pilot aijia new-task`
4. 发送消息「帮我写一首关于春天的五言绝句」：`tauri-pilot aijia type-message` + `aijia send`
5. 等 AI 完整回复完：`tauri-pilot aijia wait-reply --timeout 60`
6. 记录当前对话 ID：`tauri-pilot aijia where --json` 取 `activeConversationId`，记为 `$CONV`
7. **与用户确认可以 kill 应用**（在对话里问一句「现在 kill 应用不会影响你的未保存工作吗？」），收到肯定回复后继续
8. 强制 kill 应用：`pkill -9 -f AIjia`
9. 等 1-2 秒，重启应用：通过 `open -a AIjia`（macOS）或对应平台启动方式
10. 等应用启动完成：`tauri-pilot aijia health-check`
11. 切回刚才的对话：`tauri-pilot aijia switch-session $CONV`
12. 查看对话内消息：`tauri-pilot aijia ui-message --include-tools`

**验收标准**

- 对话列表里依然有这个对话（不会消失）
- 对话标题非空（未必和 kill 前完全一致，但能看出和这次对话相关）
- 进入对话后能看到 2 条消息：
  - 第 1 条 `role == "user"`，文本 `== "帮我写一首关于春天的五言绝句"`
  - 第 2 条 `role == "assistant"`，文本非空、是合法中文
- 文件 `~/.renlijia/users/{scope}/conversations/$CONV/messages.jsonl` 存在
- 该文件每行都是合法 JSON（`cat <file> | jq -e . > /dev/null` 不报错）

- 对话从列表里消失
- 对话进去是空的、看不到刚才发的消息
- `messages.jsonl` 文件不存在或损坏（jq 解析失败）
- 出现「文件 X 已损坏」之类的错误弹窗

---

## 意图-崩溃恢复-002: AI 正在流式吐字时崩了再开，能看到这个对话被标记为「中断」

**场景**
用户刚发了消息，AI 正在逐字流式输出（还没说完），此时应用突然崩了。用户再开应用进入这个对话，系统应该知道「这一轮被中断了」，给用户一个明确提示（不是当作正常完成对话）。

**操作步骤**
1. 应用探活：`tauri-pilot aijia health-check`
2. 推断 scope，记为 `$SCOPE`
3. 新建对话：`tauri-pilot aijia new-task`
4. 发送一条**能触发长回复**的消息「请详细解释量子纠缠的原理，至少用 300 字」：`type-message` + `send`
5. 立即用 `tauri-pilot aijia ui-message --last 1 --role assistant` poll，**确认 AI 已经开始输出但还没说完**（消息内容非空但很短）
6. 记录当前对话 ID 为 `$CONV`
7. **与用户确认可以 kill 应用**，得到肯定回复后继续
8. 强制 kill 应用：`pkill -9 -f AIjia`
9. 等 1-2 秒，重启应用
10. 等应用启动完成：`tauri-pilot aijia health-check`
11. 切回刚才的对话：`tauri-pilot aijia switch-session $CONV`
12. 查看对话内 UI 状态：`tauri-pilot aijia where --json`、`tauri-pilot aijia ui-message --include-tools`

**验收标准**

- 对话列表里有这个对话
- 进入对话后，能看到用户那条消息和 AI 的**半截回复**
- 系统在某处明示这一轮**被中断**——比如 UI 上有「这次对话被中断」之类的提示、或者末尾消息有「中断」标记
- 文件 `~/.renlijia/users/{scope}/conversations/$CONV/messages.jsonl` 存在且每行 jq 解析无错
- 应用启动后没有 hang 在 splash / 黑屏

- 对话从列表里消失
- 进对话后系统当作「正常完成」一轮，没有任何中断提示——用户会误以为 AI 真的就只说了半截就完了
- 半截回复**变成全空**（应该至少保留 kill 前已经流出来的字）
- 应用启动失败（一直卡在加载状态超过 30 秒）

---

## 意图-崩溃恢复-003: 用户在输入框打了一半字时崩了再开，草稿还在

**场景**
用户在输入框里打了一段字、还没点发送，应用突然崩了。再开应用进入这个对话，输入框里应该还能看到刚才那段没发送的草稿。

**操作步骤**
1. 应用探活：`tauri-pilot aijia health-check`
2. 推断 scope
3. 新建对话：`tauri-pilot aijia new-task`
4. 在输入框输入「这是一段没发出去的草稿测试 12345」（**不调 send**）：`tauri-pilot aijia type-message "这是一段没发出去的草稿测试 12345"`
5. 等 2 秒（让本地草稿自动保存机制有时间触发）
6. 记录当前对话 ID 为 `$CONV`
7. **与用户确认可以 kill 应用**，得到肯定回复后继续
8. 强制 kill 应用：`pkill -9 -f AIjia`
9. 等 1-2 秒，重启应用
10. 等应用启动完成：`tauri-pilot aijia health-check`
11. 切回刚才的对话：`tauri-pilot aijia switch-session $CONV`
12. 查看输入框内容：`tauri-pilot aijia where --json` 取 `hasEditor` 状态、必要时用 `tauri-pilot aijia ui-message --include-tools` 看输入区 DOM

**验收标准**

- 对话列表里有这个对话
- 进入对话后输入框内能看到 `"这是一段没发出去的草稿测试 12345"` 这段文字

- 对话从列表里消失
- 输入框是空的、草稿丢了
- 草稿字符串出现损坏（少字 / 乱码 / 错位）

---

## 意图-崩溃恢复-004: 一个对话同时有正在跑的子任务，崩了再开，对话历史 + 任务状态都明确可见

**场景**
用户的对话里触发了一个子任务（例如让员工去做某件事），任务还在跑的过程中应用崩了。再开应用进入这个对话，用户应该能看到「这个任务被中断了」之类的明确状态，不能是无声丢失。

**操作步骤**
1. 应用探活
2. 推断 scope
3. 新建对话
4. 发送一条会触发明显工具调用 / 子任务的消息（例如「帮我搜一下最新的关于 X 的 5 条资讯」这种触发 search 工具的话）：`type-message` + `send`
5. 用 `tauri-pilot aijia ui-message --include-tools --last 1` poll，**确认工具调用已经开始但还没完成**（看到 `tool_call` 状态是 executing 而非 completed）
6. 记录对话 ID 为 `$CONV`
7. **与用户确认可以 kill 应用**，得到肯定回复后继续
8. 强制 kill 应用：`pkill -9 -f AIjia`
9. 等 1-2 秒，重启应用
10. 等应用启动完成
11. 切回刚才的对话：`tauri-pilot aijia switch-session $CONV`
12. 查看对话 UI：`tauri-pilot aijia ui-message --include-tools`

**验收标准**

- 对话存在，对话历史里能看到用户那条消息
- 工具调用块在 UI 上有明确状态：要么是「已中断 / 失败」，要么是「未完成」
- 应用 UI 整体可交互（不会因为有半截任务卡住）

- 对话从列表里消失
- 工具调用块在 UI 上还停留在「执行中」转圈的状态（这说明状态没刷新过）
- 应用启动后整个界面卡死、不响应任何 `aijia` 命令

---

## 意图-崩溃恢复-005: 连续两次 kill，第二次重启依然能恢复，不会因为「已经恢复过一次」就丢数据

**场景**
应用某次崩了重启恢复对话历史成功；然后用户继续用，又崩了。第二次重启应该和第一次一样能恢复。验证恢复机制是幂等的、不是只能跑一次。

**操作步骤**
1. 应用探活
2. 推断 scope
3. 新建对话
4. 发送消息「测试连续两次 kill 的第一条」并等完整回复
5. 记录对话 ID 为 `$CONV`
6. **与用户确认可以 kill 应用（第一次）**，kill: `pkill -9 -f AIjia`
7. 重启，等 health-check 通过
8. 切回对话：`tauri-pilot aijia switch-session $CONV`，发送消息「测试连续两次 kill 的第二条」并等完整回复
9. **与用户再次确认可以 kill 应用（第二次）**，kill: `pkill -9 -f AIjia`
10. 重启，等 health-check 通过
11. 切回对话：`tauri-pilot aijia switch-session $CONV`
12. 查看对话内消息：`tauri-pilot aijia ui-message --include-tools`

**验收标准**

- 对话依然存在
- 对话里能看到 4 条消息（2 条 user + 2 条 assistant）
- 第 1 条 user 文本 `== "测试连续两次 kill 的第一条"`
- 第 3 条 user 文本 `== "测试连续两次 kill 的第二条"`
- 两条 assistant 回复都非空

- 对话消失
- 第二次重启后只剩第二段对话（说明第一次恢复留下的状态被吃掉了）
- 出现「数据库损坏」「文件冲突」之类的错误弹窗
