# rules.md — 对话

本 task 测的产品承诺：**用户能向 AI 发起任意复杂度的对话，无论模型生成耗时多长（分钟级长输出），最终都能完整收到结果，不会因网关侧 / 客户端任何中间层超时而被切断并退化成"模型未能生成回复"的兜底文案**。

UI 文案对应：应用启动后的默认对话界面、底部对话输入框、底部「发送」按钮。

---

## 意图-对话-001: 发起长生成对话，对话能完整收尾

**场景**
用户在主对话界面让 AI 跑一个会持续 2-5 分钟生成的复杂任务（例如让模型写一篇深度长文）。期望流式光标一直滚动直到模型自然结束，最终拿到完整内容，而不是在 ~2 分钟时突然终止并显示"模型未能生成回复"。本意图护栏对应 lotus 网关 chatClient timeout（120s→600s）回归——任何中间层把流式响应卡死在分钟级以下都会让这条意图 FAIL。

前提：当前登录账号的 tenant 默认 chat 模型是 sonnet 4-5 / 4-6 等长输出系列（AIjia 客户端不直接暴露模型切换；模型由后端路由）。如默认是 deepseek-v3 之类短响应模型，本意图在该环境无效，需要先在租户后台改默认模型。

**操作步骤**
1. 应用探活：`tauri-pilot aijia health-check`
2. 推断当前 scope：从 `tauri-pilot aijia where --json` 取，记为 `$SCOPE`
3. 记录当前时间 `T0`
4. 记录现有所有对话 ID（`ls ~/.renlijia/users/$SCOPE/conversations/`），记为集合 `$S_BEFORE`
5. 点击底部对话输入框
6. 输入以下 Prompt（一次性粘贴，不要分批）：
   ```
   请你作为主 AI 助手亲自直接在这条 assistant message 里输出下面文章的完整正文。

   🚫 严格禁止：
   - 调用 Agent 工具 / 委派任务给任何子 agent
   - 调用任何其它工具（Bash、Read、Write、Skill、TaskCreate、Grep、Glob 等）
   - 把生成任务拆轮转交给后续 turn

   ✅ 必须做：
   - 在你这一条 assistant message 的 content.text 里直接写出所有文章内容
   - 一次写完，不要分多轮

   文章要求：
   主题：分布式系统中的一致性与共识，全文 8000 字以上。

   1. 完整解释 CAP 定理与反例
   2. Paxos 协议原理 + 完整伪代码
   3. Raft 协议原理、leader 选举、日志复制 + 完整伪代码
   4. 实践案例：etcd、TiKV、Zookeeper 的实现差异
   5. 分布式事务：2PC / 3PC / TCC / Saga，逐一对比
   6. 每节至少给出 200 行可运行的 Go 代码示例
   7. 文末汇总对比表格

   不要省略任何细节，每节至少 1500 字。再次强调：必须由你本人直接写，不许调用任何工具。
   ```
7. 点击「发送」按钮
8. 持续观察对话界面，等待 AI 完整结束（最长允许等待 8 分钟）
9. 等结束后，找到本轮新建的对话 ID（在 `~/.renlijia/users/$SCOPE/conversations/` 下取 `mtime` 最新且不在 `$S_BEFORE` 里的子目录名），记为 `$CONV_ID`

**验收标准**

✅ 应该看到
- 发送后界面立即出现一条 assistant 流式气泡（光标动效）
- 等待期间气泡内文本持续增加（每 30 秒能观察到字数变多）
- 模型自然结束（流式光标消失），结束时刻在 `T0 + 2 分钟` 到 `T0 + 8 分钟` 之间
- 文件 `~/.renlijia/users/$SCOPE/conversations/$CONV_ID/messages.jsonl` 存在
- 该文件**恰好 2 条**记录（user + assistant；任何额外 tool / 多轮 assistant 都说明委派/分轮跑偏）
- 该文件末条 JSON 记录 `role == "assistant"`
- 该末条记录 `content.text length >= 4000`
- 该末条记录 `content.text` 包含字面值 `CAP`
- 该末条记录 `content.text` 包含字面值 `Raft`
- 该末条记录 `content.text` 包含字面值 `Paxos`
- 该末条记录 `toolCalls` 字段不存在或为空数组（主 LLM 自己处理、没委派给任何工具）

❌ 不应该看到
- 该末条记录 `content.text` 包含字面值 `模型未能生成回复`
- 该末条记录 `content.text` 包含字面值 `请尝试换一种方式提问`
- `messages.jsonl` 中**任何一条**记录 `role == "tool"`（出现 tool 记录 = 主 LLM 委派给某个工具，本意图要求"自己处理"）
- `messages.jsonl` 中**任何一条** assistant 记录的 `toolCalls.length >= 1`（同上，委派信号）
- 流式气泡在 `T0 + 1 分 50 秒` 到 `T0 + 2 分 10 秒` 之间突然停止（典型 120s timeout 切断特征）
- `messages.jsonl` 中出现两条相邻 `role == "assistant"` 记录，且其中一条 `content.text length < 200`（半截流被切后又起新轮兜底）
- 等待 8 分钟流式气泡仍在滚动（说明根本没结束，不在本意图覆盖范围）
