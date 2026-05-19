# rules.md — 权限记住决策与存储测试意图

用户在权限确认框里选择"记住"后，后续同类工具调用应该复用这次决策。

---

## 意图 1：用户选择 Session 记住后，本会话后续同工具同 scope 直接 Allow，且不写入磁盘

**场景**
用户选择"本次会话允许"，同一会话内再次调用同工具不应重复弹窗；但该规则不应跨重启保留。

**前提**
- 应用已启动，当前有一个进行中的对话
- 工具 `mcp__demo__tool` 的权限配置为「每次询问」
- 该工具尚未在本会话或 Workspace/User 级别被记住

**操作**
- 在权限弹窗中将「记住」选项选为「本次会话」，然后点击「允许」
- 在同一对话中再次发送一条会触发 `mcp__demo__tool` 的消息

**验收标准**
- 第二次触发时不再弹出权限确认弹窗，工具直接执行
- `~/.renlijia/users/{scope}/permissions.json` 文件中不存在该工具的会话级规则条目

---

## 意图 2：用户选择 Workspace 记住后，workspace 级 Allow 规则被记录

**场景**
用户希望当前项目里以后都允许该工具。

**前提**
- 应用已启动，当前有一个进行中的对话
- 工具 `mcp__demo__tool` 的权限配置为「每次询问」，权限确认弹窗已出现

**操作**
- 在权限弹窗中将「记住」选项选为「本工作区」，然后点击「允许」

**验收标准**
- 弹窗关闭，工具执行成功
- `~/.renlijia/users/{scope}/permissions.json` 中存在该工具 `mcp__demo__tool`、scope `mcp` 的规则条目
- 该条目的 decision 字段值为 Allow 或 AlwaysAllow
- 该条目的 source 字段值为 Workspace

---

## 意图 3：用户选择 User 记住后，user 级 Allow 规则被记录

**场景**
用户希望所有项目里��后都允许该工具。

**前提**
- 应用已启动，当前有一个进行中的对话
- 工具 `mcp__demo__tool` 的权限配置为「每次询问」，权限确认弹窗已出现

**操作**
- 在权限弹窗中将「记住」选项选为「所有项目」，然后点击「允许」

**验收标准**
- 弹窗关闭，工具执行成功
- `~/.renlijia/users/{scope}/permissions.json` 中存在该工具 `mcp__demo__tool`、scope `mcp` 的规则条目
- 该条目的 decision 字段值为 Allow 或 AlwaysAllow
- 该条目的 source 字段值为 User

---

## 意图 4：用户选择记住 Deny 后，后续同工具同 scope 直接 Deny

**场景**
用户明确拒绝并记住，后续不应再弹窗。

**前提**
- 应用已启动，当前有一个进行中的对话
- 工具 `mcp__demo__tool` 的权限配置为「每次询问」，权限确认弹窗已出现

**操作**
- 在权限弹窗中将「记住」选项选为「本工作区」，然后点击「拒绝」
- 在同一对话或新对话中再次发送一条会触发 `mcp__demo__tool` 的消息

**验收标准**
- 第二次触发时不再弹出权限确认弹窗
- 对话界面中该工具调用直接显示为被拒绝或错误状态
- `~/.renlijia/users/{scope}/permissions.json` 中对应规则条目的 decision 字段值为 Deny 或 AlwaysDeny
- 该条目的 source 字段值为 Workspace

---

## 意图 5：记住规则按 tool_name + scope 精确匹配

**场景**
用户只授权了某个工具的某个 scope，不应错误放行其他工具或其他 scope。

**前提**
- 应用已启动，工具 `mcp__demo__tool_a` 的权限已在「本工作区」级别被记住为允许（scope `mcp`）
- 工具 `mcp__demo__tool_b` 尚无任何已记住的规则
- 存在另一个 scope `custom:other` 未被授权

**操作**
- 发送消息依次触发以下三种工具调用并观察是否出现弹窗：
  1. `mcp__demo__tool_a`（scope `mcp`）
  2. `mcp__demo__tool_a`（scope `custom:other`）
  3. `mcp__demo__tool_b`（scope `mcp`）

**验收标准**
- `mcp__demo__tool_a` + scope `mcp`：不弹窗，工具直接执行
- `mcp__demo__tool_a` + scope `custom:other`：弹出权限确认弹窗
- `mcp__demo__tool_b` + scope `mcp`：弹出权限确认弹窗

---

## 意图 6：一次工具包含多个 scopes 时，所有 scopes 都被记录

**场景**
一个工具声明多个 capability_scope，用户选择记住后应覆盖全部 scope。

**前提**
- 应用已启动，工具 `mcp__demo__tool` 声明了 `mcp` 和 `custom:data` 两个 scope
- 权限确认弹窗已出现，「记住」选项为「本工作区」

**操作**
- 在权限弹窗中将「记住」选项选为「本工作区」，然后点击「允许」

**验收标准**
- `~/.renlijia/users/{scope}/permissions.json` 中存在 `mcp__demo__tool` + scope `mcp` 的规则条目，decision 为 Allow 或 AlwaysAllow
- `~/.renlijia/users/{scope}/permissions.json` 中存在 `mcp__demo__tool` + scope `custom:data` 的规则条目，decision 为 Allow 或 AlwaysAllow
- 两条规则的 source 字段均为 Workspace

---

## 意图 7：权限弹窗默认「记住」选项为「本次会话」

**场景**
权限弹窗默认应该建议"本次会话"，避免用户误把授权扩大到 workspace 或 user。

**前提**
- 应用已启动，工具 `mcp__demo__tool` 的权限配置为「每次询问」
- 权限确认弹窗刚刚出现，用户尚未操作任何选项

**操作**
- 观察权限弹窗中「记住」下拉选项的默认选中值

**验收标准**
- 「记住」下拉选项的默认选中值显示为「本次会话」
- 下拉列表展开后包含「本次会话」「本工作区」「所有项目」三项

---

## 意图 8：Workspace/User 权限规则持久化后重启应用仍然生效

**场景**
用户重启 app 后，之前 workspace/user 级别的授权应该仍然生效。

**前提**
- 应用已启动，工具 `mcp__demo__tool` 的权限已在「本工作区」级别被记住为允许
- `~/.renlijia/users/{scope}/permissions.json` 中已存在对应规则条目

**操作**
- 完全退出并重新启动应用
- 发送一条会触发 `mcp__demo__tool` 的消息

**验收标准**
- 重启后不再弹出该工具的权限确认弹窗
- 工具直接执行并在对话中显示为「已完成」（非错误状态）
- `~/.renlijia/users/{scope}/permissions.json` 中对应规则条目仍然存在，decision 字段值为 Allow 或 AlwaysAllow

---

## 意图 9：同一 tool_name + scope 后写规则覆盖前写规则

**场景**
用户先允许后又拒绝同一个工具，后一次选择应该生效。

**前提**
- 应用已启动，工具 `mcp__demo__tool` 的权限已在「本工作区」级别被记住为允许
- `~/.renlijia/users/{scope}/permissions.json` 中已存在 Allow 规则条目

**操作**
- 在设置中将该工具权限改回「每次询问」，再次触发该工具调用
- 在弹出的权限弹窗中将「记住」选项选为「本工作区」，然后点击「拒绝」
- 再次发送消息触发同一工具

**验收标准**
- 第二次拒绝后，后续触发该工具时不再弹窗
- 对话界面中该工具调用直接显示为被拒绝或错误状态
- `~/.renlijia/users/{scope}/permissions.json` 中对应规则条目的 decision 字段值为 Deny 或 AlwaysDeny

---

## 意图 10：Workspace 与 User 同时存在冲突规则时，优先级明确

**场景**
用户级规则和工作区规则可能冲突，系统必须有确定优先级，避免行为漂移。

**前提**
- 应用已启动
- `~/.renlijia/users/{scope}/permissions.json` 中存在两条规则：User 级 Allow 和 Workspace 级 Deny，均针对 `mcp__demo__tool` + scope `mcp`

**操作**
- 发送一条会触发 `mcp__demo__tool` 的消息，观察是否弹窗及工具执行结果

**验收标准**
- 工具不弹窗直接得到确定结果（要么直接执行，要么直接被拒绝）
- 若产品规则是 Workspace 覆盖 User：工具被拒绝，对话界面显示错误或被拒绝状态
- 若产品规则是 User 覆盖 Workspace：工具执行成功，对话界面显示「已完成」
- 执行测试前须在 test-progress.md 中记录当前实现采用哪种优先级
