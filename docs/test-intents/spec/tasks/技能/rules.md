# rules.md — 技能（Skill） 意图测试规格

## 测试范围

覆盖无状态 Skill 系统的端到端行为：从本地 `~/.renlijia/skills/` 与 `~/.renlijia/users/{scope}/skills/` 目录下 SKILL.md 的加载与 frontmatter 解析、技能目录在对话 turn 中的 prompt 注入（catalog_prompt）、对话中 LLM 通过 `Skill` 工具按需加载 SKILL.md body，到技能草稿（skill draft）的编辑、校验、发布上线（覆盖原 skill 目录）流程。关注 `plugin/skill/`、`commands/skill_management.rs`、`commands/skill_draft.rs`、前端 `src/features/skill-center/` 的一致性。

## 待覆盖的主要场景

- 场景 1：`~/.renlijia/skills/foo/SKILL.md` 存在时，新对话 turn 的 system prompt 中包含该技能的 catalog 条目（名称 + description），且技能中心页能看到该技能
- 场景 2：LLM 在对话中调用 `Skill` 工具，工具返回该 SKILL.md 的 body 文本，AI 按 body 中的指令执行
- 场景 3：SKILL.md frontmatter 字段缺失或非法（如 `name:` 为空）时 loader 跳过该 skill，不影响其他 skill 加载
- 场景 4：用户在技能中心创建草稿、编辑 SKILL.md 后保存，草稿落盘到 `~/.renlijia/users/{scope}/skill-drafts/`，正式技能目录不受影响
- 场景 5：草稿发布（publish）后 `~/.renlijia/skills/{id}/SKILL.md` 出现并与草稿内容一致，技能中心可见

---

## 意图 1：在技能中心通过「导入目录」按钮导入新技能后，技能在应用中可见

**场景**
用户准备好一个新 skill 目录（含 `SKILL.md`），通过技能中心右上角「导入技能 → 导入目录」按钮把它导入应用。导入后该 skill 出现在技能中心列表，新对话能在 catalog 中看到它。

**为什么是这条路径**：产品没有"扔目录到 `~/.renlijia/skills/` 后自动看到"的机制（`SkillRegistry` 是 in-memory，不主动 watch 磁盘）。用户在 UI 上唯一能添加新技能的入口是技能中心的导入按钮——它走 `install_custom_skill` IPC，后端会自动 `refresh_skill_registry()`。这才是产品真承诺。

**前提**
- 应用已启动并已登录有效账号。
- 准备一个临时源目录（不在 `~/.renlijia/skills/` 内，例如 `/tmp/aijia-skill-import/demo-skill/`）含 `SKILL.md`。
- `~/.renlijia/users/{scope}/skills/demo-skill/` **不存在**（首次导入场景）。

**操作**
1. 在临时目录 `/tmp/aijia-skill-import/demo-skill/` 创建 `SKILL.md`：
   ```
   ---
   name: demo-skill
   description: 演示用：一个最小可加载技能
   ---
   # demo-skill

   本技能用于意图测试。被加载后请在回复开头加上「[demo-skill]」前缀。
   ```
2. 切到技能中心：`tauri-pilot aijia goto skill-center`
3. 入队 OS dialog mock 路径：`tauri-pilot aijia skill-import-queue --paths /tmp/aijia-skill-import/demo-skill`（CLI 待补，详见 `cli-gap.md`）
4. 点导入按钮 → 选「导入目录」：`tauri-pilot aijia skill-import-pick --variant directory`（CLI 待补）
5. 等技能中心刷新（store auto-reload）。

**验收标准**

✅ 应该看到
- 文件 `~/.renlijia/users/{scope}/skills/demo-skill/SKILL.md` 存在
- 该文件首行为 `---`、内容含 `name: demo-skill` 与 `演示用：一个最小可加载技能`
- 技能中心 DOM 中存在 `[data-aijia-skill-card][data-aijia-skill-id="demo-skill"]` 节点（CLI 待补 `aijia skill-cards --json`，详见 `cli-gap.md`）
- 该卡片节点的 `data-aijia-skill-source` 属性值为 `"user"`（导入路径下沉到 user scope，不是 global）
- 技能中心 UI 上该卡片的可见文本含 `demo-skill` 与 `演示用：一个最小可加载技能`

❌ 不应该看到
- 应用日志中含 `Failed to parse skill demo-skill` 字样
- 技能中心出现重复的 `demo-skill` 卡片（多次 mock 入队 + click 不能造成多份导入）

---

## 意图 2：对话中 AI 通过 Skill 工具加载技能 body 后按指令执行

**场景**
对话中当 AI 判断应该使用某个技能时，它调用 `Skill` 工具，工具把该技能的 SKILL.md 正文返回给 AI，AI 随后按正文里的指令调整行为。

**前提**
- 意图 1 已成功完成：`demo-skill` 已通过技能中心导入按钮路径成功导入，技能中心 DOM 中能看到 `[data-aijia-skill-card][data-aijia-skill-id="demo-skill"]` 节点。
- 应用使用一个有效的 LLM API key，主模型已配置完成且可正常对话。
- 新建一个空对话并打开它。

**操作**
1. 在对话输入框输入：`请使用 demo-skill 技能回答：今天天气怎么样？` 然后点击发送。
2. 等待 AI 完成一轮回复（看到「停止」按钮变回「发送」按钮）。

**验收标准**

✅ 应该看到
- 在该对话目录 `~/.renlijia/users/{scope}/conversations/{conv_id}/` 下，`messages.jsonl` 中出现至少一条 `role` 为 `"assistant"` 的消息，其顶层 `toolCalls[].name` 字段值为 `"Skill"`，且参数中包含 `"demo-skill"`
- 同一对话的消息文件中紧随其后出现一条 `role` 为 `"tool"` 的消息，其 `content` 字段（经 `\t✓\n` 分隔后取记录）的文本中包含 `本技能用于意图测试`
- 对话界面最终展示的 AI 回复文本中**引用了** `demo-skill` SKILL.md body 的内容（包含 `本技能用于意图测试` 子串，或对该 body 描述的功能做出回应）

❌ 不应该看到
- 对话 UI 中该轮出现红色错误提示
- 「工具调用失败」之类的 toast
- AI 回复"我没有找到 demo-skill 技能"（说明 catalog 没注入或 skill 未导入成功）

---

## 意图 3：含非法 frontmatter 的目录导入失败，合法的同批导入仍生效

**场景**
用户尝试用「导入目录」按钮导入一个内含两层子目录（一个合法、一个 frontmatter 非法）的源；产品应当让合法的成功导入、非法的导入失败但**不影响合法的那个**。

**前提**
- 应用已启动并已登录。
- `~/.renlijia/users/{scope}/skills/` 不含 `good-skill` / `bad-skill` 两个子目录。

**操作**
1. 在临时位置创建两个独立源目录：
   - `/tmp/aijia-skill-import/good-skill/SKILL.md`：
     ```
     ---
     name: good-skill
     description: 一个能正常加载的技能
     ---
     # good-skill
     正文。
     ```
   - `/tmp/aijia-skill-import/bad-skill/SKILL.md`（注意 `name:` 留空）：
     ```
     ---
     name:
     description: 缺 name 的非法技能
     ---
     # bad-skill
     正文。
     ```
2. 切到技能中心：`tauri-pilot aijia goto skill-center`
3. 先导入 good-skill：`skill-import-queue --paths /tmp/aijia-skill-import/good-skill` + `skill-import-pick --variant directory`
4. 再导入 bad-skill：`skill-import-queue --paths /tmp/aijia-skill-import/bad-skill` + `skill-import-pick --variant directory`

**验收标准**

✅ 应该看到
- 第 3 步成功（toast 提示导入成功 / 技能中心出现 good-skill 卡片）
- 第 4 步**失败**（toast 提示导入失败 / 错误提示含 "name" 必填校验信息）
- 技能中心 DOM 含 `[data-aijia-skill-card][data-aijia-skill-id="good-skill"]` 节点
- `~/.renlijia/users/{scope}/skills/good-skill/SKILL.md` 存在

❌ 不应该看到
- 技能中心 DOM 出现 `[data-aijia-skill-card][data-aijia-skill-id="bad-skill"]` 节点
- `~/.renlijia/users/{scope}/skills/bad-skill/` 目录被创建（产品在导入校验失败时不应落盘）
- good-skill 同批被错误退回（一个失败影响另一个）

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

---

## 意图 7：用户在对话里提到「钉钉」时，AI 自动加载 dingtalk-workspace 技能

**场景**
用户在对话里用日常语言聊到钉钉相关需求，AI 应当根据 system prompt 中的 skill catalog 自主决定调用 `Skill` 工具加载 `dingtalk-workspace`，**不需要用户显式说「请使用 dingtalk-workspace 技能」**。这是「关键词驱动 + LLM 自主调度」的产品承诺。本意图覆盖 3 种代表性提及方式：① 口语点功能（「用钉钉发消息」）② 念中文产品词（「钉钉日历」）③ 念 CLI 名（「dws 怎么用」）。3 轮各自独立新建对话，任一轮未触发 Skill 工具即整条 FAIL。（**不**覆盖用户念 UI label「玩转钉钉」——那是意图 8 的承诺。）

**操作步骤**
1. 应用探活：`tauri-pilot aijia health-check`
2. 推断 scope；从环境记下 `{scope}`
3. 确认 `dingtalk-workspace` 已就位：检查 `~/.renlijia/skills/dingtalk-workspace/SKILL.md` 存在；如不存在，先用「技能」task 意图 1 的「导入目录」流程把它放进来，**不通过手工 `cp` 落盘**（手工落盘绕过产品入口、不复现 catalog 注入逻辑）
4. **轮 A — 口语点功能**：
   1. 新建空对话：`tauri-pilot aijia new-task`
   2. 输入：`我想用钉钉给同事发条消息提醒他开会，怎么操作？`
   3. 点发送 + 等回复：`tauri-pilot aijia send` + `tauri-pilot aijia wait-reply --timeout 90`
   4. 记下 `conv_a=$(tauri-pilot aijia where --json | jq -r .activeConversationId)`
5. **轮 B — 念中文产品词**：
   1. 新建空对话：`tauri-pilot aijia new-task`
   2. 输入：`帮我看看今天钉钉日历上有什么会`
   3. 点发送 + 等回复：`tauri-pilot aijia send` + `tauri-pilot aijia wait-reply --timeout 90`
   4. 记下 `conv_b=$(tauri-pilot aijia where --json | jq -r .activeConversationId)`
6. **轮 C — 念 CLI 名**：
   1. 新建空对话：`tauri-pilot aijia new-task`
   2. 输入：`dws 怎么用？我想看看群里待办`
   3. 点发送 + 等回复：`tauri-pilot aijia send` + `tauri-pilot aijia wait-reply --timeout 90`
   4. 记下 `conv_c=$(tauri-pilot aijia where --json | jq -r .activeConversationId)`

**验收标准**

✅ 应该看到（3 轮**每一轮都必须独立满足**，任一轮未满足即整条 FAIL）
- 轮 A 的 `~/.renlijia/users/{scope}/conversations/{conv_a}/messages.jsonl` 中存在一条 `role == "assistant"` 的记录，其顶层 `toolCalls` 数组中存在一项 `name == "Skill"` 且参数 JSON 中 `skill_id == "dingtalk-workspace"`
- 轮 A 紧随其后存在一条 `role == "tool"` 的记录，其 `toolCallId` 等于上一条对应 `toolCalls[N].id`，且 `content.text` 中包含字符串 `dingtalk-workspace-cli`
- 轮 B 的 `~/.renlijia/users/{scope}/conversations/{conv_b}/messages.jsonl` 中存在一条 `role == "assistant"` 的记录，其 `toolCalls` 中存在一项 `name == "Skill"` 且参数 JSON 中 `skill_id == "dingtalk-workspace"`
- 轮 B 紧随其后存在一条 `role == "tool"` 的记录，`content.text` 中包含字符串 `dingtalk-workspace-cli`
- 轮 C 的 `~/.renlijia/users/{scope}/conversations/{conv_c}/messages.jsonl` 中存在一条 `role == "assistant"` 的记录，其 `toolCalls` 中存在一项 `name == "Skill"` 且参数 JSON 中 `skill_id == "dingtalk-workspace"`
- 轮 C 紧随其后存在一条 `role == "tool"` 的记录，`content.text` 中包含字符串 `dingtalk-workspace-cli`

❌ 不应该看到
- 任何一轮中出现红色错误提示或「工具调用失败」类 toast
- 任何一轮 `toolCalls[].name == "Skill"` 的参数中 `skill_id` 为 `钉钉`、`dingtalk`、`dws`、`钉钉日历`（这些都不是 skill 真 id，传错会 not found）
- 任何一轮 AI 回复文本中出现「我不知道怎么操作钉钉」「我没有相关技能」等否认有能力的措辞

---

## 意图 8：用户提到 skill 的 label 文案「玩转钉钉」时，AI 也能识别并加载 dingtalk-workspace

**场景**
`dingtalk-workspace` 的 SKILL.md frontmatter 里 `metadata.label` 是「玩转钉钂」（用户在 UI 上看到的中文名），但 skill 真实 id 是英文 `dingtalk-workspace`。用户在对话里用 UI 上看到的中文文案「玩转钉钉」表达意图时，AI 应能映射到正确的 skill id 并加载，**不应**因为字面不匹配就找不到。

**操作步骤**
1. 应用探活：`tauri-pilot aijia health-check`
2. 推断 scope；确认 `~/.renlijia/skills/dingtalk-workspace/SKILL.md` 存在且其 frontmatter 中 `metadata.label` 字段值为 `玩转钉钉`（若不是，本意图前提不成立，标 FAIL 主因 = rules/CLI 问题）
3. 新建空对话：`tauri-pilot aijia new-task`
4. 在对话输入框输入：`帮我玩转钉钉，从查日历开始`
5. 点发送 + 等回复完成：`tauri-pilot aijia send` + `tauri-pilot aijia wait-reply --timeout 90`
6. 推断当前对话 id：`conv_id=$(tauri-pilot aijia where --json | jq -r .activeConversationId)`

**验收标准**

✅ 应该看到
- `~/.renlijia/users/{scope}/conversations/{conv_id}/messages.jsonl` 中存在一条 `role == "assistant"` 的记录，其 `toolCalls` 中存在一项 `name == "Skill"`
- 该 `Skill` 调用参数 JSON 中 `skill_id == "dingtalk-workspace"`
- 该 `Skill` 调用对应的 `role == "tool"` 记录的 `content.text` 中包含字符串 `dingtalk-workspace-cli`

❌ 不应该看到
- `toolCalls[].name == "Skill"` 的参数中 `skill_id == "玩转钉钉"`（按 label 字面找会 not found）
- 该 turn 出现「skill not found」类错误 tool result
- AI 回复中出现「没有名叫玩转钉钉的技能」「找不到对应技能」等措辞

---

## 意图 9：用户 scope 下 dingtalk-workspace 缺失时，AI 不应幻觉加载、应在工具层报错

**场景**
当用户 scope 下 `dingtalk-workspace` skill 不存在（被卸载、未导入、loader 跳过）时，用户提到「钉钉」相关需求，AI 应当能感知 skill catalog 中没有这条记录，不在 `Skill` 工具里硬传 `dingtalk-workspace`；即便 LLM 自信传了，工具应返回明确的 not found 错误而非空字符串或假数据。

**操作步骤**
1. 应用探活：`tauri-pilot aijia health-check`
2. 推断 scope
3. 移除 user scope 下的 dingtalk 技能（若存在）：`rm -rf ~/.renlijia/users/{scope}/skills/dingtalk-workspace/`
4. 移除 global skill 副本（若存在）：`rm -rf ~/.renlijia/skills/dingtalk-workspace/`
5. 触发 skill registry 刷新（按产品入口）：在技能中心点「刷新」按钮或重启应用 turn — 用 `tauri-pilot aijia new-task` 新建空对话足以让下一个 turn 重读 catalog
6. 在新对话中输入：`帮我用钉钉发条消息`
7. 点发送 + 等回复完成：`tauri-pilot aijia send` + `tauri-pilot aijia wait-reply --timeout 90`
8. 推断当前对话 id：`conv_id=$(tauri-pilot aijia where --json | jq -r .activeConversationId)`

**验收标准**

✅ 应该看到（两种合法分支任一满足即 PASS，分支由产品当前实现决定）
- **分支 A（LLM 看到 catalog 没钉钉、不调 Skill 工具）**：`~/.renlijia/users/{scope}/conversations/{conv_id}/messages.jsonl` 中**不存在** `toolCalls[].name == "Skill"` 且 `skill_id == "dingtalk-workspace"` 的记录；AI 回复文本中明确表达「没有钉钉技能」「需要先安装钉钉技能」类含义
- **分支 B（LLM 硬调 Skill 工具）**：存在 `toolCalls[].name == "Skill"` 且 `skill_id == "dingtalk-workspace"` 的记录，但对应的 `role == "tool"` 记录中 `isError == true` 或 `content.text` 包含 `not found` / `does not exist` 等明确错误字样

❌ 不应该看到
- 同时同对话目录下 `~/.renlijia/users/{scope}/skills/dingtalk-workspace/` 仍然存在（说明第 3 步删除未生效，本意图前提不成立）
- 工具返回 `content.text` 为空字符串但 `isError == false`（"静默成功 + 空内容" = 幻觉风险）
- AI 回复中假装钉钉 skill 存在并给出了来自该 skill 的具体步骤（如 `dws calendar list` 之类 — 没装 skill 时不该凭记忆吐 dws 命令）
