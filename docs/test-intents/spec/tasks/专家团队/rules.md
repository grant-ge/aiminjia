# rules.md — 专家团队 意图测试规格

## 测试范围

覆盖专家团队（Team）这一多 agent 协作单元的完整链路：从团队创建、Teammate 注册（markdown loader 加载子 agent 定义）、Lead idle loop 调度，到 Lead 通过 `task_notification` 分发任务给 Teammate、Teammate 完成后回写 envelope、团队级取消传播（用户停止 Lead 时所有 Teammate 同步停止）、以及团队对话历史/transcript 的持久化与隔离。关注 `runtime/agent/team*`、`tools/builtin/teammate_stop.rs`、`lead_idle.rs`、`cancellation_registry.rs` 在多 agent 场景下的正确性。

## 待覆盖的主要场景

- 场景 1：用户创建专家团队并添加 Teammate（markdown 文件落到 team 目录），Lead 启动后 `name_registry` 中可见所有 Teammate
- 场景 2：Lead 接到任务后通过 `task_notification_lead` 给 Teammate 派单，Teammate idle loop 唤醒并独立执行
- 场景 3：Teammate 完成任务后通过 `subagent_result_envelope` 回写结果，Lead 在下一轮 turn 看到 Teammate 的输出
- 场景 4：用户在团队对话中点"停止"，`cancellation_registry` 触发，Lead 和所有正在 Running 的 Teammate 同时收到取消并退出，不出现僵尸子 agent
- 场景 5：Teammate 主动调用 `teammate_stop` 工具自行下线，Lead 的 idle loop 感知到 Teammate 状态变化
- 场景 6：团队对话历史按 team scope 隔离持久化（`team_paths`），跨团队不串消息；`subagent_transcript_store` 保留每个 Teammate 的完整转录
- 场景 7：多 Teammate 并发执行时 `tool_round_concurrency` 控制范围内不互相阻塞，且事件 run_id 各自独立

---

## 意图 1：创建专家团队后，团队配置与 Teammate 列表完整落盘

**场景**
用户在专家团队页面新建一个团队，添加 2 名 Teammate 后保存。系统应该把团队元数据写到磁盘上的 team 目录，并在 UI 团队抽屉里列出 Lead + 2 名 Teammate。重启应用后这条配置不能丢。

**前提**
- 应用已启动，已登录任意账号
- 当前 workspace 下尚未创建任何专家团队
- 已新建一个空对话作为团队挂载点

**操作**
1. 在专家团队页面点击"新建团队"，团队名输入 `alpha-team`
2. 添加 2 名 Teammate：名称分别为 `researcher` 和 `writer`，各自填写 employee_id（任选两个已有员工模板）
3. 点击"保存"按钮，等待保存成功提示
4. 退出应用后重新启动，重新打开同一对话

**验收标准**
- 文件 `~/.renlijia/users/{scope}/conversations/{conv_id}/teams/alpha-team/config.json` 存在
- `config.json` 内容为合法 JSON，反序列化后 `team_name` 字段值为 `"alpha-team"`
- `config.json` 反序列化后 `lead.role` 字段值为 `"lead"`
- `config.json` 反序列化后 `teammates` 数组长度为 `2`
- `config.json` 反序列化后 `teammates[*].role` 字段值均为 `"teammate"`
- `config.json` 反序列化后 `teammates` 中存在 `name` 字段值为 `"researcher"` 的一项，且 `employee_id` 字段值非空字符串
- `config.json` 反序列化后 `teammates` 中存在 `name` 字段值为 `"writer"` 的一项
- `config.json` 反序列化后 `deleted_at` 字段不存在或值为 `null`
- 应用重启后，团队抽屉中能看到 `alpha-team` 团队条目，成员列表展示 1 名 Lead + 2 名 Teammate（顺序与创建时一致）
- 团队抽屉中点击 `researcher` 时显示其 employee 名称与头像（说明 name_registry 已绑定）

---

## 意图 2：Lead 在对话中点名 Teammate，Teammate 启动并把回复回写到团队对话

**场景**
用户在已有 `alpha-team` 的对话里向 Lead 提问，Lead 在回复中决定把子任务派给 `researcher`。系统应该唤醒 `researcher` 的 idle loop，执行后把 Teammate 输出作为新一轮消息追加到团队对话历史里，用户能在同一对话窗口看到 Teammate 的发言。

**前提**
- 应用已启动，配置了有效 API key
- 已存在一个团队 `alpha-team`，包含 Lead 和 1 名 Teammate `researcher`（来自意图 1 的状态）
- 已打开该团队对话，团队对话历史当前为空（0 条消息）

**操作**
1. 在输入框输入"请 researcher 帮我查一下最近 7 天竞品的新闻摘要"，点击发送
2. 等待 Lead 完成本轮回复，并观察 researcher 是否自动产出新消息
3. 等待 researcher 输出结束（消息气泡不再处于流式状态）

**验收标准**
- 文件 `~/.renlijia/users/{scope}/conversations/{conv_id}/teams/alpha-team/team-chat.jsonl` 存在
- `team-chat.jsonl` 至少 3 行，每行均为合法 JSON
- 第 1 行 `role` 字段值为 `"user"`，`content` 字段包含字符串 `"researcher"`
- 存在一行 `from` 或 `agent_name` 字段值为 `"team-lead"`，标志 Lead 的回复
- 存在一行 `from` 或 `agent_name` 字段值为 `"researcher"`，且 `content` 字段非空字符串
- 文件 `~/.renlijia/users/{scope}/conversations/{conv_id}/teams/alpha-team/teammates/<researcher_agent_id>.jsonl` 存在且非空
- 团队对话 UI 中至少出现 3 个消息气泡：用户消息、Lead 回复、researcher 回复（researcher 气泡上展示其名字而不是泛化的 "AI"）
- EventBus 中 `TaskStatusChanged` 事件至少出现 1 次，其中 `agent_id` 字段对应 `researcher` 且 `status` 字段值由 `"running"` 变为 `"idle"`

---

## 意图 3：用户在团队对话中点停止，Lead 与所有运行中的 Teammate 同步终止

**场景**
团队正在多 agent 协同跑任务，用户点击对话窗口的"停止"按钮。系统应当让 Lead 立即停下，所有正在 Running 的 Teammate 也一起停下，不能留下还在偷偷调用 LLM 的孤儿子 agent，也不能留下处于 `"running"` 但实际已无主的运行记录。

**前提**
- 应用已启动，团队 `alpha-team` 中 Lead + 2 名 Teammate（`researcher` 和 `writer`）均存在
- 已发送一条会触发 Lead 同时调度两名 Teammate 的复杂请求（例如"researcher 调研、writer 同时起草大纲"）
- 在 Lead 回复的流式输出过程中观察到两名 Teammate 都已进入运行状态（团队抽屉中两名 Teammate 头像显示运行中标记）

**操作**
1. 在对话底部点击"停止"按钮
2. 等待 3 秒后查看团队抽屉与对话窗口的状态

**验收标准**
- 对话窗口顶部的流式状态指示消失，输入框重新可用（不再显示"AI 正在回复"）
- 团队抽屉中 Lead 的状态指示不再显示运行中
- 团队抽屉中 `researcher` 与 `writer` 的状态指示均不再显示运行中
- EventBus 中至少出现 3 次 `TurnCompleted` 事件，每次的 `outcome` 字段值均为 `"Cancelled"`，分别对应 Lead / researcher / writer
- `~/.renlijia/users/{scope}/conversations/{conv_id}/teams/alpha-team/team-chat.jsonl` 文件存在，且最后一行不是处于 streaming 中的半截内容（最后一行 `content` 字段为完整字符串或带 `cancelled` 标记）
- 取消完成后再发一条新消息 `"在吗"`，能正常收到 Lead 回复，`TurnCompleted` 的 `outcome` 字段值为 `"Success"`（说明运行态干净，可继续使用）

---

## 意图 4：Teammate 工具执行报错时，Lead 收到错误 tool_result，团队对话继续

**场景**
某个 Teammate 执行任务时调用工具失败（例如读取一个不存在的文件）。错误必须包装成 `tool_result` 回到 Lead，让 Lead 知道这步出了问题、可以决定下一步怎么办，整个团队对话不能因为一个 Teammate 的工具报错就崩溃。

**前提**
- 应用已启动，团队 `alpha-team` 中存在 Lead + 1 名 Teammate `researcher`
- `researcher` 的 employee 模板 `tool_whitelist` 包含 `Bash` 或 `ReadFile` 之一
- 已打开团队对话

**操作**
1. 在输入框输入"让 researcher 读一下 /tmp/this-file-does-not-exist-2026.txt 这个文件的内容"，点击发送
2. 等待 Lead 调度 researcher、researcher 执行工具失败、Lead 继续回复
3. 在 researcher 工具失败后，再继续输入一条 `"那换个文件 /etc/hostname 试试"` 并发送

**验收标准**
- `~/.renlijia/users/{scope}/conversations/{conv_id}/teams/alpha-team/teammates/<researcher_agent_id>.jsonl` 中存在一行 `role` 字段值为 `"tool_result"` 且 `is_error` 字段值为 `true`（或 `content` 字段包含 `"No such file"`/`"not found"`/`"不存在"` 之一）
- `team-chat.jsonl` 中 researcher 最终落到 Lead 的回执消息存在，`content` 字段包含错误说明（含字符串 `"失败"`/`"读不到"`/`"error"`/`"not found"` 之一），不是空字符串
- 团队对话 UI 中显示 researcher 的失败说明气泡，但不出现红色"对话已崩溃 / 请重启"全局错误条
- 第二条消息 `"那换个文件 /etc/hostname 试试"` 发送后，researcher 再次被调度并正常回复 `/etc/hostname` 的内容；对应 `TurnCompleted` 的 `outcome` 字段值为 `"Success"`
- EventBus 中不出现 `RuntimePanic` 或 `outcome == "Failed"` 的全局 turn 失败事件

---

## 意图 5：多个 Teammate 并发执行时，各自的对话历史互不污染

**场景**
团队里两个 Teammate 同时接到不同任务（一个查竞品、一个写大纲）并发执行。每个 Teammate 自己的 transcript 文件只能记录自己看到的消息，不能把对方的工具调用、对方的 LLM 输出也写进来。

**前提**
- 应用已启动，团队 `alpha-team` 中存在 Lead + 2 名 Teammate `researcher` 与 `writer`
- 已打开团队对话，团队对话历史为空（0 条消息）
- `researcher` 与 `writer` 是两个不同的 employee 模板，`tool_whitelist` 不重叠

**操作**
1. 在输入框输入 `"researcher 查 SaaS 行业最近一周新闻，同时 writer 起草一份周报大纲"`，点击发送
2. 等待 Lead 同时派发两个任务，等待两个 Teammate 都执行完毕（团队抽屉中两个 Teammate 状态均回到 idle）

**验收标准**
- `~/.renlijia/users/{scope}/conversations/{conv_id}/teams/alpha-team/teammates/` 目录下存在两个 `*.jsonl` 文件（researcher 一个、writer 一个），文件名前缀分别对应两个 Teammate 的 `agent_id`
- 两个 jsonl 文件每行均为合法 JSON
- researcher 的 jsonl 文件中不出现 `tool_name` 字段值为 writer 专属工具的行（例如 writer 用了 `Bash` 而 researcher 没有 `Bash` 白名单，则 researcher 文件里不应出现 `Bash` 工具调用）
- writer 的 jsonl 文件中所有 `agent_id` 字段值（如有）均不等于 researcher 的 agent_id
- 团队 UI 中两位 Teammate 的消息气泡各自挂在对应 Teammate 名下，不出现"researcher 的回复出现在 writer 的展开记录里"
- EventBus 中两个 Teammate 各自产生独立的 `run_id`，两个 `run_id` 字段值不相等
- 团队对话 UI 滚动到底部可见两条 Teammate 回复完整呈现，且 `team-chat.jsonl` 中 `from` 字段值为 `researcher` 与 `writer` 的行各至少 1 行
