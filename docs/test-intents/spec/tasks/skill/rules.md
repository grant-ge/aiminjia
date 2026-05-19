# rules.md — 技能（Skill） 意图测试规格

## 测试范围

覆盖无状态 Skill 系统的端到端行为：从本地 `~/.renlijia/skills/` 与 `~/.renlijia/users/{scope}/skills/` 目录下 SKILL.md 的加载与 frontmatter 解析、技能目录在对话 turn 中的 prompt 注入（catalog_prompt）、对话中 LLM 通过 `load_skill` 工具按需加载 SKILL.md body，到技能草稿（skill draft）的编辑、校验、发布上线（覆盖原 skill 目录）流程。关注 `plugin/skill/`、`commands/skill_management.rs`、`commands/skill_draft.rs`、前端 `src/features/skill-center/` 的一致性。

## 待覆盖的主要场景

- 场景 1：`~/.renlijia/skills/foo/SKILL.md` 存在时，新对话 turn 的 system prompt 中包含该技能的 catalog 条目（名称 + description），且技能中心页能看到该技能
- 场景 2：LLM 在对话中调用 `load_skill` 工具，工具返回该 SKILL.md 的 body 文本，AI 按 body 中的指令执行
- 场景 3：SKILL.md frontmatter 字段缺失或非法（如 `name:` 为空）时 loader 跳过该 skill，不影响其他 skill 加载
- 场景 4：用户在技能中心创建草稿、编辑 SKILL.md 后保存，草稿落盘到 `~/.renlijia/users/{scope}/skill-drafts/`，正式技能目录不受影响
- 场景 5：草稿发布（publish）后 `~/.renlijia/skills/{id}/SKILL.md` 出现并与草稿内容一致，技能中心可见

---

## 意图 1：放置合法 SKILL.md 后，技能在应用中可见

**场景**
用户把一个新的 skill 目录放进 `~/.renlijia/skills/`，应用应该自动识别这个技能：技能中心页面能看到它，新对话也能看到它。

**前提**
- 应用已启动并已登录有效账号。
- 当前账号的 scope 已经初始化（`~/.renlijia/users/{scope}/skills/` 目录存在或可以被创建）。
- 全局技能目录 `~/.renlijia/skills/` 下**不存在**名为 `demo-skill` 的子目录。

**操作**
1. 关闭应用。
2. 在文件系统中创建目录 `~/.renlijia/skills/demo-skill/`。
3. 在该目录下创建文件 `SKILL.md`，内容为：
   ```
   ---
   name: demo-skill
   description: 演示用：一个最小可加载技能
   ---
   # demo-skill

   本技能用于意图测试。被加载后请在回复开头加上「[demo-skill]」前缀。
   ```
4. 重新启动应用并进入主界面。
5. 打开「技能中心」页面，浏览技能列表。

**验收标准**
- `~/.renlijia/skills/demo-skill/SKILL.md` 存在，文件首行为 `---`。
- 技能中心列表中出现一张技能卡片，卡片标题为 `demo-skill`，描述为 `演示用：一个最小可加载技能`，分类标签显示来源为「全局」（对应后端 `source = "global"`）。
- 应用日志中**不包含** `Failed to parse skill demo-skill` 字样。
- 在主界面发起一个新的空对话，发送任意一句话后，从 `~/.renlijia/conversations/{conv_id}/messages.*.jsonl` 抽取本轮的 system prompt 段（或在开发者面板的「查看 system prompt」处查看），其中包含字符串 `demo-skill` 与 `演示用：一个最小可加载技能`。

---

## 意图 2：对话中 AI 通过 load_skill 工具加载技能 body 后按指令执行

**场景**
对话中当 AI 判断应该使用某个技能时，它调用 `load_skill` 工具，工具把该技能的 SKILL.md 正文返回给 AI，AI 随后按正文里的指令调整行为。

**前提**
- 意图 1 中的 `demo-skill` 已经被应用识别（技能中心可见，system prompt 含其 catalog 条目）。
- 应用使用一个有效的 LLM API key，主模型已配置完成且可正常对话。
- 新建一个空对话并打开它。

**操作**
1. 在对话输入框输入：`请使用 demo-skill 技能回答：今天天气怎么样？` 然后点击发送。
2. 等待 AI 完成一轮回复（看到「停止」按钮变回「发送」按钮）。

**验收标准**
- 在该对话目录 `~/.renlijia/conversations/{conv_id}/` 下，`messages.*.jsonl` 中出现至少一条 `role` 为 `"assistant"` 的消息，其 `content` 中包含一条 tool_use 记录，`name` 字段值为 `"load_skill"`，且参数中包含 `"demo-skill"`。
- 同一对话的消息文件中紧随其后出现一条 `role` 为 `"tool"`（或对应 tool_result 形态）的消息，其内容文本中包含 `demo-skill` SKILL.md 的正文片段，例如 `本技能用于意图测试` 或 `[demo-skill]` 字样。
- 对话界面最终展示的 AI 回复文本以 `[demo-skill]` 前缀开头。
- 对话 UI 中该轮没有出现红色错误提示，没有「工具调用失败」之类的 toast。

---

## 意图 3：缺必填 frontmatter 字段的 SKILL.md 被跳过，不影响其他技能

**场景**
用户放了两个技能目录，一个合法、一个 SKILL.md frontmatter 缺 `name`（或 `name` 为空）。应用启动后，合法的那个正常出现在技能中心，非法的那个被悄悄跳过，且不会让整个技能列表加载失败。

**前提**
- 应用已退出。
- 全局技能目录 `~/.renlijia/skills/` 存在且当前为空（或不含 `good-skill` / `bad-skill` 两个子目录）。

**操作**
1. 在 `~/.renlijia/skills/` 下新建两个目录 `good-skill/` 与 `bad-skill/`。
2. 写入 `~/.renlijia/skills/good-skill/SKILL.md`：
   ```
   ---
   name: good-skill
   description: 一个能正常加载的技能
   ---
   # good-skill
   正文。
   ```
3. 写入 `~/.renlijia/skills/bad-skill/SKILL.md`（注意 `name:` 留空）：
   ```
   ---
   name:
   description: 缺 name 的非法技能
   ---
   # bad-skill
   正文。
   ```
4. 启动应用，登录并进入主界面。
5. 打开技能中心页面。

**验收标准**
- 技能中心列表中出现 `good-skill`，其描述为 `一个能正常加载的技能`。
- 技能中心列表中**不出现** `bad-skill`（无论是作为可用技能还是作为「加载失败」条目都不出现）。
- 文件系统中 `~/.renlijia/skills/bad-skill/SKILL.md` **仍然存在且内容未被修改**（应用不删除非法技能目录）。
- 应用日志中包含一条 error 级别记录，文本包含 `Failed to parse skill bad-skill`（或 `frontmatter field 'name' is required` 等同义信息），且不包含针对 `good-skill` 的解析错误。
- 在主界面新建对话发送任意一句话后，本轮 system prompt 中包含 `good-skill` 字符串，**不包含** `bad-skill` 字符串。

---

## 意图 4：技能草稿保存到草稿目录，不影响正式技能目录

**场景**
用户在「技能中心 - 新建技能」里通过小程草稿写一个新技能，编辑过程中保存的内容只进草稿区，不会让这个未完成的技能立刻出现在正式技能列表里、也不会污染线上同名技能。

**前提**
- 应用已启动并以账号 A 登录，账号 A 的 scope 已初始化。
- `~/.renlijia/users/{scope}/skill-drafts/` 目录可能存在也可能不存在；其中**不存在** `weather-helper` 草稿。
- `~/.renlijia/skills/` 与 `~/.renlijia/users/{scope}/skills/` 中均**不存在** `weather-helper` 这个技能目录。
- 技能中心已打开。

**操作**
1. 在技能中心点击「新建技能」按钮，进入小程对话/草稿界面。
2. 创建一个新草稿，设定 skill name 为 `weather-helper`，description 为 `给出穿衣建议`。
3. 在草稿编辑器中把 SKILL.md 写为：
   ```
   ---
   name: weather-helper
   description: 给出穿衣建议
   ---
   # weather-helper
   根据输入温度给出穿衣建议。
   ```
4. 点击「保存草稿」按钮（或触发草稿自动保存）。
5. 不点击「发布」。
6. 返回技能中心主列表，刷新页面。

**验收标准**
- 目录 `~/.renlijia/users/{scope}/skill-drafts/weather-helper/`（或以 conversation id 命名的对应草稿目录）存在。
- 该草稿目录下存在 `meta.json` 与 `SKILL.md` 两个文件；`SKILL.md` 内容包含字符串 `name: weather-helper` 与 `根据输入温度给出穿衣建议`。
- `meta.json` 中 `name` 字段值为 `"weather-helper"`，`description` 字段值为 `"给出穿衣建议"`，`installed_to` 字段值为 `null`。
- `~/.renlijia/skills/weather-helper/` **不存在**。
- `~/.renlijia/users/{scope}/skills/weather-helper/` **不存在**。
- 技能中心主列表中**不出现** `weather-helper` 这张技能卡片（草稿区单独有它，但正式技能列表里没有）。

---

## 意图 5：发布草稿后技能正式上线，内容与草稿一致

**场景**
用户在草稿里把一个技能写好后点击「发布」，应用把草稿内容落到正式技能目录，下一次对话和技能中心都能直接用到新技能。

**前提**
- 意图 4 已经完成：账号 A 的草稿区里存在 `weather-helper` 草稿（包含合法 frontmatter 的 SKILL.md）。
- `~/.renlijia/users/{scope}/skills/weather-helper/` **不存在**（这是首次发布）。
- 技能中心 / 小程草稿界面已打开，并定位到 `weather-helper` 草稿。

**操作**
1. 在 `weather-helper` 草稿的详情界面点击「发布」按钮。
2. 如果出现确认弹窗，点击「确认发布」。
3. 等到出现「发布成功」或类似 toast / 状态切换。
4. 切回技能中心主列表，刷新页面。
5. 新建一个空对话并发送任意一句话以触发一次 turn。

**验收标准**
- 目录 `~/.renlijia/users/{scope}/skills/weather-helper/` 存在。
- 该目录下存在 `SKILL.md` 文件，内容**逐字符等于**草稿目录 `~/.renlijia/users/{scope}/skill-drafts/weather-helper/SKILL.md` 的内容（可用 `diff` 命令验证两文件无差异）。
- 草稿目录 `~/.renlijia/users/{scope}/skill-drafts/weather-helper/` 仍然存在（发布不删除草稿），其中 `meta.json` 的 `installed_to` 字段值为 `~/.renlijia/users/{scope}/skills/weather-helper` 的绝对路径字符串。
- 技能中心列表中出现 `weather-helper` 技能卡片，描述为 `给出穿衣建议`，来源标签显示为「本地 / 用户」（对应后端 `source = "user"`）。
- 在新建对话发送任意一句话后，本轮 system prompt 中包含字符串 `weather-helper` 与 `给出穿衣建议`。
- 应用日志中**不包含** `Failed to parse skill weather-helper` 字样。

---

## 意图 6：发布会覆盖同名旧技能，新内容立即生效

**场景**
当用户对一个已经发布的技能继续在草稿里改了内容后再次点击「发布」时，正式技能目录里的 SKILL.md 应该被新内容原子替换，下一次对话立刻读到新版本，不需要重启应用。

**前提**
- 意图 5 已完成：`~/.renlijia/users/{scope}/skills/weather-helper/SKILL.md` 已存在，正文中包含字符串 `根据输入温度给出穿衣建议`。
- `weather-helper` 草稿仍存在于 `~/.renlijia/users/{scope}/skill-drafts/`。
- 应用未重启。

**操作**
1. 打开 `weather-helper` 草稿，把 SKILL.md 正文改为：
   ```
   ---
   name: weather-helper
   description: 给出穿衣建议 v2
   ---
   # weather-helper v2
   先问用户所在城市，再根据气温给出三件以内的穿衣建议。
   ```
2. 保存草稿。
3. 再次点击「发布」并确认。
4. 等待「发布成功」提示。
5. 不重启应用。新建一个空对话并发送任意一句话以触发一次 turn。

**验收标准**
- `~/.renlijia/users/{scope}/skills/weather-helper/SKILL.md` 的内容包含字符串 `给出穿衣建议 v2` 与 `先问用户所在城市`。
- 该文件**不再包含**旧版字符串 `根据输入温度给出穿衣建议`（在去掉前缀 description 行后，旧正文那一行不应再出现）。
- 技能中心列表中 `weather-helper` 卡片描述更新为 `给出穿衣建议 v2`。
- 在不重启应用的前提下，新对话第一轮的 system prompt 中包含 `给出穿衣建议 v2` 字样，**不包含** `根据输入温度给出穿衣建议` 字样。
- 应用日志中**不包含** `Failed to parse skill weather-helper`。
