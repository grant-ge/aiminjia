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
- 场景 13：重新开启已关闭技能后，聊天输入框技能选择、slash 候选、详情页使用入口、模型 catalog 与 `Skill` 工具恢复可用
- 场景 14：`skillsConfig.json` 按当前登录账号 scope 隔离，账号 A 的关闭状态不影响账号 B
- 场景 15：发现技能作为内置技能同步到本地后默认开启
- 场景 16：关闭发现技能后，自动发现与安装市场技能的入口不可用
- 场景 17：用户提出自然任务且本地缺少能力时，可以通过发现技能搜索并安装市场技能
- 场景 18：发现到唯一高置信候选时，可以自动安装并继续原任务
- 场景 19：市场无匹配技能时，不能安装无关技能
- 场景 20：技能已经安装时，不能重复安装同一技能
- 场景 21：通过自动发现安装的技能，后续关闭后仍然不能被聊天加载
- 场景 22：已安装的具名企业系统技能应直接加载，不绕市场
- 场景 23：普通网页任务应直接使用浏览器，不被发现技能过度抢占
- 场景 24：多个相近候选时，必须先问用户选择
- 场景 25：用户只描述员工查询目标、没有说技能或系统名时，仍能先发现人事技能
- 场景 26：用户只描述薪资组业务目标、没有说智能薪酬时，仍能先发现薪酬技能
- 场景 27：自动安装过的专用技能后续应直接加载，不重复搜索市场
- 场景 28：用户只说钉钉审批或待办时，已安装钉钉技能应直接生效
- 场景 29：未知业务系统没有匹配技能时，不能静默安装无关技能
- 场景 30：普通公开网页抓取仍走浏览器，不被市场发现拦截
- 场景 31：薪酬分析类候选不唯一时，必须先询问用户选择
- 场景 32：钉钉能力已安装并开启时，后续 dws 类任务直接复用已有技能和 CLI，不重复安装
- 场景 33：用户说钉钉命令不可用时，如需补齐 dws CLI，默认补到 AIjia 托管命令环境
- 场景 34：用户明确要求检查系统 PATH 里的 dws 时，使用 system 出口且不回落 AIjia 托管运行时

## 执行约束：技能启用状态改造

- 意图 16-26 默认依赖这些 `tauri-pilot aijia` 原子命令：`skill-center-open`、`skill-center-tab`、`skill-center-list --json`、`skill-market-list --json`、`skill-center-toggle --id --enabled`、`skill-market-add --id`、`skill-detail-open --id`、`skill-detail-snapshot --json`、`skill-picker-open --json`、`slash-suggestions --query --json`、`sync-builtin-skills`。如果命令缺失，runner 应标记为 `CLI gap`，不能把人工点按当成稳定自动化结果。
- 意图测试不得删除整个 `~/.renlijia/users/{scope}/skillsConfig.json`。需要准备开关状态时，只能通过产品入口或 CLI 对测试 skill id 设置开启/关闭，避免误删用户对其他技能的偏好。
- 选择市场测试技能时，必须优先使用 `skill-market-list --json` 中 `installed == false` 的项；如果当前环境没有未添加市场技能，本意图记为环境阻塞/跳过，不要卸载真实用户技能来造数据。
- 校验模型不可用不能只靠“LLM 没有调用”作为唯一证据；需要同时检查 catalog/技能入口不出现，或使用后端 focused test 证明 `Skill` 工具对 disabled/not-installed id 返回 unavailable。
- 涉及账号隔离的意图必须使用专用测试账号；如果没有两套可登录测试账号凭据，runner 应标记为环境阻塞/跳过，不得删除或改写当前真实用户的账号文件来造“新账号”现场。

## 执行约束：发现技能自动安装

- 意图 27-48 默认依赖这些 `tauri-pilot aijia` 原子命令：`skill-center-open`、`skill-center-tab`、`skill-center-list --json`、`skill-market-list --json --include-description`、`skill-center-toggle --id --enabled`、`skill-market-add --id`、`skill-picker-open --json`、`slash-suggestions --query --json`、`sync-builtin-skills`、`visible-tools --json`、`wait-agent-idle --timeout`、`pending-action-snapshot`、`dialog-snapshot`。如果命令缺失，runner 应标记为 `CLI gap`，不能把人工点按或肉眼观察当成稳定自动化结果。
- 自动发现测试需要企业市场中存在测试专用技能包：`find-skills-e2e-web-fetch`、`find-skills-e2e-choice-alpha`、`find-skills-e2e-choice-beta`、`find-skills-e2e-disable-after-install`。这些包缺失时，runner 应标记为环境阻塞，不得拿真实客户技能做安装、关闭或卸载实验。
- 自动发现测试不得删除整个 `skillsConfig.json`，不得删除用户已有真实技能目录；如需未安装状态，只能选择 `skill-market-list --json --include-description` 中 `installed == false` 的测试专用技能包。
- 真实市场技能评测只允许验证“搜索、是否询问、是否直接加载已安装技能”；除非意图明确写出目标测试技能并确认 `installed == false`，不得用真实客户技能做关闭、卸载或重复安装实验。
- 长工具链不能只依赖 `wait-reply` 判断结束；必须使用 `wait-agent-idle --timeout`，或轮询当前会话 `messages.jsonl` 中最后一条 assistant 消息的完成状态。
- 校验“关闭后不占工具上下文”时，优先使用 `visible-tools --json` 读取当前会话实际注入的工具名；不能只用 UI 开关状态推断。

## 用户视角评测口径

find-skills 的评测不能只测“用户明确说要找技能”或“用户点名 pluginId”。真实用户更常见的是直接说业务目标，所以评测集按下面几类话术覆盖：

| 话术类型 | 用户通常会说 | 期望链路 |
|---|---|---|
| 业务目标型 | “查下王小卡在哪个部门，岗位是什么” | 本地没有人事专用技能时，先 `Skill(find-skills)` 搜市场，再安装/加载 `rehcm` |
| 业务对象型 | “这个月哪些薪资组已经生成了” | 本地没有薪酬专用技能时，先搜索市场，再安装/加载 `smartcb` |
| 产品别名型 | “睿认识里查一下人” / “智能薪酬看下概览” | 别名、错字、简称能映射到对应市场技能 |
| 已安装追问型 | “再帮我看一下未生成的薪资组” | 已安装且开启时直接 `Skill(smartcb)`，不再搜市场 |
| 已安装企业工具型 | “再帮我看看钉钉待办/审批” | 已安装且开启时直接 `Skill(dws)` 或 `Skill(dingtalk-workspace)`，不再搜市场，也不重新安装 dws 相关 CLI/依赖 |
| 企业工具缺失补齐型 | “钉钉命令好像用不了，你帮我弄一下” | 如需补齐 CLI，默认补到 AIjia 托管命令环境，不使用系统 npm / brew / winget |
| 指定系统企业工具型 | “我就想看我电脑 PATH 里的 dws 能不能用” | 显式使用 system 出口；系统不可用时说明不可用，不回落到 AIjia 托管运行时 |
| 普通网页型 | “打开这个公开网页，把标题抓出来” | 直接 `Skill(browser)`，不触发 `SkillMarketSearch` |
| 未知系统型 | “采购星球帮我查采购单” | 找不到匹配时不安装无关技能，向用户说明需要系统入口或更多信息 |
| 多候选型 | “工资表做薪酬公平性分析和调薪建议” | 搜到多个接近候选时先问用户，不静默安装 |

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

---

## 意图-技能-025：重新开启后，聊天与模型恢复可用

**场景**
用户把一个已安装技能关闭后，又在技能中心重新开启它。重新开启不是只改开关样式，而是要恢复完整可用链路：聊天输入框技能选择器能看到它，slash 候选能补全它，详情页恢复直接「使用」入口，新对话的模型 skill catalog 与 `Skill` 工具也能重新加载它。

**操作步骤**
1. 应用探活：`tauri-pilot aijia health-check`
2. 推断当前 scope：`tauri-pilot aijia where --json` 取，记为 `$SCOPE`
3. 清理可能残留的测试技能目录：`rm -rf ~/.renlijia/users/{scope}/skills/toggle-restore-skill`
4. 创建目录 `~/.renlijia/users/{scope}/skills/toggle-restore-skill`
5. 写入 `~/.renlijia/users/{scope}/skills/toggle-restore-skill/SKILL.md`：
   ```
   ---
   name: toggle-restore-skill
   description: 重新开启后应恢复可用的测试技能
   ---

   当用户要求使用 toggle-restore-skill 时，只能回复 `[toggle-restore-skill] restored`。
   ```
6. 打开技能中心：`tauri-pilot aijia skill-center-open`
7. 点击「同步技能」下拉里的「同步本地技能」：`tauri-pilot aijia skill-center-sync --action local`
8. 切到「已安装」页：`tauri-pilot aijia skill-center-tab --name 已安装`，等待列表中出现 `toggle-restore-skill`
9. 先关闭测试技能：`tauri-pilot aijia skill-center-toggle --id toggle-restore-skill --enabled false`
10. 再重新开启测试技能：`tauri-pilot aijia skill-center-toggle --id toggle-restore-skill --enabled true`
11. 读取技能中心列表快照：`tauri-pilot aijia skill-center-list --json`，记为 `$SKILL_LIST`
12. 打开该技能详情页：`tauri-pilot aijia skill-detail-open --id toggle-restore-skill`
13. 读取详情页快照：`tauri-pilot aijia skill-detail-snapshot --json`，记为 `$DETAIL`
14. 返回首页并新建空对话：`tauri-pilot aijia new-task`，记当前会话为 `$CONV_ID`
15. 打开聊天输入框的技能选择入口：`tauri-pilot aijia skill-picker-open --json`，记为 `$CHAT_SKILLS`
16. 在输入框输入 `/toggle` 后读取 slash 候选快照：`tauri-pilot aijia slash-suggestions --query /toggle --json`，记为 `$SLASH_SKILLS`
17. 在输入框输入：`请使用 toggle-restore-skill 技能回应，只要加载成功就输出它要求的固定文本。`
18. `tauri-pilot aijia send` + `tauri-pilot aijia wait-reply --timeout 90`
19. 读取 `$CONV_ID/messages.jsonl`

**验收标准**
- `~/.renlijia/users/{scope}/skills/toggle-restore-skill/SKILL.md` 存在
- `~/.renlijia/users/{scope}/skillsConfig.json` 不存在，或其中 `disabledSkillIds` 不包含 `toggle-restore-skill`
- `$SKILL_LIST` 中存在 `id == "toggle-restore-skill"` 的技能项
- `$SKILL_LIST` 中 `toggle-restore-skill.enabled == true`
- `$DETAIL` 的主操作为「使用」
- `$DETAIL` 不展示「开启并使用」
- `$CHAT_SKILLS` 中存在 `id == "toggle-restore-skill"` 的技能项
- `$SLASH_SKILLS` 中存在命令为 `/toggle-restore-skill` 的候选项
- `$CONV_ID/messages.jsonl` 中存在 `toolCalls[].name == "Skill"` 且参数包含 `toggle-restore-skill` 的调用
- `$CONV_ID/messages.jsonl` 中对应的 `role == "tool"` 记录内容包含 `[toggle-restore-skill] restored`
- `$CONV_ID/messages.jsonl` 中不存在 `Skill(toggle-restore-skill)` 返回 `Unknown or unavailable skill` / `not found` / `已关闭`

---

## 意图-技能-026：账号切换后，skillsConfig 配置互不污染

**场景**
同一台电脑上登录过多个账号时，每个账号都有自己的 `~/.renlijia/users/{scope}/skillsConfig.json`。账号 A 关闭某个全局或内置技能后，账号 B 登录不应该继承 A 的关闭状态；切回账号 A 时，A 的关闭状态仍然保留。

**操作步骤**
1. 应用探活：`tauri-pilot aijia health-check`
2. 确认当前是专用测试账号 A；如果不是专用测试账号，runner 应标记为环境阻塞/跳过
3. 推断账号 A scope：`tauri-pilot aijia where --json` 取，记为 `$SCOPE_A`
4. 触发登录后的内置技能同步：`tauri-pilot aijia sync-builtin-skills`
5. 打开技能中心：`tauri-pilot aijia skill-center-open`
6. 切到「内置」页：`tauri-pilot aijia skill-center-tab --name 内置`，等待列表中出现 `dingtalk-workspace`
7. 在账号 A 关闭 `dingtalk-workspace`：`tauri-pilot aijia skill-center-toggle --id dingtalk-workspace --enabled false`
8. 读取账号 A 的 `~/.renlijia/users/{scope_a}/skillsConfig.json`
9. 打开设置并退出登录：`tauri-pilot aijia open-settings` + `tauri-pilot aijia settings-select-panel --key account` + `tauri-pilot aijia logout`
10. 登录专用测试账号 B：`tauri-pilot aijia login --account $AIJIA_E2E_ACCOUNT_B --password $AIJIA_E2E_PASSWORD_B --json`
11. 推断账号 B scope：`tauri-pilot aijia where --json` 取，记为 `$SCOPE_B`
12. 触发账号 B 的内置技能同步：`tauri-pilot aijia sync-builtin-skills`
13. 打开技能中心并切到「内置」页，读取技能中心列表快照：`tauri-pilot aijia skill-center-list --json`，记为 `$SKILL_LIST_B`
14. 返回首页并新建空对话，打开聊天输入框技能选择入口：`tauri-pilot aijia skill-picker-open --json`，记为 `$CHAT_SKILLS_B`
15. 打开设置并退出账号 B，再登录回专用测试账号 A
16. 推断切回后的 scope，记为 `$SCOPE_A_AGAIN`
17. 读取账号 A 的技能中心列表快照：`tauri-pilot aijia skill-center-list --json`，记为 `$SKILL_LIST_A_AGAIN`

**验收标准**
- `$SCOPE_A` 与 `$SCOPE_B` 不相等
- `$SCOPE_A_AGAIN == $SCOPE_A`
- `~/.renlijia/users/{scope_a}/skillsConfig.json` 中 `disabledSkillIds` 包含 `dingtalk-workspace`
- `~/.renlijia/users/{scope_b}/skillsConfig.json` 不存在，或其中 `disabledSkillIds` 不包含 `dingtalk-workspace`
- `$SKILL_LIST_B` 中存在 `id == "dingtalk-workspace"` 的技能项
- `$SKILL_LIST_B` 中 `dingtalk-workspace.enabled == true`
- `$CHAT_SKILLS_B` 中存在 `id == "dingtalk-workspace"` 的技能项
- `$SKILL_LIST_A_AGAIN` 中存在 `id == "dingtalk-workspace"` 的技能项
- `$SKILL_LIST_A_AGAIN` 中 `dingtalk-workspace.enabled == false`
- `~/.renlijia/global/skillsConfig.json` 不存在

---

## 意图-技能-027：发现技能内置后，默认开启

**场景**
用户登录后，系统同步必需内置技能。发现技能是一个真实的内置技能包，应该安装到本地、默认开启，并让新对话具备自动发现与安装市场技能的能力。

**操作步骤**
1. 应用探活：`tauri-pilot aijia health-check`
2. 推断当前 scope：`tauri-pilot aijia where --json` 取，记为 `$SCOPE`
3. 打开技能中心：`tauri-pilot aijia skill-center-open`
4. 触发登录后的内置技能同步：`tauri-pilot aijia sync-builtin-skills`
5. 切到「内置」页：`tauri-pilot aijia skill-center-tab --name 内置`
6. 读取技能中心列表快照：`tauri-pilot aijia skill-center-list --json`，记为 `$SKILL_LIST`
7. 返回首页并新建空对话：`tauri-pilot aijia new-task`
8. 读取当前会话可见工具快照：`tauri-pilot aijia visible-tools --json`，记为 `$VISIBLE_TOOLS`
9. 读取 `~/.renlijia/users/{scope}/skillsConfig.json`

**验收标准**
- `~/.renlijia/skills/find-skills/SKILL.md` 存在
- `$SKILL_LIST` 中存在 `id == "find-skills"` 的技能项
- `$SKILL_LIST` 中 `find-skills.source == "builtin"`、`find-skills.source == "global"` 或 `find-skills.category == "builtin"`
- `$SKILL_LIST` 中 `find-skills.enabled == true`
- `$VISIBLE_TOOLS` 中存在 `name == "SkillMarketSearch"` 的工具
- `$VISIBLE_TOOLS` 中存在 `name == "SkillMarketInstall"` 的工具
- `~/.renlijia/users/{scope}/skillsConfig.json` 不存在，或其中 `disabledSkillIds` 不包含 `find-skills`
- 技能中心「内置」页中 `find-skills` 的开关为开启状态

---

## 意图-技能-028：关闭发现技能后，自动发现不可用

**场景**
用户可以关闭发现技能。关闭后不只是技能中心开关变灰，而是新对话不再注入市场搜索和安装工具，聊天入口也不再展示发现技能。

**操作步骤**
1. 应用探活：`tauri-pilot aijia health-check`
2. 推断当前 scope：`tauri-pilot aijia where --json` 取，记为 `$SCOPE`
3. 打开技能中心：`tauri-pilot aijia skill-center-open`
4. 触发登录后的内置技能同步：`tauri-pilot aijia sync-builtin-skills`
5. 切到「内置」页：`tauri-pilot aijia skill-center-tab --name 内置`
6. 关闭 `find-skills`：`tauri-pilot aijia skill-center-toggle --id find-skills --enabled false`
7. 返回首页并新建空对话：`tauri-pilot aijia new-task`，记当前会话为 `$CONV_ID`
8. 读取当前会话可见工具快照：`tauri-pilot aijia visible-tools --json`，记为 `$VISIBLE_TOOLS`
9. 打开聊天输入框的技能选择入口：`tauri-pilot aijia skill-picker-open --json`，记为 `$CHAT_SKILLS`
10. 在输入框输入：`请帮我完成 find-skills-e2e-web-fetch 场景；如果当前没有对应能力，请自己查找可安装技能。`
11. `tauri-pilot aijia send`
12. 等待对话结束：`tauri-pilot aijia wait-agent-idle --timeout 180`
13. 读取 `$CONV_ID/messages.jsonl`

**验收标准**
- `~/.renlijia/users/{scope}/skillsConfig.json` 中 `disabledSkillIds` 包含 `find-skills`
- `$CHAT_SKILLS` 中不存在 `id == "find-skills"` 的技能项
- `$VISIBLE_TOOLS` 中不存在 `name == "SkillMarketSearch"`
- `$VISIBLE_TOOLS` 中不存在 `name == "SkillMarketInstall"`
- `$CONV_ID/messages.jsonl` 中不存在 `toolCalls[].name == "SkillMarketSearch"` 的调用
- `$CONV_ID/messages.jsonl` 中不存在 `toolCalls[].name == "SkillMarketInstall"` 的调用
- `~/.renlijia/users/{scope}/skills/find-skills-e2e-web-fetch/SKILL.md` 不存在
- `~/.renlijia/skills/find-skills-e2e-web-fetch/SKILL.md` 不存在

---

## 意图-技能-029：缺少能力时，自动添加市场技能

**场景**
用户不会说“有没有浏览器相关技能”，只会提出一个自然任务。当前本地没有对应技能时，Agent 可以先加载发现技能，再搜索企业市场、安装匹配技能，并继续使用新安装的技能完成任务。

**操作步骤**
1. 应用探活：`tauri-pilot aijia health-check`
2. 推断当前 scope：`tauri-pilot aijia where --json` 取，记为 `$SCOPE`
3. 打开技能中心：`tauri-pilot aijia skill-center-open`
4. 触发登录后的内置技能同步：`tauri-pilot aijia sync-builtin-skills`
5. 切到「内置」页：`tauri-pilot aijia skill-center-tab --name 内置`
6. 开启 `find-skills`：`tauri-pilot aijia skill-center-toggle --id find-skills --enabled true`
7. 切到「市场」页：`tauri-pilot aijia skill-center-tab --name 市场`
8. 读取市场列表快照：`tauri-pilot aijia skill-market-list --json --include-description`，确认 `find-skills-e2e-web-fetch.installed == false`；如果该测试专用技能不存在或已安装，本意图记为环境阻塞/跳过
9. 返回首页并新建空对话：`tauri-pilot aijia new-task`，记当前会话为 `$CONV_ID`
10. 读取当前会话可见工具快照：`tauri-pilot aijia visible-tools --json`，记为 `$VISIBLE_TOOLS`
11. 在输入框输入：`请完成 find-skills-e2e-web-fetch 场景：访问示例网页并提取标题。如果当前没有对应能力，请自己查找可安装技能。`
12. `tauri-pilot aijia send`
13. 等待对话结束：`tauri-pilot aijia wait-agent-idle --timeout 300`
14. 读取 `$CONV_ID/messages.jsonl`
15. 读取技能中心列表快照：`tauri-pilot aijia skill-center-list --json`，记为 `$SKILL_LIST_AFTER`

**验收标准**
- `$VISIBLE_TOOLS` 中存在 `name == "SkillMarketSearch"` 的工具
- `$VISIBLE_TOOLS` 中存在 `name == "SkillMarketInstall"` 的工具
- `$CONV_ID/messages.jsonl` 中存在 `toolCalls[].name == "Skill"` 且参数包含 `find-skills` 的调用
- `$CONV_ID/messages.jsonl` 中存在 `toolCalls[].name == "SkillMarketSearch"` 且参数包含 `find-skills-e2e-web-fetch` 的调用
- `$CONV_ID/messages.jsonl` 中存在 `toolCalls[].name == "SkillMarketInstall"` 且参数包含 `find-skills-e2e-web-fetch` 的调用
- `~/.renlijia/users/{scope}/skills/find-skills-e2e-web-fetch/SKILL.md` 存在，或 `~/.renlijia/skills/find-skills-e2e-web-fetch/SKILL.md` 存在
- `$SKILL_LIST_AFTER` 中存在 `id == "find-skills-e2e-web-fetch"` 的技能项
- `$SKILL_LIST_AFTER` 中 `find-skills-e2e-web-fetch.enabled == true`
- `$CONV_ID/messages.jsonl` 中存在 `toolCalls[].name == "Skill"` 且参数包含 `find-skills-e2e-web-fetch` 的调用
- `$CONV_ID/messages.jsonl` 中对应的 tool record 内容包含 `[find-skills-e2e-web-fetch]`

---

## 意图-技能-030：候选不唯一时，先询问用户

**场景**
市场搜索可能返回多个可用技能。候选不唯一时，Agent 不能静默挑一个安装；必须先向用户确认要安装哪个技能。

**操作步骤**
1. 应用探活：`tauri-pilot aijia health-check`
2. 推断当前 scope：`tauri-pilot aijia where --json` 取，记为 `$SCOPE`
3. 打开技能中心：`tauri-pilot aijia skill-center-open`
4. 触发登录后的内置技能同步：`tauri-pilot aijia sync-builtin-skills`
5. 切到「内置」页：`tauri-pilot aijia skill-center-tab --name 内置`
6. 开启 `find-skills`：`tauri-pilot aijia skill-center-toggle --id find-skills --enabled true`
7. 切到「市场」页：`tauri-pilot aijia skill-center-tab --name 市场`
8. 读取市场列表快照：`tauri-pilot aijia skill-market-list --json --include-description`，确认 `find-skills-e2e-choice-alpha.installed == false` 且 `find-skills-e2e-choice-beta.installed == false`；如果任一测试专用技能不存在或已安装，本意图记为环境阻塞/跳过
9. 返回首页并新建空对话：`tauri-pilot aijia new-task`，记当前会话为 `$CONV_ID`
10. 在输入框输入：`请完成 find-skills-e2e-choice 场景。这个任务可能有多个市场技能能处理，请在安装前让我选择。`
11. `tauri-pilot aijia send`
12. 等待对话进入等待用户确认状态：`tauri-pilot aijia wait-agent-idle --timeout 180`
13. 读取 `$CONV_ID/messages.jsonl`

**验收标准**
- `$CONV_ID/messages.jsonl` 中存在 `toolCalls[].name == "Skill"` 且参数包含 `find-skills` 的调用
- `$CONV_ID/messages.jsonl` 中存在 `toolCalls[].name == "SkillMarketSearch"` 且参数包含 `find-skills-e2e-choice` 的调用
- `$CONV_ID/messages.jsonl` 中存在面向用户的确认消息，内容同时包含 `find-skills-e2e-choice-alpha` 与 `find-skills-e2e-choice-beta`
- 在上述确认消息之前，`$CONV_ID/messages.jsonl` 中不存在 `toolCalls[].name == "SkillMarketInstall"` 的调用
- `~/.renlijia/users/{scope}/skills/find-skills-e2e-choice-alpha/SKILL.md` 不存在
- `~/.renlijia/users/{scope}/skills/find-skills-e2e-choice-beta/SKILL.md` 不存在
- `~/.renlijia/skills/find-skills-e2e-choice-alpha/SKILL.md` 不存在
- `~/.renlijia/skills/find-skills-e2e-choice-beta/SKILL.md` 不存在

---

## 意图-技能-031：市场无匹配时，不安装技能

**场景**
用户提出一个没有任何市场技能能覆盖的任务。Agent 可以尝试发现技能，但不能把无关技能安装到本地。

**操作步骤**
1. 应用探活：`tauri-pilot aijia health-check`
2. 推断当前 scope：`tauri-pilot aijia where --json` 取，记为 `$SCOPE`
3. 打开技能中心：`tauri-pilot aijia skill-center-open`
4. 触发登录后的内置技能同步：`tauri-pilot aijia sync-builtin-skills`
5. 切到「内置」页：`tauri-pilot aijia skill-center-tab --name 内置`
6. 开启 `find-skills`：`tauri-pilot aijia skill-center-toggle --id find-skills --enabled true`
7. 返回首页并新建空对话：`tauri-pilot aijia new-task`，记当前会话为 `$CONV_ID`
8. 在输入框输入：`请完成 __aijia_find_skills_no_match_9f3b__ 场景；如果本地没有对应能力，请查找市场技能，但不要安装无关技能。`
9. `tauri-pilot aijia send`
10. 等待对话结束：`tauri-pilot aijia wait-agent-idle --timeout 180`
11. 读取 `$CONV_ID/messages.jsonl`
12. 读取技能中心列表快照：`tauri-pilot aijia skill-center-list --json`，记为 `$SKILL_LIST_AFTER`

**验收标准**
- `$CONV_ID/messages.jsonl` 中存在 `toolCalls[].name == "Skill"` 且参数包含 `find-skills` 的调用
- `$CONV_ID/messages.jsonl` 中存在 `toolCalls[].name == "SkillMarketSearch"` 且参数包含 `__aijia_find_skills_no_match_9f3b__` 的调用
- `$CONV_ID/messages.jsonl` 中不存在 `toolCalls[].name == "SkillMarketInstall"` 的调用
- `$SKILL_LIST_AFTER` 中不存在 `id` 包含 `__aijia_find_skills_no_match_9f3b__` 的技能项
- `$CONV_ID/messages.jsonl` 中最后一条 assistant 消息包含 `未找到`、`没有找到` 或 `暂无合适`
- `~/.renlijia/users/{scope}/skills/__aijia_find_skills_no_match_9f3b__/SKILL.md` 不存在
- `~/.renlijia/skills/__aijia_find_skills_no_match_9f3b__/SKILL.md` 不存在

---

## 意图-技能-032：技能已安装时，不重复安装

**场景**
用户已经安装过一个市场技能。之后再次提出匹配该技能的任务时，系统可以直接使用已安装技能，不能再次执行同一个技能包的安装动作。

**操作步骤**
1. 应用探活：`tauri-pilot aijia health-check`
2. 推断当前 scope：`tauri-pilot aijia where --json` 取，记为 `$SCOPE`
3. 打开技能中心：`tauri-pilot aijia skill-center-open`
4. 触发登录后的内置技能同步：`tauri-pilot aijia sync-builtin-skills`
5. 切到「内置」页：`tauri-pilot aijia skill-center-tab --name 内置`
6. 开启 `find-skills`：`tauri-pilot aijia skill-center-toggle --id find-skills --enabled true`
7. 切到「市场」页：`tauri-pilot aijia skill-center-tab --name 市场`
8. 通过市场入口添加测试技能：`tauri-pilot aijia skill-market-add --id find-skills-e2e-web-fetch`
9. 读取 `find-skills-e2e-web-fetch/SKILL.md` 的文件路径与最后修改时间，记为 `$SKILL_FILE_BEFORE`、`$SKILL_MTIME_BEFORE`
10. 返回首页并新建空对话：`tauri-pilot aijia new-task`，记当前会话为 `$CONV_ID`
11. 在输入框输入：`请使用 find-skills-e2e-web-fetch 场景访问示例网页并提取标题。`
12. `tauri-pilot aijia send`
13. 等待对话结束：`tauri-pilot aijia wait-agent-idle --timeout 180`
14. 读取 `$CONV_ID/messages.jsonl`
15. 再次读取 `find-skills-e2e-web-fetch/SKILL.md` 的最后修改时间，记为 `$SKILL_MTIME_AFTER`

**验收标准**
- `$SKILL_FILE_BEFORE` 存在
- `$CONV_ID/messages.jsonl` 中不存在 `toolCalls[].name == "SkillMarketInstall"` 且参数包含 `find-skills-e2e-web-fetch` 的调用
- `$SKILL_MTIME_AFTER == $SKILL_MTIME_BEFORE`
- `$CONV_ID/messages.jsonl` 中存在 `toolCalls[].name == "Skill"` 且参数包含 `find-skills-e2e-web-fetch` 的调用
- `$CONV_ID/messages.jsonl` 中对应的 tool record 内容包含 `[find-skills-e2e-web-fetch]`
- 技能中心列表中 `find-skills-e2e-web-fetch` 只有一个技能项

---

## 意图-技能-033：自动添加后，关闭仍生效

**场景**
发现技能自动安装市场技能后，该技能仍然受技能中心开关控制。用户关闭这个新技能后，聊天入口、模型 catalog 与 `Skill` 工具都不能绕过关闭状态继续加载它，也不能通过发现技能再次安装同一个已关闭技能来绕过用户选择。

**操作步骤**
1. 应用探活：`tauri-pilot aijia health-check`
2. 推断当前 scope：`tauri-pilot aijia where --json` 取，记为 `$SCOPE`
3. 打开技能中心：`tauri-pilot aijia skill-center-open`
4. 触发登录后的内置技能同步：`tauri-pilot aijia sync-builtin-skills`
5. 切到「内置」页：`tauri-pilot aijia skill-center-tab --name 内置`
6. 开启 `find-skills`：`tauri-pilot aijia skill-center-toggle --id find-skills --enabled true`
7. 切到「市场」页：`tauri-pilot aijia skill-center-tab --name 市场`
8. 读取市场列表快照：`tauri-pilot aijia skill-market-list --json --include-description`，确认 `find-skills-e2e-disable-after-install.installed == false`；如果该测试专用技能不存在或已安装，本意图记为环境阻塞/跳过
9. 返回首页并新建空对话：`tauri-pilot aijia new-task`，记安装会话为 `$INSTALL_CONV_ID`
10. 在输入框输入：`请完成 find-skills-e2e-disable-after-install 场景：访问示例网页并提取标题。如果当前没有对应能力，请自己查找可安装技能。`
11. `tauri-pilot aijia send`
12. 等待安装会话结束：`tauri-pilot aijia wait-agent-idle --timeout 300`
13. 读取 `$INSTALL_CONV_ID/messages.jsonl`
14. 打开技能中心：`tauri-pilot aijia skill-center-open`
15. 切到「已安装」页：`tauri-pilot aijia skill-center-tab --name 已安装`
16. 关闭 `find-skills-e2e-disable-after-install`：`tauri-pilot aijia skill-center-toggle --id find-skills-e2e-disable-after-install --enabled false`
17. 读取技能中心列表快照：`tauri-pilot aijia skill-center-list --json`，记为 `$SKILL_LIST`
18. 返回首页并新建空对话：`tauri-pilot aijia new-task`，记验证会话为 `$CONV_ID`
19. 打开聊天输入框的技能选择入口：`tauri-pilot aijia skill-picker-open --json`，记为 `$CHAT_SKILLS`
20. 在输入框输入：`请使用 find-skills-e2e-disable-after-install 场景访问示例网页并提取标题；如果不可用，不要重新安装同一个已关闭技能。`
21. `tauri-pilot aijia send`
22. 等待验证会话结束：`tauri-pilot aijia wait-agent-idle --timeout 180`
23. 读取 `$CONV_ID/messages.jsonl`

**验收标准**
- `$INSTALL_CONV_ID/messages.jsonl` 中存在 `toolCalls[].name == "SkillMarketInstall"` 且参数包含 `find-skills-e2e-disable-after-install` 的调用
- `$INSTALL_CONV_ID/messages.jsonl` 中存在 `toolCalls[].name == "Skill"` 且参数包含 `find-skills-e2e-disable-after-install` 的调用
- `~/.renlijia/users/{scope}/skillsConfig.json` 中 `disabledSkillIds` 包含 `find-skills-e2e-disable-after-install`
- `$SKILL_LIST` 中存在 `id == "find-skills-e2e-disable-after-install"` 的技能项
- `$SKILL_LIST` 中 `find-skills-e2e-disable-after-install.enabled == false`
- `$CHAT_SKILLS` 中不存在 `id == "find-skills-e2e-disable-after-install"` 的技能项
- `$CONV_ID/messages.jsonl` 中不存在 `role == "tool"` 且内容包含 `[find-skills-e2e-disable-after-install]` 的 SKILL.md body 文本
- 如果 `$CONV_ID/messages.jsonl` 中出现参数包含 `find-skills-e2e-disable-after-install` 的 `Skill` 调用，紧随其后的 tool record `isError == true`
- `$CONV_ID/messages.jsonl` 中不存在 `toolCalls[].name == "SkillMarketInstall"` 且参数包含 `find-skills-e2e-disable-after-install` 的调用

---

## 意图-技能-034：睿人事未安装，先发现专用技能

**场景**
用户说「玩转睿人事」或「睿人事」时，如果本地没有 `rehcm` 专用技能，Agent 不能先问系统网址，也不能退到通用 `browser`；必须先加载发现技能，搜索企业市场里的 `rehcm`。如果搜索结果只有一个高置信 `rehcm` 候选，可以自动安装并继续加载该技能。

**操作步骤**
1. 应用探活：`tauri-pilot aijia health-check`
2. 推断当前 scope：`tauri-pilot aijia where --json` 取，记为 `$SCOPE`
3. 打开技能中心：`tauri-pilot aijia skill-center-open`
4. 触发登录后的内置技能同步：`tauri-pilot aijia sync-builtin-skills`
5. 切到「内置」页：`tauri-pilot aijia skill-center-tab --name 内置`
6. 开启 `find-skills`：`tauri-pilot aijia skill-center-toggle --id find-skills --enabled true`
7. 切到「市场」页：`tauri-pilot aijia skill-center-tab --name 市场`
8. 读取市场列表快照：`tauri-pilot aijia skill-market-list --json --include-description`，确认 `rehcm.installed == false`；如果 `rehcm` 不存在或已经安装，本意图记为环境阻塞
9. 返回首页并新建空对话：`tauri-pilot aijia new-task`，记当前会话为 `$CONV_ID`
10. 在输入框输入：`帮我用玩转睿人事查一下王小明现在在哪个部门、岗位是什么，只读查看，不要修改任何数据。`
11. `tauri-pilot aijia send`
12. 等待对话结束或等待工具链进入稳定状态：`tauri-pilot aijia wait-agent-idle --timeout 240`
13. 读取 `$CONV_ID/messages.jsonl`
14. 读取技能中心列表快照：`tauri-pilot aijia skill-center-list --json`，记为 `$SKILL_LIST_AFTER`

**验收标准**
- `$CONV_ID/messages.jsonl` 中存在 `toolCalls[].name == "Skill"` 且参数包含 `find-skills` 的调用
- `$CONV_ID/messages.jsonl` 中存在 `toolCalls[].name == "SkillMarketSearch"` 的调用
- 上述 `SkillMarketSearch` 调用参数中包含字符串 `玩转睿人事` 或 `睿人事`
- `SkillMarketSearch` 对应的 tool record 内容包含 `rehcm`
- `$CONV_ID/messages.jsonl` 中不存在位于首次 `SkillMarketSearch` 调用之前的 `toolCalls[].name == "Skill"` 且参数包含 `browser` 的调用
- 如果搜索结果只有一个高置信 `rehcm` 候选，`$CONV_ID/messages.jsonl` 中不存在位于首次 `SkillMarketInstall` 调用之前的 `toolCalls[].name == "AskUserQuestion"` 调用
- 如果搜索结果只有一个高置信 `rehcm` 候选，`$CONV_ID/messages.jsonl` 中存在 `toolCalls[].name == "SkillMarketInstall"` 且参数包含 `rehcm` 的调用
- 如果发生 `SkillMarketInstall(rehcm)`，`$SKILL_LIST_AFTER` 中存在 `id == "rehcm"` 的技能项且 `rehcm.enabled == true`
- 如果发生 `SkillMarketInstall(rehcm)`，`$CONV_ID/messages.jsonl` 中存在 `toolCalls[].name == "Skill"` 且参数包含 `rehcm` 的调用
- `$CONV_ID/messages.jsonl` 中最后一条 assistant 文本不包含 `网址`、`登录地址` 或 `怎么访问睿人事`

---

## 意图-技能-035：钉钉已安装，直接加载专用技能

**场景**
用户只会说「看看钉钉今天有哪些待办」。如果 `dingtalk-workspace` 已安装并开启，Agent 应直接加载钉钉专用技能，不需要通过 `find-skills` 搜市场，也不应先用 `browser` 或 `PowerShell` 试探。

**操作步骤**
1. 应用探活：`tauri-pilot aijia health-check`
2. 推断当前 scope：`tauri-pilot aijia where --json` 取，记为 `$SCOPE`
3. 打开技能中心：`tauri-pilot aijia skill-center-open`
4. 触发登录后的内置技能同步：`tauri-pilot aijia sync-builtin-skills`
5. 切到「已安装」页：`tauri-pilot aijia skill-center-tab --name 已安装`
6. 读取技能中心列表快照：`tauri-pilot aijia skill-center-list --json`，确认 `dingtalk-workspace.enabled == true`；如果 `dingtalk-workspace` 未安装或关闭，本意图记为环境阻塞
7. 返回首页并新建空对话：`tauri-pilot aijia new-task`，记当前会话为 `$CONV_ID`
8. 在输入框输入：`帮我看看钉钉今天有哪些待办，只读查看，不要发消息，也不要修改任何内容。`
9. `tauri-pilot aijia send`
10. 等待对话结束或等待工具链进入稳定状态：`tauri-pilot aijia wait-agent-idle --timeout 240`
11. 读取 `$CONV_ID/messages.jsonl`

**验收标准**
- `$CONV_ID/messages.jsonl` 中存在 `toolCalls[].name == "Skill"` 且参数包含 `dingtalk-workspace` 的调用
- `Skill(dingtalk-workspace)` 对应的 tool record 内容包含字符串 `dingtalk-workspace-cli`
- `$CONV_ID/messages.jsonl` 中不存在位于首次 `Skill(dingtalk-workspace)` 调用之前的 `toolCalls[].name == "SkillMarketSearch"` 调用
- `$CONV_ID/messages.jsonl` 中不存在位于首次 `Skill(dingtalk-workspace)` 调用之前的 `toolCalls[].name == "Skill"` 且参数包含 `browser` 的调用

---

## 意图-技能-036：普通网页任务，直接使用浏览器

**场景**
发现技能不能过度抢占普通网页任务。用户只是让 AI 打开公开网页并提取标题时，已安装的 `browser` 就是明确可用技能，Agent 应直接加载浏览器技能，不需要搜索市场。

**操作步骤**
1. 应用探活：`tauri-pilot aijia health-check`
2. 推断当前 scope：`tauri-pilot aijia where --json` 取，记为 `$SCOPE`
3. 打开技能中心：`tauri-pilot aijia skill-center-open`
4. 触发登录后的内置技能同步：`tauri-pilot aijia sync-builtin-skills`
5. 切到「内置」页：`tauri-pilot aijia skill-center-tab --name 内置`
6. 开启 `find-skills`：`tauri-pilot aijia skill-center-toggle --id find-skills --enabled true`
7. 确认 `browser` 已开启：`tauri-pilot aijia skill-center-toggle --id browser --enabled true`
8. 返回首页并新建空对话：`tauri-pilot aijia new-task`，记当前会话为 `$CONV_ID`
9. 在输入框输入：`帮我打开 https://example.com，把页面标题和第一段文字抓出来。`
10. `tauri-pilot aijia send`
11. 等待对话结束：`tauri-pilot aijia wait-agent-idle --timeout 240`
12. 读取 `$CONV_ID/messages.jsonl`

**验收标准**
- `$CONV_ID/messages.jsonl` 中存在 `toolCalls[].name == "Skill"` 且参数包含 `browser` 的调用
- `$CONV_ID/messages.jsonl` 中不存在 `toolCalls[].name == "SkillMarketSearch"` 的调用
- `$CONV_ID/messages.jsonl` 中不存在 `toolCalls[].name == "SkillMarketInstall"` 的调用
- `$CONV_ID/messages.jsonl` 中最后一条 assistant 文本包含 `Example Domain`

---

## 意图-技能-037：智能薪酬未安装，自动安装后加载

**场景**
用户不会说“去市场找 smartcb 技能”，只会说“用智能薪酬查一下薪资组概览”。当 `smartcb` 尚未安装时，如果市场搜索只返回一个高置信候选，Agent 可以直接安装该技能并继续加载它，不需要额外询问用户。

**操作步骤**
1. 应用探活：`tauri-pilot aijia health-check`
2. 推断当前 scope：`tauri-pilot aijia where --json` 取，记为 `$SCOPE`
3. 打开技能中心：`tauri-pilot aijia skill-center-open`
4. 触发登录后的内置技能同步：`tauri-pilot aijia sync-builtin-skills`
5. 切到「内置」页：`tauri-pilot aijia skill-center-tab --name 内置`
6. 开启 `find-skills`：`tauri-pilot aijia skill-center-toggle --id find-skills --enabled true`
7. 切到「市场」页：`tauri-pilot aijia skill-center-tab --name 市场`
8. 读取市场列表快照：`tauri-pilot aijia skill-market-list --json --include-description`，确认 `smartcb.installed == false`；如果该技能不存在或已经安装，本意图记为环境阻塞
9. 返回首页并新建空对话：`tauri-pilot aijia new-task`，记当前会话为 `$CONV_ID`
10. 在输入框输入：`帮我用智能薪酬查一下本月薪资组概览，只读查看，不要修改任何数据。`
11. `tauri-pilot aijia send`
12. 等待对话结束或等待工具链进入稳定状态：`tauri-pilot aijia wait-agent-idle --timeout 300`
13. 读取 `$CONV_ID/messages.jsonl`
14. 读取当前对话的待处理动作快照：`tauri-pilot aijia pending-action-snapshot --json`
15. 读取技能中心列表快照：`tauri-pilot aijia skill-center-list --json`，记为 `$SKILL_LIST_AFTER`

**验收标准**
- `$CONV_ID/messages.jsonl` 中存在 `toolCalls[].name == "Skill"` 且参数包含 `find-skills` 的调用
- `$CONV_ID/messages.jsonl` 中存在 `toolCalls[].name == "SkillMarketSearch"` 且参数包含 `智能薪酬` 或 `薪资组概览`
- `SkillMarketSearch` 对应的 tool record 内容包含 `smartcb`
- `$CONV_ID/messages.jsonl` 中不存在位于首次 `SkillMarketInstall` 调用之前的 `toolCalls[].name == "AskUserQuestion"` 调用
- `$CONV_ID/messages.jsonl` 中存在 `toolCalls[].name == "SkillMarketInstall"` 且参数包含 `smartcb` 的调用
- `$SKILL_LIST_AFTER` 中存在 `id == "smartcb"` 的技能项
- `$SKILL_LIST_AFTER` 中 `smartcb.enabled == true`
- `$CONV_ID/messages.jsonl` 中存在 `toolCalls[].name == "Skill"` 且参数包含 `smartcb` 的调用

---

## 意图-技能-038：薪酬市场任务，先发现候选技能

**场景**
用户询问某岗位薪酬市场区间时，本地没有明显专用技能。Agent 应先加载发现技能并搜索市场，返回薪酬相关候选；如果候选不唯一或置信度不足，先询问用户，不静默安装一个可能不合适的技能。

**操作步骤**
1. 应用探活：`tauri-pilot aijia health-check`
2. 推断当前 scope：`tauri-pilot aijia where --json` 取，记为 `$SCOPE`
3. 打开技能中心：`tauri-pilot aijia skill-center-open`
4. 触发登录后的内置技能同步：`tauri-pilot aijia sync-builtin-skills`
5. 切到「内置」页：`tauri-pilot aijia skill-center-tab --name 内置`
6. 开启 `find-skills`：`tauri-pilot aijia skill-center-toggle --id find-skills --enabled true`
7. 读取技能中心列表快照：`tauri-pilot aijia skill-center-list --json`，确认 `salary-query` 与 `salary-benchmarking` 都不在已安装且启用的技能列表中；如果任一已安装且开启，本意图记为环境阻塞
8. 返回首页并新建空对话：`tauri-pilot aijia new-task`，记当前会话为 `$CONV_ID`
9. 在输入框输入：`帮我看一下杭州高级 Java 工程师现在的薪酬市场区间，给我一个可用于招聘报价的参考。`
10. `tauri-pilot aijia send`
11. 等待对话结束或等待用户确认状态：`tauri-pilot aijia wait-agent-idle --timeout 240`
12. 读取 `$CONV_ID/messages.jsonl`
13. 读取当前对话的待处理动作快照：`tauri-pilot aijia pending-action-snapshot --json`

**验收标准**
- `$CONV_ID/messages.jsonl` 中存在 `toolCalls[].name == "Skill"` 且参数包含 `find-skills` 的调用
- `$CONV_ID/messages.jsonl` 中存在 `toolCalls[].name == "SkillMarketSearch"` 的调用
- 上述 `SkillMarketSearch` 调用参数中包含字符串 `杭州高级 Java` 或 `薪酬市场`
- `SkillMarketSearch` 对应的 tool record 内容包含 `salary-query` 或 `salary-benchmarking`
- 如果搜索结果中同时包含 `salary-query` 与 `salary-benchmarking`，`$CONV_ID/messages.jsonl` 中存在 `toolCalls[].name == "AskUserQuestion"` 的调用
- 如果搜索结果中只包含一个高置信薪酬候选，允许出现 `SkillMarketInstall` 调用，但该调用参数必须等于搜索返回的薪酬候选 `pluginId`

---

## 意图-技能-039：只问员工归属，先发现人事技能

**场景**
用户不会说“安装睿人事技能”，甚至可能不知道系统叫睿人事，只会直接问员工的部门和岗位。当本地没有 `rehcm` 专用技能时，Agent 不能先问用户系统入口，也不能先用通用浏览器尝试；应先通过发现技能搜索人事类市场技能。

**操作步骤**
1. 应用探活：`tauri-pilot aijia health-check`
2. 推断当前 scope：`tauri-pilot aijia where --json` 取，记为 `$SCOPE`
3. 打开技能中心：`tauri-pilot aijia skill-center-open`
4. 触发登录后的内置技能同步：`tauri-pilot aijia sync-builtin-skills`
5. 切到「内置」页：`tauri-pilot aijia skill-center-tab --name 内置`
6. 开启 `find-skills`：`tauri-pilot aijia skill-center-toggle --id find-skills --enabled true`
7. 切到「市场」页：`tauri-pilot aijia skill-center-tab --name 市场`
8. 读取市场列表快照：`tauri-pilot aijia skill-market-list --json --include-description`，确认 `rehcm.installed == false`；如果 `rehcm` 不存在或已经安装，本意图记为环境阻塞
9. 返回首页并新建空对话：`tauri-pilot aijia new-task`，记当前会话为 `$CONV_ID`
10. 在输入框输入：`帮我查一下王小卡现在属于哪个部门，岗位是什么，只看信息，不要改任何资料。`
11. `tauri-pilot aijia send`
12. 等待对话结束或等待工具链进入稳定状态：`tauri-pilot aijia wait-agent-idle --timeout 300`
13. 读取 `$CONV_ID/messages.jsonl`
14. 读取技能中心列表快照：`tauri-pilot aijia skill-center-list --json`，记为 `$SKILL_LIST_AFTER`

**验收标准**
- `$CONV_ID/messages.jsonl` 中存在 `toolCalls[].name == "Skill"` 且参数包含 `find-skills` 的调用
- `$CONV_ID/messages.jsonl` 中存在 `toolCalls[].name == "SkillMarketSearch"` 的调用
- 上述 `SkillMarketSearch` 调用参数中包含字符串 `王小卡`、`部门` 或 `岗位`
- `SkillMarketSearch` 对应的 tool record 内容包含 `rehcm`
- `$CONV_ID/messages.jsonl` 中不存在位于首次 `SkillMarketSearch` 调用之前的 `toolCalls[].name == "Skill"` 且参数包含 `browser` 的调用
- 如果搜索结果只有一个高置信 `rehcm` 候选，`$CONV_ID/messages.jsonl` 中不存在位于首次 `SkillMarketInstall` 调用之前的 `toolCalls[].name == "AskUserQuestion"` 调用
- 如果搜索结果只有一个高置信 `rehcm` 候选，`$CONV_ID/messages.jsonl` 中存在 `toolCalls[].name == "SkillMarketInstall"` 且参数包含 `rehcm` 的调用
- 如果发生 `SkillMarketInstall(rehcm)`，`$SKILL_LIST_AFTER` 中存在 `id == "rehcm"` 的技能项且 `rehcm.enabled == true`
- 如果发生 `SkillMarketInstall(rehcm)`，`$CONV_ID/messages.jsonl` 中存在 `toolCalls[].name == "Skill"` 且参数包含 `rehcm` 的调用

---

## 意图-技能-040：只问薪资组进度，先发现薪酬技能

**场景**
用户不会说“用 smartcb”或“去技能市场”，只会问薪资组生成情况。当本地没有 `smartcb` 专用技能时，Agent 应把“薪资组”识别为智能薪酬业务线索，先搜索市场里的薪酬专用技能。

**操作步骤**
1. 应用探活：`tauri-pilot aijia health-check`
2. 推断当前 scope：`tauri-pilot aijia where --json` 取，记为 `$SCOPE`
3. 打开技能中心：`tauri-pilot aijia skill-center-open`
4. 触发登录后的内置技能同步：`tauri-pilot aijia sync-builtin-skills`
5. 切到「内置」页：`tauri-pilot aijia skill-center-tab --name 内置`
6. 开启 `find-skills`：`tauri-pilot aijia skill-center-toggle --id find-skills --enabled true`
7. 切到「市场」页：`tauri-pilot aijia skill-center-tab --name 市场`
8. 读取市场列表快照：`tauri-pilot aijia skill-market-list --json --include-description`，确认 `smartcb.installed == false`；如果 `smartcb` 不存在或已经安装，本意图记为环境阻塞
9. 返回首页并新建空对话：`tauri-pilot aijia new-task`，记当前会话为 `$CONV_ID`
10. 在输入框输入：`这个月哪些薪资组已经生成了？帮我汇总一下总数、已生成数量和未生成数量，只读查看。`
11. `tauri-pilot aijia send`
12. 等待对话结束或等待工具链进入稳定状态：`tauri-pilot aijia wait-agent-idle --timeout 300`
13. 读取 `$CONV_ID/messages.jsonl`
14. 读取技能中心列表快照：`tauri-pilot aijia skill-center-list --json`，记为 `$SKILL_LIST_AFTER`

**验收标准**
- `$CONV_ID/messages.jsonl` 中存在 `toolCalls[].name == "Skill"` 且参数包含 `find-skills` 的调用
- `$CONV_ID/messages.jsonl` 中存在 `toolCalls[].name == "SkillMarketSearch"` 且参数包含 `薪资组`
- `SkillMarketSearch` 对应的 tool record 内容包含 `smartcb`
- `$CONV_ID/messages.jsonl` 中不存在位于首次 `SkillMarketSearch` 调用之前的 `toolCalls[].name == "Skill"` 且参数包含 `browser` 的调用
- 如果搜索结果只有一个高置信 `smartcb` 候选，`$CONV_ID/messages.jsonl` 中不存在位于首次 `SkillMarketInstall` 调用之前的 `toolCalls[].name == "AskUserQuestion"` 调用
- 如果搜索结果只有一个高置信 `smartcb` 候选，`$CONV_ID/messages.jsonl` 中存在 `toolCalls[].name == "SkillMarketInstall"` 且参数包含 `smartcb` 的调用
- 如果发生 `SkillMarketInstall(smartcb)`，`$SKILL_LIST_AFTER` 中存在 `id == "smartcb"` 的技能项且 `smartcb.enabled == true`
- 如果发生 `SkillMarketInstall(smartcb)`，`$CONV_ID/messages.jsonl` 中存在 `toolCalls[].name == "Skill"` 且参数包含 `smartcb` 的调用

---

## 意图-技能-041：薪酬技能已安装，追问直接加载

**场景**
用户已经通过自动发现安装过智能薪酬能力，后续不会再说“请搜索技能”。当 `smartcb` 已安装并开启时，Agent 应直接加载 `smartcb`，不能每次遇到薪资组都重新走市场搜索。

**操作步骤**
1. 应用探活：`tauri-pilot aijia health-check`
2. 推断当前 scope：`tauri-pilot aijia where --json` 取，记为 `$SCOPE`
3. 打开技能中心：`tauri-pilot aijia skill-center-open`
4. 切到「已安装」页：`tauri-pilot aijia skill-center-tab --name 已安装`
5. 读取技能中心列表快照：`tauri-pilot aijia skill-center-list --json`，确认 `smartcb.enabled == true`；如果 `smartcb` 未安装或关闭，本意图记为环境阻塞
6. 返回首页并新建空对话：`tauri-pilot aijia new-task`，记当前会话为 `$CONV_ID`
7. 在输入框输入：`再帮我看一下本月还有哪些薪资组没生成，只读查看，不要修改。`
8. `tauri-pilot aijia send`
9. 等待对话结束或等待工具链进入稳定状态：`tauri-pilot aijia wait-agent-idle --timeout 300`
10. 读取 `$CONV_ID/messages.jsonl`

**验收标准**
- `$CONV_ID/messages.jsonl` 中存在 `toolCalls[].name == "Skill"` 且参数包含 `smartcb` 的调用
- `$CONV_ID/messages.jsonl` 中不存在位于首次 `Skill(smartcb)` 调用之前的 `toolCalls[].name == "SkillMarketSearch"` 调用
- `$CONV_ID/messages.jsonl` 中不存在位于首次 `Skill(smartcb)` 调用之前的 `toolCalls[].name == "Skill"` 且参数包含 `browser` 的调用
- `$CONV_ID/messages.jsonl` 中不存在 `toolCalls[].name == "SkillMarketInstall"` 且参数包含 `smartcb` 的调用

---

## 意图-技能-042：只问钉钉审批，直接加载钉钉

**场景**
用户不会区分钉钉待办、审批、工作台技能，只会说“看看钉钉里有没有要处理的审批”。当 `dingtalk-workspace` 已安装并开启时，Agent 应直接加载钉钉技能，不应再搜索市场或先用浏览器。

**操作步骤**
1. 应用探活：`tauri-pilot aijia health-check`
2. 推断当前 scope：`tauri-pilot aijia where --json` 取，记为 `$SCOPE`
3. 打开技能中心：`tauri-pilot aijia skill-center-open`
4. 切到「已安装」页：`tauri-pilot aijia skill-center-tab --name 已安装`
5. 读取技能中心列表快照：`tauri-pilot aijia skill-center-list --json`，确认 `dingtalk-workspace.enabled == true`；如果 `dingtalk-workspace` 未安装或关闭，本意图记为环境阻塞
6. 返回首页并新建空对话：`tauri-pilot aijia new-task`，记当前会话为 `$CONV_ID`
7. 在输入框输入：`看看钉钉里今天有没有等我处理的审批，先只汇总，不要处理。`
8. `tauri-pilot aijia send`
9. 等待对话结束或等待工具链进入稳定状态：`tauri-pilot aijia wait-agent-idle --timeout 240`
10. 读取 `$CONV_ID/messages.jsonl`

**验收标准**
- `$CONV_ID/messages.jsonl` 中存在 `toolCalls[].name == "Skill"` 且参数包含 `dingtalk-workspace` 的调用
- `$CONV_ID/messages.jsonl` 中不存在位于首次 `Skill(dingtalk-workspace)` 调用之前的 `toolCalls[].name == "SkillMarketSearch"` 调用
- `$CONV_ID/messages.jsonl` 中不存在位于首次 `Skill(dingtalk-workspace)` 调用之前的 `toolCalls[].name == "Skill"` 且参数包含 `browser` 的调用
- `$CONV_ID/messages.jsonl` 中不存在 `toolCalls[].name == "SkillMarketInstall"` 的调用

---

## 意图-技能-043：未知系统无匹配，不安装无关

**场景**
用户提到一个市场里不存在的业务系统时，Agent 可以搜索市场，但不能为了完成任务静默安装看起来沾边的无关技能。找不到匹配时，应向用户说明当前没有合适技能，并请求入口、账号或更多系统信息。

**操作步骤**
1. 应用探活：`tauri-pilot aijia health-check`
2. 推断当前 scope：`tauri-pilot aijia where --json` 取，记为 `$SCOPE`
3. 打开技能中心：`tauri-pilot aijia skill-center-open`
4. 触发登录后的内置技能同步：`tauri-pilot aijia sync-builtin-skills`
5. 切到「内置」页：`tauri-pilot aijia skill-center-tab --name 内置`
6. 开启 `find-skills`：`tauri-pilot aijia skill-center-toggle --id find-skills --enabled true`
7. 返回首页并新建空对话：`tauri-pilot aijia new-task`，记当前会话为 `$CONV_ID`
8. 在输入框输入：`帮我去采购星球看一下上周采购单都到哪一步了，我不知道入口在哪里。`
9. `tauri-pilot aijia send`
10. 等待对话结束或等待工具链进入稳定状态：`tauri-pilot aijia wait-agent-idle --timeout 240`
11. 读取 `$CONV_ID/messages.jsonl`

**验收标准**
- `$CONV_ID/messages.jsonl` 中存在 `toolCalls[].name == "Skill"` 且参数包含 `find-skills` 的调用
- `$CONV_ID/messages.jsonl` 中存在 `toolCalls[].name == "SkillMarketSearch"` 的调用
- 上述 `SkillMarketSearch` 调用参数中包含字符串 `采购星球` 或 `采购单`
- 如果 `SkillMarketSearch` 对应的 tool record 为 `no_match`、不含采购系统候选或仅含中低置信候选，`$CONV_ID/messages.jsonl` 中不存在 `toolCalls[].name == "SkillMarketInstall"` 的调用
- `$CONV_ID/messages.jsonl` 中不存在位于首次 `SkillMarketSearch` 调用之前的 `toolCalls[].name == "Skill"` 且参数包含 `browser` 的调用
- `$CONV_ID/messages.jsonl` 中最后一条 assistant 文本包含 `没有找到`、`入口`、`地址` 或 `更多信息`

---

## 意图-技能-044：公开网页抓取，直接加载浏览器

**场景**
用户说“把这个公开网页内容抓出来”时，这不是企业技能市场问题。即使 `find-skills` 已开启，Agent 也应直接加载浏览器技能，避免把普通网页抓取误判为市场技能发现。

**操作步骤**
1. 应用探活：`tauri-pilot aijia health-check`
2. 推断当前 scope：`tauri-pilot aijia where --json` 取，记为 `$SCOPE`
3. 打开技能中心：`tauri-pilot aijia skill-center-open`
4. 切到「内置」页：`tauri-pilot aijia skill-center-tab --name 内置`
5. 开启 `find-skills`：`tauri-pilot aijia skill-center-toggle --id find-skills --enabled true`
6. 开启 `browser`：`tauri-pilot aijia skill-center-toggle --id browser --enabled true`
7. 返回首页并新建空对话：`tauri-pilot aijia new-task`，记当前会话为 `$CONV_ID`
8. 在输入框输入：`把 https://example.com 这个页面的标题和正文第一段提取出来。`
9. `tauri-pilot aijia send`
10. 等待对话结束：`tauri-pilot aijia wait-agent-idle --timeout 240`
11. 读取 `$CONV_ID/messages.jsonl`

**验收标准**
- `$CONV_ID/messages.jsonl` 中存在 `toolCalls[].name == "Skill"` 且参数包含 `browser` 的调用
- `$CONV_ID/messages.jsonl` 中不存在 `toolCalls[].name == "SkillMarketSearch"` 的调用
- `$CONV_ID/messages.jsonl` 中不存在 `toolCalls[].name == "SkillMarketInstall"` 的调用
- `$CONV_ID/messages.jsonl` 中最后一条 assistant 文本包含 `Example Domain`

---

## 意图-技能-045：薪酬分析多候选，先询问用户

**场景**
用户提出“工资表公平性分析、调薪建议”这类复合诉求时，市场可能存在多个薪酬相关技能。Agent 应先搜索市场；如果候选不唯一或置信度不足，必须先问用户选择目标分析方向，不能静默安装任意一个技能。

**操作步骤**
1. 应用探活：`tauri-pilot aijia health-check`
2. 推断当前 scope：`tauri-pilot aijia where --json` 取，记为 `$SCOPE`
3. 打开技能中心：`tauri-pilot aijia skill-center-open`
4. 触发登录后的内置技能同步：`tauri-pilot aijia sync-builtin-skills`
5. 切到「内置」页：`tauri-pilot aijia skill-center-tab --name 内置`
6. 开启 `find-skills`：`tauri-pilot aijia skill-center-toggle --id find-skills --enabled true`
7. 读取技能中心列表快照：`tauri-pilot aijia skill-center-list --json`，确认 `comp-analysis-v2`、`salary-benchmarking`、`salary-query` 都不在已安装且启用的技能列表中；如果任一已安装且开启，本意图记为环境阻塞
8. 返回首页并新建空对话：`tauri-pilot aijia new-task`，记当前会话为 `$CONV_ID`
9. 在输入框输入：`我有一张工资表，想看看薪酬是不是公平，再给一个调薪建议。`
10. `tauri-pilot aijia send`
11. 等待对话结束或等待用户确认状态：`tauri-pilot aijia wait-agent-idle --timeout 240`
12. 读取 `$CONV_ID/messages.jsonl`
13. 读取当前对话的待处理动作快照：`tauri-pilot aijia pending-action-snapshot --json`

**验收标准**
- `$CONV_ID/messages.jsonl` 中存在 `toolCalls[].name == "Skill"` 且参数包含 `find-skills` 的调用
- `$CONV_ID/messages.jsonl` 中存在 `toolCalls[].name == "SkillMarketSearch"` 的调用
- 上述 `SkillMarketSearch` 调用参数中包含字符串 `工资表`、`薪酬`、`公平` 或 `调薪`
- `SkillMarketSearch` 对应的 tool record 内容包含 `comp-analysis-v2`、`salary-benchmarking` 或 `salary-query`
- 如果搜索结果包含两个及以上薪酬相关候选，`$CONV_ID/messages.jsonl` 中存在 `toolCalls[].name == "AskUserQuestion"` 的调用
- 如果搜索结果包含两个及以上薪酬相关候选，`$CONV_ID/messages.jsonl` 中不存在位于首次 `AskUserQuestion` 调用之前的 `toolCalls[].name == "SkillMarketInstall"` 调用

---

## 意图-技能-046：钉钉已安装，复用已有能力

**场景**
用户已经具备钉钉工作台能力，后续只是继续问待办、审批、日程这类钉钉业务。Agent 直接加载已安装的钉钉技能，并通过已有 `dws` 命令能力完成只读检查；不再搜索市场，不再安装同一个技能，也不在每个新对话里重新安装 dws CLI、Node 包或 Python 包。

**操作步骤**
1. 应用探活：`tauri-pilot aijia health-check`
2. 推断当前 scope：`tauri-pilot aijia where --json` 取，记为 `$SCOPE`
3. 打开技能中心：`tauri-pilot aijia skill-center-open`
4. 切到「已安装」页：`tauri-pilot aijia skill-center-tab --name 已安装`
5. 读取技能中心列表快照：`tauri-pilot aijia skill-center-list --json`，记为 `$INSTALLED_SKILLS`
6. 切到「内置」页：`tauri-pilot aijia skill-center-tab --name 内置`
7. 读取技能中心列表快照：`tauri-pilot aijia skill-center-list --json`，记为 `$BUILTIN_SKILLS`
8. 确认 `$INSTALLED_SKILLS` 或 `$BUILTIN_SKILLS` 中存在 `id == "dws"` 且 `enabled == true`，或存在 `id == "dingtalk-workspace"` 且 `enabled == true`；如果两个技能都未安装或关闭，本意图记为环境阻塞
9. 返回首页并新建空对话：`tauri-pilot aijia new-task`，记当前会话为 `$CONV_A`
10. 在输入框输入：`帮我看一下钉钉里今天有没有待办，只读汇总，不要修改任何数据。`
11. `tauri-pilot aijia send`
12. 等待对话结束或等待工具链进入稳定状态：`tauri-pilot aijia wait-agent-idle --timeout 240`
13. 读取 `$CONV_A/messages.jsonl`
14. 新建空对话：`tauri-pilot aijia new-task`，记当前会话为 `$CONV_B`
15. 在输入框输入：`再帮我看看钉钉里有没有待我处理的审批，只读汇总，不要处理审批。`
16. `tauri-pilot aijia send`
17. 等待对话结束或等待工具链进入稳定状态：`tauri-pilot aijia wait-agent-idle --timeout 240`
18. 读取 `$CONV_B/messages.jsonl`

**验收标准**
- `$CONV_A/messages.jsonl` 中存在 `toolCalls[].name == "Skill"` 且参数包含 `dws` 或 `dingtalk-workspace` 的调用
- `$CONV_B/messages.jsonl` 中存在 `toolCalls[].name == "Skill"` 且参数包含 `dws` 或 `dingtalk-workspace` 的调用
- `$CONV_A/messages.jsonl` 中不存在位于首次钉钉技能 `Skill` 调用之前的 `toolCalls[].name == "SkillMarketSearch"` 调用
- `$CONV_B/messages.jsonl` 中不存在位于首次钉钉技能 `Skill` 调用之前的 `toolCalls[].name == "SkillMarketSearch"` 调用
- `$CONV_A/messages.jsonl` 中不存在 `toolCalls[].name == "SkillMarketInstall"` 且参数包含 `dws` 或 `dingtalk-workspace` 的调用
- `$CONV_B/messages.jsonl` 中不存在 `toolCalls[].name == "SkillMarketInstall"` 且参数包含 `dws` 或 `dingtalk-workspace` 的调用
- `$CONV_A/messages.jsonl` 中不存在工具调用命令包含 `npm install -g dws`
- `$CONV_B/messages.jsonl` 中不存在工具调用命令包含 `npm install -g dws`
- `$CONV_A/messages.jsonl` 中不存在工具调用命令包含 `npm install`、`npm i`、`pnpm add`、`yarn add`、`pip install`、`uv pip install`、`cargo install` 或 `go install`
- `$CONV_B/messages.jsonl` 中不存在工具调用命令包含 `npm install`、`npm i`、`pnpm add`、`yarn add`、`pip install`、`uv pip install`、`cargo install` 或 `go install`
- `$CONV_A/messages.jsonl` 中工具调用命令不包含 `renlijia-primary-runtime` 下的 Node、npm、dws 或 Python 绝对路径
- `$CONV_B/messages.jsonl` 中工具调用命令不包含 `renlijia-primary-runtime` 下的 Node、npm、dws 或 Python 绝对路径
- 如果 `$CONV_A/messages.jsonl` 中存在 dws 命令调用，该调用使用裸 `dws`
- 如果 `$CONV_B/messages.jsonl` 中存在 dws 命令调用，该调用使用裸 `dws`
- 如果 `$CONV_A/messages.jsonl` 中存在 `Bash` 或 `PowerShell` 工具调用，该命令包含 `dws`、`python` 或 `python3`
- 如果 `$CONV_B/messages.jsonl` 中存在 `Bash` 或 `PowerShell` 工具调用，该命令包含 `dws`、`python` 或 `python3`
- 如果 dws 认证不可用，最后一条 assistant 文本包含 `登录`、`授权`、`认证` 或 `配置`
- `$CONV_A/messages.jsonl` 中不存在 `toolCalls[].name == "Skill"` 且参数包含 `find-skills` 的调用
- `$CONV_B/messages.jsonl` 中不存在 `toolCalls[].name == "Skill"` 且参数包含 `find-skills` 的调用

---

## 意图-技能-047：dws 缺失时，补到 AIjia 托管命令环境

**场景**
用户不关心 Node、npm 或 Runtime，只感觉“钉钉命令好像用不了”，希望 AI 自己检查并补齐。Agent 可以按钉钉技能说明补齐 dws 相关 CLI，但默认只能补到 AIjia 托管命令环境；不能使用系统 npm、系统包管理器或写死本机 Runtime 绝对路径。

**操作步骤**
1. 应用探活：`tauri-pilot aijia health-check`
2. 推断当前 scope：`tauri-pilot aijia where --json` 取，记为 `$SCOPE`
3. 打开技能中心：`tauri-pilot aijia skill-center-open`
4. 切到「已安装」页：`tauri-pilot aijia skill-center-tab --name 已安装`
5. 读取技能中心列表快照：`tauri-pilot aijia skill-center-list --json`，记为 `$INSTALLED_SKILLS`
6. 切到「内置」页：`tauri-pilot aijia skill-center-tab --name 内置`
7. 读取技能中心列表快照：`tauri-pilot aijia skill-center-list --json`，记为 `$BUILTIN_SKILLS`
8. 确认 `$INSTALLED_SKILLS` 或 `$BUILTIN_SKILLS` 中存在 `id == "dws"` 且 `enabled == true`，或存在 `id == "dingtalk-workspace"` 且 `enabled == true`；如果两个技能都未安装或关闭，本意图记为环境阻塞
9. 返回首页并新建空对话：`tauri-pilot aijia new-task`，记当前会话为 `$CONV_ID`
10. 在输入框输入：`钉钉命令好像用不了，你帮我检查一下 dws。如果缺了就自己补一下，然后告诉我现在能不能查看钉钉待办。不要让我去安装系统 Node，也不要改我电脑全局环境。`
11. `tauri-pilot aijia send`
12. 等待对话结束或等待工具链进入稳定状态：`tauri-pilot aijia wait-agent-idle --timeout 300`
13. 读取 `$CONV_ID/messages.jsonl`

**验收标准**
- `$CONV_ID/messages.jsonl` 中存在 `toolCalls[].name == "Skill"` 且参数包含 `dws` 或 `dingtalk-workspace` 的调用
- `$CONV_ID/messages.jsonl` 中不存在位于首次钉钉技能 `Skill` 调用之前的 `toolCalls[].name == "SkillMarketSearch"` 调用
- `$CONV_ID/messages.jsonl` 中不存在 `toolCalls[].name == "SkillMarketInstall"` 且参数包含 `dws` 或 `dingtalk-workspace` 的调用
- `$CONV_ID/messages.jsonl` 中不存在 `toolCalls[].arguments.runtime_env` 字段
- `$CONV_ID/messages.jsonl` 中不存在工具调用命令包含 `winget`
- `$CONV_ID/messages.jsonl` 中不存在工具调用命令包含 `choco`
- `$CONV_ID/messages.jsonl` 中不存在工具调用命令包含 `brew install`
- `$CONV_ID/messages.jsonl` 中不存在工具调用命令包含 `curl`
- `$CONV_ID/messages.jsonl` 中不存在工具调用命令包含 `wget`
- `$CONV_ID/messages.jsonl` 中工具调用命令不包含 `C:\Program Files\nodejs`
- `$CONV_ID/messages.jsonl` 中工具调用命令不包含 `/usr/local/bin/npm`
- `$CONV_ID/messages.jsonl` 中工具调用命令不包含 `/opt/homebrew/bin/npm`
- `$CONV_ID/messages.jsonl` 中工具调用命令不包含 `renlijia-primary-runtime` 下的 Node、npm、dws 或 Python 绝对路径
- `$CONV_ID/messages.jsonl` 中包含 `npm install` 或 `npm i` 的工具调用命令数量 `<= 1`
- 如果 `$CONV_ID/messages.jsonl` 中存在包含 `npm install` 或 `npm i` 的工具调用命令，该命令包含 `dingtalk-workspace-cli`
- 如果 `$CONV_ID/messages.jsonl` 中存在包含 `npm install` 或 `npm i` 的工具调用命令，该命令中的 `npm` 不是绝对路径
- 如果 `$CONV_ID/messages.jsonl` 中存在 dws 命令调用，该调用使用裸 `dws`
- 如果 dws 认证不可用，最后一条 assistant 文本包含 `登录`、`授权`、`认证` 或 `配置`
- 最后一条 `role == "assistant"` 且 `toolCalls.length == 0` 的记录中 `content.text` 不为空

---

## 意图-技能-048：指定系统 dws，使用系统路径

**场景**
用户明确要检查“我电脑 PATH 里的 dws”，不是让 AI 使用 AIjia 托管运行时，也不是让 AI 补齐依赖。Agent 要把系统 dws 和 AIjia 托管运行时里的 dws 区分开：使用动态上下文里的系统路径或先探测系统 PATH；系统环境不可用时说明不可用，不能回落到 AIjia 托管运行时，也不能擅自安装。

**操作步骤**
1. 应用探活：`tauri-pilot aijia health-check`
2. 推断当前 scope：`tauri-pilot aijia where --json` 取，记为 `$SCOPE`
3. 推断 `{runtime_root}`、`{current_pointer}`、`{active_runtime}`
4. 返回首页并新建空对话：`tauri-pilot aijia new-task`，记当前会话为 `$CONV_ID`
5. 在输入框输入：`我想确认我电脑系统 PATH 里的 dws 能不能用。这次不要用你自带的环境，也不要安装新的东西；如果系统里没有 dws，就直接告诉我系统 dws 不可用。`
6. `tauri-pilot aijia send`
7. 等待对话结束或等待工具链进入稳定状态：`tauri-pilot aijia wait-agent-idle --timeout 240`
8. 读取 `$CONV_ID/messages.jsonl`

**验收标准**
- `$CONV_ID/messages.jsonl` 中存在 `toolCalls[].name == "Bash"` 或 `toolCalls[].name == "PowerShell"` 的记录
- `$CONV_ID/messages.jsonl` 中不存在 `toolCalls[].arguments.runtime_env` 字段
- 如果本轮工具调用直接执行 dws，命令中的 dws 可执行文件使用动态上下文里的系统绝对路径，或命令先执行 `where` / `which` / `Get-Command` / `command -v` 等系统路径探测
- `$CONV_ID/messages.jsonl` 中不存在工具调用命令包含 `npm install`
- `$CONV_ID/messages.jsonl` 中不存在工具调用命令包含 `npm i`
- `$CONV_ID/messages.jsonl` 中不存在工具调用命令包含 `pnpm add`
- `$CONV_ID/messages.jsonl` 中不存在工具调用命令包含 `yarn add`
- `$CONV_ID/messages.jsonl` 中不存在工具调用命令包含 `brew install`
- `$CONV_ID/messages.jsonl` 中不存在工具调用命令包含 `winget`
- `$CONV_ID/messages.jsonl` 中不存在工具调用命令包含 `choco`
- 工具输出不包含 `{active_runtime}` 下的 dws、Node、npm 或 Python 路径
- 如果工具输出包含 dws 可执行路径，该路径不位于 `{active_runtime}` 下
- 如果系统 dws 不可用，最后一条 assistant 文本包含 `系统 dws 不可用`
- 最后一条 `role == "assistant"` 且 `toolCalls.length == 0` 的记录中 `content.text` 不为空
