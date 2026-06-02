# rules.md — 日程（定时任务）

本 task 测的产品承诺：**用户能创建一次性 / 循环定时任务，到点自动跑、能立即触发、能暂停 / 恢复、能软取消（恢复）/ 永久删除（清历史），并有完整执行历史可查**。

UI 文案对应：侧边栏入口「定时任务」，列表表头「执行频率」，新建按钮「新建」打开「新建日程」表单。

---

## 意图-日程-001: 创建一次性定时任务后，items 目录落盘且字段完整

**场景**
用户在「定时任务」页点「新建」，填写标题/Prompt/执行员工/未来执行时间，保存后该任务立即出现在列表中，并以独立 JSON 文件持久化到用户 scope 下；产品语义是"保存"（不立刻派活、等到点才跳对话页）。

**操作步骤**
1. 应用探活：`tauri-pilot aijia health-check`
2. 推断当前 scope（形如 `t_{tenantId}__u_{userId}`）：从 `tauri-pilot aijia where --json` 取，记为 `$SCOPE`
3. 清空测试目标目录：`rm -rf ~/.renlijia/users/$SCOPE/agenda/items/`
4. 确认 `~/.renlijia/users/$SCOPE/employees/` 下至少有 1 个 `lifecycle=active` 员工 record；无则跳过本意图（先去 数字员工 task 跑雇佣意图）；记任一 active 员工 id 为 `$EMP_ID`、name 为 `$EMP_NAME`
5. 记录当前时间 `T0`（用 `date -u +%Y-%m-%dT%H:%M:%SZ` 取）
6. 点击主侧边栏「定时任务」入口；等列表页打开
7. 点击页面右上角「新建」按钮；等待 Sheet 节点 `[data-aijia-agenda-editor]` 出现
8. 在「执行员工」下拉选择 `$EMP_NAME`
9. 在节点 `[data-aijia-agenda-field="title"]` 输入 `早会提醒`
10. 在节点 `[data-aijia-agenda-field="prompt"]` 输入 `提醒我今天��三件事`
11. 在「频率」下拉选择 `一次性`
12. 在「开始时间」选择本地时间 `T0 + 30 分钟`
13. 「工作目录」保持默认
14. 点击节点 `[data-aijia-agenda-action="save"]`（按钮文字「保存」）；等 Sheet 收起（页面回到任务列表）

**验收标准**

应该看到：
- Sheet 收起后，任务列表出现一行 `[data-aijia-agenda-row][data-aijia-agenda-title="早会提醒"]`
- 目录 `~/.renlijia/users/$SCOPE/agenda/items/` 存在
- 该目录下有恰好 1 个文件，文件名形如 `agenda-{uuid}.json`
- 该文件是合法 JSON，且：
  - `title == "早会提醒"`
  - `prompt == "提醒我今天的三件事"`
  - `organizerEmployeeId == "$EMP_ID"`
  - `participants.length == 1`，第 0 项 `employeeId == "$EMP_ID"`
  - `timezone == "Asia/Shanghai"`
  - `rule == null`
  - `status == "active"`
  - `occurrenceCount == 0`
  - `nextFireAt` 不为 null，等于 `startAt`
  - `createdAt` 和 `updatedAt` 在 `T0 ± 1 分钟` 内
- 该任务行 `data-aijia-agenda-status` 属性值为 `"active"`
- 跑测过程中**没有**新对话目录被创建（保存语义：到点才派活）

不应该看到：
- JSON 含 `personaId` / `organizerPersonaId` 旧字段名
- 任务列表出现 2 条 `[data-aijia-agenda-title="早会提醒"]`
- `agenda/items/` 目录下出现多个文件
- 保存后立即跳转到对话页（应该停在任务列表）

---

## 意图-日程-002: 一次性定时任务到点后，自动新建对话并执行 Prompt

**场景**
用户创建了未来 2 分钟后触发的一次性任务，到点后不需要做任何操作，应用应自动新建对话、把任务 Prompt 发给执行员工跑，并在 occurrences 历史里追加记录。

**操作步骤**
1. 应用探活
2. 推断 scope，记为 `$SCOPE`；推断任一 active 员工 id 为 `$EMP_ID`
3. 记录现有所有对话 ID（用 `ls ~/.renlijia/users/$SCOPE/conversations/` 取所有子目录名），记为集合 `$S_BEFORE`
4. 记录当前时间 `T0`
5. 点击主侧边栏「定时任务」入口、「新建」按钮
6. 执行员工选 `$EMP_ID` 对应行、标题 `测试自动触发`、Prompt `请说一句你好`、频率 `一次性`、开始时间 `T0 + 2 分钟`、时区 `Asia/Shanghai`、点击 `[data-aijia-agenda-action="save"]`
7. 记录新建任务的 `agenda-{uuid}` 文件名（在 `~/.renlijia/users/$SCOPE/agenda/items/` 下取 mtime 最新的文件），记为 `$AGENDA`
8. 等到 `T0 + 4 分钟`（确保 runner 至少 tick 一次处理 due）
9. 在任务列表点击该行的「编辑」图标，或点击行进入详情；切到「执行历史」Tab

**验收标准**

应该看到：
- 「执行历史」Tab 至少 1 条记录
- 目录 `~/.renlijia/users/$SCOPE/agenda/occurrences/$AGENDA/` 存在
- 该目录下至少 1 个 `YYYY-MM.jsonl` 文件
- 该 jsonl 末条记录是合法 JSON，且：
  - `agendaItemId == "$AGENDA"`
  - `triggerSource == "scheduled"`
  - `status` 在 `T0 + 5 分钟` 时已收敛为 `"succeeded"` 或 `"failed"`（不长期停在 `"running"`）
  - `conversationId` 不为空
- `conversationId` 字段值是一个**新对话 ID**（不在 `$S_BEFORE` 集合内）
- 该新对话目录 `~/.renlijia/users/$SCOPE/conversations/<conversationId>/messages.jsonl` 存在，第 1 条记录 `role == "user"`、`content.text` 包含 `请说一句你好`
- 任务文件 `~/.renlijia/users/$SCOPE/agenda/items/$AGENDA`：
  - `occurrenceCount == 1`
  - `nextFireAt == null`
  - `status == "completed"`

不应该看到：
- `triggerSource == "manual_run_now"`（说明不是自然触发）
- 出现 2 条以上 occurrence（一次性任务只该跑 1 次）
- 任务 `status` 仍停在 `"active"`
- `conversationId` 出现在 `$S_BEFORE` 集合里

---

## 意图-日程-003: 用户点「立即运行」，独立产生一条 occurrence 且不消耗自然到点

**场景**
用户不想等到自然到点，在任务列表点行右侧「立即运行」图标（Play）。应用应立刻创建一条 occurrence 并启动新对话，但**不消耗下一次自然到点**（`nextFireAt` 不动）。

**操作步骤**
1. 应用探活
2. 推断 scope，记为 `$SCOPE`；推断任一 active 员工 id 为 `$EMP_ID`
3. 记录当前时间 `T0`
4. 在「定时任务」页创建一个 `T0 + 1 小时`后才到点的一次性任务 `测试立即运行`，执行员工 `$EMP_ID`、Prompt `请回应"在"一个字`、频率 `一次性`、点击「保存」
5. 记录新建任务的 `agenda-{uuid}` 文件名，记为 `$AGENDA`
6. 读 `~/.renlijia/users/$SCOPE/agenda/items/$AGENDA` 取 `nextFireAt` 字段值，记为 `$T_NEXT_BEFORE`
7. 在列表中悬停该行，点击「立即运行」图标（`aria-label="立即运行 测试立即运行"` 的 button）
8. 等任务对应行的图标按钮停止 spinner、列表数据刷新（最长 30 秒）
9. 切到该任务的「执行历史」Tab

**验收标准**

应该看到：
- 「执行历史」Tab 至少 1 条记录
- 目录 `~/.renlijia/users/$SCOPE/agenda/occurrences/$AGENDA/` 存在
- 该目录下 `YYYY-MM.jsonl` 末条记录：
  - `triggerSource == "manual_run_now"`
  - `status` 在 30 秒内收敛为 `"succeeded"` 或 `"failed"`
  - `conversationId` 不为空，对应对话目录 `~/.renlijia/users/$SCOPE/conversations/<id>/` 存在
- 任务文件 `~/.renlijia/users/$SCOPE/agenda/items/$AGENDA`：
  - `nextFireAt == "$T_NEXT_BEFORE"`（手动触发不消耗自然调度）
  - `occurrenceCount == 0`（手动 run-now 不计入循环计数器）

不应该看到：
- `triggerSource == "scheduled"`
- `nextFireAt` 字段值改变
- `occurrenceCount` 增加
- 30 秒后图标按钮仍在 spinner 状态

---

## 意图-日程-004: 暂停定时任务后，到点不再自动触发，执行历史无新增

**场景**
用户暂时不想被这个任务打扰，点列表行的「暂停」图标。即使自然到点也不能再产生 occurrence。

**操作步骤**
1. 应用探活
2. 推断 scope，记为 `$SCOPE`；推断任一 active 员工 id 为 `$EMP_ID`
3. 记录当前时间 `T0`
4. 创建任务 `测试暂停`，执行员工 `$EMP_ID`、Prompt `请说"已触发"`、频率 `一次性`、开始时间 `T0 + 2 分钟`、点击「保存」
5. 记录任务 `agenda-{uuid}` 文件名，记为 `$AGENDA`
6. 在任务列表悬停该行，点击「暂停」图标（`aria-label="暂停 测试暂停"`）；等该行 `data-aijia-agenda-status` 属性变为 `"paused"`
7. 等到 `T0 + 4 分钟`（runner 至少跑 2 次 tick）
8. 重新打开该任务的「执行历史」Tab

**验收标准**

应该看到：
- 任务文件 `~/.renlijia/users/$SCOPE/agenda/items/$AGENDA` 中 `status == "paused"`
- 目录 `~/.renlijia/users/$SCOPE/agenda/occurrences/$AGENDA/` 不存在 **或** 存在但其下任一 `YYYY-MM.jsonl` 的 occurrence 总条数 == 0
- 「执行历史」Tab 显示「暂无执行记录」或对应空状态
- 该任务文件 `occurrenceCount == 0`
- 列表行 `[data-aijia-agenda-id="$AGENDA"]` 的 `data-aijia-agenda-status` 属性值为 `"paused"`

不应该看到：
- 任务 `status` 仍是 `"active"`
- occurrences 目录下出现新 jsonl 记录
- 「执行历史」Tab 显示有任何记录

---

## 意图-日程-005: 暂停任务恢复 active 后，下次到点正常触发

**场景**
用户改主意了把暂停的任务恢复成 active（点同一个图标位置，此时是「启用」），下一次自然到点应该能正常产生 occurrence——验证暂停没有损坏后续调度。

**操作步骤**
1. 应用探活
2. 推断 scope，记为 `$SCOPE`；推断任一 active 员工 id 为 `$EMP_ID`
3. 记录当前时间 `T0`
4. 创建任务 `测试暂停恢复`，执行员工 `$EMP_ID`、频率 `每天` + 每 1 天 + 永不结束、开始时间 `T0 + 1 分钟`、Prompt `请说"已恢复"`、点击「保存」
5. 记录任务文件名为 `$AGENDA`
6. 立即在列表悬停该行，点击「暂停」图标
7. 等 2 分钟（确保第一次原本会触发的窗口已过）
8. 在列表悬停该行，点击「启用」图标（暂停态时图标变为 Play，aria-label `启用 测试暂停恢复`）
9. 等到下一次自然到点 + 2 分钟
10. 切到「执行历史」Tab

**验收标准**

应该看到：
- 任务文件 `status == "active"`、`nextFireAt` 不为 null
- 目录 `~/.renlijia/users/$SCOPE/agenda/occurrences/$AGENDA/` 存在
- 「执行历史」Tab 至少 1 条 `triggerSource == "scheduled"` 的记录
- 最新一条 occurrence 的 `firedAt` 在「步骤 8 启用」之后
- 该 occurrence 的 `status` 在 30 秒内收敛为 `"succeeded"` 或 `"failed"`

不应该看到：
- 暂停期间错过的那次触发被「补跑」——表现为出现 2 条以上 occurrence 且其中有 `firedAt` 在「步骤 8 启用」之前的
- 任务 `status` 还停在 `"paused"`

---

## 意图-日程-006: 取消任务后未来不再触发，已产生的执行历史在「已取消」列表里仍可查询

**场景**
用户在列表点行的「取消」图标（X 红色）软删除一个任务。取消后该任务从主列表消失（进"已取消"分组），未来到点不再触发；已经跑过的 occurrence 历史**保留**（不被销毁），用户可点页面顶部「查看已取消」进入分组重新看到。

**操作步骤**
1. 应用探活
2. 推断 scope，记为 `$SCOPE`；推断任一 active 员工 id 为 `$EMP_ID`
3. 创建任务 `测试取消`，执行员工 `$EMP_ID`、频率 `每天` + 每 1 天、开始时间为 `T0 + 1 分钟`、Prompt `请说"运行中"`、点击「保存」
4. 记录任务文件名为 `$AGENDA`
5. 等到第一次触发并跑完（约 2 分钟），确认「执行历史」Tab 至少 1 条记录
6. 记录 occurrences 目录路径 `$OCC_PATH = ~/.renlijia/users/$SCOPE/agenda/occurrences/$AGENDA/`
7. 在主列表悬停该行，点击「取消」图标（X 红色，aria-label `取消 测试取消`）；在弹出的 Radix `AlertDialog`（标题「取消此定时任务？」）中点击「确认取消」
8. 等到主列表里该任务消失
9. 等到下一次原本会触发的时刻 + 2 分钟
10. 点击列表页顶部「查看已取消」按钮（文字含「已取消」），进入已取消分组

**验收标准**

应该看到：
- 主列表中没有 `[data-aijia-agenda-title="测试取消"]` 行
- 任务文件 `~/.renlijia/users/$SCOPE/agenda/items/$AGENDA` **依然存在**（软删除不删盘）
- 任务文件 `status == "cancelled"`，`nextFireAt == null`
- 目录 `$OCC_PATH` **依然存在**，原有 jsonl 文件依然存在、内容完整
- 在原 jsonl 文件中没有任何 `firedAt` 在「步骤 7 取消」之后的新 occurrence
- 「已取消」列表中出现 `[data-aijia-agenda-title="测试取消"]` 行，该行 `data-aijia-agenda-status` 属性值为 `"cancelled"`

不应该看到：
- 取消任务后 occurrences 目录被一起删了（应该等"永久删除"才删历史）
- 取消后又出现了新的 occurrence 记录（说明 runner 没拦住）
- 主列表里该任务没消失
- 任务文件直接消失（应该是改 status 而不是 unlink）

---

## 意图-日程-007: 永久删除已取消任务，items 与 occurrences 全部从磁盘抹除

**场景**
用户在「已取消」列表点行的「永久删除」图标��Trash2 红色）做硬删除。这一步会把 items JSON 与对应的 occurrences 历史**一起**从磁盘抹除，无法恢复。

**操作步骤**
1. 应用探活
2. 推断 scope，记为 `$SCOPE`；推断任一 active 员工 id 为 `$EMP_ID`
3. 创建任务 `测试永久删除`，执行员工 `$EMP_ID`、频率 `每天`、开始时间 `T0 + 1 分钟`、Prompt `请说"运行"`、点击「保存」；记任务文件名为 `$AGENDA`
4. 等到第一次触发跑完（约 2 分钟）
5. 在主列表悬停该行，点击「取消」图标 → 确认弹窗
6. 点击列表页顶部「查看已取消」进入已取消分组；确认能看到 `[data-aijia-agenda-title="测试永久删除"]` 行
7. 悬停该行，点击「永久删除」图标（Trash2，aria-label `永久删除 测试永久删除`）
8. 在弹出的 Radix `AlertDialog`（标题「永久删除此任务？」，描述含「此操作会从磁盘抹除任务及其执行历史」）中点击「确认永久删除」
9. 等列表刷新

**验收标准**

应该看到：
- 「已取消」列表中**没有** `[data-aijia-agenda-title="测试永久删除"]` 行
- 文件 `~/.renlijia/users/$SCOPE/agenda/items/$AGENDA` 不存在
- 目录 / 文件 `~/.renlijia/users/$SCOPE/agenda/occurrences/$AGENDA`（或 `.../$AGENDA/`）不存在
- 主列表中也不存在该任务

不应该看到：
- items 文件被删但 occurrences 目录仍残留
- 任何「确认弹窗」自动 accept 没有用户实际确认
- 该任务又出现在主列表 / 已取消列表里
