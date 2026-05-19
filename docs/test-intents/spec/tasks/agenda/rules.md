# rules.md — 日程（Agenda）意图测试规格

## 测试范围
覆盖日程从创建到触发再到执行历史的完整链路：用户在前端创建一次性或循环日程，到时间后由 runner 自动新建对话并触发 AI 跑预设 prompt，occurrence 被记录、可查询，日程可暂停 / 恢复 / 立即触发。仅验证日程编排与运行态约束，不深入 LLM 输出质量。

## 待覆盖的主要场景
- 场景 1：创建一次性日程，到点自动新建对话并触发 prompt，occurrence 记录为 success
- 场景 2：创建循环日程（cron / 每日），多次到点后多条 occurrence 按时序产生
- 场景 3：日程暂停后到点不触发，occurrence 不新增；恢复后下一次到点正常触发
- 场景 4：用户点"立即触发"，跳过下次自然到点的依赖，独立产生一条 occurrence
- 场景 5：执行失败（如 LLM 报错、被 cancel）时 occurrence 记录为 failed，不影响下一次调度
- 场景 6：删除日程后未来到点不再触发，已产生的 occurrence 历史仍可查询
- 场景 7：occurrence 列表可按日程过滤，时间倒序，分页可用

---

## 意图 1：创建一次性日程后，items/{id}.json 落盘且字段完整

**场景**
用户在日程页点「新建」，填写标题、prompt、组织者员工、一个未来的执行时间点（不设循环规则），保存后该日程立即出现在列表中，并以一个独立 JSON 文件持久化到用户 scope 下。

**前提**
- 应用已启动并已登录，scope 为 `t_{tenantId}__u_{userId}`
- 已雇佣至少一个数字员工（如默认的 `default`），`~/.renlijia/users/{scope}/employees/` 下存在对应 record
- 目录 `~/.renlijia/users/{scope}/agenda/items/` 在测试前不存在或为空
- 当前系统时间为 `T0`，可用一个 `T0 + 30 分钟` 的时间作为日程触发时间

**操作**
1. 在主侧边栏点击「日程」入口
2. 点击「新建日程」按钮，AgendaItemEditor sheet 打开
3. 在「标题」输入框输入 `"早会提醒"`
4. 在「Prompt」输入框输入 `"提醒我今天的三件事"`
5. 在「组织者员工」下拉选择 `default`（或任一已雇佣员工）
6. 在「开始时间」选择本地时间 `T0 + 30 分钟`
7. 在「频率」选择 `一次性`
8. 「时区」保持默认 `Asia/Shanghai`
9. 点击「保存」按钮，等待 sheet 关闭

**验收标准**
- AgendaItemEditor sheet 关闭，日程列表中出现一行 title 为 `"早会提醒"` 的条目
- 目录 `~/.renlijia/users/{scope}/agenda/items/` 存在
- 该目录下存在恰好 1 个文件，文件名形如 `agenda-{uuid}.json`
- 该文件为合法 JSON，包含以下字段且字段值符合：
  - `title` = `"早会提醒"`
  - `prompt` = `"提醒我今天的三件事"`
  - `organizerEmployeeId` = `"default"`（或所选员工 ID）
  - `participants` 是长度为 1 的数组，第 0 项 `employeeId` 与 `organizerEmployeeId` 相同
  - `timezone` = `"Asia/Shanghai"`
  - `rule` = `null`
  - `status` = `"active"`
  - `occurrenceCount` = `0`
  - `nextFireAt` 不为 `null`，转换后等于 `startAt`
  - `createdAt` / `updatedAt` 字段值在 `T0` ± 1 分钟范围内
- 该 JSON 不包含 `personaId`、`organizerPersonaId` 这类旧字段名（写入用 camelCase 新字段名）

---

## 意图 2：一次性日程到期，系统自动新建对话并执行 prompt，occurrence 记录落盘

**场景**
用户创建了一个未来 2 分钟后触发的一次性日程。到点后用户不需要做任何操作，应用应自动新建一个对话、把日程 prompt 发给组织者员工开始执行，并在 occurrence 历史里追加一条 Running → Succeeded 的记录。

**前提**
- 应用已启动并已登录，agenda runner 已注册（启动后约 60 秒第一个 tick）
- 网络可达 LLM 网关，当前账号余额可发起对话
- 按意图 1 的步骤创建过一个一次性日程 `agenda-X`，`startAt` = `T0 + 2 分钟`，`status` = `active`，`occurrenceCount` = `0`
- 目录 `~/.renlijia/users/{scope}/agenda/occurrences/agenda-X/` 在测试前不存在
- 目录 `~/.renlijia/users/{scope}/conversations/` 下记录当前所有 conv_id（记为集合 `S_before`）

**操作**
1. 在系统时钟到达 `T0 + 3 分钟` 之前，不要触发任何手动操作（任由 runner 自动 tick）
2. 等待至 `T0 + 4 分钟`，确保至少有一次 tick 已经处理该 due 项
3. 打开日程列表，点击日程 `agenda-X`，切到「执行历史」Tab

**验收标准**
- 目录 `~/.renlijia/users/{scope}/agenda/occurrences/agenda-X/` 存在
- 该目录下存在文件 `YYYY-MM.jsonl`（`YYYY-MM` 等于 `T0 + 2 分钟` 的 UTC 年月）
- 该 jsonl 文件至少 1 行，每行为合法 JSON
- 文件中最后一条 occurrence 的 `status` 字段值为 `"succeeded"`（若执行尚未完成允许临时为 `"running"`，但 3 分钟内必须收敛为 `"succeeded"` 或 `"failed"`）
- 该 occurrence 的 `agendaItemId` = `"agenda-X"`
- 该 occurrence 的 `triggerSource` = `"scheduled"`
- 该 occurrence 的 `conversationId` 不为空，且对应的 conv_id 是 `~/.renlijia/users/{scope}/conversations/` 下 `S_before` 不包含的**新** conv_id（即由 dispatcher 新建）
- 在新对话目录 `~/.renlijia/users/{scope}/conversations/{conversationId}/messages.0.jsonl` 中，第 1 行 `role` = `"user"`，`content.text` 包含日程的 prompt `"提醒我今天的三件事"`
- 日程文件 `~/.renlijia/users/{scope}/agenda/items/agenda-X.json` 中 `occurrenceCount` 字段值为 `1`，`nextFireAt` 字段值为 `null`，`status` 字段值为 `"completed"`
- 「执行历史」Tab 中显示 1 条记录，状态文本为 `succeeded`

---

## 意图 3：用户手动「立即运行」日程，occurrence 立即被创建，trigger 来源标记为 manual

**场景**
用户不想等到自然到点，在日程详情里点「立即运行」按钮要求现在就跑。应用应立刻创建一条 occurrence 并启动新对话，不消耗下一次自然到点（即 `nextFireAt` 不动）。

**前提**
- 应用已启动并已登录
- 存在一个状态为 `active` 的日程 `agenda-Y`，`startAt` 在未来 1 小时之后（远未到自然触发点）
- `agenda-Y` 当前 `occurrenceCount` = `0`，`nextFireAt` 不为 `null`，记 `T_next_before` = 该字段的值
- 目录 `~/.renlijia/users/{scope}/agenda/occurrences/agenda-Y/` 不存在

**操作**
1. 在日程列表点击 `agenda-Y`，AgendaItemDetail sheet 打开
2. 在 sheet 顶部点击「立即运行」按钮
3. 等待按钮 spinner 消失（最长 30 秒）
4. 切到「执行历史」Tab

**验收标准**
- 「执行历史」Tab 显示至少 1 条 occurrence 记录
- 目录 `~/.renlijia/users/{scope}/agenda/occurrences/agenda-Y/` 存在
- 该目录下的 `YYYY-MM.jsonl` 文件至少 1 行，最后一条 occurrence 的 `triggerSource` 字段值为 `"manual_run_now"`
- 该 occurrence 的 `status` 字段值在 30 秒内收敛到 `"succeeded"` 或 `"failed"`（不长期停在 `"running"`）
- 该 occurrence 的 `conversationId` 不为空，对应的 conv 在 `~/.renlijia/users/{scope}/conversations/` 下确实存在
- 日程文件 `~/.renlijia/users/{scope}/agenda/items/agenda-Y.json` 中 `nextFireAt` 字段值仍等于 `T_next_before`（手动触发不消耗自然调度）
- 该文件 `occurrenceCount` 字段值仍为 `0`（手动 run-now 不计入循环计数器）

---

## 意图 4：暂停日程后，到点不再自动触发，occurrence 目录无新增记录

**场景**
用户暂时不想被这个日程打扰，在日程详情里把状态从 active 改为 paused。即使自然到点也不能再产生 occurrence。

**前提**
- 应用已启动并已登录，agenda runner 正常运行
- 存在一个日程 `agenda-Z`，`status` = `"active"`，`startAt` = `T0 + 2 分钟`
- 目录 `~/.renlijia/users/{scope}/agenda/occurrences/agenda-Z/` 不存在
- 记录该目录的当前 occurrence 文件个数 `N_before` = `0`

**操作**
1. 在日程列表点击 `agenda-Z`，sheet 打开
2. 切到「设置」Tab，点击「暂停」按钮（或在编辑器中将状态切换到 paused 后保存）
3. 在系统时钟到达 `T0 + 4 分钟` 之前不做其他操作
4. 等待至 `T0 + 4 分钟`（runner 至少跑过 2 个 tick）
5. 重新打开 `agenda-Z` 的详情「执行历史」Tab

**验收标准**
- 日程文件 `~/.renlijia/users/{scope}/agenda/items/agenda-Z.json` 中 `status` 字段值为 `"paused"`
- 目录 `~/.renlijia/users/{scope}/agenda/occurrences/agenda-Z/` 不存在，或存在但其下任一 `YYYY-MM.jsonl` 文件中的 occurrence 总条数仍为 `N_before`（即 0）
- 「执行历史」Tab 显示「暂无执行记录」
- 该日程文件 `occurrenceCount` 字段值仍为 `0`
- 在 `T0 + 5 分钟` 时把状态改回 `active` 后保存：文件 `status` 重新为 `"active"`，`nextFireAt` 不为 `null`，等待下次 tick 后能正常产生 1 条 `succeeded` occurrence（验证暂停未损坏后续调度）

---

## 意图 5：用户查看日程执行历史，occurrence 列表按时间倒序显示，含执行时间与状态

**场景**
一个循环日程已经跑过若干次（既有成功也可能有失败）。用户在详情页「执行历史」Tab 想看到所有已执行记录，按时间从新到旧排序，每条记录都能看到执行时间和最终状态。

**前提**
- 应用已启动并已登录
- 存在一个日程 `agenda-W`，至少已有 3 条 occurrence 历史，分别落在不同时间点 `T_a < T_b < T_c`
- 目录 `~/.renlijia/users/{scope}/agenda/occurrences/agenda-W/` 下存在至少 1 个 `YYYY-MM.jsonl` 文件，文件中至少包含 3 条不同 `id` 的 occurrence 记录
- 其中至少 1 条状态为 `"succeeded"`，至少 1 条状态为 `"failed"`（可在测试准备阶段用 mock LLM 或人为关网模拟一次失败）

**操作**
1. 在日程列表点击 `agenda-W`
2. 切到「执行历史」Tab
3. 等待列表渲染完成（不出现 loading 占位）

**验收标准**
- 「执行历史」Tab 显示至少 3 行 occurrence 记录
- 第 1 行对应的 `firedAt` 时间 ≥ 第 2 行的 `firedAt`，第 2 行 ≥ 第 3 行（倒序排列）
- 每行同时显示 `firedAt`（RFC3339 时间串或本地化时间）和 `status` 文本
- 至少有一行状态文本为 `succeeded`，至少有一行状态文本为 `failed`
- 列表中显示的 occurrence 数量 ≤ 接口 limit 上限（默认 50）
- 该 Tab 中所有 occurrence 的 `agendaItemId` 字段值均为 `"agenda-W"`（即列表确实按当前日程过滤，不串到其他日程）
- 任一 `status` = `"failed"` 的行：其 `errorSummary` 字段在原始 jsonl 中不为 `null`，UI 上对应的简短错误文本不为空字符串
