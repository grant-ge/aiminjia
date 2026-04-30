# rules.md — 权限记住决策与存储测试意图

用户在权限确认框里选择"记住"后，后续同类工具调用应该复用这次决策。

---

## 意图 1：用户选择 Session 记住后，本会话后续同工具同 scope 直接 Allow，且不写入磁盘

**场景**
用户选择"本次会话允许"，同一会话内再次调用同工具不应重复弹窗；但该规则不应跨重启保留。

**前提**
- 工具名 `mcp__demo__tool`
- scope `mcp`
- PermissionStore 初始为空

**操作**
- 持久化 Allow 决策到 Session destination
- 再次对 `mcp__demo__tool + mcp` 调用权限管线
- 检查磁盘上的 workspace/user 权限文件

**断言**
- 第二次结果为 Allow，不是 Ask
- workspace/user 权限文件中不存在该 session 规则

---

## 意图 2：用户选择 Workspace 记住后，workspace 级 Allow 规则被记录

**场景**
用户希望当前项目里以后都允许该工具。

**前提**
- 工具名 `mcp__demo__tool`
- scope `mcp`

**操作**
- 持久化 Allow 决策到 Workspace destination
- 读取 PermissionStore 中 `mcp__demo__tool + mcp` 的规则

**断言**
- store 中存在该工具该 scope 的规则
- 规则 decision 为 Allow / AlwaysAllow
- 规则 source 为 Workspace

---

## 意图 3：用户选择 User 记住后，user 级 Allow 规则被记录

**场景**
用户希望所有项目里以后都允许该工具。

**前提**
- 工具名 `mcp__demo__tool`
- scope `mcp`

**操作**
- 持久化 Allow 决策到 User destination
- 读取 PermissionStore 中 `mcp__demo__tool + mcp` 的规则

**断言**
- store 中存在该工具该 scope 的规则
- 规则 decision 为 Allow / AlwaysAllow
- 规则 source 为 User

---

## 意图 4：用户选择记住 Deny 后，后续同工具同 scope 直接 Deny

**场景**
用户明确拒绝并记住，后续不应再弹窗。

**前提**
- 工具名 `mcp__demo__tool`
- scope `mcp`

**操作**
- 持久化 Deny 决策到 Workspace destination
- 再次对同工具同 scope 调用权限管线

**断言**
- 结果为 Deny，不是 Ask
- store 中对应规则 decision 为 Deny / AlwaysDeny
- 规则 source 为 Workspace

---

## 意图 5：记住规则按 tool_name + scope 精确匹配

**场景**
用户只授权了某个工具的某个 scope，不应错误放行其他工具或其他 scope。

**前提**
- 为工具 A：`mcp__demo__tool_a` 的 scope X：`mcp` 记录 Allow
- 工具 B 为 `mcp__demo__tool_b`
- scope Y 为 `custom:other`

**操作**
- 检查工具 A + scope X
- 检查工具 A + scope Y
- 检查工具 B + scope X

**断言**
- 工具 A + scope X 结果为 Allow
- 工具 A + scope Y 不因该规则直接 Allow（应为 Ask 或 Deny，取决于 pipeline）
- 工具 B + scope X 不因该规则直接 Allow（应为 Ask 或 Deny，取决于 pipeline）

---

## 意图 6：一次工具包含多个 scopes 时，所有 scopes 都被记录

**场景**
一个工具声明多个 capability_scope，用户选择记住后应覆盖全部 scope。

**前提**
- 工具名 `mcp__demo__tool`
- scopes 为 `mcp` 和 `custom:data`

**操作**
- 调用 persist_permission_decision() 记录 Allow 到 Workspace
- 分别读取 `mcp` 和 `custom:data` 的规则

**断言**
- scope `mcp` 有 allow 规则
- scope `custom:data` 有 allow 规则
- 两条规则 source 都是 Workspace

---

## 意图 7：Ask 事件携带默认记住目标 Session

**场景**
权限弹窗默认应该建议"本次会话"，避免用户误把授权扩大到 workspace 或 user。

**前提**
- 权限管线对 `mcp__demo__tool + mcp` 产生 Ask

**操作**
- 读取 Ask 决策中的 default_destination 和 remember_options

**断言**
- default_destination 为 Session
- remember_options 包含 Session、Workspace、User 三项

---

## 意图 8：Workspace/User 权限规则持久化后可跨 PermissionStore 实例读取

**场景**
用户重启 app 后，之前 workspace/user 级别的授权应该仍然生效。

**前提**
- 使用文件型 PermissionStore，根目录为 TempDir
- 记录一条 Workspace 级 Allow 规则：`mcp__demo__tool + mcp`

**操作**
- 丢弃旧 store 实例
- 用同一个 TempDir 重新构造 PermissionStore
- 查询 `mcp__demo__tool + mcp`

**断言**
- 重新构造后的 store 能读到该规则
- 规则 decision 为 Allow / AlwaysAllow
- 权限管线结果为 Allow，不再 Ask

---

## 意图 9：同一 tool_name + scope 后写规则覆盖前写规则

**场景**
用户先允许后又拒绝同一个工具，后一次选择应该生效。

**前提**
- 工具名 `mcp__demo__tool`
- scope `mcp`

**操作**
- 先记录 Workspace Allow
- 再记录 Workspace Deny
- 查询同工具同 scope

**断言**
- 最终规则 decision 为 Deny / AlwaysDeny
- 权限管线结果为 Deny，不是 Allow

---

## 意图 10：Workspace 与 User 同时存在冲突规则时，优先级明确

**场景**
用户级规则和工作区规则可能冲突，系统必须有确定优先级，避免行为漂移。

**前提**
- User 级记录：`mcp__demo__tool + mcp` 为 Allow
- Workspace 级记录：同工具同 scope 为 Deny

**操作**
- 查询最终生效规则

**断言**
- 最终结果必须确定（不能随机）
- 若当前产品规则是 workspace 覆盖 user，则结果为 Deny
- 若当前产品规则是 user 覆盖 workspace，则结果为 Allow
- 测试执行前必须在 test-progress.md 中记录当前实现采用哪种优先级
