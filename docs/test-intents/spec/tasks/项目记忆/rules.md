# rules.md — project-memory-tool（记忆工具层 / 产品行为）意图测试规格

## 测试范围

覆盖 AI 调用 `WriteMemory` / `SearchMemory` 工具时面向用户的可观察行为：工具执行后文件实际落盘、检索能返回相关记忆内容、下一轮对话能在动态上下文中看到刚写入的记忆。不包含 ProjectMemoryService 内部 recall 评分 / index 重建细节（那归 `memory-service` 测试集）。

## 待覆盖的主要场景

- 场景 1：AI 使用 `WriteMemory` 工具保存记忆后，`project_memories/{bucket}/entries/` 下出现对应 entry 文件，frontmatter 字段完整
- 场景 2：AI 使用 `SearchMemory` 工具检索，返回与 query 相关的记忆内容，结果作为 tool_result 注入对话
- 场景 3：刚写入的记忆在紧跟的下一轮 turn 的动态上下文消息里出现（`[项目记忆]` 段落注入链路打通），AI 能引用记忆内容回答问题
- 场景 4：同一名称的记忆被重复保存时，旧文件被覆盖，`entries/` 目录下只有一个对应文件，目录条目总数不增加

---

## 意图 1：AI 调用 WriteMemory 后，entry 文件落盘且 frontmatter 字段完整

**场景**
AI 在对话中通过 `WriteMemory` 工具保存一条用户偏好记忆。执行完成后，对应的 `.md` 文件必须出现在磁盘上，并且文件头部的 YAML frontmatter 包含 `type` / `name` / `description` 三个字段，内容与工具调用参数一致。

**前提**
- 应用已启动，配置了有效 API key
- 当前 workspace 路径确定（`~/.renlijia/users/{scope}/project_memories/` 目录不要求预先存在，工具会自动创建）
- 已打开一个空对话（消息历史 0 条）

**操作**
1. 在输入框输入 `"帮我记住：我偏好用箱型图展示薪资分布数据，不用柱状图"`，点击发送
2. 等待 AI 调用 `WriteMemory` 工具并完成回复（对话中出现工具调用气泡和成功提示）

**验收标准**
- `~/.renlijia/users/{scope}/project_memories/{bucket}/entries/` 目录下出现至少 1 个新 `.md` 文件（文件名格式为 `<slug>-<hex>.md`）
- 该 `.md` 文件第 1 行内容为 `---`（frontmatter 开始标记）
- 该 `.md` 文件 frontmatter 中存在 `type:` 行，其值为 `user_preference` / `project_constraint` / `reference_info` / `feedback` 之一
- 该 `.md` 文件 frontmatter 中存在 `name:` 行，其值非空字符串
- 该 `.md` 文件 frontmatter 中存在 `description:` 行，其值非空字符串
- 该 `.md` 文件 frontmatter 之后的正文（`---\n\n` 之后的内容）包含字符串 `"箱型图"` 或 `"boxplot"` 之一
- `~/.renlijia/users/{scope}/project_memories/{bucket}/MEMORY.md` 文件存在（`rebuild_index` 被触发），内容包含该条 entry 的链接行（格式为 `- [name](entries/...)` ）
- 对话窗口中工具调用气泡显示 `WriteMemory` 已成功，tool_result 中 `status` 字段值为 `"saved"`
- `~/.renlijia/users/{scope}/conversations/{conv_id}/messages.jsonl` 中存在一条记录 `role` 字段值为 `"tool"` 或 `"tool_result"`，该记录 JSON 的 `content` 字段包含字符串 `"saved"`

---

## 意图 2：AI 调用 SearchMemory 时，tool_result 包含匹配条目的 name 与 content

**场景**
对话中 AI 调用 `SearchMemory` 检索一个关键词，系统应该把与该词相关的记忆条目以结构化 JSON 返回给 AI 作为 tool_result。返回结果中必须包含命中条目的 `name` 字段和 `content` 字段，`count` 字段值大于 0。

**前提**
- 应用已启动，配置了有效 API key
- `~/.renlijia/users/{scope}/project_memories/{bucket}/entries/` 目录下已存在至少 1 个 `.md` 文件，其 frontmatter `name` 字段包含 `"箱型图"` 或正文包含 `"boxplot"`（来自意图 1 的状态，或手动创建）
- 已打开一个空对话

**操作**
1. 在输入框输入 `"查一下我之前有没有记录关于数据可视化图表偏好的内容"`，点击发送
2. 等待 AI 调用 `SearchMemory` 工具，并完整回复

**验收标准**
- `~/.renlijia/users/{scope}/conversations/{conv_id}/messages.jsonl` 中存在一条记录 `role` 字段值为 `"tool"` 或 `"tool_result"`，该记录 JSON 的 `content` 字段包含字符串 `"SearchMemory"` 或工具调用结果 JSON
- tool_result 反序列化后 `status` 字段值为 `"ok"`
- tool_result 反序列化后 `count` 字段值大于 `0`
- tool_result 反序列化后 `results` 数组第 1 个元素的 `name` 字段值非空字符串
- tool_result 反序列化后 `results` 数组第 1 个元素的 `content` 字段值非空字符串，且包含 `"箱型图"` 或 `"boxplot"` 之一
- AI 最终回复（`role: "assistant"` 的消息）包含字符串 `"箱型图"` 或 `"boxplot"` 之一（说明 AI 从 tool_result 中读到了记忆内容并在回复中引用）
- `TurnCompleted` 事件 `outcome` 字段值为 `"Success"`

---

## 意图 3：新写入的记忆在下一轮对话的动态上下文中出现，AI 能无需工具调用直接引用

**场景**
第 1 轮 AI 通过 `WriteMemory` 保存了一条用户偏好（"用箱型图"）。第 2 轮用户问 AI"我之前告诉过你我偏好用什么图表？"，AI 应该直接说出"箱型图"，而不需要再主动调用 `SearchMemory`。这说明记忆注入链路（`load_context` → `render_for_prompt` → `[项目记忆]` 段落）已打通，下一轮 turn 的动态上下文里有相关内容。

**前提**
- 应用已启动，配置了有效 API key
- 已完成意图 1 的操作：AI 已成功调用 `WriteMemory` 保存"偏好箱型图"的用户偏好，对应 entry 文件已落盘
- 同一对话窗口处于就绪状态

**操作**
1. 在同一对话中继续输入 `"你还记得我偏好用什么图表来展示薪资数据吗？"`，点击发送
2. 等待 AI 完整回复

**验收标准**
- AI 回复（`role: "assistant"` 的消息 `content.text` 字段）包含字符串 `"箱型图"` 或 `"boxplot"` 之一
- 本轮消息对应的 `messages.jsonl` 中**不存在**新的 `tool_name` 字段值为 `"SearchMemory"` 的记录（说明记忆是通过 `[项目记忆]` 段落注入的，不是靠 AI 主动检索）
- 若用调试 dump 查看本轮 turn 发出的 dynamic context 消息，其文本中包含字符串 `"[项目记忆]"` 或 `"箱型图"` 或 `"boxplot"` 之一（可通过查看 `TurnStarted` 事件携带的 context 段落，或直接在 `build_iteration_context` 输出中确认）
- `TurnCompleted` 事件 `outcome` 字段值为 `"Success"`
- 对话消息列表中本轮共 2 条消息（user + assistant），未出现 tool_call / tool_result 气泡

---

## 意图 4：同名记忆重复保存时，文件被覆盖，entries/ 条目数量不增加

**场景**
用户在两轮对话中分别要求 AI 保存名字相同的记忆（比如都叫"可视化偏好"），第二次保存的内容与第一次不同。系统应该用第二次内容覆盖同名文件（因为 `name + description` 的哈希决定文件名，相同 name 会映射到相同文件路径），不能让 `entries/` 目录越来越大，也不能让 AI 以为有两条独立记录。

**前提**
- 应用已启动，配置了有效 API key
- `~/.renlijia/users/{scope}/project_memories/{bucket}/entries/` 目录当前为空（或无任何文件名包含 `"可视化偏好"` 的条目）
- 已打开一个空对话

**操作**
1. 在输入框输入 `"帮我记住一条用户偏好，名称叫【可视化偏好】，内容是：我喜欢用折线图展示趋势"`，点击发送，等待 AI 调用 `WriteMemory` 成功
2. 记录此时 `entries/` 目录下 `.md` 文件数量（记为 N）
3. 再在输入框输入 `"刚才那条偏好更新一下，名称还是【可视化偏好】，但内容改成：我喜欢用面积图展示趋势"`，点击发送，等待 AI 调用 `WriteMemory` 成功
4. 第二次保存完成后查看 `entries/` 目录

**验收标准**
- 第二次 `WriteMemory` 成功后，`entries/` 目录下 `.md` 文件数量仍为 N（不增加为 N+1）
- 第二次保存后该文件的正文（frontmatter 之后）包含字符串 `"面积图"`，不再只含 `"折线图"`
- 第二次保存后该文件的 frontmatter `name:` 行值为 `"可视化偏好"`（与第一次一致）
- `~/.renlijia/users/{scope}/project_memories/{bucket}/MEMORY.md` 文件中关于"可视化偏好"的链接行只有 1 条，不出现重复行
- 调用 `SearchMemory`（query: `"可视化偏好"` 或 `"面积图"`），返回 `count == 1`，`results[0].content` 包含 `"面积图"` 而不是 `"折线图"`（说明旧值已被新值替换）
- 两次 `WriteMemory` 均在 tool_result 中返回 `status: "saved"`，第二次 `path` 字段与第一次完全相同（同一文件路径被覆盖写入）
