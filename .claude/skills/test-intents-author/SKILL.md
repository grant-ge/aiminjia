---
name: test-intents-author
description: Use when writing, modifying, or deleting intent test specs (rules.md) for the AIjia app. Triggers from inside test-intents skill routing: "加一条意图", "改 X task 的 rules", "拆这条复合意图", "删 意图-XXX-NNN", "新建 X task".
license: Internal
---

# test-intents-author

## What this skill does

**怎么写 / 改 / 删一条意图**的完整方法论。本 skill 不负责跑意图（那是 `test-intents-runner`）。

## When to Use

- 加新意图到现有 task 的 rules.md
- 改老意图（标题 / 步骤 / 验收）
- 拆复合意图
- 删意图（硬删除）
- 新建一个 task（极少触发，先按 §6.1 验证是否真该开新 task）

## 1. ID 与排序

格式 **`意图-<task>-<NNN>`**：
- `<task>` = 该意图所属 task 中文名（如 `日程` / `技能` / `登录`）
- `<NNN>` = 三位顺序号
- **新意图取当前最大序号 +1**
- **删除意图不回收 ID**——会出现 ID 跳号（如 002 → 004），接受
- **rules.md 内多条意图按 ID 升序排列**——新加意图直接 **append 到文件末尾**，不重排
- **作废 = 硬删除整段**：不留废弃标记 / 不留占位 / 不留替代指针。任何废弃痕迹都会误导未来读 rules.md 的 agent。git log 承担历史解释成本

## 2. 标题命名（4 条硬规则）

格式 **`<触发条件>，<可观察结果>`**——两段、中文逗号 `，` 分隔。

1. **两段、中文逗号分隔**——超过 2 段说明是复合意图、回去拆
2. **不带技术名**——不写 React 组件名、IPC 命令名、DOM selector
3. **≤ 30 字**（不含 `意图-<task>-NNN:` 前缀）——超过说明信息密度低
4. **不用 ✓ ✗ `/` 这类符��当连接符**——这是验收 bullet 的符号

**反例 → 正例**：

| ❌ 现行漂移 | ✅ 改正 |
|---|---|
| 日程暂停 + T0+5min 恢复后验证调度未损坏 | 拆 2 条：`日程暂停后，到点不触发` + `日程暂停恢复后，下次到点正常触发` |
| 派活后调用 employee_active_run 返回 Some | 派活后，员工卡片显示「运行中」状态 |
| 导入 skill 草稿不存在 ✓ 正式目录出现 ✓ 技能中心可见 ✓ | 拆 3 条，各自只测一件事 |

## 3. 字段集（3 段，全部必填）

| 段名 | 内容 |
|---|---|
| **场景** | 1-3 句话，PM 视角描述用户在做什么、期望看到什么 |
| **操作步骤** | 编号步骤；agent 从第 1 步顺序跑到最后一步；**第一步永远是** `tauri-pilot aijia health-check` 探活；后续步骤包含搭环境命令 + 主测操作 |
| **验收标准** | 用两种 bullet 组织：`✅ 应该看到` 列 PASS 条件、`❌ 不应该看到` 列必须主动检查的反向陷阱 |

**没有「前提」段**——搭环境步骤直接放在「操作步骤」段最前面（agent 反正都要顺序跑）。
**没有「判定提示」段**——边界容忍写进验收的容忍范围（"`T0 ± 1 分钟`"）；通用诊断套路在 `test-intents-runner` skill 的经验库。

## 4. 「操作步骤」段措辞规则

### 4.1 一条意图一件事

由 ID 系统强制：一条 `意图-<task>-<NNN>` 只能有一组「操作步骤」+ 一组「验收标准」。

### 4.2 禁用技术术语 / 组件名 / DOM selector

| ❌ 不要这样写 | ✅ 改成这样 |
|---|---|
| 等待 `AgendaItemEditor` sheet 打开 | 等新建表单展开（能看到「标题」「Prompt」「开始时间」等输入项） |
| 等待 `[data-testid="agenda-editor"]` 出现 | 等新建表单展开（能看到「标题」「Prompt」「开始时间」等输入项） |
| 通过 IPC 调用 `employee_active_run` | 点击员工卡片右上角的「派活」按钮 |
| 触发 `tauri-pilot aijia click` | 点击「保存」按钮 |

### 4.3 等待信号写用户能看到的 UI 文案

「等表单展开（能看到「标题」「Prompt」等输入项）」——agent 跑时会自己把「能看到 X 文案」翻译成 `aijia ui-message` / `aijia where` 的 poll 逻辑，rules.md **不操心这层**。

## 5. 「验收标准」6 条书写规则

### 规则 1：每条 bullet 必须可机器观察

禁主观词：「成功」「正常」「合理」「正确」「应该」——agent 无法机械判定。

| ❌ 模糊 | ✅ 具体 |
|---|---|
| 日程被成功创建 | 日程列表出现一行标题为 `早会提醒` 的条目 + JSON 文件存在 + `status == "active"` |
| 保存成功 | 「新建日程」表单收起 + `agenda-{uuid}.json` 文件存在 |
| 字段值正确 | `title == "早会提醒"`、`status == "active"`（每个字段单独列） |

### 规则 2：每条 bullet 是一个独立判定单元，不复合

一条 bullet 只断言一件事。

```
❌ 复合：日程已创建且能被 cron 调度（说明 active）
✅ 拆开：
  - 列表出现一行标题为 `早会提醒` 的条目
  - JSON 文件 `status == "active"`
  - JSON 文件 `nextFireAt` 不为 null
```

### 规则 3：用 6 种标准断言形式之一

| 形式 | 示例 |
|---|---|
| **UI 出现** | 日程列表出现一行标题为 `早会提醒` 的条目 |
| **UI 消失** | 「新建日程」表单收起 |
| **文件存在** | 文件 `~/.renlijia/users/{scope}/agenda/items/agenda-*.json` 存在 |
| **文件不存在** | `~/.renlijia/users/{scope}/agenda/items/` 下没有以 `tmp-` 开头的文件 |
| **字段精确匹配** | JSON 中 `status == "active"` |
| **字段范围匹配** | JSON 中 `createdAt` 在 `T0 ± 1 分钟` 内 |

### 规则 4：路径用字面值或带变量的明确模式

变量只允许这 4 个开箱可用，其它变量必须在「操作步骤」段先用 CLI 推断出来再用：

| 变量 | 含义 |
|---|---|
| `{scope}` | 当前登录用户的 scope（形如 `t_{tenantId}__u_{userId}`） |
| `{tenantId}` | 租户 ID |
| `{userId}` | 用户 ID |
| `T0` | 测试开始时刻（人 / agent 跑前 capture） |

```
❌ 推测：agenda 相关目录下
✅ 字面：~/.renlijia/users/{scope}/agenda/items/
✅ 模式：~/.renlijia/users/{scope}/agenda/items/agenda-*.json
```

### 规则 5：字段断言用 5 种运算符之一

| 运算 | 示例 |
|---|---|
| `==` 精确等于 | `title == "早会提醒"` |
| `!=` 不等于 | `occurrenceCount != 0` |
| `not null` / `is null` | `nextFireAt` 不为 `null`；`rule == null` |
| `length == N` / `length >= N` | `participants.length == 1` |
| 时间/数值范围 | `createdAt` 在 `T0 ± 1 分钟` 内 |

### 规则 6：「❌ 不应该看到」段的断言必须本身是反向陈述

```
✅ JSON 中不含 `personaId` 字段
✅ 日程列表中没有第二行标题为 `早会提醒` 的条目
✅ `agenda/runs/` 目录下没有以 `job-001-` 开头的子目录
```

`❌ 不应该看到` 段允许为空（写「无」），但**段头必须保留**——提醒下次修订者主动想反向陷阱。

## 6. 「操作步骤」允许使用的命令（白/黑名单）

### 白名单

| 类别 | 示例 |
|---|---|
| 删除 scope 内具体子目录 | `rm -rf ~/.renlijia/users/{scope}/agenda/items/` |
| 删除 scope 内具体文件 / glob | `rm ~/.renlijia/users/{scope}/agenda/items/agenda-*.json` |
| 列目录 / 检查存在 | `ls`, `test -d`, `test -f`, `stat` |
| 创建 scope 内具体子目录 | `mkdir -p ~/.renlijia/users/{scope}/agenda/notes/` |
| 写测试 fixture 文件 | `echo "..." > ~/.renlijia/users/{scope}/...` |
| `tauri-pilot aijia` CLI 子命令 | `tauri-pilot aijia health-check` / `cleanup-test-sessions` 等 |

### 黑名单

| 类别 | 例子 |
|---|---|
| 操作 `~/.renlijia/` 根目录 | `rm -rf ~/.renlijia/` |
| 操作家目录 / 系统目录 | `rm -rf ~/`、`rm -rf /tmp/*` |
| 操作 git 仓库 | `git reset --hard`、`git clean -fd`、`git checkout .` |
| 启停应用 / 系统进程 | `killall AIjia`、`pkill -9 ...` |
| 任何 `sudo` 命令 | 全部 |
| 网络操作 | `curl`、`wget`、`ssh ...` |
| 修改环境变量 / shell profile | `export AIJIA_TEST_MODE=1`、`source ~/.zshrc` |

### 例外：崩溃恢复 task

`崩溃恢复/rules.md` 允许 `kill <pid>` / `pkill AIjia`，但**每条意图必须**：
- 在「操作步骤」段开头加 `⚠️` 警告说本意图会 kill 应用
- agent 跑前在对话里**先和作者确认**可以 kill

## 7. 跨意图依赖 = 禁止引用

意图之间**不允许**有顺序依赖。`意图-日程-003`「暂停日程不再触发」需要先有一条 active 日程的场景，处理方式是把「创建一条 active 日程」**直接写进 `003` 自己的「操作步骤」段最前面**，**不引用** `意图-日程-001`。

**禁止的写法**：
```
操作步骤：
1. 先成功跑过 意图-日程-001 的「操作步骤」段
```

**正确的写法**：
```
操作步骤：
1. 应用探活：`tauri-pilot aijia health-check`
2. 推断 scope
3. 清空 `~/.renlijia/users/{scope}/agenda/items/`
4. 用 `tauri-pilot aijia` 创建一条 active 日程：标题 `测试用日程`、cron `* * * * *`
5. 等该日程出现在日程列表
6. ...（后续主测动作）
```

理由：每条意图原子可独立跑，agent 不需要按 ID 顺序调度；FAIL 时也不会触发「前置 FAIL → 后续全 SKIP」连锁。

## 8. 写后自查清单（3 层 13 项硬伤）

写完每条意图按下面 3 层过一遍，命中即返工：

### Layer 1 语义层（每条 bullet 单独检查）

| 硬伤 | 例子 |
|---|---|
| 代指不清晰 | 「该条目」「这个文件」——指哪个？换字面路径或字段名 |
| 表述模糊 | 「成功」「正常」「合理」「正确」「应该」 |
| 标准过于宽泛 | 「字段值符合预期」——预期是什么？|
| 表述拗口 | 一句话 3 层嵌套从句 |

### Layer 2 结构层

| 硬伤 | 例子 |
|---|---|
| 非原子化 | 「文件存在且 `status == active`」——拆开 |
| 多个独立约束硬塞一条 | 「保存按钮消失 + 列表出现新条目 + JSON 落盘」——拆 3 条 |
| 冗余 | ✅ 段「列表出现 1 条目」+「列表长度 == 1」——同一断言两种说法 |
| 包含关系 | 「文件 X 存在」+「字段 X 合法」——后者隐含前者，删冗 |
| 标准之间冲突 | ✅ 写 `status == "active"`、❌ 写 `status != "paused"`——逻辑互斥但表面不矛盾 |
| 格式不规范 | 字段断言不用 `==` 而用「等于」；路径不带反引号；字段名不用 camelCase；UI 文案不用「」 |
| **意图标题违规** | 超 30 字 / 含技术名 / 含 ✓✗`/` 符号连接 → 重写 |

### Layer 3 完备性层

| 硬伤 | 例子 |
|---|---|
| 遗漏关键约束 | 只断言 UI 出现、不断言 JSON 落盘——前端 mock 假数据也会 PASS |
| 包含无关约束 | 验收里出现「应用版本 == v0.5.26」——版本不是这条意图管的事 |
| 标准本身错误 | `status == "Active"` 但实际枚举是小写 `active` |

**返工原则**：命中即改 rules.md，**不要写「agent 自己判断一下」之类的兜底**。

## 9. 写作 / review 流程 = 不走 PR

1. 在对话里和作者确认要加 / 改 / 删的意图是哪条、承诺是什么
2. 按 §1-§8 写出 rules.md diff
3. 在对话里把 diff 展示给作者
4. 作者 review **承诺方向**（这是产品承诺吗？该写在这个 task 吗？和现有意图冲突吗？）—— §5 / §8 形式合规由你自己已查
5. 通过 → commit；不通过 → 反馈修改点，重写循环

**不走 PR 的理由**：单条意图改一行也开 PR 太重；形式合规由你按 §5 / §8 自检；承诺方向只能人判，对话即时 review 比 PR 慢回复快。

## 10. 新建一个 task 的判定（极少触发）

判定一个东西要不要单独立 task，看以下 2 条标准（**全部满足**才开新 task）：

1. **UI 上有独立入口**——顶栏 tab / 侧栏菜单 / 独立子页面，用户能在脑里说出「我去 X 里做 Y」
2. **有独立的核心承诺**——背后的产品故事不会被现有 task 完全覆盖

**不设「至少 N 条意图」门槛**——单条意图的微型 task 也允许。

**命名规范**：
- 中文目录名、2-5 个汉字、复用 UI 上呈现的文案
- 禁用英文 task 名（如不能叫 `agenda` 而要叫 `日程`）

**决策流程**：动 task 列表前先按上面 2 条标准判定，通过后再修改目录结构 + 在 commit message 里说明触发哪条规则；作者对话 review 后 commit。

## Red Flags（看到自己在这样想就停下来）

| 想法 | 应该 |
|---|---|
| 「标题太长不优雅但意思到位」 | > 30 字必重写（§2 规则 3） |
| 「写「字段值合理」就行，agent 能懂」 | 禁主观词（§5 规则 1） |
| 「这条意图需要先跑 意图-XXX」 | 意图自给自足（§7） |
| 「这一条复合的不拆了吧，反正都相关」 | 一条意图一件事（§4.1） |
| 「鉴于 X 不太确定，我加一句『agent 自行判断』兜底」 | 不允许（§8 返工原则） |
| 「✅ 段写完就够了，❌ 段没东西可写省略掉吧」 | 段头必留写「无」（§5 规则 6） |
| 「这个 task 太大了，按文件大小拆吧」 | 拆 task 看产品工作面、不看文件大小 |
