# rules.md — 技能（Skill） 意图测试规格

## 测试范围

覆盖无状态 Skill 系统的端到端行为：从本地 `~/.renlijia/skills/` 与 `~/.renlijia/users/{scope}/skills/` 目录下 SKILL.md 的加载与 frontmatter 解析、技能目录在对话 turn 中的 prompt 注入（catalog_prompt）、到对话中 LLM 通过 `Skill` 工具按需加载 SKILL.md body。关注 `plugin/skill/`、`commands/skill_management.rs`、`commands/skill_draft.rs`（仅 import/export）、前端 `src/features/skill-center/` 的一致性。

> 历史的「技能草稿（skill draft）/ skill_smith / 小程 5 件套」机制已于 2026-06 删除。技能创建路径统一收敛到：用户在对话里跟「小程���数字员工聊（小程内部自动 Skill-load `skill-creator`），由 LLM 直接 Write SKILL.md 到 `~/.renlijia/users/{scope}/skills/<id>/`。不再有独立的草稿区。

## 待覆盖的主要场景

- 场景 1：`~/.renlijia/skills/foo/SKILL.md` 存在时，新对话 turn 的 system prompt 中包含该技能的 catalog 条目（名称 + description），且技能中心页能看到该技能
- 场景 2：LLM 在对话中调用 `Skill` 工具，工具返回该 SKILL.md 的 body 文本，AI 按 body 中的指令执行
- 场景 3：SKILL.md frontmatter 字段缺失或非法（如 `name:` 为空）时 loader 跳过该 skill，不影响其他 skill 加载
- 场景 4：用户在技能中心关闭已安装技能后，关闭状态持久化到当前用户的 `skillsConfig.json`，聊天入口不再展示该技能
- 场景 5：关闭技能后，新对话的技能 catalog 与 `Skill` 工具都不能再让该技能生效
- 场景 6：市场页只负责添加与查看详情，已添加但关闭的技能不在市场卡片上展示开关或关闭状态
- 场景 7：登录同步只自动安装必需内置技能，必需内置技能默认开启
- 场景 8：用户关闭必需内置技能后，后续同步不能自动重新开启
- 场景 9：市场技能未手动添加前，不进入聊天入口、catalog 或 `Skill` 工具可用集合
- 场景 10：市场技能手动添加后，默认开启并进入聊天入口、catalog 和 `Skill` 工具可用集合
- 场景 11：点击“更新官方技能”后，市场里未添加的技能仍然不安装、不进入聊天入口
- 场景 12：点击“更新官方技能”后，用户已经关闭的内置技能仍保持关闭

## 执行约束：技能启用状态改造

- 意图 16-24 默认依赖这些 `tauri-pilot aijia` 原子命令：`skill-center-open`、`skill-center-tab`、`skill-center-list --json`、`skill-market-list --json`、`skill-center-toggle --id --enabled`、`skill-market-add --id`、`skill-detail-open --id`、`skill-picker-open --json`、`slash-suggestions --query --json`、`sync-builtin-skills`。如果命令缺失，runner 应标记为 `CLI gap`，不能把人工点按当成稳定自动化结果。
- 意图测试不得删除整个 `~/.renlijia/users/{scope}/skillsConfig.json`。需要准备开关状态时，只能通过产品入口或 CLI 对测试 skill id 设置开启/关闭，避免误删用户对其他技能的偏好。
- 选择市场测试技能时，必须优先使用 `skill-market-list --json` 中 `installed == false` 的项；如果当前环境没有未添加市场技能，本意图记为环境阻塞/跳过，不要卸载真实用户技能来造数据。
- 校验模型不可用不能只靠“LLM 没有调用”作为唯一证据；需要同时检查 catalog/技能入口不出现，或使用后端 focused test 证明 `Skill` 工具对 disabled/not-installed id 返回 unavailable。

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

- 文件 `~/.renlijia/users/{scope}/skills/demo-skill/SKILL.md` 存在
- 该文件首行为 `---`、内容含 `name: demo-skill` 与 `演示用：一个最小可加载技能`
- 技能中心 DOM 中存在 `[data-aijia-skill-card][data-aijia-skill-id="demo-skill"]` 节点（CLI 待补 `aijia skill-cards --json`，详见 `cli-gap.md`）
- 该卡片节点的 `data-aijia-skill-source` 属性值为 `"user"`（导入路径下沉到 user scope，不是 global）
- 技能中心 UI 上该卡片的可见文本含 `demo-skill` 与 `演示用：一个最小可加载技能`

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

- 在该对话目录 `~/.renlijia/users/{scope}/conversations/{conv_id}/` 下，`messages.jsonl` 中出现至少一条 `role` 为 `"assistant"` 的消息，其顶层 `toolCalls[].name` 字段值为 `"Skill"`，且参数中包含 `"demo-skill"`
- 同一对话的消息文件中紧随其后出现一条 `role` 为 `"tool"` 的消息，其 `content` 字段（经 `\t✓\n` 分隔后取记录）的文本中包含 `本技能用于意图测试`
- 对话界面最终展示的 AI 回复文本中**引用了** `demo-skill` SKILL.md body 的内容（包含 `本技能用于意图测试` 子串，或对该 body 描述的功能做出回应）

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

- 第 3 步成功（toast 提示导入成功 / 技能中心出现 good-skill 卡片）
- 第 4 步**失败**（toast 提示导入失败 / 错误提示含 "name" 必填校验信息）
- 技能中心 DOM 含 `[data-aijia-skill-card][data-aijia-skill-id="good-skill"]` 节点
- `~/.renlijia/users/{scope}/skills/good-skill/SKILL.md` 存在

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

- 轮 A / 轮 B / 轮 C 每一轮都必须独立满足以下判定项，任一轮未满足即整条 FAIL
- 轮 A 的 `~/.renlijia/users/{scope}/conversations/{conv_a}/messages.jsonl` 中存在一条 `role == "assistant"` 的记录，其顶层 `toolCalls` 数组中存在一项 `name == "Skill"` 且参数 JSON 中 `skill_id == "dingtalk-workspace"`
- 轮 A 紧随其后存在一条 `role == "tool"` 的记录，其 `toolCallId` 等于上一条对应 `toolCalls[N].id`，且 `content.text` 中包含字符串 `dingtalk-workspace-cli`
- 轮 B 的 `~/.renlijia/users/{scope}/conversations/{conv_b}/messages.jsonl` 中存在一条 `role == "assistant"` 的记录，其 `toolCalls` 中存在一项 `name == "Skill"` 且参数 JSON 中 `skill_id == "dingtalk-workspace"`
- 轮 B 紧随其后存在一条 `role == "tool"` 的记录，`content.text` 中包含字符串 `dingtalk-workspace-cli`
- 轮 C 的 `~/.renlijia/users/{scope}/conversations/{conv_c}/messages.jsonl` 中存在一条 `role == "assistant"` 的记录，其 `toolCalls` 中存在一项 `name == "Skill"` 且参数 JSON 中 `skill_id == "dingtalk-workspace"`
- 轮 C 紧随其后存在一条 `role == "tool"` 的记录，`content.text` 中包含字符串 `dingtalk-workspace-cli`

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

- `~/.renlijia/users/{scope}/conversations/{conv_id}/messages.jsonl` 中存在一条 `role == "assistant"` 的记录，其 `toolCalls` 中存在一项 `name == "Skill"`
- 该 `Skill` 调用参数 JSON 中 `skill_id == "dingtalk-workspace"`
- 该 `Skill` 调用对应的 `role == "tool"` 记录的 `content.text` 中包含字符串 `dingtalk-workspace-cli`

- `toolCalls[].name == "Skill"` 的参数中 `skill_id == "玩转钉钉"`（按 label 字面找会 not found）
- 该 turn 出现「skill not found」类错误 tool result
- AI 回复中出现「没有名叫玩转钉钉的技能」「找不到对应技能」等措辞

---

## 意图 9：在技能详情页点击「使用」后，首页只预置一个技能 chip

**场景**
用户在技能中心进入某个技能详情页，点击右上角「使用」按钮。产品承诺不是立刻创建对话并自动运行该技能，而是回到首页，把该技能作为用户下一轮输入的显式意图预置到输入框中。这个预置动作必须是一次性的：即使前端在 React StrictMode / dev 环境下重放 mount effect，首页输入框也只能出现一个 skill chip。

**前提**
- 应用已启动并已登录。
- 技能中心至少存在一个可见技能，例如 `biz-proposal`；若本机没有该技能，可换成任意已安装技能，并在报告中记录实际 skill id / label mapping。
- 首页输入框当前为空；若已有草稿，先手动清空（不删除任何本地文件或技能目录）。

**操作步骤**
1. 应用探活：`tauri-pilot aijia health-check`
2. 切到技能中心：`tauri-pilot aijia goto skill-center`
3. 找到目标技能卡片：DOM 中应存在 `[data-aijia-skill-card][data-aijia-skill-id="{skill_id}"]`
4. 点击目标技能卡片进入详情页；确认页面显示该技能名称与「使用」按钮
5. 点击「使用」按钮
6. 等待路由回到首页；读取当前路由和首页输入框 DOM

**验收标准**

应该看到：
- 点击「使用」后当前路由为 `home`，没有自动新建 chat 路由
- 首页输入框 `.ProseMirror` 中存在且仅存在 1 个 `[data-rich-composer-skill-token]` 节点
- 该节点的 `data-id == "{skill_id}"`，`data-label` 等于该技能 UI label，`data-command` 等于该技能 trigger（如 `/biz-proposal`）
- `~/.renlijia/users/{scope}/conversations/` 中没有因为本次点击新增空对话；换言之，「使用」按钮只表达下一轮输入意图，不应立即触发 LLM turn
- 如果随后在该 chip 后补充文本并发送，消息发送 payload 中只携带 1 个 skill token（同一技能不得重复进入本轮 payload.skills）

不应该看到：
- 首页输入框出现 2 个或更多相同 `data-id` 的 `[data-rich-composer-skill-token]`
- 点击「使用」后仍停留在技能详情页，或跳到 chat 路由并自动发送消息
- UI 上同时出现 pending skill chip 和一段裸露的 `/skill-id` 文本（同一意图被重复序列化）
- 因重复 chip 导致发送时同一技能被重复加载、重复出现在用户消息附件/技能列表中

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

- 以下两个分支任一满足即 PASS
- **分支 A（同对话双技能均加载）**：`~/.renlijia/users/{scope}/conversations/{conv_id}/messages.jsonl` 中 `toolCalls[].name == "Skill"` 调用的 `skill_id` 去重集合 == `{"html-ppt", "dingtalk-workspace"}`
- **分支 B（先主后副、AI 在 reply 中预告第二个技能）**：jsonl 中至少存在 1 条 `toolCalls[].name == "Skill"` 调用，且 `skill_id` ∈ `{"html-ppt", "dingtalk-workspace"}`；并且 jsonl 中最后一条 `role == "assistant"` 且 `toolCalls.length == 0` 的记录，其 `content.text` 中同时包含字符串 `钉钉` 和 `PPT`（说明 LLM 在文本里 acknowledge 了两个领域、把第二个技能留给后续轮处理）
- 任一被加载的 `Skill` 调用紧随其后的 `role == "tool"` 记录 `isError != true` 且 `content.text` 长度 `!= 0`

- jsonl 中**无任何** `toolCalls[].name == "Skill"` 调用（一个技能都没加载 = 完全没理解需求）
- `toolCalls[].name == "Skill"` 的参数中 `skill_id` 出现非 `html-ppt` / `dingtalk-workspace` 字面（如 `ppt` / `dingtalk` / `钉钉` / `年度总结 PPT` 等错误字面 id）
- AI 回复中出现「我没有 PPT 生成技能」「无法生成 PPT」「无法发送钉钉」等否认能力的措辞
- 任一 `Skill` 调用对应的 tool record `content.text` 包含 `not found` / `does not exist` 等错误字样

---

## 意图 11：用 html-ppt 生成长 PPT 期间，UI 始终有可见反馈

**场景**
客户反馈："发送了一个很长的 PPT 制作要求，agent 执行了几次工具后，UI 上 90 秒看不到任何文字或工具状态更新，客户以为客户端无响应了。"本意图测的是**客户感知层面的"可见反馈"承诺**——不是测后端 stream 是否断、不是测客户端是否 retry。从用户点发送到 AI 整轮回复结束期间，UI 上"用户能看到的信号"（最后一条 assistant 文本长度 + `messageCount` 即气泡总数）任一在 90 秒滚动窗口内必须有变化；两个指标连续 90 秒同时不变 = 客户感知"看着像死了" = FAIL。

**为什么不主动 cancel**：实验证明监测脚本在 90 秒主动 cancel 会截胡 LLM 正在算的长 tool args（一次性 generate 10000+ token 的 Edit / Bash args 是合理需求，可耗时 60-160 秒），导致测试 false positive。正确做法是**纯观察**：让 turn 自然跑完，从最终 `wait-reply` 返回 + 采样时间序列回头判定窗口内有没有真的"无可见信号"。

**操作步骤**
1. 应用探活：`tauri-pilot aijia health-check`
2. 推断 scope；从环境记下 `{scope}`
3. 确认 `~/.renlijia/skills/html-ppt/SKILL.md` 存在且 frontmatter 中 `name: html-ppt`；若不存在，先按「技能」task 意图 1 的「导入目录」路径把它导入，**不通过手工 `cp` 落盘**
4. 新建空对话：`tauri-pilot aijia new-task`
5. 在对话输入框输入一段长 prompt：`请用 html-ppt 技能帮我生成一份 30 页的产品发布会演讲稿 PPT，主题是「2026 年新一代 AI 工作台正式发布」。结构如下：第 1 页封面（含主标题、副标题、日期、品牌 logo）；第 2 页目录；第 3-4 页公司简介与团队；第 5-6 页行业痛点与市场背景；第 7-8 页产品定位与差异化；第 9-13 页 5 个核心功能（每页 1 个，含 3 个亮点 + 1 张示意图）；第 14-15 页技术架构亮点；第 16-18 页 3 个标杆客户案例（每页 1 个，含背景 / 方案 / 量化收益）；第 19-20 页性能指标与对比图表；第 21-22 页商业化路径与定价方案；第 23-24 页 partner / 生态合作；第 25 页路线图；第 26-27 页风险与应对；第 28 页 Q&A 引导；第 29 页致谢；第 30 页结语 + 联系方式。主题用 sharp-mono，每页都要有 data-anim 入场动画。`
6. 记下 `T0=$(date +%s)`
7. 点发送：`tauri-pilot aijia send`
8. **并发纯观察（不 cancel）**：主线程跑 `tauri-pilot aijia wait-reply --timeout 1800`（30 分钟上限——30 页 PPT 实测约 15 分钟，留 100% buffer 应对网络抖动）；副进程每 10 秒采样以下二元组并落到 `samples.log`：
   - `text_len = tauri-pilot aijia ui-message --json | jq '[.[] | select(.role=="assistant")][-1].text | length'`
   - `msg_count = tauri-pilot aijia where --json | jq '.messageCount'`（**用 `messageCount`，不用 `aijia tool-calls --json`——后者在 turn 进行中始终返回空数组**）
   监测脚本**仅记录、不 cancel**；前 3 次采样作为 grace period 不计入静默窗口（AI 还没启动）；之后任一指标增长就重置静默计数
9. `wait-reply` 返回后 `T1=$(date +%s)`
10. 推断当前对话 id：`conv_id=$(tauri-pilot aijia where --json | jq -r .routeObj.conversationId)`

**验收标准**

- 步骤 8 的 `wait-reply --timeout 1800` 在 1800 秒内返回 `ok`（即 `T1 - T0 < 1800`）
- 步骤 8 采样的 `(text_len, msg_count)` 时间序列中，**不存在**任何连续 9 次（≥ 90 秒）两个指标都未增长的窗口（从 grace period 之后算起）
- `~/.renlijia/users/{scope}/conversations/{conv_id}/messages.jsonl` 中存在至少 1 条 `role == "assistant"` 的记录，其 `toolCalls` 中存在一项 `name == "Skill"` 且参数 JSON 中 `skill_id == "html-ppt"`
- 该 `Skill` 调用紧随其后的 `role == "tool"` 记录的 `content.text` 长度 `!= 0` 且 `isError != true`
- 最终一条 `role == "assistant"` 且 `toolCalls.length == 0` 的记录中 `content.text` 长度 `>= 200`（确保 AI 真的产出了 PPT 内容而不是只回了一句"好的"）
- workspace 下出现至少 1 个 PPT 产物文件（如 `index.html` 或 `*.html`），大小 `>= 50 KB`（30 页 PPT 实测 100+ KB）
- 跑完后 `tauri-pilot aijia health-check` 仍返回 ok

- 步骤 8 采样序列出现 ≥ 90 秒的"双指标同时零增长窗口"（这是客户感知"看着像死了"的硬证据）
- 对话 UI 中本轮出现红色错误提示或「工具调用失败」类 toast
- 应用日志中出现 `panic` 或 `Failed to write` 等致命错误字样
- `wait-reply` 在 1800 秒超时返回错误（turn 真跑不完）

---

## 意图 12：技能通过 skill-creator 装完后，无需重启即可被新对话 catalog 和 Skill 工具加载

**场景**
用户通过小程数字员工（或任何带 skill-creator 的对话）创建并安装一个新技能后，立刻在另一个新对话里用它。期望 catalog 含新技能 + Skill 工具能 load。本意图护栏对应 refresh_skill_registry / RefreshSkills RuntimeTool / load_skill miss-retry 三个机制的整体闭环。

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
6. AI 应该完成创建（init + edit + validate + install + **RefreshSkills**）
7. 立刻 `tauri-pilot aijia new-task` 开新空对话，记 `$CONV_2`
8. 输入：`请使用 hello-world 技能回应一下。`
9. send + wait-reply --timeout 90

**验收标准**

- `~/.renlijia/users/{scope}/skills/hello-world/SKILL.md` 存在
- `$CONV_1/messages.jsonl` 中含 `"name":"RefreshSkills"` 的 toolCall（证明 step 8 跑过）
- `$CONV_2/messages.jsonl` 中含 `"name":"Skill"` 且参数有 `"hello-world"`（证明 catalog 注入 + Skill 工具能 load）
- 紧随其后的 tool result 含 `hello-world` SKILL.md body 关键词（比如返回 "Hi!"）
- AI 在 $CONV_2 最终输出引用了 SKILL.md 内容（含 `[hello-world]` 或 `Hi!` 子串）

- AI 在 $CONV_2 回 "我没有找到 hello-world 技能"（说明 catalog 未刷新）
- 任何"请重启应用"的提示
- `Skill('hello-world')` 工具调用返回 `Unknown or unavailable skill`

---


## 意图 13：多轮迭代修改 PPT 时，每轮 UI 始终有可见反馈

**场景**
用户先让 AI 用 `html-ppt` 生成一份基础 PPT，再连续两轮修改（换主题、加新页）。客户反馈第 2/3 轮迭代上下文累积时更容易让 UI 显得"卡死"。承诺同意图 11：每轮 turn 期间 UI 可见信号（最后一条 assistant 文本长度 + `messageCount`）任一在 90 秒滚动窗口内必须有变化；3 轮任一轮出现连续 90 秒双指标同时零增长 = 整条 FAIL。

**为什么不主动 cancel**：同意图 11 — 实验已证明长 tool args 计算期间 90s cancel 会 false positive，采用纯观察模式。

**操作步骤**
1. 应用探活：`tauri-pilot aijia health-check`
2. 推断 scope；记下 `{scope}`
3. 确认 `~/.renlijia/skills/html-ppt/SKILL.md` 存在；若不存在按「技能」task 意图 1 导入
4. 新建空对话：`tauri-pilot aijia new-task`；首次 `send` 之后再推断 `conv_id=$(tauri-pilot aijia where --json | jq -r .routeObj.conversationId)`
5. **轮 1（基础生成）**：
   1. 输入：`请用 html-ppt 帮我生成一份 12 页的技术分享 PPT，主题是「Rust 异步编程入门」，包含封面、目录、5 个核心概念、3 个代码示例、总结、Q&A、结语，主题用 minimal-white`
   2. 记 `T0_a=$(date +%s)`，`tauri-pilot aijia send`
   3. 主线程跑 `wait-reply --timeout 1200`（20 分钟，12 页比 30 页更轻量），副进程每 10 秒采样 `(text_len, msg_count)` 落到 samples.log，**仅记录、不 cancel**
   4. `wait-reply` 返回后 `T1_a=$(date +%s)`
6. **轮 2（换主题）**：
   1. 输入：`把整份 PPT 的主题换成 dracula，其他不变`
   2. 记 `T0_b=$(date +%s)`，`tauri-pilot aijia send`
   3. 同轮 1 步骤 c：纯观察 + `wait-reply --timeout 1200`
   4. `wait-reply` 返回后 `T1_b=$(date +%s)`
7. **轮 3（加新页）**：
   1. 输入：`在结语前加一页「常见踩坑」，列 4 个常见错误及避免方法`
   2. 记 `T0_c=$(date +%s)`，`tauri-pilot aijia send`
   3. 同轮 1 步骤 c
   4. `wait-reply` 返回后 `T1_c=$(date +%s)`

**验收标准**

- 轮 1 / 轮 2 / 轮 3 每一轮都必须独立满足以下判定项，任一轮未满足即整条 FAIL
- 轮 1 / 轮 2 / 轮 3 的 `wait-reply --timeout 1200` 均返回 `ok`（即 `T1_a - T0_a < 1200`、`T1_b - T0_b < 1200`、`T1_c - T0_c < 1200`）
- 轮 1 / 轮 2 / 轮 3 各自采样的 `(text_len, msg_count)` 时间序列中均**不存在**任何连续 9 次（≥ 90 秒）两个指标都未增长的窗口（从 grace period 之后算起）
- `~/.renlijia/users/{scope}/conversations/{conv_id}/messages.jsonl` 中至少有 1 条 `role == "assistant"` 的记录 `toolCalls[].name == "Skill"` 且 `skill_id == "html-ppt"`（轮 1 必定触发；轮 2/3 是否复用 skill body 由 LLM 决定，不强约束）
- 3 轮各自最后一条 `role == "assistant"` 且 `toolCalls.length == 0` 的记录中 `content.text` 长度均 `>= 100`
- 跑完 3 轮后 `tauri-pilot aijia health-check` 仍返回 ok

- 任一轮采样序列出现 ≥ 90 秒的"双指标同时零增长窗口"
- 任一轮 `wait-reply` 在 1200 秒超时返回错误
- 同对话中出现 `role == "tool"` 记录的 `content.text` 包含 `context length exceeded` / `too many tokens` 字样（应当通过上下文压缩规避，而不是直接报错）
- 应用日志中出现 `panic` / `Failed to write` 等致命错误字样

---

## 意图 14：对话安装技能，列表立即显示

**场景**
用户在对话里让小程创建并安装一个新技能。期望 AI 安装完成后主动调用 `RefreshSkills`，让后端 registry 立即刷新；用户不重启应用，直接打开技能中心就能看到刚安装的用户技能；再开新对话时也能通过 `Skill` 工具加载该技能。本意图护栏“对话创建技能 -> RefreshSkills -> 技能中心列表 -> 新对话 Skill catalog”这条链路。

**操作步骤**
1. 应用探活：`tauri-pilot aijia health-check`
2. 推断当前 scope：`tauri-pilot aijia where --json` 取，记为 `$SCOPE`
3. 确认 `~/.renlijia/skills/skill-creator/SKILL.md` 存在；若不存在，先按「技能」task 意图 1 的导入路径安装，不通过手工 `cp` 落盘
4. 清理可能残留的测试技能目录：`rm -rf ~/.renlijia/users/{scope}/skills/refresh-link-skill`
5. 新建跟小程数字员工的对话：
   - `tauri-pilot aijia employee-open-card --name 小程`
   - `tauri-pilot aijia employee-wait-drawer`
   - `tauri-pilot aijia employee-drawer-action --action dispatch`
6. 等到自动跳转到 chat 路由，记下 `$CONV_1=tauri-pilot aijia where --json | jq -r .sessionId`
7. 输入 prompt：`帮我创建一个 refresh-link-skill 技能，触发条件是用户说"refresh link"，技能内容是返回"[refresh-link-skill] ok"。完成安装后刷新技能列表，然后告诉我装好了。`
8. `tauri-pilot aijia send` + `tauri-pilot aijia wait-reply --timeout 300`
9. 打开技能中心：`tauri-pilot aijia skill-center-open`
10. 读取技能中心列表快照：`tauri-pilot aijia skill-center-list --json`，记为 `$SKILL_LIST`
11. 立刻 `tauri-pilot aijia new-task` 开新空对话，记 `$CONV_2`
12. 输入：`请使用 refresh-link-skill 技能回应一下。`
13. `tauri-pilot aijia send` + `tauri-pilot aijia wait-reply --timeout 90`

**验收标准**

应该看到：
- `~/.renlijia/users/{scope}/skills/refresh-link-skill/SKILL.md` 存在
- `$CONV_1/messages.jsonl` 中存在 `toolCalls[].name == "RefreshSkills"` 的调用
- `$CONV_1/messages.jsonl` 中 `RefreshSkills` 紧随其后的 tool record `isError != true`
- `$SKILL_LIST` 中存在 `id == "refresh-link-skill"` 或 `name == "refresh-link-skill"` 的技能项
- `$SKILL_LIST` 中该技能项来源为用户级技能（如 `scope == "user"`、`source == "user"` 或路径位于 `~/.renlijia/users/{scope}/skills/`）
- `$CONV_2/messages.jsonl` 中存在 `toolCalls[].name == "Skill"` 且参数中包含 `refresh-link-skill`
- `$CONV_2` 最终 assistant 回复包含 `[refresh-link-skill]` 或 `ok`

不应该看到：
- AI 安装完成后提示「请重启应用」「重启后生效」
- 技能文件已存在，但技能中心列表中没有 `refresh-link-skill`
- `$CONV_2` 中 `Skill('refresh-link-skill')` 返回 `Unknown or unavailable skill` / `not found`
- 技能中心打开或列表刷新时出现 `syncFailed` / `loadFailed` / 「同步失败」类 toast

---

## 意图 15：点击同步本地技能，磁盘技能入列表

**场景**
用户或外部工具已经把一个合法技能目录写入当前用户的本地 skills 目录，但前端内存里的技能 registry 还不知道它。用户进入技能中心点击「同步技能」里的本地同步入口后，期望后端重扫本地 user/global skills，触发 registry 更新事件，技能中心列表立即出现该技能；随后新对话也能通过 `Skill` 工具加载。本意图护栏技能中心“同步本地技能”按钮和后端 `refreshSkillRegistry` 的兜底同步路径。

**操作步骤**
1. 应用探活：`tauri-pilot aijia health-check`
2. 推断当前 scope：`tauri-pilot aijia where --json` 取，记为 `$SCOPE`
3. 清理可能残留的测试技能目录：`rm -rf ~/.renlijia/users/{scope}/skills/manual-sync-skill`
4. 创建目录 `~/.renlijia/users/{scope}/skills/manual-sync-skill`
5. 写入 `~/.renlijia/users/{scope}/skills/manual-sync-skill/SKILL.md`：
   ```
   ---
   name: manual-sync-skill
   description: 当用户说 manual sync 时返回固定文本
   ---

   当用户要求使用 manual-sync-skill 时，回复 `[manual-sync-skill] synced`。
   ```
6. 打开技能中心：`tauri-pilot aijia skill-center-open`
7. 记录同步前技能中心列表快照：`tauri-pilot aijia skill-center-list --json`，记为 `$LIST_BEFORE`
8. 点击「同步技能」下拉里的「同步本地技能」：`tauri-pilot aijia skill-center-sync --action local`
9. 等待同步完成和列表刷新：`tauri-pilot aijia skill-center-wait-sync --timeout 30`
10. 记录同步后技能中心列表快照：`tauri-pilot aijia skill-center-list --json`，记为 `$LIST_AFTER`
11. 立刻 `tauri-pilot aijia new-task` 开新空对话，记 `$CONV_ID`
12. 输入：`请使用 manual-sync-skill 技能回应一下。`
13. `tauri-pilot aijia send` + `tauri-pilot aijia wait-reply --timeout 90`

**验收标准**

应该看到：
- `~/.renlijia/users/{scope}/skills/manual-sync-skill/SKILL.md` 存在
- `$LIST_AFTER` 中存在 `id == "manual-sync-skill"` 或 `name == "manual-sync-skill"` 的技能项
- `$LIST_AFTER` 中该技能项来源为用户级技能（如 `scope == "user"`、`source == "user"` 或路径位于 `~/.renlijia/users/{scope}/skills/`）
- `$LIST_AFTER` 中 `manual-sync-skill` 的更新时间晚于或等于步骤 8 的同步触发时间
- `$CONV_ID/messages.jsonl` 中存在 `toolCalls[].name == "Skill"` 且参数中包含 `manual-sync-skill`
- `$CONV_ID` 最终 assistant 回复包含 `[manual-sync-skill]` 或 `synced`

不应该看到：
- 点击同步本地技能后仍需要重启应用才出现卡片
- `$LIST_AFTER` 中没有 `manual-sync-skill`，但文件系统中 `SKILL.md` 已存在
- 技能中心出现 `syncFailed` / `loadFailed` / 「同步失败」类 toast
- 新对话中 `Skill('manual-sync-skill')` 返回 `Unknown or unavailable skill` / `not found`

---

## 意图-技能-016：关闭技能后，聊天入口隐藏

**场景**
用户已经安装了一个技能，但暂时不希望它参与后续对话。用户在技能中心「已安装」页关闭该技能后，技能仍然留在管理列表里，但聊天输入框的技能选择入口不再展示它，关闭状态写入当前登录用户的本地配置文件。

**操作步骤**
1. 应用探活：`tauri-pilot aijia health-check`
2. 推断当前 scope：`tauri-pilot aijia where --json` 取，记为 `$SCOPE`
3. 清理可能残留的测试技能目录：`rm -rf ~/.renlijia/users/{scope}/skills/toggle-hidden-skill`
4. 创建目录 `~/.renlijia/users/{scope}/skills/toggle-hidden-skill`
5. 写入 `~/.renlijia/users/{scope}/skills/toggle-hidden-skill/SKILL.md`：
   ```
   ---
   name: toggle-hidden-skill
   description: 关闭后不应出现在聊天入口
   ---

   当用户要求使用 toggle-hidden-skill 时，回复 `[toggle-hidden-skill] visible`。
   ```
6. 打开技能中心：`tauri-pilot aijia skill-center-open`
7. 点击「同步技能」下拉里的「同步本地技能」：`tauri-pilot aijia skill-center-sync --action local`
8. 切到「已安装」页：`tauri-pilot aijia skill-center-tab --name 已安装`，等待列表中出现 `toggle-hidden-skill`
9. 先确保测试技能处于开启态：`tauri-pilot aijia skill-center-toggle --id toggle-hidden-skill --enabled true`
10. 再关闭测试技能：`tauri-pilot aijia skill-center-toggle --id toggle-hidden-skill --enabled false`
11. 读取技能中心列表快照：`tauri-pilot aijia skill-center-list --json`，记为 `$SKILL_LIST`
12. 返回首页并新建空对话：`tauri-pilot aijia new-task`
13. 打开聊天输入框的技能选择入口：`tauri-pilot aijia skill-picker-open --json`，记为 `$CHAT_SKILLS`
14. 在输入框输入 `/toggle` 后读取 slash 候选快照：`tauri-pilot aijia slash-suggestions --query /toggle --json`，记为 `$SLASH_SKILLS`

**验收标准**
- `~/.renlijia/users/{scope}/skills/toggle-hidden-skill/SKILL.md` 存在
- `~/.renlijia/users/{scope}/skillsConfig.json` 存在
- `~/.renlijia/users/{scope}/skillsConfig.json` 中 `disabledSkillIds` 包含 `toggle-hidden-skill`
- `$SKILL_LIST` 中存在 `id == "toggle-hidden-skill"` 或 `name == "toggle-hidden-skill"` 的技能项
- `$SKILL_LIST` 中 `toggle-hidden-skill.enabled == false`
- `$CHAT_SKILLS` 中不存在 `id == "toggle-hidden-skill"` 的技能项
- `$SLASH_SKILLS` 中不存在命令为 `/toggle-hidden-skill` 的候选项
- `~/.renlijia/global/skillsConfig.json` 不存在
- 技能中心没有删除 `toggle-hidden-skill` 的本地目录

---

## 意图-技能-017：关闭技能后，模型不再加载

**场景**
用户关闭某个已安装技能后，即使下一轮对话里明确提到这个技能名，模型也不能再通过技能目录或 `Skill` 工具加载它。这个意图验证后端过滤，而不是只验证前端开关样式。

**操作步骤**
1. 应用探活：`tauri-pilot aijia health-check`
2. 推断当前 scope：`tauri-pilot aijia where --json` 取，记为 `$SCOPE`
3. 清理可能残留的测试技能目录：`rm -rf ~/.renlijia/users/{scope}/skills/toggle-runtime-skill`
4. 创建目录 `~/.renlijia/users/{scope}/skills/toggle-runtime-skill`
5. 写入 `~/.renlijia/users/{scope}/skills/toggle-runtime-skill/SKILL.md`：
   ```
   ---
   name: toggle-runtime-skill
   description: 关闭后模型不应加载的测试技能
   ---

   当用户要求使用 toggle-runtime-skill 时，只能回复 `[toggle-runtime-skill] loaded`。
   ```
6. 打开技能中心：`tauri-pilot aijia skill-center-open`
7. 点击「同步技能」下拉里的「同步本地技能」：`tauri-pilot aijia skill-center-sync --action local`
8. 切到「已安装」页：`tauri-pilot aijia skill-center-tab --name 已安装`，等待列表中出现 `toggle-runtime-skill`
9. 先确保测试技能处于开启态：`tauri-pilot aijia skill-center-toggle --id toggle-runtime-skill --enabled true`
10. 再关闭测试技能：`tauri-pilot aijia skill-center-toggle --id toggle-runtime-skill --enabled false`
11. 新建空对话：`tauri-pilot aijia new-task`，记当前会话为 `$CONV_ID`
12. 输入：`请使用 toggle-runtime-skill 技能回应，只要加载成功就输出它要求的固定文本。`
13. `tauri-pilot aijia send` + `tauri-pilot aijia wait-reply --timeout 90`
14. 读取 `$CONV_ID/messages.jsonl`

**验收标准**
- `~/.renlijia/users/{scope}/skillsConfig.json` 中 `disabledSkillIds` 包含 `toggle-runtime-skill`
- `$CONV_ID/messages.jsonl` 中不存在 `toolCalls[].name == "Skill"` 且参数包含 `toggle-runtime-skill` 的成功调用
- `$CONV_ID/messages.jsonl` 中不存在 `role == "tool"` 且内容包含 `[toggle-runtime-skill] loaded` 的记录
- `$CONV_ID/messages.jsonl` 中不存在最终 assistant 回复包含 `[toggle-runtime-skill] loaded`
- 如果 `$CONV_ID/messages.jsonl` 中出现参数包含 `toggle-runtime-skill` 的 `Skill` 调用，紧随其后的 tool record `isError == true`
- 如果 `$CONV_ID/messages.jsonl` 中出现参数包含 `toggle-runtime-skill` 的 `Skill` 调用，紧随其后的 tool record 内容包含 `disabled`、`unavailable`、`未启用` 或 `已关闭` 之一

---

## 意图-技能-018：回到市场后，只显示添加状态

**场景**
用户在「已安装」页关闭一个企业下发或平台可添加的技能后，回到「市场」页查看同一个技能。市场卡片只表达「可添加 / 已添加」和进入详情，不展示关闭开关、不展示「已关闭」标签，也不出现「去对话」之类的额外入口。

**操作步骤**
1. 应用探活：`tauri-pilot aijia health-check`
2. 推断当前 scope：`tauri-pilot aijia where --json` 取，记为 `$SCOPE`
3. 打开技能中心：`tauri-pilot aijia skill-center-open`
4. 切到「市场」页：`tauri-pilot aijia skill-center-tab --name 市场`，读取市场列表快照：`tauri-pilot aijia skill-market-list --json`，选择一个可添加或已添加的技能，记为 `$MARKET_SKILL_ID` 与 `$MARKET_SKILL_NAME`
5. 如果 `$MARKET_SKILL_ID` 未添加，通过 `tauri-pilot aijia skill-market-add --id $MARKET_SKILL_ID` 完成添加
6. 切到「已安装」页：`tauri-pilot aijia skill-center-tab --name 已安装`，等待列表中出现 `$MARKET_SKILL_ID`
7. 关闭该技能：`tauri-pilot aijia skill-center-toggle --id $MARKET_SKILL_ID --enabled false`
8. 切回「市场」页：`tauri-pilot aijia skill-center-tab --name 市场`，定位 `$MARKET_SKILL_ID` 对应卡片
9. 点击 `$MARKET_SKILL_ID` 对应卡片进入详情页：`tauri-pilot aijia skill-detail-open --id $MARKET_SKILL_ID`

**验收标准**
- 市场页 `$MARKET_SKILL_ID` 卡片可见
- 市场页 `$MARKET_SKILL_ID` 卡片展示「已添加」
- 市场页 `$MARKET_SKILL_ID` 卡片不展示「已关闭」
- 市场页 `$MARKET_SKILL_ID` 卡片不展示开关控件
- 市场页 `$MARKET_SKILL_ID` 卡片不展示「去对话」
- 已安装页 `$MARKET_SKILL_ID` 技能项的开关处于关闭状态
- 详情页展示 `$MARKET_SKILL_NAME`
- 详情页展示「开启并使用」
- 详情页展示「保持关闭」
- 详情页不展示「使用」作为主按钮

---

## 意图-技能-019：登录同步后，内置技能默认开启

**场景**
用户登录后，AI 小家会确保产品必需的内置基础技能存在，例如创建技能能力和钉钉工作台技能包装层。它们不是市场技能，不需要用户先点「+」；在用户没有手动关闭过这些技能时，它们应当已安装并处于开启状态。钉钉能力的真实 skill id 是 `dingtalk-workspace`，`dws` 只是 CLI/展示 shorthand。

**操作步骤**
1. 应用探活：`tauri-pilot aijia health-check`
2. 推断当前 scope：`tauri-pilot aijia where --json` 取，记为 `$SCOPE`
3. 触发登录后的内置技能同步：`tauri-pilot aijia sync-builtin-skills`
4. 如果同步结果显示 `skill-creator` 或 `dingtalk-workspace` 在 `skipped` 中，记录为环境阻塞，不继续断言默认开启
5. 打开技能中心：`tauri-pilot aijia skill-center-open`
6. 切到「内置」页：`tauri-pilot aijia skill-center-tab --name 内置`
7. 读取技能中心列表快照：`tauri-pilot aijia skill-center-list --json`，记为 `$SKILL_LIST`
8. 返回首页并新建空对话：`tauri-pilot aijia new-task`
9. 打开聊天输入框的技能选择入口：`tauri-pilot aijia skill-picker-open --json`，记为 `$CHAT_SKILLS`

**验收标准**
- `$SKILL_LIST` 中存在 `id == "skill-creator"` 的技能项
- `$SKILL_LIST` 中 `skill-creator.enabled == true`
- `$SKILL_LIST` 中存在 `id == "dingtalk-workspace"` 的技能项
- `$SKILL_LIST` 中 `dingtalk-workspace.enabled == true`
- `$SKILL_LIST` 中 `skill-creator.source` 为 `global`、`tenant` 或 `builtin` 之一
- `$SKILL_LIST` 中 `dingtalk-workspace.source` 为 `global`、`tenant` 或 `builtin` 之一
- `$CHAT_SKILLS` 中存在 `id == "skill-creator"` 的技能项
- `$CHAT_SKILLS` 中存在 `id == "dingtalk-workspace"` 的技能项
- 市场中未添加的其他技能没有因为步骤 3 自动进入 `$SKILL_LIST`

---

## 意图-技能-020：关闭内置后，同步保持关闭

**场景**
用户可以关闭内置基础技能。关闭后即使再次触发登录同步或更新技能，系统也只能确保技能文件存在，不能覆盖用户关闭选择。

**操作步骤**
1. 应用探活：`tauri-pilot aijia health-check`
2. 推断当前 scope：`tauri-pilot aijia where --json` 取，记为 `$SCOPE`
3. 触发登录后的内置技能同步：`tauri-pilot aijia sync-builtin-skills`
4. 打开技能中心：`tauri-pilot aijia skill-center-open`
5. 切到「内置」页：`tauri-pilot aijia skill-center-tab --name 内置`，等待列表中出现 `dingtalk-workspace`
6. 关闭 `dingtalk-workspace`：`tauri-pilot aijia skill-center-toggle --id dingtalk-workspace --enabled false`
7. 再次触发内置技能同步：`tauri-pilot aijia sync-builtin-skills`
8. 读取技能中心列表快照：`tauri-pilot aijia skill-center-list --json`，记为 `$SKILL_LIST`
9. 返回首页并新建空对话：`tauri-pilot aijia new-task`
10. 打开聊天输入框的技能选择入口：`tauri-pilot aijia skill-picker-open --json`，记为 `$CHAT_SKILLS`

**验收标准**
- `~/.renlijia/users/{scope}/skillsConfig.json` 中 `disabledSkillIds` 包含 `dingtalk-workspace`
- `$SKILL_LIST` 中存在 `id == "dingtalk-workspace"` 的技能项
- `$SKILL_LIST` 中 `dingtalk-workspace.enabled == false`
- `$CHAT_SKILLS` 中不存在 `id == "dingtalk-workspace"` 的技能项
- 再次同步后 `~/.renlijia/skills/dingtalk-workspace/SKILL.md` 仍存在
- 再次同步后 `dingtalk-workspace` 没有从 `disabledSkillIds` 中被删除

---

## 意图-技能-021：未添加市场技能，聊天不可使用

**场景**
市场里的企业/平台技能默认只是可发现目录，不是已安装技能。用户未点击「+」之前，它不能进入聊天输入框，也不能进入模型可用技能目录；即使用户直接在对话里念技能 id，`Skill` 工具也不能加载它。

**操作步骤**
1. 应用探活：`tauri-pilot aijia health-check`
2. 推断当前 scope：`tauri-pilot aijia where --json` 取，记为 `$SCOPE`
3. 打开技能中心：`tauri-pilot aijia skill-center-open`
4. 切到「市场」页：`tauri-pilot aijia skill-center-tab --name 市场`，读取市场列表快照：`tauri-pilot aijia skill-market-list --json`，选择一个 `installed == false` 的技能，记为 `$MARKET_ONLY_ID`；如果没有这样的技能，本意图记为环境阻塞/跳过
5. 读取技能中心列表快照：`tauri-pilot aijia skill-center-list --json`，记为 `$SKILL_LIST`
6. 返回首页并新建空对话：`tauri-pilot aijia new-task`，记当前会话为 `$CONV_ID`
7. 打开聊天输入框的技能选择入口：`tauri-pilot aijia skill-picker-open --json`，记为 `$CHAT_SKILLS`
8. 在输入框输入：`请使用 $MARKET_ONLY_ID 技能回应一下。`
9. `tauri-pilot aijia send` + `tauri-pilot aijia wait-reply --timeout 90`
10. 读取 `$CONV_ID/messages.jsonl`

**验收标准**
- 市场页 `$MARKET_ONLY_ID` 卡片展示「+」
- `$SKILL_LIST` 中不存在 `id == $MARKET_ONLY_ID` 的技能项
- `$CHAT_SKILLS` 中不存在 `id == $MARKET_ONLY_ID` 的技能项
- `~/.renlijia/skills/$MARKET_ONLY_ID/SKILL.md` 不存在
- `~/.renlijia/users/{scope}/skills/$MARKET_ONLY_ID/SKILL.md` 不存在
- `$CONV_ID/messages.jsonl` 中不存在 `role == "tool"` 且内容包含 `$MARKET_ONLY_ID` 的 SKILL.md body 文本
- 如果 `$CONV_ID/messages.jsonl` 中出现参数包含 `$MARKET_ONLY_ID` 的 `Skill` 调用，紧随其后的 tool record `isError == true`

---

## 意图-技能-022：添加市场技能后，聊天可使用

**场景**
市场里的企业/平台技能默认不可用。用户点击某个市场技能卡片右上角「+」完成添加后，该技能进入已安装集合并默认开启，随后聊天输入框可以选择它，新对话也可以通过 `Skill` 工具加载它。

**操作步骤**
1. 应用探活：`tauri-pilot aijia health-check`
2. 推断当前 scope：`tauri-pilot aijia where --json` 取，记为 `$SCOPE`
3. 打开技能中心：`tauri-pilot aijia skill-center-open`
4. 切到「市场」页：`tauri-pilot aijia skill-center-tab --name 市场`，读取市场列表快照：`tauri-pilot aijia skill-market-list --json`，选择一个 `installed == false` 的技能，记为 `$MARKET_INSTALL_ID`；如果没有这样的技能，本意图记为环境阻塞/跳过
5. 添加市场技能：`tauri-pilot aijia skill-market-add --id $MARKET_INSTALL_ID`，等待安装完成
6. 读取技能中心列表快照：`tauri-pilot aijia skill-center-list --json`，记为 `$SKILL_LIST`
7. 返回首页并新建空对话：`tauri-pilot aijia new-task`，记当前会话为 `$CONV_ID`
8. 打开聊天输入框的技能选择入口：`tauri-pilot aijia skill-picker-open --json`，记为 `$CHAT_SKILLS`
9. 在输入框输入：`请使用 $MARKET_INSTALL_ID 技能回应一下。`
10. `tauri-pilot aijia send` + `tauri-pilot aijia wait-reply --timeout 90`
11. 读取 `$CONV_ID/messages.jsonl`

**验收标准**
- 市场页 `$MARKET_INSTALL_ID` 卡片展示「已添加」
- `~/.renlijia/users/{scope}/skills/$MARKET_INSTALL_ID/SKILL.md` 存在
- `$SKILL_LIST` 中存在 `id == $MARKET_INSTALL_ID` 的技能项
- `$SKILL_LIST` 中 `$MARKET_INSTALL_ID.enabled == true`
- 如果 `~/.renlijia/users/{scope}/skillsConfig.json` 存在，文件中 `disabledSkillIds` 不包含 `$MARKET_INSTALL_ID`
- `$CHAT_SKILLS` 中存在 `id == $MARKET_INSTALL_ID` 的技能项
- `$CONV_ID/messages.jsonl` 中存在 `toolCalls[].name == "Skill"` 且参数包含 `$MARKET_INSTALL_ID` 的调用
- `$CONV_ID/messages.jsonl` 中不存在 `Skill($MARKET_INSTALL_ID)` 返回 `Unknown or unavailable skill` / `not found` / `已关闭`

---

## 意图-技能-023：更新官方后，未添加不安装

**场景**
用户点击“更新官方技能”时，市场里的未添加技能仍然只是可发现目录。更新动作只能更新必需内置和本地已安装技能，不能把未添加的企业/平台技能安装进本地，也不能让它进入聊天可用技能列表。

**操作步骤**
1. 应用探活：`tauri-pilot aijia health-check`
2. 推断当前 scope：`tauri-pilot aijia where --json` 取，记为 `$SCOPE`
3. 打开技能中心：`tauri-pilot aijia skill-center-open`
4. 切到「市场」页：`tauri-pilot aijia skill-center-tab --name 市场`，读取市场列表快照：`tauri-pilot aijia skill-market-list --json`，选择一个 `installed == false` 的技能，记为 `$MARKET_UPDATE_ONLY_ID`；如果没有这样的技能，本意图记为环境阻塞/跳过
5. 触发更新官方技能：`tauri-pilot aijia sync-builtin-skills`
6. 再次读取市场列表快照：`tauri-pilot aijia skill-market-list --json`，记为 `$MARKET_AFTER`
7. 读取技能中心列表快照：`tauri-pilot aijia skill-center-list --json`，记为 `$SKILL_LIST`
8. 返回首页并新建空对话：`tauri-pilot aijia new-task`，记当前会话为 `$CONV_ID`
9. 打开聊天输入框的技能选择入口：`tauri-pilot aijia skill-picker-open --json`，记为 `$CHAT_SKILLS`
10. 在输入框输入：`请使用 $MARKET_UPDATE_ONLY_ID 技能回应一下。`
11. `tauri-pilot aijia send` + `tauri-pilot aijia wait-reply --timeout 90`
12. 读取 `$CONV_ID/messages.jsonl`

**验收标准**
- `$MARKET_AFTER` 中 `$MARKET_UPDATE_ONLY_ID.installed == false`
- 市场页 `$MARKET_UPDATE_ONLY_ID` 卡片展示「+」
- `$SKILL_LIST` 中不存在 `id == $MARKET_UPDATE_ONLY_ID` 的技能项
- `$CHAT_SKILLS` 中不存在 `id == $MARKET_UPDATE_ONLY_ID` 的技能项
- `~/.renlijia/skills/$MARKET_UPDATE_ONLY_ID/SKILL.md` 不存在
- `~/.renlijia/users/{scope}/skills/$MARKET_UPDATE_ONLY_ID/SKILL.md` 不存在
- `$CONV_ID/messages.jsonl` 中不存在 `role == "tool"` 且内容包含 `$MARKET_UPDATE_ONLY_ID` 的 SKILL.md body 文本
- 如果 `$CONV_ID/messages.jsonl` 中出现参数包含 `$MARKET_UPDATE_ONLY_ID` 的 `Skill` 调用，紧随其后的 tool record `isError == true`

---

## 意图-技能-024：更新官方后，关闭仍保留

**场景**
用户关闭内置技能后，再点击“更新官方技能”，系统可以更新技能文件，但不能重新打开该技能。关闭状态仍写在当前登录用户的 `skillsConfig.json`，聊天输入框和模型技能目录也不能出现它。

**操作步骤**
1. 应用探活：`tauri-pilot aijia health-check`
2. 推断当前 scope：`tauri-pilot aijia where --json` 取，记为 `$SCOPE`
3. 触发登录后的内置技能同步：`tauri-pilot aijia sync-builtin-skills`
4. 打开技能中心：`tauri-pilot aijia skill-center-open`
5. 切到「内置」页：`tauri-pilot aijia skill-center-tab --name 内置`，等待列表中出现 `dingtalk-workspace`
6. 关闭 `dingtalk-workspace`：`tauri-pilot aijia skill-center-toggle --id dingtalk-workspace --enabled false`
7. 触发更新官方技能：`tauri-pilot aijia sync-builtin-skills`
8. 读取技能中心列表快照：`tauri-pilot aijia skill-center-list --json`，记为 `$SKILL_LIST`
9. 返回首页并新建空对话：`tauri-pilot aijia new-task`，记当前会话为 `$CONV_ID`
10. 打开聊天输入框的技能选择入口：`tauri-pilot aijia skill-picker-open --json`，记为 `$CHAT_SKILLS`
11. 在输入框输入：`请使用 dingtalk-workspace 技能回应一下。`
12. `tauri-pilot aijia send` + `tauri-pilot aijia wait-reply --timeout 90`
13. 读取 `$CONV_ID/messages.jsonl`

**验收标准**
- `~/.renlijia/users/{scope}/skillsConfig.json` 中 `disabledSkillIds` 包含 `dingtalk-workspace`
- `~/.renlijia/skills/dingtalk-workspace/SKILL.md` 存在
- `$SKILL_LIST` 中存在 `id == "dingtalk-workspace"` 的技能项
- `$SKILL_LIST` 中 `dingtalk-workspace.enabled == false`
- `$CHAT_SKILLS` 中不存在 `id == "dingtalk-workspace"` 的技能项
- `$CONV_ID/messages.jsonl` 中不存在 `role == "tool"` 且内容包含 `dingtalk-workspace` 的 SKILL.md body 文本
- 如果 `$CONV_ID/messages.jsonl` 中出现参数包含 `dingtalk-workspace` 的 `Skill` 调用，紧随其后的 tool record `isError == true`
- 更新官方技能后 `dingtalk-workspace` 没有从 `disabledSkillIds` 中被删除
