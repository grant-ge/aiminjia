# rules.md — 技能（Skill） 意图测试规格

## 测试范围

覆盖无状态 Skill 系统的端到端行为：从本地 `~/.renlijia/skills/` 与 `~/.renlijia/users/{scope}/skills/` 目录下 SKILL.md 的加载与 frontmatter 解析、技能目录在对话 turn 中的 prompt 注入（catalog_prompt）、到对话中 LLM 通过 `Skill` 工具按需加载 SKILL.md body。关注 `plugin/skill/`、`commands/skill_management.rs`、`commands/skill_draft.rs`（仅 import/export）、前端 `src/features/skill-center/` 的一致性。

> 历史的「技能草稿（skill draft）/ skill_smith / 小程 5 件套」机制已于 2026-06 删除。技能创建路径统一收敛到：用户在对话里跟「小程���数字员工聊（小程内部自动 Skill-load `skill-creator`），由 LLM 直接 Write SKILL.md 到 `~/.renlijia/users/{scope}/skills/<id>/`。不再有独立的草稿区。

## 待覆盖的主要场景

- 场景 1：`~/.renlijia/skills/foo/SKILL.md` 存在时，新对话 turn 的 system prompt 中包含该技能的 catalog 条目（名称 + description），且技能中心页能看到该技能
- 场景 2：LLM 在对话中调用 `Skill` 工具，工具返回该 SKILL.md 的 body 文本，AI 按 body 中的指令执行
- 场景 3：SKILL.md frontmatter 字段缺失或非法（如 `name:` 为空）时 loader 跳过该 skill，不影响其他 skill 加载

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

<!--
意图 4 / 5 / 6（技能草稿保存、发布、覆盖发布）已删除 (2026-06)。
原依赖 SkillDraftStore + SkillDraftBanner UI + skill_smith 5 件套，全部已删。
现统一收敛为「小程对话内 LLM 直接 Write SKILL.md 到 user_skills_dir」路径，
无独立草稿区、无 publish 动作。
对应 UI 测试场景按需在「员工 / 小程」task 下重写（题面为对话式创建并验证 SKILL.md 落盘）。

意图编号不重排，4/5/6 永久跳过 — 跑测试的 runner 需要识别"缺号"即跳过，不算 FAIL。
-->

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

## 意图 10：用户一句话涉及多个技能时，AI 同对话加载全部

**场景**
用户在一句自然语言里同时提到涉及不同技能领域的需求（如「做 PPT + 用钉钉发消息」），AI 应理解这是跨技能的需求并加载所有相关技能；允许同 turn 串行加载，也允许先加载主技能、并在回复中明确告知用户后续会用第二个技能。本意图覆盖最小双技能组合：`html-ppt`（PPT 生成）+ `dingtalk-workspace`（钉钉）。

**操作步骤**
1. 应用探活：`tauri-pilot aijia health-check`
2. 推断 scope；从环境记下 `{scope}`
3. 确认两个 skill 已就位：检查 `~/.renlijia/skills/html-ppt/SKILL.md` 和 `~/.renlijia/skills/dingtalk-workspace/SKILL.md` 均存在（任一不存在，先按「技能」task 意图 1 的「导入目录」流程导入，不通过手工 `cp` 落盘）
4. 新建空对话：`tauri-pilot aijia new-task`
5. 在对话输入框输入：`帮我生成一份年度总结 PPT，做完后用钉钉发给自己`
6. 点发送 + 等回复完成：`tauri-pilot aijia send` + `tauri-pilot aijia wait-reply --timeout 120`
7. 推断当前对话 id：`conv_id=$(tauri-pilot aijia where --json | jq -r .routeObj.conversationId)`

**验收标准**

✅ 应该看到（双分支任一满足即 PASS）
- **分支 A（同对话双技能均加载）**：`~/.renlijia/users/{scope}/conversations/{conv_id}/messages.jsonl` 中 `toolCalls[].name == "Skill"` 调用的 `skill_id` 去重集合 == `{"html-ppt", "dingtalk-workspace"}`
- **分支 B（先主后副、AI 在 reply 中预告第二个技能）**：jsonl 中至少存在 1 条 `toolCalls[].name == "Skill"` 调用，且 `skill_id` ∈ `{"html-ppt", "dingtalk-workspace"}`；并且 jsonl 中最后一条 `role == "assistant"` 且 `toolCalls.length == 0` 的记录，其 `content.text` 中同时包含字符串 `钉钉` 和 `PPT`（说明 LLM 在文本里 acknowledge 了两个领域、把第二个技能留给后续轮处理）
- 任一被加载的 `Skill` 调用紧随其后的 `role == "tool"` 记录 `isError != true` 且 `content.text` 长度 `!= 0`

❌ 不应该看到
- jsonl 中**无任何** `toolCalls[].name == "Skill"` 调用（一个技能都没加载 = 完全没理解需求）
- `toolCalls[].name == "Skill"` 的参数中 `skill_id` 出现非 `html-ppt` / `dingtalk-workspace` 字面（如 `ppt` / `dingtalk` / `钉钉` / `年度总结 PPT` 等错误字面 id）
- AI 回复中出现「我没有 PPT 生成技能」「无法生成 PPT」「无法发送钉钉」等否认能力的措辞
- 任一 `Skill` 调用对应的 tool record `content.text` 包含 `not found` / `does not exist` 等错误字样

---

## 意图 11：技能通过 skill-creator 装完后，无需重启即可被新对话 catalog 和 Skill 工具加载

**场景**
用户通过小程数字员工（或任何带 skill-creator 的对话）创建并安装一个新技能后，立刻在另一个新对话里用它。期望 catalog 含新技能 + Skill 工具能 load。本意图护栏对应 refresh_skill_registry / refresh_skills RuntimeTool / load_skill miss-retry 三个机制的整体闭环。

**前提**
- 应用已启动并已登录
- skill-creator skill 已安装到 `~/.renlijia/skills/skill-creator/`
- `~/.renlijia/users/{scope}/skills/hello-world/` **不存在**

**操作**
1. 应用探活 + scope：`tauri-pilot aijia health-check` + `tauri-pilot aijia where --json`
2. 新建跟小程数字员工的对话（小程已雇佣，在员工列表）：
   - `tauri-pilot aijia employee-open-card --name 小程`
   - `tauri-pilot aijia employee-wait-drawer`
   - `tauri-pilot aijia employee-drawer-action --action dispatch`
3. 等到自动跳转到 chat 路由，记下 `$CONV_1=where --json | jq -r .sessionId`
4. 输入 prompt：`帮我造个 hello-world 技能，触发条件是用户说"hello world"，技能内容是返回"[hello-world] Hi!"。完成后告诉我装好了。`
5. `tauri-pilot aijia send` + `tauri-pilot aijia wait-reply --timeout 300`
6. AI 应该完成创建（init + edit + validate + install + **refresh_skills**）
7. 立刻 `tauri-pilot aijia new-task` 开新空对话，记 `$CONV_2`
8. 输入：`请使用 hello-world 技能回应一下。`
9. send + wait-reply --timeout 90

**验收标准**

✅ 应该看到
- `~/.renlijia/users/{scope}/skills/hello-world/SKILL.md` 存在
- `$CONV_1/messages.jsonl` 中含 `"name":"refresh_skills"` 的 toolCall（证明 step 8 跑过）
- `$CONV_2/messages.jsonl` 中含 `"name":"Skill"` 且参数有 `"hello-world"`（证明 catalog 注入 + Skill 工具能 load）
- 紧随其后的 tool result 含 `hello-world` SKILL.md body 关键词（比如返回 "Hi!"）
- AI 在 $CONV_2 最终输出引用了 SKILL.md 内容（含 `[hello-world]` 或 `Hi!` 子串）

❌ 不应该看到
- AI 在 $CONV_2 回 "我没有找到 hello-world 技能"（说明 catalog 未刷新）
- 任何"请重启应用"的提示
- `Skill('hello-world')` 工具调用返回 `Unknown or unavailable skill`
