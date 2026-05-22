# `tauri-pilot aijia` CLI 缺失清单

> 来源：跑日程 + 项目记忆 两个 task 实战暴露的 gap
> 更新：2026-05-22
> 定位：补充 `tauri-pilot aijia` 业务子命令；底层仍走 webview DOM 操作（click / fill / eval），**不**改为直调内部 IPC

## 背景

按 runner skill 铁则「e2e 脚本只走 `aijia` 子命令，禁直接 click/eval」。目前 16 个子命令偏聊天流，跑除项目记忆之外的 task 几乎都被迫绕过铁则用 generic `tauri-pilot click/eval/fill`。本清单按优先级列出最缺的 11 个子命令。

每个子命令都是对 DOM 操作的封装（一组 click + fill + wait），**不是**调用内部 IPC 跳过 UI。

## 用前确认：CLI 是不是新版

新增 CLI 命令在 sibling `../tauri-pilot/` 仓库编译后**必须装到 PATH** 才能用——`pnpm dev:with-pilot` 起项目**不会**编译 CLI。

```bash
# 看 PATH 上的 tauri-pilot 跟仓库新代码同步否
tauri-pilot aijia --help | grep -c '^  [a-z]'   # 当前应是 35 条

# 装新版（每次 CLI 仓库代码有改动后跑一次）
cd ../tauri-pilot
cargo install --path crates/tauri-pilot-cli --force
```

跑 `aijia <verb>` 报 `unrecognized subcommand` 多半是这条没跑。

## ✅ 已实现（2026-05-20 批次，**原子化拆分**）

每条命令做一个原子 UI 动作。复杂流程在 rules.md / runner shell 里串成序列，**不**塞一条大命令。

### Auth / 导航 / 诊断

| 命令 | 原子动作 |
|---|---|
| `aijia login --account --password [--timeout 20]` | 提交登录表单（fill 两字段 + 点 submit + 等 LoginPage unmount）—— 用户视角"完成登录"为单一动作，豁免拆分 |
| `aijia goto <page>` | 点 `[data-aijia-nav={key}]`（home / employees / expert-teams / skill-center / schedules / channel） |
| `aijia where --json` | 现在含 `scope / tenantId / userId / loggedIn`（来自 `__aijia.authStore`） |
| `aijia handle-dialog --action accept\|dismiss [--timeout 10]` | 点 `[data-aijia-confirm-dialog] [data-aijia-confirm-action]` |
| `aijia tool-calls [--turn last\|N]` | 读 chatStore.messages 按 user 消息分轮，返回 tool_calls 清单 |
| `aijia tool-bubble [--turn last\|N]` | 扫 DOM 工具气泡（绕过 ui-message 过滤） |

### 定时任务（agenda）— 原子组合

每条都是单步原子，按需组合：

| 原子命令 | 动作 |
|---|---|
| `aijia agenda-open-new` | 点 `[data-aijia-agenda-new]` |
| `aijia agenda-wait-editor [--timeout 5]` | 等 `[data-aijia-agenda-editor]` mount |
| `aijia agenda-wait-editor-closed [--timeout 5]` | 等编辑器 unmount |
| `aijia agenda-fill --field title\|prompt --value <s>` | fill 编辑器单个字段 |
| `aijia agenda-set-frequency --value once\|daily\|weekly\|monthly\|yearly` | 选频率 select |
| `aijia agenda-set-start-at --value <YYYY-MM-DDTHH:MM>` | fill datetime-local |
| `aijia agenda-set-employee --name <s>` | 选员工 select（substring 匹配 option text） |
| `aijia agenda-save` | 点保存按钮（不等关闭） |
| `aijia agenda-cancel` | 点取消按钮 |
| `aijia agenda-wait-row --title <s> [--timeout 5]` | 等指定 title 的 row 出现，返回 agendaId / status |
| `aijia agenda-row-action --title --action run-now\|pause\|resume\|edit\|cancel\|restore\|purge` | 点 row 内单个动作按钮；`cancel` 弹 ConfirmDialog，自行 chain `handle-dialog --action accept` |

**典型组合**（rules.md 范例）：

```bash
aijia goto schedules
aijia agenda-open-new
aijia agenda-wait-editor
aijia agenda-fill --field title --value "早会提醒"
aijia agenda-fill --field prompt --value "提醒我今天的三件事"
aijia agenda-set-frequency --value once
aijia agenda-set-start-at --value "2026-05-20T15:30"
aijia agenda-set-employee --name "小研"
aijia agenda-save
aijia agenda-wait-editor-closed
aijia agenda-wait-row --title "早会提醒"
```

```bash
# 暂停一条已有的
aijia agenda-row-action --title "早会提醒" --action pause

# 删除（带确认）
aijia agenda-row-action --title "早会提醒" --action cancel
aijia handle-dialog --action accept
```

### 数字员工派活 — 原子组合

| 原子命令 | 动作 |
|---|---|
| `aijia employee-open-card --name <s>` | 点 `[data-aijia-employee-card][data-aijia-employee-name=...]` |
| `aijia employee-wait-drawer [--timeout 3]` | 等 `[data-aijia-employee-drawer]` mount |
| `aijia employee-click-dispatch` | 点 drawer 内 `[data-aijia-employee-action="dispatch"]`（不等 chat 跳转） |
| `aijia employee-close-drawer` | 点 drawer 内 `[data-aijia-employee-action="close"]` |

**典型组合**：

```bash
aijia goto employees
aijia employee-open-card --name "小研"
aijia employee-wait-drawer
# 取派活前的 activeConversationId 用来对比
prev=$(aijia where --json | jq -r .sessionId)
aijia employee-click-dispatch
# 等 chatStore 切到新对话
until [ "$(aijia where --json | jq -r .sessionId)" != "$prev" ]; do sleep 1; done
```

### 设置 + 登出 — 原子组合

| 原子命令 | 动作 |
|---|---|
| `aijia open-settings` | 点侧栏底部「设置」按钮 |
| `aijia settings-wait [--timeout 3]` | 等 `[data-aijia-settings-shell]` mount |
| `aijia settings-select-panel --key <k>` | 点左侧菜单 panel（`account` / `account-billing` / `archived` / `runtime` / `about`） |
| `aijia settings-close` | 点设置 X 按钮关闭 |
| `aijia logout` | 点 General panel 的"退出登录"按钮（前置：settings 打开 + panel=account） |

**典型组合（登出）**：

```bash
aijia open-settings
aijia settings-wait
aijia settings-select-panel --key account
aijia logout
# 等回到登录态
until [ "$(aijia where --json | jq -r .loggedIn)" = "false" ]; do sleep 1; done
```

注：logout 后 SettingsModal 自动关闭，AuthGate 重挂 LoginPage，`where.loggedIn=false`。

### Composer 上传文件 — 原子组合（dev mock 路径）

`+` 号按钮调用 OS file dialog（`@tauri-apps/plugin-dialog::open`），webview JS 无法操作 OS 进程。**唯一解**：dev-only mock 队列预设返回值，绕开 OS dialog 但跑完下游真实链路（`makePendingAttachment` → `insertAttachmentTokens` → 编辑器显示 token）。

| 原子命令 | 动作 |
|---|---|
| `aijia composer-queue-files --paths "/a,/b"` | 把路径塞到 `__aijia._pickAttachmentsMockQueue`，**仅 dev**有效 |
| `aijia composer-click-plus` | 点 `[data-aijia-composer-plus]`；下次 `pickAttachments()` 消费一批队列 |

**典型组合**：

```bash
# 用户视角"点 + 选了两个文件"
aijia composer-queue-files --paths "/Users/x/test.txt,/Users/x/img.png"
aijia composer-click-plus
# 真实下游：makePendingAttachment → insertAttachmentTokens → composer 显示
# 后续直接 aijia send 发送（messages.jsonl 会带 attachments 字段）
```

**约定 + 边界**：
- queue 是**一次性消费**：一次 click-plus 消化一批 paths
- 如果 click-plus 时队列为空，会真的弹 OS dialog（用户视角无差别）
- release build (`import.meta.env.DEV === false`) tree-shake 掉 mock 检查，生产 binary 零影响
- 这条豁免仅针对 **OS-level modal**（NSOpenPanel / Windows file dialog）—— 它在 webview 之外、不是 AIjia 的代码、不是被测对象

## ⏳ 待做

### 按场景列：跑测时被卡住的 CLI 缺口（2026-05-21 收口，按 `test-intents-cli-author` skill 铁则审计）

> **铁则**（再次强调）：每条 CLI = 一个原子 UI 动作（点一个按钮 / 填一个字段 / 等一个状态 / 读一段状态）；多步流程由 rules.md 串联，**不许写一体命令**；走 webview DOM eval，**不走 IPC**；selector 优先级 `data-aijia-*` > id > aria-label > textContent。

> **状态约定**：
> - ✅ 已实现：CLI + selector 全部落地，rules 可直接用（指向下方"已批量补完"段）
> - ⏳ 真未做：CLI 缺口，给出**原子拆分清单 + selector 前置**
> - ⏳ 阻塞在 rules.md：CLI 已具备，问题在 rules 漂移或语义未定，应走 author skill
> - ❌ 不做：产品已废弃 / 环境依赖太重 / 不属于 UI 操作范畴。**给出 agent 退路**

### 全场景收口汇总（2026-05-21 末）

| 场景 | task / 意图 | 状态 | 下一步归属 |
|---|---|---|---|
| A 雇佣员工 | 数字员工-001 | ✅ 已实现 | rules 可直接用 |
| B 员工资源配置表单 | 数字员工-002 | ✅ 已实现 | rules 可直接用 |
| C 员工 cron toggle | 数字员工-005 | ✅ 已实现 | rules 可直接用 |
| D 员工解雇 | 数字员工-006 | ✅ 已实现（两原子组合） | rules 可直接用 |
| E 归档员工拒派 | 数字员工-007 | ✅ 已实现（`employee-status.dispatchDisabled`） | rules 可直接用 |
| F 日程编辑 | 日程 task 大部分意图 | ⏳ rules 漂移 | **author** 重写 rules.md |
| G 登录页 selector | 登录-002 / 005 | ✅ 已实现 | rules 可直接用 |
| H 崩溃恢复 | 崩溃恢复 task 全部 | ❌ 不属于 aijia CLI 范畴 | 用脚本壳 / shell |
| I 新建对话时选 workspace | 工作空间 task | ✅ 已实现（dev mock 队列） | **author** 按现实重写 rules.md |
| J 人格管理 | 人格 task 全部 | ❌ 产品已废弃（CLAUDE.md 2026-05-10） | **author** 删除 task 或迁档 |
| K 专家团队 | 专家团队 task 全部 | ✅ 已实现（产品是静态启动器，1 个原子） | **author** 按现实重写 rules.md |
| L 钉钉频道 | 钉钉频道 task 全部 | ❌ 环境依赖太重 | 标 task 为人工跑、不归 e2e |
| M 设置面板填字段 | 设置 task 全部 | ❌ 产品无此功能 | **author** 删除 task |
| N 待办队列 | 待办队列 task 全部 | ⏳ rules + 产品语义阻塞 | **author** 找产品 owner 对齐 pending 触发条件 |

**今日 CLI 端总结**：

- ✅ 已实现 **8 个**（A/B/C/D/E/G/I/K），全部走原子 UI 动作 + DOM selector
- ⏳ **CLI 端无真未做**——剩下的不是产品空中楼阁、就是 rules 漂移、就是不归 aijia CLI 范畴
- 阻塞总数 = 5 个（F / H / J / L / M / N），其中 4 个走 author（F / J / M / N），H 走脚本壳兜底，L 标人工跑

**下一步推进**：

1. **author skill**：F（日程 rules 重写）/ I（工作空间 rules 按现实重写）/ J（人格 task 删除/迁档）/ K（专家团队 rules 按现实重写）/ M（设置 task 删除）/ N（待办队列 rules 找产品对齐）共 6 项
2. **runner skill 跑测**：补 8 个 ✅ 场景对应的意图（数字员工 1/2/5/6/7、登录 2/5、工作空间、专家团队）
3. **手工 / 脚本壳**：H 崩溃恢复（cli-gap.md 给了 `kill-app` shell 包装样板，但归脚本壳不归 aijia CLI）

---

#### 场景 A：雇佣员工（数字员工 task 意图 1）— ✅ 已实现

7 条原子 CLI（`hire-open / hire-wait / hire-select-template / hire-next / hire-prev / hire-fill / hire-save`）+ 配套 selector 全部落地。典型组合见下方"已批量补完"段。

#### 场景 B：员工资源配置表单（数字员工 task 意图 2）— ✅ 已实现

5 条原子 CLI（`resource-fill / resource-add-row / resource-remove-row / resource-save / resource-cancel`）+ 5 个 ResourceConfigForm 的 `data-aijia-resource-*` 属性全部落地。`resource-fill --row N` 支持 MonitoringUrlsForm 的多行表单。SchemaForm 字段名从 JSON Schema property name 派生。

#### 场景 C：员工 cron toggle（数字员工 task 意图 5）— ✅ 已实现

合并进统一的 `aijia employee-drawer-action --action <verb>` —— `toggle-cron` / `toggle-cron-badge` / `edit-cron` / `add-cron-trigger` 都是 verb 之一。卡片上 cron 暂停/恢复按钮另有 `aijia employee-card-toggle-cron --name X` 走 card（不开 drawer）。

#### 场景 D：员工解雇（数字员工 task 意图 6）— ✅ 已实现（由两条已有原子组合）

解雇 = "点 fire 按钮" + "Radix Dialog 确认"，**是两个原子**，不是一条 `employee-fire`。已落地：

```bash
aijia employee-open-card --name "小研"
aijia employee-wait-drawer
aijia employee-drawer-action --action fire     # 弹 Radix ConfirmDialog
aijia handle-dialog --action accept            # 确认解雇
```

`window.confirm()` 已改造成 `requestConfirm` Radix（`EmployeeDrawer.tsx` 行 247 + 270 + 升级模板那条同步改了）。**不需要新 verb**。

#### 场景 E：归档员工拒派验收（数字员工 task 意图 7）— ✅ 已实现（2026-05-21）

`EmployeeCard.tsx` 加 `data-aijia-employee-dispatch-disabled={emp.lifecycle === 'archived' ? 'true' : 'false'}`。`aijia employee-status` 输出多吐 `dispatchDisabled: bool` 字段，**不新增 verb**。

```bash
aijia employee-status --name "小研" | jq .dispatchDisabled   # 验收：archived 后为 true
```

> rules 里"直接 invoke `employee_trigger(id)` 测后端拒绝"这条**不补 CLI**——后端 IPC 验收不归 L4 意图测试。author 应改 rules 用 UI 信号。

#### 场景 F：日程编辑（日程 task 部分意图）— ⏳ 阻塞在 rules.md

`agenda-*` 11 个原子命令已全部落地。问题在 rules.md 跟当前 UI 命名漂移（"日程" → "定时任务"，"标题" 字段不存在，"组织者员工" → "负责员工"，"default 员工 record" 现实不存在等，详见 `run-reports/2026-05-20.md` Task 1）。

**agent 退路**：用 `test-intents-author` skill 重写日程 rules.md，按当前 UI 形态对齐。

#### 场景 G：登录页 selector — ✅ 已实现

`[data-aijia-login-error]` + `[data-aijia-product-name]` 两个 selector 已在 LoginPage.tsx / TenantHeader.tsx 落地。CLI 不需要新 verb，rules 可直接用 `document.querySelector('[data-aijia-login-error]')?.textContent` 读出错误文本。

> 仍剩产品 bug：`authStore.isAuthPending` 在 401 后不复位 false（导致 `aijia login` 错密码返 `timeout` 而非 `login_failed`）。CLI 端已正确判到 `transitional` 阶段，等待产品修复 store。

#### 场景 H：崩溃恢复（崩溃恢复 task 全部）— ❌ 不属于 aijia CLI 范畴

**澄清**：`aijia` 子命令是"webview 内的 UI 操作打包器"。`pkill -9 + 重启 dev server + 等 socket` 是**操作系统进程级动作**，跟 webview DOM 毫无关系，硬塞进 aijia 命令会破坏"aijia = UI 原子"的语义边界。

**agent 退路**（在 runner skill / rules.md 里直接用 shell，建议沉淀进 `scripts/aijia-restart.sh`）：

```bash
# 1. 杀进程（只杀 aijia 子进程，不要 pkill aijia 它会连带杀父 cargo）
pkill -9 -f 'target/(debug|release)/aijia' || true

# 2. 等 socket 消失
while [ -S /tmp/tauri-pilot-com.aijia.app.sock ]; do sleep 0.2; done

# 3. 重启（用户另一个 terminal 已经在跑 pnpm dev:with-pilot，崩溃后它会自动 spawn）
# 或：cd lotus-app && pnpm dev:with-pilot &

# 4. 等 socket 回来
until [ -S /tmp/tauri-pilot-com.aijia.app.sock ]; do sleep 1; done

# 5. 等 webview ready
until aijia health-check --json 2>/dev/null | jq -e .ok > /dev/null; do sleep 1; done
```

这 5 步**不要**当 aijia 子命令做。原因：① kill 不是 UI 动作 ② socket 监控不是 UI 动作 ③ aijia CLI 在 socket 重建期间无法工作（封装它自己的重启很矛盾）。由 runner agent shell 直接跑。

#### 场景 I：新建对话时选 workspace（工作空间 task）— ✅ 已实现（2026-05-21）

**产品真相**：lotus-app **只有一个 workspace 动作 = "新建对话时选一个目录"**，没有独立的"授权 / 撤销 / 切换"流程。
- 入口在首页 composer 底部 workspace bar 的 DropdownMenu
- 三种来源：① 最近用过的（recent，state 在 `useHomeStore.recentWorkspaces`）② 默认项目（`get_default_folder` IPC）③ 选其他目录（OS folder dialog，走 IPC `pick_local_directory`）
- **发送时**（`handleSubmit`）`authorize_local_directory` IPC 被调用，授权和新对话**绑定**
- ChatTopBar 只读展示 workspace，已建对话**无法切换**

调研：`revokeAuthorizedWorkspace` 在 `src/lib/tauri.ts` 导出但**无任何 UI 组件调用**；不存在已授权列表 UI；无 `list_workspaces` / `set_active_workspace` IPC。所以旧版 cli-gap.md 列的 `workspace-authorize / workspace-revoke / select-workspace` 都是空中楼阁需求，**已删除**。

**lotus-app 改造**：

- `src/main.tsx` dev hook 加 `_pickDirectoryMockQueue: [] as string[]`
- `src/lib/tauri.ts::pickLocalDirectory` 第一行加 dev mock 检查（仿 `useChatAttachments::pickAttachments`）；命中跳过 OS dialog 直接 return queued path；下游 `authorizeLocalDirectory` + state 写盘真跑
- `HomeTaskComposerCard.tsx` workspace bar selector：
  - trigger 按钮：`data-aijia-workspace-trigger`
  - recent 行：`data-aijia-workspace-recent` + `data-aijia-workspace-path={ws.rootPath}`
  - "使用默认项目" item：`data-aijia-workspace-action="pick-default"`
  - "选择其他目录" item：`data-aijia-workspace-action="pick-other"`

**3 条原子 CLI**：

| CLI | 原子动作 |
|---|---|
| `aijia workspace-queue-path --path <p>` | 入队 OS folder dialog 的 mock 返回值 |
| `aijia workspace-open-picker` | 点 workspace bar trigger 打开 dropdown |
| `aijia workspace-pick --variant default\|other\|recent [--path <p>]` | 点 dropdown 内一个 item；`other` 触发 OS dialog（或消费 mock 队列）；`recent` 必须配合 `--path` |

**典型组合**（rules.md 范例）：

```bash
# 选一个新目录新建对话
aijia goto home
aijia workspace-queue-path --path "/Users/x/myproject"   # 入队
aijia workspace-open-picker
aijia workspace-pick --variant other                     # 消费 mock，pickLocalDirectory return queued path
aijia type-message "帮我看看这个项目结构"
aijia send                                               # handleSubmit 触发 authorize_local_directory
aijia wait-reply
# 验收：新会话 conv.json 含 workspacePath = "/Users/x/myproject"
```

```bash
# 用默认项目新建对话
aijia workspace-open-picker
aijia workspace-pick --variant default
aijia type-message "你好"
aijia send
```

```bash
# 复用最近用过的 workspace
aijia workspace-open-picker
aijia workspace-pick --variant recent --path "/Users/x/myproject"
```

> **OS folder dialog mock 合规性**：webview JS 操作不了 NSOpenPanel，但前端 `pickLocalDirectory()` 那一行是合法 mock 点——它是 lotus-app 自己代码的入口，下游 `authorize_local_directory` IPC + zustand `homeStore` + composer bar 状态更新全部真跑。同 `composer-queue-files` 模式。

#### 场景 J：人格管理（人格 task 全部）— ❌ 产品已废弃

memory `project_persona_deprecation_2026-05-10.md` 写明 persona 准备废弃，agenda 已改派 Employee，PR-5 完结。

**agent 退路**：
- 人格 task 的 rules.md 整组应该删除（走 author skill 走"deprecated"流程，迁移到 `archived-rules/` 目录）
- 不要花时间补 CLI；如果某条意图描述的能力被合并到了"数字员工"里，author 把它迁过去用 employee CLI

#### 场景 K：专家团队（专家团队 task）— ✅ 已实现（2026-05-21）

**产品真相**：专家团是**静态启动器**——`src/features/expert-teams/teams.ts` 硬编码 8 个 team，每个团的 experts 也是硬编码。没有"创建团队 / 添加 teammate / 删除"等 CRUD。用户视角只有一个动作：**点 card 启动新对话**。

调研：
- 入口 `src/App.tsx:69` `case 'expert-teams': <ExpertTeamsPage />`
- 点 card 走 `handleStart`（`ExpertTeamsPage.tsx:21-66`）：`createConversation` → `renameConversation` → `setExpertTeam(convId, teamId)`（localStorage 打标）→ `setRoute({ kind: 'chat', conversationId })`
- 无 `expert_team_*` IPC，无 drawer，无添加 teammate UI，无删除按钮

所以旧版 cli-gap.md 列的 `expert-team-create / fill / save / open-card / drawer-action / pick-teammate` 等 10 条 CLI 全是空中楼阁，**已删除**。

**lotus-app 改造**：

`ExpertTeamCard.tsx` 加 selector：
- `data-aijia-expert-team-card`
- `data-aijia-expert-team-id={team.id}`
- `data-aijia-expert-team-name={team.name}`

**1 条原子 CLI**：

| CLI | 原子动作 |
|---|---|
| `aijia expert-team-start --name <s>` | 点 ExpertTeamCard 启动新对话 |

**典型组合**（rules.md 范例）：

```bash
aijia goto expert-teams
aijia expert-team-start --name "增长团"
# 验收 1：route 切到 chat
aijia where --json | jq .route                      # 期望 "chat"
# 验收 2：新对话 localStorage 含 expertTeamId
# 验收 3：发消息后 director prompt 注入
aijia type-message "我们要拉新用户，怎么做？"
aijia send
aijia wait-reply
```

> **不能测的**：自定义团队 / 自定义 experts / 添加成员等，**产品没这些功能**。如果 rules 写了这类意图，让 author 删 / 迁档。

#### 场景 L：钉钉频道（钉钉频道 task 全部）— ❌ 环境依赖太重，建议人工跑

跑这个 task 需要：① 真实租户 ② 钉钉 AppKey/AppSecret/RobotCode ③ 一个能 @机器人的群 ④ 钉钉 webhook 回调能打到本机。**凭据不能 mock**（钉钉服务端会验签），网络回调也无法在 e2e 环境复现。

**agent 退路**：
- rules.md 钉钉 task 改为"semi-automated"标识，只跑 UI 路径（凭据用占位 string，落盘行为可以验，但发消息不会真打到钉钉）
- 真凭据 / 真消息这部分走人工 QA，每个 release 候选版人工 smoke test 一次（参考 release 流程文档）
- 不为这条 task 在 e2e 体系下补 CLI；如果将来 lotus 提供 sandbox 钉钉模拟器，再考虑

如果你只想测"填表保存"那一步：

```bash
# 用占位凭据，验证表单保存路径
aijia goto channel
# 后续 channel-* 原子命令同 team / settings 模式
```

selector 命名约定与 settings 一致：`data-aijia-channel-field={key}`、`data-aijia-channel-action="save"`。

#### 场景 M：设置面板填字段（设置 task 全部）— ❌ 不做（产品无此功能）

**2026-05-21 复核结论**：grep `src/components/settings/` 0 命中 `model` / `apiKey` / `baseUrl`。实际的 SettingsMenu 7 个 panel：

| key | 实际功能 |
|---|---|
| `account` | 退出登录、个人信息展示（GeneralPanel.tsx）|
| `account-billing` | personal 租户余额 / 月消耗 / 流水（AccountBillingPanel.tsx）|
| `archived` | 已归档会话列表（ArchivedPanel.tsx）|
| `runtime` | Node/Python/uv 诊断（RuntimePanel.tsx）|
| `about` | 版本信息（AboutPanel.tsx）|
| `usage` / `permissions` / `mcp` / `sso` / `shortcuts` | `disabled: true`，UI 占位不可点 |

理由：lotus-app 的 LLM 走 lotus gateway（OpenAI 兼容协议），凭据由租户后台配置，**桌面端用户不暴露 API key / 模型切换 UI**（参见 CLAUDE.md「lotus LLM 接入与 model id 路由现状」）。

**agent 退路**：
- rules.md 设置 task "改 API key / 切模型" 等意图是**对产品的错误假设**，应整组删除或迁档（走 author skill）
- 设置面板能验的事很少：① 退出登录 → 已有 `aijia logout` ② 计费数据展示 → 用 `aijia eval` 读 DOM（read-only 兜底，违反铁则但仅限读）③ 已归档会话恢复 → 如果真要测，补一条 `aijia archived-restore --id <s>`（冷门，等真用时再补）
- **不要**为不存在的功能补 CLI

> 历史教训沉淀：写场景待补清单时**先用 grep 验证产品功能是否存在**，再列 CLI。本场景之前列了 `settings-fill --field model.apiKey` 是凭设计想象写的，产品根本没这个字段。同类错误以前在 rules.md 漂移问题里也出现过（"日程" → "定时任务"等）。

#### 场景 N：待办队列（待办队列 task 全部）— ⏳ 阻塞在 rules + 产品语义

CLI 端无明显缺口（聊天流复用 `type-message / send / wait-reply / where`）。**阻塞在 pending 触发条件不明确**：
- HANDOFF：实测找不到 pending 触发方式
- `session_id ↔ conv_id` 命名混乱（memory `project_intent_test_gaps_2026-05-20.md`）

**agent 退路**：
1. author skill 先找产品 owner 对齐 `enqueue_or_send` 的"忙判定"信号（streaming 中？toolCall 中？AgentIdle 之前？）
2. 对齐后看是否需要 mock provider（让流式可控）来稳定复现 pending 入队
3. 如果产品认为"用户连发"应该走前端 dedup / 节流而不入队，rules 整组应该改语义（pending 不是用户视角的概念）
4. 都没着落前，把 task 标 `BLOCKED-PENDING-PRODUCT`，agent 跳过

---

### 状态汇总

| 状态 | 场景 |
|---|---|
| ✅ 已实现，可直接跑 | A 雇佣 / B 资源配置 / C cron toggle / D 解雇 / E 派活按钮 disabled / G 登录 selector / I 新建对话选 workspace / K 专家团队启动 |
| ⏳ 阻塞在 rules.md | F 日程（rules 漂移）/ N 待办队列（pending 语义未定）|
| ❌ 不做（已给 agent 退路） | H 崩溃恢复（走 shell 脚本，不是 aijia CLI）/ J 人格（产品废弃，rules 应迁档）/ L 钉钉（环境依赖太重，半自动 + 人工 QA）/ M 设置 fill（产品无模型/API key 切换 UI，意图是对产品的错误假设）|

> **2026-05-21 收口**：14 个场景里 8 个 ✅ 已实现、2 个 ⏳ 阻塞 rules.md（CLI 无能为力）、4 个 ❌ 不做（产品没此功能 / 不属于 CLI 范畴）。**CLI 端无 ⏳ 真未做**。后续工作的瓶颈在 ① rules.md 重写（F / N）② 删 / 迁档 J / M 这类对产品的错误假设。

---

### 已实现但需要修复的 CLI 行为问题

仅列「现成 CLI 有 bug / 不准」的，不重复列上面待补的清单。

| CLI | 问题 | 修复方向 |
|---|---|---|
| `aijia wait-reply` | 工具链长 turn 中提前误判停止（详见下方独立段） | 改监听 `AgentIdle` 事件而非只看 `isStreaming` |
| `aijia login` 错密码 | 已修一半（不再误报 logged_in），但 reason 仍 `timeout` 而非 `login_failed` —— 根因在产品 `isAuthPending` 不复位 | 产品方先修 `isAuthPending`，CLI 再改判定 |
| `aijia screenshot` | `html-to-image` 序列化 + 并发 eval 时报 `eval timed out`（runner skill §5.5）| 用 `tauri-pilot screenshot <path>` raw 命令兜底 |

---

### `aijia upload-file <local-path>` — ✅ 已用 composer-queue-files + composer-click-plus 实现（见上方"Composer 上传文件"段）

- Drag-drop 路径（HTML5 drop）确实进不去；但 + 号按钮路径已通过 dev-only mock 队列方案打通
- 该方案不破坏铁则：唯一 mock 的是 OS-level dialog 本身（不在 AIjia 代码范围），下游 `makePendingAttachment / insertAttachmentTokens` 全跑真实代码

**端到端验收（2026-05-20 17:46）**：

```bash
$ aijia composer-queue-files --paths "$HOME/Downloads/aijia-upload-test/test-upload.txt" --json
{ "ok": true, "queueDepth": 1, "queued": 1 }

$ aijia composer-click-plus --json
{ "ok": true }

# 验收 DOM：
# document.querySelector(".ProseMirror") 内出现
#   <span class="react-renderer node-attachmentToken">
#     <span class="shrink-0 ...">CSV</span>
#     <span class="truncate">test-upload.txt</span>
#     <button aria-label="remove attachment">…</button>
#   </span>
# → 文件名 test-upload.txt 渲染正确，chip 完整可见可删
```

**发现产品 bug 候选**：`.txt` 文件 chip 上的类型标签显示为 `CSV`（应该是 `TXT` 或 `文本`）。`makePendingAttachment` 推断 chip 类型时对 `.txt` 扩展名 fallback 到了 `CSV`——可能是字符串对照表缺 txt 行、或 mime 推断 `text/plain` → 错路由到 csv。建议看 attachment-chip 的类型标签推断逻辑。

**端到端发送 + 落盘验收（2026-05-20 17:53）**：

继续测把 attachment 真发出去看 messages.jsonl 落盘格式：

```bash
$ aijia new-task             # 新建对话避免污染
$ aijia composer-queue-files --paths "$HOME/Downloads/aijia-upload-test/test-upload.txt"
$ aijia composer-click-plus  # chip 在编辑器显示
$ aijia type-message "请用一句话总结这个文件的内容"
$ aijia send                 # 真发送，消耗 token
$ aijia wait-reply --timeout 60
```

新对话 `48bc5180-d7e3-44e7-93ab-e935d8ac0569` 的 `messages.jsonl` 第 1 行 user 消息 `content.files[0]`：

```json
{
  "fileName": "test-upload.txt",
  "fileType": "csv",                                          // ❌ 应该是 "txt" 或 "text"
  "fileSize": 0,                                              // ❌ 实际 139 字节
  "mimeType": null,                                           // ❌ 应该是 "text/plain"
  "status": "uploaded",
  "filePath": "/Users/a20250311/Downloads/aijia-upload-test/test-upload.txt",
  "id": "/Users/a20250311/Downloads/aijia-upload-test/test-upload.txt",
  "kind": "file"
}
```

而且 `conversations/48bc5180.../uploads/` 目录为空，整个 scope 下也没找到 `test-upload.*` 副本（`find ~/.renlijia/users/t_28__u_54 -name 'test-upload*'` 零命中）—— 当前实现只把源路径写进 messages，不拷贝副本。**这一条是设计观察，不是 bug**：rules.md 数字员工 task 没有任何意图要求"`uploads/` 必须存在副本"，CLAUDE.md 提到 `conversations/{id}/uploads/` 是仓库总览的存储结构描述，不等同于某条意图承诺。**只依据 rules.md 写下来的字段判 bug**，副本机制等到有 author 写明确的意图后再判。

**新增产品 bug 候选（rules-anchored，3 条都已在 messages.jsonl 磁盘上有证据）**：

1. **`.txt` → `fileType: "csv"`** —— 类型标签推断错误
2. **`fileSize: 0`** —— size 探测没读真实文件大小（应该是 139）
3. **`mimeType: null`** —— mime 推断没执行

## ⚠️ `aijia wait-reply` 在工具链长 turn 中提前误判停止（2026-05-21）

**问题**：跑数字员工 task 意图 3 时，dispatch 后 AI 走完整工具链（`Skill → SearchMemory + 4×WebSearch → Write → WriteMemory`，共 9 个 tool_use turn）。`wait-reply --timeout 120` 在 dispatch 之后 ~10s 内就返 `{ok:true, stableTicks:3, streaming:false}`。但 chatStore 此时只有 1 条 user，**0 条 assistant text**——AI 实际还在跑工具调用阶段。

**症状识别**：
- `aijia ui-message --include-tools` 只返 1 条 user，0 条 assistant
- `aijia last-reply` 返 None / 报 NoneType error
- 截图看 UI 仍在显示"思考中..."、tool bubble 还在 spinner
- 磁盘 `messages.jsonl` 看到多条 `role: assistant, content.text: ""` 但 `toolCalls: [...]` 非空 —— assistant 在做 tool_use only turn，正文还没出

**根因**：`wait-reply` stability window 只看 `isStreaming === false` 的 3 连续 ticks。但 AI 走多步工具链时，每步 tool_use 完成 → 等 tool_result → 下次 tool_use 之间，`isStreaming` 会瞬时为 false 多个 tick；这些"工具间隙"被误判为 turn 结束。

**正确语义**：
- 真完成应该是 `streamDone` 事件触发后、且**没有**待 ack 的 tool_use；或者更准确：`AgentIdle` 事件后才算 turn 完成
- runner skill §5.2 已写「3 连续 ready tick」但没区分"工具间隙 vs 真 idle"——需要升级

**临时绕路**（agent 跑测时手动用）：

```bash
# wait-reply 返回后再等一阵看 messageCount 是否还在增长
prev=$(aijia where --json | jq -r .messageCount)
sleep 30
cur=$(aijia where --json | jq -r .messageCount)
[ "$cur" = "$prev" ] || echo "AI 仍在跑，messageCount $prev → $cur"
```

**修复方向**：
- `wait-reply` 应监听 `AgentIdle` 事件（runtime 已发，TauriEventAdapter 已映射）而不是只看 isStreaming
- 或者新增 `aijia wait-agent-idle [--timeout 300]` 区分两种语义；当前 wait-reply 保留给"单 turn 简单回复"场景

实战出处：数字员工 task 意图 3，2026-05-21 09:50。dispatch → wait-reply 10s 就返 ok，实际 AI 在工具链跑了 2 分钟才真出 text。



**conv.json 落盘正确**：标题被 AI 提取为 "E2E上传测试文件内"（截断了"容"），说明前置链路是真在跑、LLM 真在用 attachment 内容。

### `aijia search --query <q> --scope local|global` — P2 待 UI

- 当前 grep 没找到 search 面板组件 / `Cmd+K` 入口；可能未实现
- 待 lotus-app 加搜索 UI 后再封 CLI

## 与现有清单的关系

本批补完后，runner skill §4.2 的命令清单从 16 增至 ~35（含原子化拆分），13 个 task 中除 upload / search / 钉钉外的意图主路径都能用原子命令序列覆盖。

## 设计原则（这次实战沉淀，与 `.claude/skills/test-intents-cli-author/SKILL.md` 一致）

1. **原子 + 组装**：一条 CLI 做一个原子 UI 动作；多步流程由 rules.md 串
2. **selector 优先级**：`data-aijia-*` > id > aria-label > textContent
3. **每个可被 CLI 操作的元素，lotus-app 侧加 `data-aijia-<noun>`**（详见 skill 的命名约定表）
4. **永远不能模拟"靠 React store 推 state"绕过 UI**——CLI 是 UI 操作打包器，不是 IPC / store 替身
5. **失败信号必须结构化**：`{ok: bool, reason?: snake_case, ...诊断字段}`，不抛非零退出码（除 socket 断连等 infra 故障）

## ⚠️ rules.md 不应让 agent 删盘上数据来"满足前提"（2026-05-20）

**问题**：登录 task `rules.md` 多条意图（1 / 2 / 3 / 5）的「前提」段写：

> `~/.renlijia/global/auth/active_account.json` 在测试前不存在（或先手动删除以确保从 0 开始）

这违反 runner skill §6 环境契约「直接在真实 `~/.renlijia/` 跑、不隔离、跑后**不**清理」。如果 agent 严格按字面执行，会把当前登录账号的 active_account.json 删掉、甚至连带 `users/t_28__u_54/` 整个 scope 目录（含历史会话 / 项目记忆 / agenda / employees 等真实数据）一起清，**破坏现场**。

**正确做法**：

- 「从 0 开始」并不必须，登录这条意图测的是「登录后**写入**这一步」，验收应该用 **mtime 变化** + sha256 改变 + 登录后字段非空来判定，不依赖"测试前文件不存在"
- 类似「文件不存在」「目录为空」的前提语句，rules 作者**不应该写**——意图测试不准 sandbox / 不准清理，作者写了等于把��条意图变成"必须先破坏现场才能跑"
- 如果某条意图本质上要测"首次登录冷启路径"（即 active_account.json 真的从未写过的代码分支），那只有**新电脑 / 新用户**能复现，应该在 rules 里直接写明「本意图只在新电脑上跑，否则 SKIPPED」，而不是让 agent 删文件硬复现

**待 author 修复的范围**（rules.md 提级修订）：

| Task / 意图 | 违反前提 | 改写方向 |
|---|---|---|
| 登录 / 意图 1 | `active_account.json` 不存在 | 改为：记录登录前 mtime + sha256，验收登录后 mtime 改变 + 字段非空 |
| 登录 / 意图 2 | `active_account.json` 不存在 | 改为：记录登录前 mtime + sha256，验收登录失败后**未写入**（mtime 不变 + sha256 一致） |
| 登录 / 意图 3 | （隐含：登录态 → 登出后清理） | 现状 OK，但写「文件被删除或内容已被清空」要明确二选一，不要让 agent 现场猜 |
| 登录 / 意图 5 | `users/{scope}/brand.json` 不存在 | 改为：登录前若 brand.json 已存在则记录 mtime + 内容 hash，验收登录后写入 / 覆盖；或明确"非首次登录可 SKIPPED" |

**追加项**：runner skill §6 环境契约里补一条「rules 写的前提不准要求 agent 删除现有文件 / 目录；agent 看到这种前提应记录为 `FAIL 主因 = rules/CLI 问题` 并跳过破坏性步骤」。

实战出处：登录 task 意图 1，2026-05-20 跑测。用户原话「为什么要删除啊 我操了 这个肯定是 rule 有问题」。

## ✅ `aijia login` 错密码场景误报已修复（2026-05-20）

**之前的问题**：见下面的"问题"段——错密码返 `ok: true, outcome: "logged_in"`，调用方紧接着 `where` 又会拿到 `loggedIn: false`，race condition 误判。

**修复要点（user 2026-05-20 反馈）**：
- ❌ 旧：DOM probe `#account` 消失就判 logged_in —— 被 `<FullscreenLoader/>` 中间态骗到
- ✅ 新：直接读 `authStore.{isLoggedIn, isAuthPending}`：
  - `isAuthPending=true` → 还在等，继续 poll（之前误判的窗口现在被这条吞掉）
  - `isLoggedIn=true && isAuthPending=false` → 真成功 + 返 `tenantId/userId/scope`
  - `isLoggedIn=false && isAuthPending=false` + 看到 `.text-destructive` → 真失败 + 返 `{ok:false, reason:"login_failed", error:"..."}`
  - 中间瞬态 → 继续 poll
- 返参补 `tenantId/userId/scope`，调用方拿到 `ok:true` 就有数据，不用再调 `where` race

**实测验证（2026-05-20 17:25）**：

```bash
$ aijia login --account '18267316753@pzctest' --password '18267316753' --json
{ "ok": true, "outcome": "logged_in", "scope": "t_28__u_54", "tenantId": 28, "userId": 54 }
# ✅ 正密码：一次拿全 scope/tenantId/userId，不再 race condition

$ aijia login --account '18267316753@pzctest' --password 'wrong' --json
{ "ok": false, "reason": "timeout", "lastProbe": {"phase": "transitional", "ready": false}, "timeoutSec": 20 }
# ⚠️ 错密码：从 "误报 logged_in" 改成了 "timeout"，但还差一步——
# 期望 {ok:false, reason:"login_failed", error:"..."}，实测卡在 transitional 阶段超时
# 根因不是 CLI 而是产品：authStore.isAuthPending 在 401 后没复位为 false
# （详见登录 task 意图 2 报告：错误提示"只闪了一下"，store 也没正确退出 pending）
```

**剩余 gap（非 CLI，是产品 bug）**：错密码后 `authStore.isAuthPending` 应在收到 401 时立即设为 false，并把后端 error 文本写入 `loginError` state（让 LoginCard 渲染 `.text-destructive`）。这是登录 task 意图-登录-002 FAIL 的根因。

## ⚠️ `aijia login` 错密码场景误报 `ok: true / outcome: "logged_in"`（2026-05-20，已修复见上）

**问题**：跑登录 task 意图 2（错密码登录应该失败）时，调用：

```bash
tauri-pilot aijia login --account '18267316753@pzctest' --password 'wrong-pwd-001' --json
# → {"ok": true, "outcome": "logged_in"}
```

但实际：

- `aijia where --json` 仍 `loggedIn: false`
- `~/.renlijia/global/auth/active_account.json` mtime 未变（还是意图 1 留下的 17:11，错密码登录尝试后未被覆盖）
- dev server log 里**没有任何 cloudLogin 调用 trace**（log 末尾停在 logout 那行）
- DOM 里密码框已被清空（`value=""`），账号框保留 `18267316753@pzctest`——说明 LoginCard 确实在登录页停下了（这是 UI 预期行为）

**根因猜测**：当前实现可能是「填表单 → 点登录按钮 → 等 LoginPage 这个组件 unmount」。如果实际后端验证失败，UI 不会 unmount LoginCard 而是清密码框 + 显错误，但当前 CLI 的"unmount 等待"逻辑要么没等到要么 race condition 误判为成功；也可能根本没真的发请求（按钮 disabled 状态没识别）。

**正确行为应该是**：

| 真实情景 | `aijia login` 应该返 |
|---|---|
| 后端 200，cloudLogin 成功，LoginPage unmount | `{ok: true, outcome: "logged_in"}` |
| 后端 401 / 错密码 / 账号不存在，UI 显错误 | `{ok: false, reason: "invalid_credentials", error_text: "<UI 上的错误提示文本>"}` |
| 后端超时 / 网络断 | `{ok: false, reason: "network_timeout"}` |
| 账号被锁 | `{ok: false, reason: "account_locked"}` |

判定 `ok: false` 的信号应该是「LoginPage 仍然挂载且 `data-aijia-login-error`（或 `.text-destructive`）有非空文本」，**不**等同于"超时未 unmount"。

**实际验证方法**（实现时参考）：
1. 提交后等 LoginPage unmount，超时 `--timeout` 秒
2. 超时后回看 LoginPage 是否还在 → 还在 = 失败，去读 `data-aijia-login-error` 文本作为 reason 给 caller
3. 加 `data-aijia-login-error` 属性到 `src/components/auth/LoginCard.tsx`（或现在挂错误文本的节点）方便 CLI 抓取

**影响的意图**：
- 意图-登录-002（核心被这条挡掉）
- 未来任何走 `aijia login` 失败路径的意图（账号锁、租户错路由、网络断）

实战出处：登录 task 意图 2，2026-05-20 跑测。用户原话「你这个记录到 cli 的问题里面吧 我让人修复」。

## ✅ `data-aijia-nav` 已落地（2026-05-20 补 CLI 第二轮）

`src/components/sidebar/SidebarNav.tsx` 第 31-44 行的 6 个 nav button 已加 `data-aijia-nav={key}` 属性。`aijia goto employees | expert-teams | skill-center | schedules | channel | home` 单点解锁。

之前的现象：`aijia goto <page>` 一律返 `{"ok": false, "reason": "nav_button_not_found"}`，整 task 7（数字员工）等大半个 task 矩阵被卡死。

## ⚠️ `data-aijia-nav` 在 lotus-app `SidebarNav.tsx` 完全没落地（2026-05-20，已修复见上）

**问题**：`aijia goto <page>` 一律返 `{"ok": false, "page": "...", "reason": "nav_button_not_found"}`。

DOM 探查（已登录态，主界面）：
- `document.querySelectorAll("[data-aijia-nav]")` → `[]`（零命中）
- 侧栏 8 个按钮（AI小家 / 新任务 / 数字员工 / 专家团 / 技能中心 / 定时任务 / IM 频道 / 项目）`dataNav` 全部 `null`

源码 `src/components/sidebar/SidebarNav.tsx` 的 6 个 nav button 标签上没有 `data-aijia-nav={key}` 属性。需要在 line 33 之后 button 标签内加：

```tsx
<button
  key={key}
  type="button"
  data-aijia-nav={key}            // ← 加这行
  onClick={() => onSelect(key)}
  ...
>
```

**影响范围**：所有用 `aijia goto employees / expert-teams / skill-center / schedules / channel / home` 的意图，包括数字员工 / 专家团队 / 技能 / 工作空间 / 日程等大部分 task 都被这条挡住。

## ✅ 数字员工 task CLI + selector 已批量补完（2026-05-20 第二轮）

按"全走 UI、不绕过 IPC"原则做了以下补充。**铁则重申：CLI 是 UI 操作打包器，不是 IPC 替身**。`employee_active_run / employee_stop_run` IPC 后端虽然已注册（src-tauri/src/commands/employees.rs:307,335），但 CLI **不调用它们**——running 态查看通过 EmployeeCard 上的 `data-aijia-employee-status` attr 读取，停止 run 通过 drawer 的"停止"按钮（`data-aijia-employee-action="stop"`）走 UI。

### lotus-app 补的 selector

| 文件 | 加的 attr |
|---|---|
| `src/components/sidebar/SidebarNav.tsx` | `data-aijia-nav={key}` |
| `src/components/auth/LoginPage.tsx` | `data-aijia-login-error`（错误 div） |
| `src/components/sidebar/TenantHeader.tsx` | `data-aijia-product-name`（productName div） |
| `src/features/home/EmployeesPage.tsx` | `data-aijia-hire-button="template-market"` |
| `src/features/employees/EmployeeCard.tsx` | `data-aijia-hire-button="add-card"` + `data-aijia-employee-status={status}` + `data-aijia-employee-cron-enabled={...}` + `data-aijia-employee-action="pause-cron\|resume-cron"`（card 上的 cron 按钮） |
| `src/features/employees/EmployeeDrawer.tsx` | 7 个新 action attr：`view-chat / stop / edit-cron / toggle-cron / toggle-cron-badge / add-cron-trigger / config-resource / fire`；解雇 + 升级模板的 `window.confirm` 换成 `requestConfirm`（走 Radix ConfirmDialog，`aijia handle-dialog --action accept` 可统一拦） |
| `src/features/employees/HireWizard.tsx` | root `data-aijia-hire-wizard` + `data-aijia-hire-step={1\|2\|3}` + 模板 card `data-aijia-hire-template` / `data-aijia-hire-template-id` / `data-aijia-hire-template-name` + 字段 `data-aijia-hire-field="name\|cron"` + 底部 `data-aijia-hire-action="prev\|next\|save"` |
| `src/features/employees/forms/MonitoringUrlsForm.tsx` | root `data-aijia-resource-form="monitoring-urls"` + 每行 `data-aijia-resource-row={i}` + 字段 `data-aijia-resource-field="name\|url\|tags"` + `data-aijia-resource-action="add-row\|remove-row\|save\|cancel"` |
| `src/features/employees/forms/SalesTableConfigForm.tsx` | root + `data-aijia-resource-field="shareUrl\|baseId\|tableId\|fieldMapping"` + save/cancel action |
| `src/features/employees/forms/CustomerSupportConfigForm.tsx` | root + `data-aijia-resource-field="greeting\|closing"` + save/cancel action |
| `src/features/employees/forms/TechSupportConfigForm.tsx` | root + save/cancel action |
| `src/features/employees/forms/WeeklyReportConfigForm.tsx` | root + `data-aijia-resource-field="watchGroups"` + save/cancel action |
| `src/features/employees/forms/SchemaForm.tsx` | root + 每个 FieldRow `data-aijia-resource-field={schemaPropertyName}` + save/cancel action |

### tauri-pilot 加的 CLI（13 条原子）

```
hire-open --variant template-market|add-card        # 点雇佣入口
hire-wait [--timeout 3]                              # 等 wizard mount
hire-select-template --id <s> | --name <s>           # 点模板 card
hire-next | hire-prev | hire-save                    # 底部按钮
hire-fill --field name|cron --value <s>              # 填字段
employee-status --name <s>                           # 读 card 上的 status + cronEnabled
employee-drawer-action --action <verb>               # 点 drawer 内任意 action（10 verb）
employee-card-toggle-cron --name <s>                 # 点 card 上的 cron 暂停/恢复
resource-fill --field <s> --value <s> [--row N]      # 填资源表单字段
resource-add-row | resource-remove-row --row N       # MonitoringUrlsForm 行操作
resource-save | resource-cancel                      # 资源表单底部按钮
```

新 CLI 共 13 条，PATH 上的 `tauri-pilot aijia --help` 子命令数从 35 → 47 → **59 条**。装到 PATH：

```bash
cd ../tauri-pilot && cargo install --path crates/tauri-pilot-cli --force
```

### 典型组合（数字员工-001 雇佣 monitoring-urls 类员工）

```bash
aijia goto employees
aijia hire-open --variant template-market
aijia hire-wait
aijia hire-select-template --name "竞争情报员"        # step 1
aijia hire-fill --field name --value "小研"           # step 2
aijia hire-next                                       # → step 3
aijia resource-fill --row 0 --field name --value "公司 A"
aijia resource-fill --row 0 --field url --value "https://example.com/news"
aijia resource-fill --row 0 --field tags --value "竞品, 新闻"
aijia resource-save                                   # 完成雇佣
```

### 典型组合（数字员工-006 解雇）

```bash
aijia goto employees
aijia employee-open-card --name "小研"
aijia employee-wait-drawer
aijia employee-drawer-action --action fire             # 弹 Radix ConfirmDialog
aijia handle-dialog --action accept                    # 确认解雇
```

### 现在仍未跑过的 task（非阻塞性原因）

| Task | 状态 | 阻塞原因 |
|---|---|---|
| 工作空间 | SKIPPED | `aijia select-workspace` 仍返 `not_implemented`，workspace picker selectors 未确定 |
| 崩溃恢复 | SKIPPED | `aijia kill-app` 未实现，需要 `pkill -9 + 重启 dev server + 等 socket` 包装 |
| 钉钉频道 | SKIPPED | 需真实租户级钉钉机器人凭据（AppKey/Secret/RobotCode），环境依赖太重 |
| 设置（API key） | SKIPPED | 缺 `aijia settings-fill --field model.apiKey` 包装 |
| 待办队列 | SKIPPED | 跟产品 owner 确认 `enqueue_or_send` 触发条件再写 rules |

## 数字员工 task 缺失 CLI 清单（2026-05-20）✅ 已批量补完，见上一节

跑数字员工 task 时发现，除了 4 个 `employee-*` 命令（open-card / wait-drawer / click-dispatch / close-drawer）和上面的 `aijia goto` selector，整套 task 还缺以下 CLI / selector：

### 缺失 CLI（按意图分组）

**意图 1 雇佣员工**：
- `aijia hire-wizard-open` — 点主页"雇佣员工"按钮（缺 `data-aijia-hire-button` selector）
- `aijia hire-wait` — 等 HireWizard 挂载
- `aijia hire-select-template --name <s>` — 模板网格按名称点 card
- `aijia hire-next` / `aijia hire-prev` — 步骤间切换
- `aijia hire-fill --field name --value <s>` — 第 2 步填员工名
- `aijia hire-save` — 第 3 步点保存完成雇佣
- 配套 selector：`data-aijia-hire-wizard / data-aijia-hire-template[name=...] / data-aijia-hire-step / data-aijia-hire-action="next|prev|save"`

**意图 2 资源表单**：
- `aijia employee-resource-fill --field <s> --value <s>` — 在 ResourceConfigForm 里填字段
- `aijia employee-resource-add-row` — monitoring-urls 表单加一行
- `aijia employee-resource-save` — 保存资源配置（区别于 dispatch）
- `aijia employee-card-status --name <s>` — 读卡片状态（`needs-setup / idle / running / has-report` 等）
- 配套 selector：`data-aijia-resource-form / data-aijia-resource-field=... / data-aijia-resource-action="add|save"`
- `data-aijia-employee-status` on EmployeeCard 显示状态短码

**意图 3 派活成功路径**：
- ✅ `aijia goto employees`（待 `data-aijia-nav` 补完）
- ✅ `aijia employee-open-card` / `wait-drawer` / `click-dispatch` 已有
- 验收期间需要：`aijia tool-calls --turn last` / `aijia ui-message --include-tools` 已有
- 配套：dispatch 返新对话后 `where.sessionId` 必须切到新值——目前的 `employee-click-dispatch` 文档说"caller polls"，但实测员工页根本进不去（被 `data-aijia-nav` 卡死）

**意图 4 Running 态查看**：
- `aijia employee-active-run --name <s>` — 包装 `employee_active_run(id)` Tauri 命令，返 `{ok, conversationId?, startedAt?}`（用员工 name 反查 id）
- `aijia employee-stop-run --name <s>` — 点 drawer 的"停止"按钮，封 `employee_stop_run`
- `aijia employee-card-status` 同意图 2

**意图 5 cron toggle**：
- `aijia employee-toggle-cron --name <s> --action enable|disable` — drawer 内"暂停 cron / 恢复"按钮
- 配套 selector：`data-aijia-employee-cron-toggle` on the toggle button
- 验收 `employee.json.cronEnabled / nextRunAt` 直接读盘即可，不需要 CLI

**意图 6 解雇员工**：
- `aijia employee-fire --name <s>` — drawer 内"解雇此员工"链接
- 配套 selector：`data-aijia-employee-action="fire"`
- 浏览器原生 `window.confirm` 弹窗如果是 native browser dialog，不能用 `aijia handle-dialog`（后者只处理 Radix AlertDialog）→ 应该改用 Radix ConfirmDialog（仓库已有），同时 CLI 补 `aijia native-dialog-accept` 或换实现

**意图 7 归档员工拒派**：
- `aijia employee-dispatch-button-state --name <s>` — 读派活按钮 `{disabled, text}`
- `aijia employee-trigger-direct --id <s>` — 绕过 UI 直接 invoke `employee_trigger(id)`，用于测后端拒绝路径（**注意**：这条违反"不绕过 UI"铁则，但意图本身就是"绕过 UI 测后端拒绝"，是合理例外）

### 不能跑的意图（汇总）

| 意图 | 阻塞 | 状态 |
|---|---|---|
| 数字员工-001 雇佣 | 全套 hire-wizard CLI 缺 + `data-aijia-nav` 缺 | SKIPPED |
| 数字员工-002 资源表单 | 资源表单 CLI 缺 + `data-aijia-nav` 缺 | SKIPPED |
| 数字员工-003 派活 | `data-aijia-nav` 缺 | SKIPPED |
| 数字员工-004 Running 态 | `employee_active_run` 没包装 + `data-aijia-nav` 缺 | SKIPPED |
| 数字员工-005 cron toggle | toggle CLI 缺 + `data-aijia-nav` 缺 | SKIPPED |
| 数字员工-006 解雇 | fire CLI 缺 + native confirm 缺 + `data-aijia-nav` 缺 | SKIPPED |
| 数字员工-007 归档拒派 | dispatch-button-state CLI 缺 + employee-trigger-direct 缺 + `data-aijia-nav` 缺 | SKIPPED |

**整 task 7 条全 SKIPPED**，主因都是 `data-aijia-nav` 没落地导致 `aijia goto employees` 永远到不了员工页。



---

## 2026-05-22 批次：意图测试 4 轮跑测（Haiku/Sonnet × 裸/加强 prompt）暴露的 4 个新 CLI 缺口

来源：2026-05-22 用同一份 prompt 对两个模型分别跑两轮意图测试，Sonnet 加强版 5/5 PASS 时识别出 2 个真缺口；同期审计技能 task rules.md 时识别出另 2 个缺口。

### 1. `aijia skill-refresh` — P0（解锁技能 task）

**封装**：`reload_skill` Tauri 命令（`src-tauri/src/lib.rs:1058` 注册，前端 wrapper `src/lib/tauri.ts:1848`，底层是 `plugin::skill::global_sync::reload_skill_registry`）。

**用途**：放好新 SKILL.md / good-skill / bad-skill 后**热加载** SkillRegistry，让 catalog 在不重启 app 的前提下生效。

**为什么必须有**：技能 task 全 6 条意图都隐含"放文件 → 让产品看到" 这一步；rules 老版本写"关闭应用→重启"违反"不许 kill app"约束（已在 2026-05-22 的 rules 修正中改写为"触发 reload_skill"，**等本 CLI 落地**）。

**签名建议**：`tauri-pilot aijia skill-refresh [--scope global|user]`，返回 `{ok, reloaded_count}`。

### 2. `aijia skill-list --json` — P0（技能 task 验收）

**封装**：`list_skills` Tauri 命令（`src-tauri/src/lib.rs:969`）。

**用途**：列当前 SkillRegistry 中的全部 skill `[{id, name, description, source, installedAt}]`，意图测试用来验收"放好 SKILL.md → 触发 reload → catalog 出现 demo-skill" 这条链路最后一步。

**为什么必须有**：当前没办法在不发对话的情况下验技能是否真进 catalog；2026-05-22 跑测时 Sonnet 只能问 AI"列出你的可用技能"——这违反 L4 testing 应"直接观察可观察信号"的原则。

**签名建议**：`tauri-pilot aijia skill-list --json` 返回 `[{id, name, description, source, ...}]`。

### 3. `aijia employee-open-card --id <id>` 与 `aijia employee-status --id <id>` — P1（多同名员工）

**实测背景**：2026-05-22 跑测环境累计有 6 个员工，4 个都叫"小研"。`--name 小研` 只匹配第一个，无法精确定位特定 employee id。

**用途**：当多个员工同名时按 employee id 精确定位卡片 / drawer / 状态读取。

**变更**：现有 `employee-open-card --name <substr>` 与 `employee-status --name <substr>` 加 `--id <employee-id>` 参数（互斥，二选一）。`--id` 走 `[data-aijia-employee-id="..."]` selector 直接命中。

**临时绕路**：跑测前给员工改个独特 name（runner 实际正在用这招）。

### 4. `aijia agenda-wait-occurrence --agenda-id <id> --status succeeded|failed --timeout <s>` — P2（提速日程意图）

**用途**：`agenda-row-action --action run-now` 后，目前只能轮询磁盘 `~/.renlijia/.../agenda/occurrences/{agendaId}/*.jsonl` 取末条记录的 `status` 等待收敛——加 CLI 包装后 runner 一行命令即可。

**实现**：内部 poll items.json + occurrences jsonl，匹配 `--status` 时返回 `{ok, occurrenceId, status, conversationId, finishedAt}`；**注意 jsonl 是 append-update 语义**（runner skill §5.7b 已注明），实现要 tail 取最新条而非 head。

**优先级 P2**：runner 可以自己写 poll 循环替代，不算阻塞，只是减负。
