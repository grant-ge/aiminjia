# rules.md — 日程

本 task 测的产品承诺：**用户能创建一次性 / 循环日程，到点自动跑、能立即触发、能暂停 / 恢复、有完整执行历史可查**。

13 个 task 中作为新模板示范，按 §2.1.2 标题命名 + §2.2 字段集（3 段）+ §2.4 验收书写规则 写。

---

## 意图-日程-001: 创建一次性日程后，items 目录落盘且字段完整

**场景**
用户在日程页点「新建」，填写标题/Prompt/组织者员工/未来执行时间，保存后该日程立即出现在列表中，并以独立 JSON 文件持久化到用户 scope 下。

**操作步骤**
1. 应用探活：`tauri-pilot aijia health-check`
2. 推断当前 scope（形如 `t_{tenantId}__u_{userId}`）：从 `tauri-pilot aijia where --json` 取，记为 `$SCOPE`
3. 清空测试目标目录：`rm -rf ~/.renlijia/users/$SCOPE/agenda/items/`
4. 确认 `~/.renlijia/users/$SCOPE/employees/` 下至少有 `default` 员工 record；无则跳过本意图（先去 数字员工 task 跑创建意图）
5. 记录当前时间 `T0`（用 `date -u +%Y-%m-%dT%H:%M:%SZ` 取）
6. 点击主侧边栏「日程」入口
7. 点击「新建日程」按钮；等新建表单展开（能看到「标题」「Prompt」「开始时间」等输入项）
8. 在「标题」输入 `早会提醒`
9. 在「Prompt」输入 `提醒我今天的三件事`
10. 在「组织者员工」下拉选择 `default`
11. 在「开始时间」选择本地时间 `T0 + 30 分钟`
12. 在「频率」选择 `一次性`
13. 「时区」保持默认 `Asia/Shanghai`
14. 点击「保存」；等表单收起（页面回到日程列表）

**验收标准**

✅ 应该看到
- 表单收起后，日程列表出现一行标题为 `早会提醒` 的条目
- 目录 `~/.renlijia/users/$SCOPE/agenda/items/` 存在
- 该目录下有恰好 1 个文件��文件名形如 `agenda-{uuid}.json`
- 该文件是合法 JSON，且：
  - `title == "早会提醒"`
  - `prompt == "提醒我今天的三件事"`
  - `organizerEmployeeId == "default"`
  - `participants.length == 1`，第 0 项 `employeeId == organizerEmployeeId`
  - `timezone == "Asia/Shanghai"`
  - `rule == null`
  - `status == "active"`
  - `occurrenceCount == 0`
  - `nextFireAt` 不为 null，等于 `startAt`
  - `createdAt` 和 `updatedAt` 在 `T0 ± 1 分钟` 内

❌ 不应该看到
- JSON 含 `personaId` / `organizerPersonaId` 旧字段名（说明实现还在写老字段）
- 日程列表出现 2 条标题为 `早会提醒` 的条目（说明保存被点了两次）
- `agenda/items/` 目录下出现多个文件（应该只创建 1 个）

---

## 意图-日程-002: 一次性日程到点后，自动新建对话并执行 Prompt

**场景**
用户创建了未来 2 分钟后触发的一次性日程，到点后不需要做任何操作，应用应自动新建对话、把日程 Prompt 发给组织者员工执行，并在执行历史里追加记录。

**操作步骤**
1. 应用探活
2. 推断 scope，记为 `$SCOPE`
3. 记录当前时间 `T0`
4. 记录现有所有对话 ID：`tauri-pilot aijia list-sessions --json` 取所有 `id`，记为集合 `$S_BEFORE`
5. 点击主侧边栏「日程」入口、「新建日程」
6. 标题 `测试自动触发`、Prompt `请说一句你好`、组织者 `default`、频率 `一次性`、开始时间 `T0 + 2 分钟`、时区 `Asia/Shanghai`、点保存
7. 记录新建日程的 `agenda-{uuid}` 文件名（在 `~/.renlijia/users/$SCOPE/agenda/items/` 下取最新文件），记为 `$AGENDA`
8. 等到 `T0 + 4 分钟`（确保 runner 至少 tick 一次处理 due）
9. 在日程列表点击该日程，切到「执行历史」Tab

**验收标准**

✅ 应该看到
- 「执行历史」Tab 至少 1 行记录
- 目录 `~/.renlijia/users/$SCOPE/agenda/occurrences/$AGENDA/` 存在
- 该目录下至少 1 个 `YYYY-MM.jsonl` 文件
- 该 jsonl 末行是合法 JSON，且：
  - `agendaItemId == $AGENDA`
  - `triggerSource == "scheduled"`
  - `status` 在 `T0 + 5 分钟` 时已经收敛为 `"succeeded"` 或 `"failed"`（不长期停在 `"running"`）
  - `conversationId` 不为空
- `conversationId` 字段值是一个**新对话 ID**（不在 `$S_BEFORE` 集合内）
- 该新对话目录 `~/.renlijia/users/$SCOPE/conversations/<conversationId>/messages.jsonl` 存在，第 1 条记录 `role == "user"`、`content.text` 包含 `提醒我今天的三件事`（注：这里 prompt 是 `请说一句你好`——验收里要按本意图实际填的 prompt 串改）
- 日程文件 `~/.renlijia/users/$SCOPE/agenda/items/$AGENDA` 中：
  - `occurrenceCount == 1`
  - `nextFireAt == null`
  - `status == "completed"`

❌ 不应该看到
- `triggerSource == "manual_run_now"`（说明不是自然触发）
- 出现 2 条以上 occurrence（一次性日程只该跑 1 次）
- 日程 `status` 仍停在 `"active"`（说明状态机没推进）
- `conversationId` 出现在 `$S_BEFORE` 集合里（说明复用了老对话而非新建）

---

## 意图-日程-003: 用户点「立即运行」，独立产生一条 occurrence 且不消耗自然到点

**场景**
用户不想等到自然到点，在日程详情里点「立即运行」要求现在就跑。应用应立刻创建一条 occurrence 并启动新对话，但**不消耗下一次自然到点**（`nextFireAt` 不动）。

**操作步骤**
1. 应用探活
2. 推断 scope，记为 `$SCOPE`
3. 记录当前时间 `T0`
4. 创建一个 `T0 + 1 小时`后才到点的一次性日程 `测试立即运行`，组织者 `default`、Prompt `请回应"在"一个字`、频率 `一次性`、点保存
5. 记录新建日程的 `agenda-{uuid}` 文件名，记为 `$AGENDA`
6. 读 `~/.renlijia/users/$SCOPE/agenda/items/$AGENDA` 取 `nextFireAt` 字段值，记为 `$T_NEXT_BEFORE`
7. 在日程列表点击该日程；等详情面板打开
8. 点击「立即运行」按钮
9. 等按钮 spinner 消失（最长 30 秒）
10. 切到「执行历史」Tab

**验收标准**

✅ 应该看到
- 「执行历史」Tab 至少 1 行记录
- 目录 `~/.renlijia/users/$SCOPE/agenda/occurrences/$AGENDA/` 存在
- 该目录下 `YYYY-MM.jsonl` 末行：
  - `triggerSource == "manual_run_now"`
  - `status` 在 30 秒内收敛为 `"succeeded"` 或 `"failed"`（不停在 `"running"`）
  - `conversationId` 不为空，对应对话目录 `~/.renlijia/users/$SCOPE/conversations/<id>/` 存在
- 日程文件 `~/.renlijia/users/$SCOPE/agenda/items/$AGENDA`：
  - `nextFireAt == $T_NEXT_BEFORE`（手动触发不消耗自然调度）
  - `occurrenceCount == 0`（手动 run-now 不计入循环计数器）

❌ 不应该看到
- `triggerSource == "scheduled"`（说明被记成自然到点了）
- `nextFireAt` 字段值改变（说明手动触发吃掉了自然到点）
- `occurrenceCount` 增加（手动触发不应该影响这个字段）
- spinner 30 秒后还在转

---

## 意图-日程-004: 暂停日程后，到点不再自动触发，执行历史无新增

**场景**
用户暂时不想被这个日程打扰，把状态从 active 改为 paused。即使自然到点也不能再产生 occurrence。

**操作步骤**
1. 应用探活
2. 推断 scope，记为 `$SCOPE`
3. 记录当前时间 `T0`
4. 创建日程 `测试暂停`，组织者 `default`、Prompt `请说"已触发"`、频率 `一次性`、开始时间 `T0 + 2 分钟`、点保存
5. 记录日程 `agenda-{uuid}` 文件名，记为 `$AGENDA`
6. 在日程列表点击该日程；切到「设置」Tab
7. 点击「暂停」按钮；等状态徽标变为「已暂停」
8. 等到 `T0 + 4 分钟`（runner 至少跑 2 次 tick）
9. 重新打开该日程的「执行历史」Tab

**验收标准**

✅ 应该看到
- 日程文件 `~/.renlijia/users/$SCOPE/agenda/items/$AGENDA` 中 `status == "paused"`
- 目录 `~/.renlijia/users/$SCOPE/agenda/occurrences/$AGENDA/` 不存在 **或** 存在但其下任一 `YYYY-MM.jsonl` 的 occurrence 总条数 == 0
- 「执行历史」Tab 显示「暂无执行记录」
- 该日程文件 `occurrenceCount == 0`

❌ 不应该看到
- 日程 `status` 仍是 `"active"`
- occurrences 目录下出现新 jsonl 行（说明暂停没拦住调度）
- 执行历史 Tab 显示有 1 行记录

---

## 意图-日程-005: 暂停日程恢复 active 后，下次到点正常触发

**场景**
紧接 意图-004 的场景，用户改主意了把暂停的日程恢复成 active，下一次自然到点应该能正常产生 occurrence——验证暂停没有损坏后续调度。

**操作步骤**
1. 应用探活
2. 推断 scope，记为 `$SCOPE`
3. 记录当前时间 `T0`
4. 创建日程 `测试暂停恢复`，频率 `每日`、cron 表达式让其 1-2 分钟后第一次触发、组织者 `default`、Prompt `请说"已恢复"`、点保存
5. 记录日程文件名为 `$AGENDA`
6. 立即在日程详情把状态切到「暂停」、保存
7. 等 1 分钟（确保第一次原本会触发的窗口已过）
8. 把状态切回「激活」、保存
9. 等到下一次 cron 触发时刻 + 2 分钟
10. 切到「执行历史」Tab

**验收标准**

✅ 应该看到
- 日程文件 `status == "active"`、`nextFireAt` 不为 null
- 目录 `~/.renlijia/users/$SCOPE/agenda/occurrences/$AGENDA/` 存在
- 「执行历史」Tab 至少 1 行 `triggerSource == "scheduled"` 的记录
- 最新一条 occurrence 的 `firedAt` 在「步骤 8 切回激活」之后
- 该 occurrence 的 `status` 在 30 秒内收敛为 `"succeeded"` 或 `"failed"`

❌ 不应该看到
- 暂停期间错过的那次触发被「补跑」（应该按"暂停就跳过"，不补）——表现为出现 2 条以上 occurrence 且其中有 `firedAt` 在「切回激活」之前的
- 日程 `status` 还停在 `"paused"`

---

## 意图-日程-006: 删除日程后未来不再触发，已产生的执行历史仍可查询

**场景**
用户删除了一个日程，未来到点不应该再触发；但已经跑过的 occurrence 历史仍要能查到（不能因为删日程把历史也清了）。

**操作步骤**
1. 应用探活
2. 推断 scope，记为 `$SCOPE`
3. 创建一个日程 `测试删除`，频率 `每日`，让它 1 分钟后第一次触发，Prompt `请说"运行中"`、组织者 `default`、点保存
4. 记录日程文件名为 `$AGENDA`
5. 等到第一次触发并跑完（约 2 分钟），确认「执行历史」Tab 至少 1 行
6. 记录 occurrences 文件路径 `$OCC_PATH = ~/.renlijia/users/$SCOPE/agenda/occurrences/$AGENDA/`
7. 在日程详情点击「删除」按钮、确认弹窗
8. 等到日程列表里该日程消失
9. 等到下一次原本会触发的时刻 + 2 分钟
10. 重新打开「日程列表」，从顶层入口点击「执行历史」总览（如果 UI 上有）查询该日程历史

**验收标准**

✅ 应该看到
- 日程列表里没有 `测试删除` 这一行
- 日程文件 `~/.renlijia/users/$SCOPE/agenda/items/$AGENDA` 不存在
- 目录 `$OCC_PATH` **依然存在**，原有 jsonl 文件依然存在、内容完整
- 在原 jsonl 文件中没有任何 `firedAt` 在「步骤 7 删除」之后的新 occurrence
- 如果 UI 上有「全部执行历史」入口，能在那里看到该日程之前的 occurrence 记录

❌ 不应该看到
- 删除日程后 occurrences 目录被一起删了（用户的历史数据被销毁）
- 删除后又出现了新的 occurrence 行（说明 runner 没拦住）
- 日程列表里日程没消失

---

## 意图-日程-007: 执行历史按时间倒序显示，含执行时间与状态

**场景**
一个日程已经跑过若干次（既有成功也有失败）。用户在详情页「执行历史」Tab 想看到所有已执行记录，按时间从新到旧排序，每条都能看到执行时间和最终状态。

**操作步骤**
1. 应用探活
2. 推断 scope
3. 准备一个已经跑过至少 3 次的日程 `$AGENDA`：
   - 创建一个每分钟触发一次的循环日程 `测试历史`，组织者 `default`、Prompt `请说"运行"`、点保存
   - 等 3-4 分钟（确保至少跑 3 次）
4. 在日程列表点击该日程，切到「执行历史」Tab
5. 等列表渲染完成（不出现 loading 占位）

**验收标准**

✅ 应该看到
- 「执行历史」Tab 至少 3 行
- 第 1 行（最上方）的 `firedAt` ≥ 第 2 行 ≥ 第 3 行（倒序）
- 每行同时显示 `firedAt`（时间字符串）和 `status` 文本
- 至少有一行 `status` 文本为 `succeeded`
- 列表所有行的 `agendaItemId == $AGENDA`（不串到其他日程）
- 列表显示行数 ≤ 50（默认 limit）

❌ 不应该看到
- 列表行的时间是顺序而非倒序
- 列表里出现其他日程的 occurrence
- 行上某条 occurrence 的状态文本为空 / undefined / 显示成原始枚举名 `EnumValue.Succeeded` 这种
