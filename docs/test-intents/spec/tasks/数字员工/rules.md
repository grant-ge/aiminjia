# rules.md — 数字员工 意图测试规格

## 测试范围

覆盖数字员工的完整生命周期：从模板雇佣、资源/技能配置、派活前置补全（trigger prechecks）、dispatch prompt 生成与执行、cron 定时自动触发、运行态守卫（ActiveRunGuard），到状态机迁移（Active / Paused / Archived / Running）与软删除清理。关注后端 `runtime/employee/` 与 Tauri 命令 `commands/employees.rs`、以及前端 `src/features/employees/` 的雇佣向导和派活入口在端到端链路上的正确性。

## 待覆盖的主要场景

- 场景 1：用户从模板雇佣员工（HireWizard），员工目录写入快照 `template/template.json` 并在 `EmployeeStore` 中可见
- 场景 2：派活前置检查 (`runTriggerPrechecks`) 按模板 `requiresAttachment / resourceConfigKind / requiresDingtalk` 弹出对应配置表单，未配置完整时阻断派活
- 场景 3：派活成功后 `build_dispatch_prompt` 拼出包含资源信息且以"请立即开始按职责执行"结尾的 prompt，并真正驱动一次 chat turn
- 场景 4：cron 表达式到点后 scheduler 自动派活，运行态在 `EmployeeActiveRuns` 中可见并通过事件下发到前端
- 场景 5：用户手动暂停 / 恢复员工（lifecycle Active ↔ Paused），cron tick 在 Paused 状态下不再触发
- 场景 6：用户归档员工（lifecycle Archived），过 7 天后由 `purge_old_archived` 自动物理清理目录
- 场景 7：员工正在 Running 时被取消（用户停止 / panic / 提前 return），`ActiveRunGuard` 在 Drop 时正确清理活跃运行记录，不泄漏
- 场景 8：snapshot-first 读取——派活时 `effective_tool_whitelist / effective_system_prompt_extra / effective_default_skill_id` 优先读 `template/template.json`，缺失时回退到 record 字段

---

## 意图 1：从模板雇佣数字员工后，员工记录落盘且在列表中可见

**场景**
用户从 HireWizard 选中一个内置模板（如「小研」），完成雇佣向导后，员工目录被创建、模板快照被冷冻到磁盘，员工出现在主页员工列表中。

**前提**
- 应用已启动并完成登录，当前 user scope 已就绪
- `~/.renlijia/users/{scope}/employees/` 目录可写
- 当前还没有 `templateId === 'builtin:xiaoyuan'` 的员工

**操作**
- 用户在主页点击「雇佣员工」打开 HireWizard
- 第 1 步：在模板网格中选中「小研 行业/竞品调研员」
- 第 2 步：保持默认名称「小研」，点击下一步
- 第 3 步：在 monitoring-urls 资源表单里至少添加一行 `name=测试源, url=https://example.com`，点击保存完成雇佣

**验收标准**
- `~/.renlijia/users/{scope}/employees/{id}/` 目录存在
- 该目录下存在 `employee.json` 文件，文件中 `templateId` 字段值为 `"builtin:xiaoyuan"`，`name` 字段值为 `"小研"`，`lifecycle` 字段值为 `"active"`
- 该目录下存在 `template/template.json` 文件，文件中 `templateId` 字段值为 `"builtin:xiaoyuan"`，`version` 字段非空字符串
- 该目录下存在 `template/manifest.json` 文件，文件中 `sha256` 字段非空
- 主页员工网格中出现头像「🔍」+ 名称「小研」的卡片
- 调用 `employee_list` 命令返回结果中包含这条新员工记录

---

## 意图 2：派活前置检查未配置资源时弹出配置表单，配置完成后可正常派活

**场景**
对于 `resourceConfigKind === 'monitoring-urls'` 的员工，若 `resourceConfig.monitoringTargets` 为空，点击派活不会真正发起 dispatch，而是弹出资源配置表单；用户填好保存后再次点派活才进入对话页。

**前提**
- 已雇佣员工「小研」（templateId `builtin:xiaoyuan`），`lifecycle === 'active'`
- 该员工 `resourceConfig.monitoringTargets` 为空数组 `[]` 或字段不存在（未配置任何监控目标；HireWizard step 3 空保存得到的就是 `{"monitoringTargets": []}`，前端 `deriveStatus` 据此判定 `needs-setup`）
- 该员工卡片在主页显示状态为 `needs-setup`（橙色圆点 + 需配置）

**操作**
- 用户点击「小研」卡片打开 EmployeeDrawer
- 用户点击底部「现在派活」按钮
- 在弹出的资源配置弹窗中填入 `name=竞品 A`, `url=https://competitor.example.com`，点击保存
- 用户再次点击「现在派活」按钮

**验收标准**
- 第一次点击派活后，没有跳转到对话页，UI 上出现资源配置弹窗（包含 monitoringTargets 输入区）
- 第一次点击派活后，没有调用 `employee_trigger` 命令，没有产生新的 `conversations/{id}/` 目录
- 保存资源后，磁盘上 `employee.json` 中 `resourceConfig.monitoringTargets` 字段是一个长度 ≥ 1 的数组，第 0 项的 `url` 字段值为 `"https://competitor.example.com"`
- 主页员工卡片上的状态从 `needs-setup` 变为 `idle`
- 第二次点击派活后，UI 跳转到新对话页，URL/路由的 `conversationId` 为新生成的 ID
- `~/.renlijia/users/{scope}/conversations/{newConvId}/` 目录存在，`conv.json` 文件存在

---

## 意图 3：用户点击派活后，AI 开始执行并在对话中产生回复

**场景**
对于已经配好资源的员工，点击派活后 `employee_trigger` 真正创建对话、向 LLM 发起一次 chat turn，dispatch prompt 含「请立即开始按职责执行」结尾，AI 流式输出回复并落盘。

**前提**
- 应用已启动、登录、API key 有效
- 已雇佣员工「小研」（`builtin:xiaoyuan`），且 `resourceConfig.monitoringTargets` 已配置至少一行
- 该员工 `lifecycle === 'active'`，主页卡片状态为 `idle`

**操作**
- 用户点击「小研」卡片打开 EmployeeDrawer
- 用户点击底部「现在派活」按钮
- 等待 AI 流式输出完成（出现 StreamDone / AgentIdle 事件后视为完成）

**验收标准**
- UI 跳转到新对话页
- `~/.renlijia/users/{scope}/conversations/{convId}/messages.jsonl` 文件存在
- 文件第 1 条记录 `role` 字段值为 `"user"`，`content.text` 末尾以 `"请立即开始按职责执行"` 结尾
- 文件第 1 条记录 `content.text` 包含「小研」的角色描述（如 `"行业/竞品调研员"` 或 `systemPromptExtra` 关键词「竞品与行业调研」）
- 文件至少存在第 2 条记录 `role` 字段值为 `"assistant"` 的记录，且 `content.text` 不为空
- EventBus 中出现 `TurnCompleted` 事件，`outcome` 字段值为 `"Success"`
- 派活后 `employee.json` 中 `lastRunAt` 字段被更新（不为 null，且时间戳晚于派活前的值）

---

## 意图 4：派活后自动跳转到对话页，执行完成后回员工页能看到「有新汇报」

**场景**
派活会立刻把用户带到新对话页（drawer 自动关闭），AI 在对话页流式输出回复；执行完成后用户回到员工页，对应员工卡片状态变为 `has-report`（有未读汇��）。

**前提**
- 应用已启动、登录、API key 有效
- 已雇佣员工「小研」且资源已配置，`lifecycle === 'active'`
- 该员工当前没有正在跑的 active run
- 跑测前员工卡片状态为 `idle`（无未读汇报）

**操作**
- 用户点击「小研」卡片打开 EmployeeDrawer
- 用户点击底部「现在派活」
- 等待 UI 自动跳转到新对话页（drawer 自动关闭）
- 等待 AI 流式输出完成（出现 AgentIdle 事件 / 顶部「停止」按钮恢复为「发送」）
- 用户从顶部导航回到员工页

**验收标准**

- 派活后 UI 自动跳转到新对话页，URL/路由的 `conversationId` 为新生成的 ID
- 跳转后 EmployeeDrawer 不再可见（自动关闭）
- 对话页 assistant 气泡 `content.text` 不为空（AI 真的产生了回复）
- 派活后员工 `employee.json` 中 `lastRunAt` 字段被更新（时间戳晚于派活前的值），且 `lastError` 字段值为 `null`
- 执行完成后回到员工页，「小研」卡片节点 `[data-aijia-employee-id={id}]` 上 `data-aijia-employee-status` 属性值为 `"has-report"`
- 该卡片节点的可见文本中包含 `"有新汇报"` 子串
- `EventBus` 中存在 `TurnCompleted` 事件，`outcome` 字段值为 `"Success"`

- 派活后 drawer 仍然停留在原页面（产品行为是自动跳转 + 关 drawer）
- 跳转后回员工页，「小研」卡片 `data-aijia-employee-status` 仍为 `"running"`（说明 ActiveRunGuard Drop 时未清理）
- `employee.json` 中 `lastError` 字段非 null（执行有错误）

---

## 意图 5：暂停 cron 后定时不再触发，恢复后正常触发

**场景**
对于配置了 cron 表达式的员工，用户点击「暂停 cron」（`cronEnabled` 置 false）后，scheduler 到点不再自动派活；点击「恢复」后 cron 重新生效。

**前提**
- 已雇佣员工「小研」（`builtin:xiaoyuan`），`cron` 字段非空（默认 `"0 9 * * 1"`）
- 该员工 `cronEnabled === true`，`lifecycle === 'active'`
- 资源配置已完成（避免 needs-setup 干扰）
- 当前没有正在跑的 active run

**操作**
- 用户点击「小研」卡片打开 EmployeeDrawer
- 在「触发器」分组点击 cron 状态切换按钮，把状态从「已启用」切到「已暂停」
- 关闭 Drawer
- 重新打开 Drawer，再次点击切换按钮恢复为「已启用」

**验收标准**
- 第一次点击切换后，磁盘上 `employee.json` 中 `cronEnabled` 字段值为 `false`
- 第一次点击切换后，`employee.json` 中 `nextRunAt` 字段值为 `null`（或不存在）
- 暂停期间主页卡片不显示「下次执行 …」相对时间提示
- 第二次点击切换恢复后，`employee.json` 中 `cronEnabled` 字段值为 `true`
- 恢复后 `employee.json` 中 `nextRunAt` 字段值是一个未来时间戳（晚于当前时刻），且匹配 cron 表达式 `"0 9 * * 1"` 的下一次触发点
- 期间手动点击「现在派活」依然能正常发起 dispatch（cronEnabled 不影响 on-demand）

---

## 意图 6：解雇员工后从列表中消失，员工目录被清理

**场景**
用户在 EmployeeDrawer 底部点击「解雇此员工」并确认后，员工记录被硬删除（PR-7 起改为立即硬删除，不再走 7 天软删除回收站），员工不再出现在主页列表中。

**前提**
- 已雇佣员工「小研」，`lifecycle === 'active'`
- 磁盘上 `~/.renlijia/users/{scope}/employees/{id}/` 目录存在
- 主页员工网格中能看到这条员工卡片
- 当前没有正在跑的 active run

**操作**
- 用户点击「小研」卡片打开 EmployeeDrawer
- 点击底部「解雇此员工」链接
- 在弹出的 Radix `ConfirmDialog` 弹窗（标题/正文含「确定删除「小研」吗？此操作不可恢复」）中点击「确认」

**验收标准**
- `employee_delete` 命令返回 `true`
- 调用 `employee_list` 返回的列表中不再包含该员工 id
- 主页员工网格中不再渲染头像「🔍」+「小研」的卡片
- `~/.renlijia/users/{scope}/employees/{id}/` 目录不存在（已被物理删除）
- 同 id 调用 `employee_get(id)` 返回 Err（记录不存在）
- 后续 cron tick（60s 内的下一次 scheduler 扫描）不会尝试派活这个 id，没有对应的 dispatch 错误日志

---

## 意图 7：lifecycle 为 archived 的员工对用户不可见，且不会被自动调度触发

**场景**
PR-7 起 `employee_delete` 改为硬删除，正常用户操作不再产生 archived 员工。但历史升级遗留的 archived 记录仍可能存在，产品的承诺是：这类记录在主页 / 收件箱里全部隐藏，scheduler tick 也跳过它们，不会偷偷自动派活到对话历史里。

**前提**
- 应用已启动并登录
- 通过手工编辑磁盘构造一条 archived 员工：把任一现存员工的 `~/.renlijia/users/{scope}/employees/{id}/employee.json` 中的 `lifecycle` 字段改为 `"archived"`，记该员工 id 为 `$ARCHIVED_ID`、name 为 `$ARCHIVED_NAME`
- 重启应用让前端 store 重新加载
- 记录当前时间为 `T0`
- 该员工设置了一个会在 `T0 + 2 分钟` 触发的 cron（如 `"* * * * *"`），且 `cronEnabled === true`

**操作**
- 主页打开员工网格，浏览全部卡片
- 打开收件箱（顶部导航「收件箱」），浏览发件人列表
- 等待 `T0 + 3 分钟`（确保 scheduler 至少 tick 过一次 archived 员工原本应触发的时间点）
- 重新打开员工 `$ARCHIVED_ID` 的 `~/.renlijia/users/{scope}/employees/{id}/` 目录，查看是否有新生成的子目录或文件

**验收标准**

- 主页员工网格中**没有** `[data-aijia-employee-id="$ARCHIVED_ID"]` 节点（卡片被前端 filter 掉）
- 收件箱发件人列表中**没有** `$ARCHIVED_NAME` 这一项
- `T0 + 3 分钟` 时该员工 `employee.json` 中 `lastRunAt` 字段值仍等于构造前的旧值（scheduler 未触发新一轮）
- `T0 + 3 分钟` 时 `~/.renlijia/users/{scope}/conversations/` 下没有新增以该员工 id 派��的对话目录
- `~/.renlijia/users/{scope}/employees/$ARCHIVED_ID/employee.json` 文件**仍存在**（数据没被删除，只是 UI 隐藏 + scheduler 跳过），其中 `lifecycle == "archived"`

- 主页或收件箱里出现 archived 员工的卡片 / 发件人入口
- archived 员工目录下出现新的 `conv-*.json` / `dispatch-*.json` 等调度产物
- 应用日志中出现包含 `$ARCHIVED_ID` 的 `dispatch_employee_run` / `cron tick fire` 行
