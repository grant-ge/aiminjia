# rules.md — search-tool（搜索工具）意图测试规格

## 测试范围

覆盖 AI 触发搜索工具后的端到端行为：搜索结果作为 tool_result 注入对话、搜索失败时返回错误 tool_result 而不是让 turn 崩、搜索结果在对话历史中持久可见。**搜索现在是云端唯一路径**：登录后走云端搜索（lotus `/v1/search`），失败时回退到无需 key 的 Bing 抓取；本地 Bocha / Tavily API key 配置已下线（产品不再提供本地搜索 key 入口）。不包含具体结果的字段排序或得分计算细节。

## 待覆盖的主要场景

- 场景 1：AI 发起一次搜索调用，工具执行成功后 tool_result 含若干条结果（title / url / snippet 等可读字段）
- 场景 2：搜索后端返回 HTTP 错误 / 超时时，tool_result 中是结构化错误描述，turn 继续推进而不是崩溃
- 场景 3：搜索结果作为 tool_result 写入对话历史，重开对话仍能看到这段工具调用与结果
- 场景 4：搜索 query 为空 / 过长 / 含非法字符时，工具层做参数校验，返回校验错误而不是直发后端
- 场景 5：未登录（无云端）且 Bing 兜底也不可用时，工具返回明确的"搜索不可用"错误而不是抛栈

---

## 意图 1：AI 触发 WebSearch 工具后，搜索结果作为 tool_result 写入对话历史

**场景**
用户问一个需要实时信息的问题，AI 选择调用 WebSearch 工具。工具跑完后结果应当作为一条 tool_result 出现在对话历史里（前端能看到工具调用气泡 + 结果摘要），AI 在下一步回复中能引用结果回答用户。

**前提**
- 应用已启动并登录（搜索走云端 `/v1/search`，无需本地 key）
- 新建一个空对话，记录 conv_id
- 在「设置 → 工具」中确认 `WebSearch` 工具处于启用状态

**操作**
1. 在输入框输入 `"用网络搜索查一下：2025 年诺贝尔物理学奖得主是谁？把来源 URL 也告诉我。"`，点击发送
2. 等待 AI 完整回复结束（流式输出停止 + 工具气泡折叠完成）
3. 打开 `~/.renlijia/users/{scope}/conversations/{conv_id}/messages.jsonl`

**验收标准**
- 对话区出现一个工具调用气泡，名为 `WebSearch` 或显示 `WebSearch` 字样
- 工具气泡展开后可见至少 1 条结果项，每条至少包含 url（含 `http://` 或 `https://`）和一段非空摘要文本
- `messages.jsonl` 中至少有 1 条记录的 JSON 解析后存在 tool_use 标识（含 `tool_name` / `name` 字段等于 `"WebSearch"`），且存在另 1 条记录包含 tool_result（`tool_use_id` / `tool_call_id` 字段与上一条匹配）
- 包含 tool_result 的那一行 JSON 中，`content` 或等价字段为非空字符串，且字符串中至少出现一个 `http` 开头的 URL
- AI 最终 assistant 回复 `content.text` 字段非空，且其中至少出现 1 个 `http` 开头的 URL（说明 AI 真的引用了搜索结果，而不是脑补）

---

## 意图 2：搜索后端均不可用时，AI 收到明确的错误 tool_result 而不是 turn 崩溃

**场景**
用户没有登录云端（云端搜索不可用），且 Bing 兜底也取不到结果。用户依然让 AI 用搜索回答问题，系统应当在工具层返回一条明确的"搜索不可用"错误，让 AI 能据此告知用户「我搜不动」，而不是 turn 异常退出 / 整段崩溃。

**前提**
- 应用已启动但**未登录云端**（处于游客 / 离线状态，或登录后明确清除了 session）→ 云端搜索不可用，仅剩 Bing 兜底
- 新建一个空对话，记录 conv_id

**操作**
1. 在输入框输入 `"请用 WebSearch 工具查一下：明天北京天气。"`，点击发送
2. 等待 AI 完整回复结束

**验收标准**
- 对话区出现一个 `WebSearch` 工具调用气泡，且气泡状态显示为「失败」或带错误标识（不是 spinning 卡死）
- `~/.renlijia/users/{scope}/conversations/{conv_id}/messages.jsonl` 中存在一条 tool_result 记录，其 `content` 字段字符串中包含 `搜索不可用` 或 `搜索引擎暂时无法访问` 或 `请基于已有知识回答` 这三段子串中的至少一段
- 同一 turn 的 assistant 最终 `content.text` 非空（AI 拿到错误后继续生成了用户可读回复，没有 turn 崩溃 / 空回复）
- EventBus 中存在 `TurnCompleted` 事件，且 `outcome` 字段值为 `"Success"`（说明工具失败被工具层吸收，turn 本身完成正常）
- 应用未弹出"应用崩溃 / unexpected error"原生对话框

---

## 意图 3：搜索结果落盘到 messages.jsonl，关闭应用重启后仍能在对话历史中查看

**场景**
用户跑完一段含搜索的对话之后关掉应用，第二天再打开应用、点回这条对话，工具调用气泡和搜索结果应当还能正常展示出来——而不是历史回放时工具结果丢失只剩 AI 回复。

**前提**
- 应用已启动并登录
- 已完成「意图 1」的对话 conv_id：messages.jsonl 中已经存在一对 tool_use / tool_result 行 + 一条 assistant 最终回复

**操作**
1. 完整退出应用（cmd+Q / 关闭窗口并退出后台）
2. 重新启动应用并登录
3. 在对话列表中点击 conv_id 对应的对话
4. 滚动查看历史

**验收标准**
- 该对话在列表中存在，标题或预览中至少一项与「意图 1」中发送的提问内容相关
- 进入对话后，对话历史按顺序展示：用户提问气泡 → `WebSearch` 工具气泡 → assistant 回复气泡
- 点开 `WebSearch` 工具气泡，结果区至少能看到 1 条结果项（与「意图 1」结束时落盘的内容一致），结果项中包含 url（含 `http://` 或 `https://`）
- 重启后 `messages.jsonl` 内容与重启前完全一致：记录条数相同、每一条记录 JSON 内容字节比对一致（可用 `diff` 验证启动前后的文件副本）
- 重启后 AI 回复气泡 `content.text` 字段值与重启前展示内容一致
